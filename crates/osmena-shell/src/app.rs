//! One open profile: its engine, its browser model, and its native
//! windows, with the dispatch loop over them (see the crate docs). The
//! process's own side, the event loop and what every profile shares, is
//! `crate::shell`.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use osmena_chrome::fatal::report_fatal;
use osmena_chrome::menus;
use osmena_core::profile::DEFAULT_PROFILE_NAME;
use osmena_core::{
    Bookmarks, BookmarksFile, Brand, Browser, Command, CommandKind, DownloadPolicy, EngineAction,
    EngineEvent, EngineInfo, ExtensionSource, InstalledExtensions, Keymap, LoadState, PageCommand,
    Profile, Session, TabId, UpdateStatus, WindowGeometry, WindowId, apply, bookmarks_html,
    dispatch, window_losing_panel,
};
use osmena_engine::{Engine, StripSnapshot, TabHost, Waker};
use url::Url;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};

use crate::clipboard;
use crate::default_browser;
use crate::favicons::FaviconCache;
use crate::launch::LaunchFetcher;
use crate::load::Opened;
use crate::media_prompt;
use crate::profiles;
use crate::repaint;
use crate::settings_window::{SettingsOutcome, SettingsWindow};
use crate::tab_drop::{DropTarget, drop_target, frontmost, note_focus};
use crate::update::{self, Updater};
use crate::window::{BrowserWindow, DroppedTabs};

/// How long after a history change the file is written, so a run of
/// page loads is saved once.
const HISTORY_SAVE_DELAY: Duration = Duration::from_secs(3);
/// How long after the windows or tabs last changed the session is written.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);
/// How far apart restored tabs are warmed for their titles and favicons
/// (`EngineAction::WarmTabs`), the first one this long after the restore.
const WARM_SPACING: Duration = Duration::from_millis(500);
/// How long a warm waits for the favicon once its page has loaded.
const WARM_GRACE: Duration = Duration::from_secs(3);
/// How long a warm may take altogether, page loaded or not.
const WARM_LIMIT: Duration = Duration::from_secs(20);

/// A restored tab loading out of sight for its title and favicon
/// (`Command::WarmTab`); `App::tick_warms` says when it ends.
struct Warm {
    started: Instant,
    /// When its page finished loading, if it has.
    loaded: Option<Instant>,
    /// Whether the page reported its favicon since the warm began (the
    /// tab may have had one from disk before).
    favicon: bool,
}
/// How long after the start a link the system hands over still counts as
/// the one the browser was launched for (`App::startup_tab`).
const LAUNCH_URL_WINDOW: Duration = Duration::from_secs(10);

/// The user events: the engine asked us to spin its message loop, the
/// `--exit-after` timer ran out, an item of the menu bar was picked
/// (its id waits in `native_menu::take_menu_ids`), or the update check
/// has something to report.
#[derive(Debug)]
pub enum AppEvent {
    Wake,
    ExitAfter(std::time::Duration),
    Menu,
    Update(UpdateStatus),
    /// A Launch Panel fetcher's answer (`crate::launch`), for the
    /// profile of this directory name.
    Launch(String, osmena_core::launch::LaunchUpdate),
    /// Another start of the browser handed over its command line
    /// (`crate::instance`).
    Relaunch(crate::instance::Relaunch),
    /// The system asks this browser to open these (a link clicked in
    /// another app, a page opened with it from the Finder).
    OpenUrls(Vec<Url>),
    /// The system answered the request to make this the default browser;
    /// the status is read again.
    DefaultBrowserAnswered,
    /// Text the system typed with no key event of its own (on Windows,
    /// the character of an Alt code; `crate::alt_code`), for whatever
    /// has the keyboard. Only Windows sends it.
    #[cfg_attr(not(windows), allow(dead_code))]
    Typed(String),
}

pub(crate) struct WinitWaker(pub(crate) Mutex<EventLoopProxy<AppEvent>>);

impl Waker for WinitWaker {
    fn wake(&self) {
        let proxy = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(e) = proxy.send_event(AppEvent::Wake) {
            log::warn!("failed to wake the event loop: {e}");
        }
    }
}

/// What an open profile asks of the process, which the shell carries
/// out once the profile's turn is over (`ProfileApp::take_requests`).
#[derive(Debug)]
pub(crate) enum Request {
    /// Open `profile` (or bring its windows forward, if it is open) with
    /// `urls` as tabs.
    OpenProfile { profile: String, urls: Vec<Url> },
    /// This profile is closing because it was deleted: if nothing else
    /// is open, the default profile takes over.
    HandOver,
    /// The user quit the browser: every other profile quits too, each
    /// through its own guard (`force` skips it: the update's restart).
    QuitAll { force: bool },
    /// A profile was made or deleted: every profile lists them again.
    ProfilesChanged,
}

/// What every open profile shares with the process.
#[derive(Clone)]
pub(crate) struct Shared {
    pub proxy: EventLoopProxy<AppEvent>,
    /// Checks for and installs newer builds; `None` for a build that
    /// cannot update itself.
    pub updater: Rc<Option<Updater>>,
    /// The directory names of the profiles open in this process.
    pub open: Rc<RefCell<BTreeSet<String>>>,
    /// The directory names of the profiles deleted while this process
    /// ran them: not given to a new profile in this run.
    pub retired: Rc<RefCell<BTreeSet<String>>>,
}

pub struct ProfileApp<E: Engine> {
    brand: &'static Brand,
    engine_info: &'static EngineInfo,
    browser: Browser,
    keymap: Keymap,
    // Windows are declared before `engine` so they drop first: every tab
    // host and GL surface must be gone before the engine shuts down.
    windows: HashMap<winit::window::WindowId, BrowserWindow<E>>,
    settings_window: Option<SettingsWindow<E::Surface>>,
    pending_actions: Vec<EngineAction>,
    engine: Option<E>,
    shared: Shared,
    /// What this profile asked of the process since the shell last
    /// looked.
    requests: Vec<Request>,
    /// A Quit that came from another profile is not sent round again.
    relayed_quit: bool,
    /// This profile's windows are gone and its engine is to go: the
    /// shell takes it away (`ProfileApp::shutdown`).
    exiting: bool,
    /// The first window could not be opened, so the profile has nothing
    /// to show.
    window_failed: bool,
    /// When the history last changed without being saved since.
    history_dirty: Option<Instant>,
    /// The strip as the engine was last told it (`Engine::strip_changed`):
    /// the next is sent only when it differs. `None` until the first, and
    /// always for an engine that runs no extensions.
    last_strip: Option<StripSnapshot>,
    /// The user deleted the profile this browser runs: its directory
    /// goes once the engine has let go of it, on the way out.
    delete_on_exit: Option<Profile>,
    /// This profile runs the automatic update checks: one open profile
    /// does, the shell says which.
    update_lead: bool,
    /// When the next automatic update check is due.
    update_check_due: Option<Instant>,
    /// The favicons of visited sites on disk, for the bookmarks bar and
    /// the Launch Panel's most visited sites.
    favicons: FaviconCache,
    /// Fetches the Launch Panel's weather and Drive files.
    launch: LaunchFetcher,
    /// When the windows or tabs last changed without the session being
    /// written since.
    session_dirty: Option<Instant>,
    /// Restored tabs still to warm, in strip order, and when the next
    /// one is due (`EngineAction::WarmTabs`).
    warm_queue: VecDeque<TabId>,
    warm_next: Option<Instant>,
    /// The tabs warming now.
    warms: HashMap<TabId, Warm>,
    /// Where a window about to open goes (`EngineAction::PlaceWindow`
    /// arrives before its `OpenWindow`).
    pending_geometry: HashMap<WindowId, WindowGeometry>,
    /// The browser windows in the order they last had the keyboard,
    /// latest first: which of two overlapping windows is in front, for
    /// tabs dropped on them (`tab_drop::frontmost`).
    focus_order: Vec<winit::window::WindowId>,
    /// The session was written for this exit already (before the
    /// windows went), so `exiting` leaves it alone.
    session_final: bool,
    /// When this browser started.
    started: Instant,
    /// The tab the first window opened on the home page because the
    /// start named nothing to open: when the system launched the browser
    /// for a link, the link arrives a moment later and takes this tab's
    /// place instead of opening beside it.
    startup_tab: Option<TabId>,
    /// Asks the event loop to read the default-browser status again.
    default_browser_proxy: Arc<Mutex<EventLoopProxy<AppEvent>>>,
}

impl<E: Engine> ProfileApp<E> {
    /// An open profile: `opened` is its model with the first windows in
    /// it, `engine` the engine started for it. Nothing is on screen
    /// until [`ProfileApp::start`].
    pub(crate) fn new(
        brand: &'static Brand,
        engine_info: &'static EngineInfo,
        opened: Opened,
        engine: E,
        shared: Shared,
    ) -> ProfileApp<E> {
        let Opened {
            mut browser,
            keymap,
            actions,
            homepage_tab,
        } = opened;
        browser.profiles = browser.profile.siblings(brand);
        browser.default_browser = default_browser::status();
        log::info!("default browser: {:?}", browser.default_browser);
        profiles::claim(&browser.profile.pid_path());
        // A guest's directory goes with its browser.
        let delete_on_exit = browser.profile.guest.then(|| browser.profile.clone());
        if browser.profile.guest {
            log::info!(
                "running as a guest: profile {:?} goes at exit",
                browser.profile.name
            );
        }
        let mut favicons = FaviconCache::new(browser.profile.favicons_dir());
        favicons.preload(bookmarked_hosts(&browser.bookmarks));
        let launch = LaunchFetcher::new(
            brand,
            browser.profile.name.clone(),
            browser.profile.google_path(),
            shared.proxy.clone(),
        );
        browser.launch.drive = launch.initial_drive_state();
        let default_browser_proxy = Arc::new(Mutex::new(shared.proxy.clone()));
        ProfileApp {
            brand,
            engine_info,
            browser,
            keymap,
            windows: HashMap::new(),
            settings_window: None,
            pending_actions: actions,
            engine: Some(engine),
            shared,
            requests: Vec::new(),
            relayed_quit: false,
            exiting: false,
            window_failed: false,
            history_dirty: None,
            last_strip: None,
            delete_on_exit,
            update_lead: false,
            update_check_due: None,
            favicons,
            launch,
            session_dirty: None,
            warm_queue: VecDeque::new(),
            warm_next: None,
            warms: HashMap::new(),
            pending_geometry: HashMap::new(),
            focus_order: Vec::new(),
            session_final: false,
            started: Instant::now(),
            startup_tab: homepage_tab,
            default_browser_proxy,
        }
    }

    /// Tell the engine what the profile's settings say and open the
    /// first windows.
    pub(crate) fn start(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(engine) = self.engine.as_ref() {
            if self.engine_info.downloads {
                engine.configure_downloads(self.download_policy());
            }
            // Every engine gets the list; one without a hook ignores it
            // (the method is defaulted) and still blocks what the user
            // types through the model.
            engine.configure_blocked_sites(self.browser.blocked.clone());
        }
        log::info!(
            "profile {:?} at {}",
            self.browser.profile.name,
            self.browser.profile.dir.display()
        );
        let actions = std::mem::take(&mut self.pending_actions);
        self.perform_all(actions, event_loop);
        // The first windows are open: the engine learns the strip before
        // any extension can ask about it.
        self.sync_strip();
    }

    /// The profile's directory name.
    pub(crate) fn name(&self) -> &str {
        &self.browser.profile.name
    }

    pub(crate) fn profile(&self) -> &Profile {
        &self.browser.profile
    }

    /// The profile's windows are gone (or never came): the shell takes
    /// it away.
    pub(crate) fn is_exiting(&self) -> bool {
        self.exiting
    }

    /// What this profile asked of the process since the last call.
    pub(crate) fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// Whether `id` is one of this profile's windows, its settings
    /// window included.
    pub(crate) fn owns_window(&self, id: winit::window::WindowId) -> bool {
        self.windows.contains_key(&id)
            || self
                .settings_window
                .as_ref()
                .is_some_and(|sw| sw.winit_id() == id)
    }

    /// Whether one of this profile's windows has the keyboard.
    pub(crate) fn has_focus(&self) -> bool {
        self.windows.values().any(|w| w.has_focus())
            || self
                .settings_window
                .as_ref()
                .is_some_and(|sw| sw.has_focus())
    }

    /// Bring this profile's window (the focused one, else any) to the
    /// front.
    pub(crate) fn bring_to_front(&mut self) {
        if let Some(id) = self.menu_window()
            && let Some(w) = self.window_for_core_id(id)
        {
            w.bring_to_front();
        }
    }

    /// Where the browser is with updates, as this profile shows it.
    pub(crate) fn update_status(&self) -> &UpdateStatus {
        &self.browser.update
    }

    /// This profile runs the automatic update checks from now on, or no
    /// longer does.
    pub(crate) fn set_update_lead(&mut self, lead: bool) {
        if self.update_lead == lead {
            return;
        }
        self.update_lead = lead;
        if lead {
            self.schedule_update_check(update::FIRST_CHECK_DELAY);
        } else {
            self.update_check_due = None;
        }
    }

    /// Quit this profile because another one was asked to quit the
    /// browser: through its own guard, unless `force`.
    pub(crate) fn quit_relayed(&mut self, force: bool, event_loop: &ActiveEventLoop) {
        if self.exiting {
            return;
        }
        self.relayed_quit = true;
        if force {
            self.perform(EngineAction::Quit, event_loop);
        } else {
            self.run_commands(vec![Command::Quit], event_loop);
        }
        self.relayed_quit = false;
    }

    /// Show `status` as where the browser is with updates: another
    /// profile moved it.
    pub(crate) fn set_update_status(&mut self, status: UpdateStatus) {
        self.browser.update = status;
        self.redraw_all();
    }

    /// The addresses a command line names, read as the address bar
    /// would with this profile's search engine; what is neither a URL
    /// nor searchable is logged and left out.
    pub(crate) fn typed_urls(&self, typed: &[String]) -> Vec<Url> {
        typed
            .iter()
            .filter_map(|text| {
                let url = osmena_chrome::chrome::location_input_to_url(text)
                    .or_else(|| self.browser.settings.search.url_for(text));
                if url.is_none() {
                    log::warn!("not a URL: {text:?}");
                }
                url
            })
            .collect()
    }

    /// The open tabs, as `--exit-after` logs them.
    pub(crate) fn describe_tabs(&self) -> Vec<String> {
        self.browser
            .windows
            .iter()
            .flat_map(|w| w.tabs.iter())
            .map(|t| format!("{:?} at {}", t.title, t.url))
            .collect()
    }

    /// Open another profile with `urls`: in this process when the engine
    /// runs them together (the shell does it once this turn is over),
    /// else as a browser process of its own.
    fn open_profile(&mut self, profile: &str, urls: &[Url]) -> std::io::Result<()> {
        if self.engine_info.profiles_in_process {
            self.requests.push(Request::OpenProfile {
                profile: profile.to_string(),
                urls: urls.to_vec(),
            });
            Ok(())
        } else {
            crate::process::open_in_profile(profile, urls)
        }
    }

    /// Read the default-browser status again and show it if it changed.
    pub(crate) fn refresh_default_browser(&mut self) {
        let status = default_browser::status();
        if status != self.browser.default_browser {
            log::info!("default browser: {status:?}");
            self.browser.default_browser = status;
            self.redraw_all();
        }
    }

    /// Open what the system handed over: each URL in a new tab of the
    /// focused window (or any), brought to the front. The link the
    /// browser was launched for loads in the tab the start opened on
    /// the home page instead, while that tab is still on it.
    pub(crate) fn open_urls(&mut self, urls: Vec<Url>, event_loop: &ActiveEventLoop) {
        let Some(window) = self.menu_window() else {
            // No browser window: the first URL opens one, the rest tabs.
            let mut urls = urls.into_iter();
            if let Some(first) = urls.next() {
                self.run_commands(vec![Command::OpenInNewWindow(first)], event_loop);
                self.open_urls(urls.collect(), event_loop);
            }
            return;
        };
        let mut urls = urls.into_iter();
        let mut commands = Vec::new();
        let homepage = self.browser.homepage();
        if let Some(tab) = self.startup_tab.take()
            && self.started.elapsed() < LAUNCH_URL_WINDOW
            && self
                .browser
                .window(window)
                .and_then(|w| w.tab(tab))
                .is_some_and(|t| t.url == homepage)
            && let Some(first) = urls.next()
        {
            commands.push(Command::Navigate { tab, url: first });
        }
        commands.extend(urls.map(|url| Command::NewTab {
            window,
            url: Some(url),
        }));
        self.run_commands(commands, event_loop);
        if let Some(w) = self.window_for_core_id(window) {
            w.bring_to_front();
        }
    }

    /// Restored tabs to warm, a moment apart, in this order. Until each
    /// is, its favicon is the one on disk for its site, if there is one.
    fn warm_tabs(&mut self, tabs: Vec<TabId>) {
        let hosts: Vec<String> = tabs
            .iter()
            .filter_map(|tab| self.browser.tab(*tab))
            .filter_map(|t| Bookmarks::site_key(&t.url))
            .collect();
        self.favicons.preload(hosts.into_iter());
        for tab in &tabs {
            if let Some(t) = self.browser.tab_mut(*tab)
                && t.favicon.is_none()
                && let Some(host) = Bookmarks::site_key(&t.url)
                && let Some(icon) = self.favicons.icons().get(&host)
            {
                t.favicon = Some(icon.clone());
            }
        }
        self.warm_queue.extend(tabs);
        if self.warm_next.is_none() && !self.warm_queue.is_empty() {
            self.warm_next = Some(Instant::now() + WARM_SPACING);
        }
    }

    /// Start the next warm when it is due and end the ones that are
    /// done. A warm ends once its page has loaded and reported its
    /// favicon, a moment after loading with no favicon coming, or after
    /// long enough whatever happened; a tab the user showed or closed in
    /// the meantime is no longer warming and is simply forgotten.
    /// Returns when this wants running again.
    fn tick_warms(&mut self, event_loop: &ActiveEventLoop) -> Option<Instant> {
        let now = Instant::now();
        if let Some(due) = self.warm_next
            && due <= now
        {
            self.warm_next = None;
            // A tab shown or closed before its turn asks for nothing;
            // the next one in line takes the turn.
            while let Some(tab) = self.warm_queue.pop_front() {
                let actions = dispatch(&mut self.browser, &Command::WarmTab(tab));
                if actions.is_empty() {
                    continue;
                }
                if let Some(t) = self.browser.tab(tab) {
                    log::debug!("warming {tab:?} at {}", t.url);
                }
                self.warms.insert(
                    tab,
                    Warm {
                        started: now,
                        loaded: None,
                        favicon: false,
                    },
                );
                self.perform_all(actions, event_loop);
                break;
            }
            if !self.warm_queue.is_empty() {
                self.warm_next = Some(now + WARM_SPACING);
            }
        }
        let mut next = self.warm_next;
        let mut done = Vec::new();
        for (tab, warm) in &self.warms {
            let over = match self.browser.tab(*tab) {
                Some(t) if t.warming => {
                    let limit = warm.started + WARM_LIMIT;
                    let deadline = warm.loaded.map_or(limit, |at| (at + WARM_GRACE).min(limit));
                    if (warm.loaded.is_some() && warm.favicon) || deadline <= now {
                        true
                    } else {
                        next = repaint::earliest(next, Some(deadline));
                        false
                    }
                }
                _ => true,
            };
            if over {
                done.push(*tab);
            }
        }
        for tab in done {
            let warm = self.warms.remove(&tab);
            if let (Some(warm), Some(t)) = (warm, self.browser.tab(tab))
                && t.warming
            {
                log::debug!(
                    "{tab:?} warmed in {:.1} s: title {:?}, favicon {}",
                    warm.started.elapsed().as_secs_f32(),
                    t.title,
                    if warm.favicon { "in" } else { "not reported" }
                );
            }
            let actions = dispatch(&mut self.browser, &Command::WarmDone(tab));
            self.perform_all(actions, event_loop);
        }
        next
    }

    /// Write the session a moment from now.
    fn session_changed(&mut self) {
        self.session_dirty.get_or_insert_with(Instant::now);
    }

    /// The windows and tabs as they stand, with their places on screen.
    fn capture_session(&self) -> Session {
        let windows = &self.windows;
        let geometry = |id: WindowId| {
            windows
                .values()
                .find(|w| w.core_id() == id)
                .and_then(|w| w.geometry())
        };
        Session::capture(&self.browser, &geometry)
    }

    fn write_session(&mut self, session: &Session) {
        self.session_dirty = None;
        let path = self.browser.profile.session_path();
        if let Err(e) = session.save(&path) {
            log::error!("cannot save the session to {}: {e}", path.display());
        }
    }

    /// Write the windows and tabs as they stand.
    fn save_session(&mut self) {
        let session = self.capture_session();
        self.write_session(&session);
    }

    /// The session as it stands is what the next start should bring
    /// back: written now, before the windows go, and not again at exit.
    fn save_session_for_exit(&mut self, session: Option<Session>) {
        if self.session_final || self.delete_on_exit.is_some() {
            return;
        }
        let session = session.unwrap_or_else(|| self.capture_session());
        self.write_session(&session);
        self.session_final = true;
    }

    /// `geometry`, unless its top-left corner is off every monitor (one
    /// was unplugged): then only the size is kept.
    fn on_a_monitor(event_loop: &ActiveEventLoop, geometry: WindowGeometry) -> WindowGeometry {
        let (x, y) = (geometry.x + 32, geometry.y + 32);
        let visible = event_loop.available_monitors().any(|m| {
            let p = m.position();
            let s = m.size();
            x >= p.x
                && y >= p.y
                && x < p.x.saturating_add(s.width as i32)
                && y < p.y.saturating_add(s.height as i32)
        });
        if visible {
            geometry
        } else {
            WindowGeometry {
                x: i32::MIN,
                y: i32::MIN,
                ..geometry
            }
        }
    }

    /// Whether this build checks for updates on its own: it can, and
    /// the setting says so.
    fn checks_automatically(&self) -> bool {
        self.update_lead && self.shared.updater.is_some() && self.browser.settings.check_for_updates
    }

    /// Put the next automatic check `delay` from now, unless one is
    /// already due sooner.
    fn schedule_update_check(&mut self, delay: Duration) {
        if !self.checks_automatically() {
            self.update_check_due = None;
            return;
        }
        let at = Instant::now() + delay;
        self.update_check_due = Some(match self.update_check_due {
            Some(due) if due < at => due,
            _ => at,
        });
    }

    /// The automatic check is due: run it as the menu entry would, and
    /// book the next one.
    fn automatic_update_check(&mut self, event_loop: &ActiveEventLoop) {
        self.update_check_due = None;
        if !self.checks_automatically() {
            return;
        }
        self.schedule_update_check(update::CHECK_INTERVAL);
        if self.browser.update.can_check() {
            self.run_commands(vec![Command::CheckForUpdates], event_loop);
        }
    }

    /// The update check reported: show it.
    pub(crate) fn update_reported(&mut self, status: UpdateStatus) {
        if matches!(self.browser.update, UpdateStatus::Installing(_)) {
            // On the way out; nothing changes that.
            return;
        }
        match &status {
            UpdateStatus::Ready { update, file } => {
                log::info!("{} is ready at {}", update.id, file.display())
            }
            UpdateStatus::Failed(message) => log::warn!("update: {message}"),
            _ => {}
        }
        self.browser.update = status;
        self.redraw_all();
    }

    /// A Launch Panel fetcher answered: fold it in and redraw, so an
    /// open panel shows it.
    pub(crate) fn launch_reported(&mut self, update: osmena_core::launch::LaunchUpdate) {
        if let osmena_core::launch::LaunchUpdate::GoogleConnected(Ok(account)) = &update {
            log::info!("Google Drive connected as {account}");
        }
        self.browser
            .launch
            .apply(update, osmena_core::history::unix_now());
        self.redraw_all();
    }

    /// The title of a browser window: the product, and the profile's
    /// name when it is not the default one.
    fn window_title(&self) -> String {
        if self.browser.profile.is_default() {
            self.brand.name.to_string()
        } else {
            format!(
                "{} \u{2013} {}",
                self.brand.name,
                self.browser.profile_name()
            )
        }
    }

    /// Something went wrong that the user should see: the status area
    /// shows it until dismissed.
    pub(crate) fn report_error(&mut self, message: String, event_loop: &ActiveEventLoop) {
        log::error!("{message}");
        let actions = apply(&mut self.browser, &EngineEvent::EngineError(message));
        self.perform_all(actions, event_loop);
    }

    /// List the other profiles again (one may have been made or deleted,
    /// here or in another browser).
    pub(crate) fn refresh_profiles(&mut self) {
        self.browser.profiles = self.browser.profile.siblings(self.brand);
        self.redraw_all();
    }

    /// Whether the bookmarks are the default profile's file.
    fn bookmarks_shared(&self) -> bool {
        self.browser.settings.profile.share_bookmarks && !self.browser.profile.is_default()
    }

    /// Read the bookmarks from their file again (the default profile's,
    /// when shared: another browser may have changed it).
    fn reload_bookmarks(&mut self, event_loop: &ActiveEventLoop) {
        let path = self.browser.bookmarks_path();
        match Bookmarks::load(&path) {
            Ok(bookmarks) => {
                self.browser.bookmarks = bookmarks;
                self.favicons
                    .preload(bookmarked_hosts(&self.browser.bookmarks));
            }
            Err(e) => self.report_error(
                format!("cannot read bookmarks from {}: {e}", path.display()),
                event_loop,
            ),
        }
    }

    /// The system's file dialog for a bookmarks file, then the import or
    /// the export itself.
    fn pick_bookmarks_file(&mut self, which: BookmarksFile, event_loop: &ActiveEventLoop) {
        let dialog = rfd::FileDialog::new().add_filter("HTML bookmarks file", &["html", "htm"]);
        match which {
            BookmarksFile::Import => {
                let Some(path) = dialog.set_title("Import bookmarks").pick_file() else {
                    return;
                };
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let source = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.run_commands(
                            vec![Command::ImportBookmarks { text, source }],
                            event_loop,
                        );
                    }
                    Err(e) => self
                        .report_error(format!("cannot read {}: {e}", path.display()), event_loop),
                }
            }
            BookmarksFile::Export => {
                let Some(path) = dialog
                    .set_title("Export bookmarks")
                    .set_file_name("bookmarks.html")
                    .save_file()
                else {
                    return;
                };
                self.run_commands(vec![Command::ExportBookmarks(path)], event_loop);
            }
        }
    }

    /// Install the default profile's extensions that this profile lacks,
    /// from the snapshot the default profile's browser wrote.
    fn copy_extensions_from_default(&mut self, event_loop: &ActiveEventLoop) {
        let Ok(default) = self.browser.profile.sibling(DEFAULT_PROFILE_NAME) else {
            return;
        };
        let snapshot = match InstalledExtensions::load(&default.extensions_path()) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                self.report_error(
                    format!("cannot read the default profile's extensions: {e}"),
                    event_loop,
                );
                return;
            }
        };
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        let missing = snapshot.missing_from(&self.browser.extensions);
        log::info!(
            "copying {} extension(s) from the default profile",
            missing.len()
        );
        for extension in missing {
            engine.install_extension(ExtensionSource::Store {
                id: extension.id.clone(),
            });
        }
    }

    /// True if the profile's first window could not be opened.
    pub(crate) fn window_failed(&self) -> bool {
        self.window_failed
    }

    fn window_for_tab(&mut self, tab: TabId) -> Option<&mut BrowserWindow<E>> {
        self.windows.values_mut().find(|w| w.hosts_tab(tab))
    }

    fn window_for_core_id(&mut self, id: WindowId) -> Option<&mut BrowserWindow<E>> {
        self.windows.values_mut().find(|w| w.core_id() == id)
    }

    /// The command for tabs dragged off the strip of `source` and let go:
    /// to the other browser window of this profile under the pointer, or
    /// to a new window where they were dropped ([`drop_target`] has the
    /// rule).
    fn tab_drop_command(
        &self,
        source: winit::window::WindowId,
        drop: DroppedTabs,
    ) -> Option<Command> {
        let under: Vec<winit::window::WindowId> = self
            .windows
            .iter()
            .filter(|(id, w)| **id != source && w.contains_screen_point(drop.point))
            .map(|(id, _)| *id)
            .collect();
        let other = frontmost(&under, &self.focus_order).and_then(|id| self.windows.get(&id));
        let index = other.and_then(|w| w.strip_drop_index(drop.point));
        let other = other.map(BrowserWindow::core_id);
        let source = self.windows.get(&source)?;
        let whole_window = self
            .browser
            .window(source.core_id())
            .is_some_and(|w| w.tabs.iter().all(|tab| drop.tabs.contains(&tab.id)));
        match drop_target(
            other,
            source.contains_screen_point(drop.point),
            whole_window,
        ) {
            DropTarget::Window(window) => Some(Command::MoveTabsToWindow {
                tabs: drop.tabs,
                window,
                index,
                folder: drop.folder,
            }),
            DropTarget::NewWindow => Some(Command::MoveTabsToNewWindow {
                tabs: drop.tabs,
                place: drop.place,
                folder: drop.folder,
            }),
            DropTarget::Nothing => None,
        }
    }

    pub(crate) fn run_commands(&mut self, commands: Vec<Command>, event_loop: &ActiveEventLoop) {
        if !commands.is_empty() {
            self.session_changed();
        }
        for command in commands {
            // Quit is the browser's: the other open profiles hear of it
            // too, each with its own guard for downloads still going.
            if matches!(command, Command::Quit) && !self.relayed_quit {
                self.requests.push(Request::QuitAll { force: false });
            }
            // Closing the last window or tab quits: the session to come
            // back to is the one before the close, not the empty model
            // after it.
            let before_close = matches!(
                command,
                Command::CloseWindow(_)
                    | Command::CloseTab(_)
                    | Command::CloseTabs(_)
                    | Command::CloseFolder(_)
            )
            .then(|| self.capture_session());
            // The find bar is the engine's to fill; an engine that cannot
            // search never gets a bar (its entries are disabled, but the
            // chord is still bound).
            if matches!(
                command,
                Command::OpenFind(_)
                    | Command::FindText { .. }
                    | Command::FindNext(_)
                    | Command::FindPrevious(_)
            ) && !self.engine_info.find
            {
                continue;
            }
            // Shared bookmarks are another browser's too: start from what
            // is on disk, so its additions survive ours.
            if command.changes_bookmarks() && self.bookmarks_shared() {
                self.reload_bookmarks(event_loop);
            }
            let actions = dispatch(&mut self.browser, &command);
            if actions.iter().any(|a| matches!(a, EngineAction::Quit))
                && let Some(session) = before_close
            {
                self.save_session_for_exit(Some(session));
            }
            self.perform_all(actions, event_loop);
        }
    }

    /// The browser window the menu bar acts on: the focused one, else
    /// any (a settings window may have the focus).
    fn menu_window(&self) -> Option<WindowId> {
        self.windows
            .values()
            .find(|w| w.has_focus())
            .or_else(|| self.windows.values().next())
            .map(BrowserWindow::core_id)
    }

    /// The update entry of the menus, from the model's status.
    fn update_menu(&self) -> menus::UpdateMenu<'_> {
        menus::UpdateMenu {
            brand: self.brand.name,
            status: &self.browser.update,
            can_update: self.brand.can_update(),
        }
    }

    /// What the menu bar shows while this profile's window is in
    /// front.
    pub(crate) fn menu_bar_entries(&self) -> Vec<menus::MenuEntry> {
        let window = self.menu_window().and_then(|id| self.browser.window(id));
        let profile_name = self.browser.profile_name();
        let updates = self.update_menu();
        menus::menu_bar(&menus::MenuBarInput {
            brand: self.brand.name,
            window,
            keymap: &self.keymap,
            can_reopen: !self.browser.closed_tabs.is_empty(),
            can_find: self.engine_info.find,
            can_download: self.engine_info.downloads,
            bookmarks: menus::BookmarksMenu {
                bookmarks: &self.browser.bookmarks,
                bar_shown: self.browser.settings.show_bookmarks_bar,
            },
            history: &self.browser.history,
            profile: menus::CurrentProfile {
                dir: &self.browser.profile.name,
                name: &profile_name,
                guest: self.browser.profile.guest,
            },
            profiles: &self.browser.profiles,
            updates,
        })
    }

    /// Tell the engine the strip, if it differs from the one it has: the
    /// tabs, folders, shown panel and focus of every window. Called once
    /// the windows first open and then before every wait, by which time
    /// the commands and engine events of this turn have been applied, so
    /// a reply to an extension's request always precedes the strip that
    /// shows its effect. Only an engine that runs extensions
    /// (`EngineInfo::extensions` is `Some`) is told, whether or not it
    /// has side panels: its extensions ask about tabs and windows either
    /// way.
    ///
    /// Building the snapshot clones each tab's address and title, a few
    /// microseconds for a hundred tabs, and no engine without extensions
    /// pays it.
    fn sync_strip(&mut self) {
        if self.engine_info.extensions.is_none() {
            return;
        }
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        // The focused window is the model's (`Browser::focused`, set in
        // `window_event`): the same one a request that names no window
        // lands in.
        let snapshot = StripSnapshot::of(&self.browser, self.browser.focused);
        if self.last_strip.as_ref() == Some(&snapshot) {
            return;
        }
        snapshot.lend(|windows| engine.strip_changed(windows));
        self.last_strip = Some(snapshot);
    }

    /// Run a command picked from the menu bar. An editing command goes
    /// to whatever has the keyboard: a text field of the chrome or of
    /// the settings window gets it as the egui events its key chord
    /// would have been, the page otherwise.
    pub(crate) fn menu_pick(&mut self, command: Command, event_loop: &ActiveEventLoop) {
        if let Command::Page { command: page, .. } = &command
            && let Some(events) = edit_events(page)
        {
            if let Some(sw) = self.settings_window.as_mut()
                && sw.has_focus()
            {
                sw.inject_egui_events(events);
                return;
            }
            if let Some(w) = self.windows.values_mut().find(|w| w.has_focus())
                && w.egui_wants_keyboard()
            {
                w.inject_egui_events(events);
                return;
            }
        }
        self.run_commands(vec![command], event_loop);
    }

    fn perform_all(&mut self, actions: Vec<EngineAction>, event_loop: &ActiveEventLoop) {
        for action in actions {
            if self.exiting {
                return;
            }
            self.perform(action, event_loop);
        }
    }

    fn perform(&mut self, action: EngineAction, event_loop: &ActiveEventLoop) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        match action {
            EngineAction::PlaceWindow { window, geometry } => {
                self.pending_geometry
                    .insert(window, Self::on_a_monitor(event_loop, geometry));
            }
            EngineAction::OpenWindow { window, tab, url } => {
                let frameless = !self.browser.settings.system_title_bar;
                let title = self.window_title();
                let geometry = self.pending_geometry.remove(&window);
                match BrowserWindow::open(
                    event_loop,
                    engine,
                    self.brand,
                    self.engine_info,
                    window,
                    &title,
                    frameless,
                    geometry,
                ) {
                    Ok(mut bw) => {
                        bw.host_mut().create_tab(tab, url);
                        bw.focus_tab(tab);
                        self.windows.insert(bw.winit_id(), bw);
                    }
                    Err(e) => {
                        // Keep the model honest: the window never existed.
                        // If it was the last one, this quits instead of
                        // waiting forever with nothing on screen.
                        if self.windows.is_empty() {
                            self.window_failed = true;
                        }
                        report_fatal(
                            self.brand,
                            &format!(
                                "{}: cannot open a window (profile {:?} at {}): {e}",
                                self.brand.binary,
                                self.browser.profile.name,
                                self.browser.profile.dir.display()
                            ),
                        );
                        let actions = dispatch(&mut self.browser, &Command::CloseWindow(window));
                        self.perform_all(actions, event_loop);
                    }
                }
            }
            EngineAction::CloseWindow(window) => {
                self.windows.retain(|_, w| w.core_id() != window);
                self.focus_order.retain(|id| self.windows.contains_key(id));
            }
            EngineAction::CreateTab { window, tab, url } => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.host_mut().create_tab(tab, url);
                    w.focus_tab(tab);
                    w.request_redraw();
                }
            }
            EngineAction::CreateHiddenTab { window, tab, url } => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.host_mut().create_tab(tab, url);
                    w.host().hide(tab);
                    w.request_redraw();
                }
            }
            EngineAction::AdoptTab {
                window,
                tab,
                pending,
                background,
            } => {
                let adopted = match self.window_for_core_id(window) {
                    Some(w) => {
                        let adopted = w.host_mut().adopt_tab(pending, tab);
                        if adopted && background {
                            w.host().hide(tab);
                        } else if adopted {
                            w.focus_tab(tab);
                        }
                        w.request_redraw();
                        Some(adopted)
                    }
                    None => None,
                };
                match adopted {
                    // The opener's window closed meanwhile; let the engine
                    // forget the request.
                    None => {
                        if let Some(engine) = self.engine.as_ref() {
                            engine.discard_pending(pending);
                        }
                    }
                    Some(true) => {}
                    Some(false) => {
                        // apply already added and activated the tab, but the
                        // engine has no content for it. Close it again, as
                        // dispatch would, so the model matches the engine.
                        log::warn!("engine had no pending tab {pending:?} to adopt");
                        let actions = dispatch(&mut self.browser, &Command::CloseTab(tab));
                        self.perform_all(actions, event_loop);
                    }
                }
            }
            EngineAction::Navigate { tab, url } => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().navigate(tab, url);
                }
            }
            EngineAction::Back(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().back(tab);
                }
            }
            EngineAction::Forward(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().forward(tab);
                }
            }
            EngineAction::Reload(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().reload(tab);
                }
            }
            EngineAction::Stop(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().stop(tab);
                }
            }
            EngineAction::Show(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().show(tab);
                    w.focus_tab(tab);
                    w.request_redraw();
                }
            }
            EngineAction::Hide(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().set_focused(tab, false);
                    w.host().hide(tab);
                }
            }
            EngineAction::CloseTab(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.close_tab(tab);
                }
            }
            EngineAction::WarmTabs(tabs) => self.warm_tabs(tabs),
            EngineAction::FocusAddressBar(window) => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.focus_address_bar();
                }
            }
            EngineAction::FocusFind(window) => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.focus_find();
                }
            }
            EngineAction::Find {
                tab,
                text,
                forward,
                match_case,
                find_next,
            } => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.find(tab, &text, forward, match_case, find_next);
                }
            }
            EngineAction::StopFinding {
                tab,
                clear_selection,
            } => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.stop_finding(tab, clear_selection);
                }
            }
            EngineAction::SetZoom { tab, percent } => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.set_zoom(tab, percent);
                    w.request_redraw();
                }
            }
            EngineAction::SaveZoom => {
                let path = self.browser.profile.zoom_path();
                if let Err(e) = self.browser.zoom.save(&path) {
                    log::error!("cannot save the zoom levels to {}: {e}", path.display());
                }
            }
            EngineAction::SaveBlockedSites => {
                let path = self.browser.profile.blocked_path();
                if let Err(e) = self.browser.blocked.save(&path) {
                    log::error!("cannot save the blocked sites to {}: {e}", path.display());
                }
            }
            EngineAction::ConfigureBlockedSites(sites) => engine.configure_blocked_sites(sites),
            EngineAction::ReplaceWithBlank(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().replace_with_blank(tab);
                    // The block page is the chrome's: paint it now rather
                    // than when the engine next produces a frame.
                    w.request_redraw();
                }
            }
            EngineAction::ToggleDownloads(window) => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.toggle_downloads();
                }
            }
            EngineAction::CancelDownload(id) => {
                if let Some(engine) = &self.engine {
                    engine.cancel_download(id);
                }
            }
            EngineAction::PauseDownload(id) => {
                if let Some(engine) = &self.engine {
                    engine.pause_download(id);
                }
            }
            EngineAction::ResumeDownload(id) => {
                if let Some(engine) = &self.engine {
                    engine.resume_download(id);
                }
            }
            EngineAction::StartDownload { tab, url } => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.start_download(tab, url);
                }
            }
            EngineAction::OpenPath(path) => {
                if let Err(e) = open_path(&path) {
                    self.report_error(format!("cannot open {}: {e}", path.display()), event_loop);
                }
            }
            EngineAction::RevealPath(path) => {
                if let Err(e) = reveal_path(&path) {
                    self.report_error(format!("cannot show {}: {e}", path.display()), event_loop);
                }
            }
            EngineAction::OpenDownloadsFolder => {
                let dir = self.download_policy().directory;
                let _ = std::fs::create_dir_all(&dir);
                if let Err(e) = open_path(&dir) {
                    self.report_error(format!("cannot open {}: {e}", dir.display()), event_loop);
                }
            }
            EngineAction::SaveDownloads => {
                self.save_downloads();
                self.redraw_all();
            }
            EngineAction::PickFolder => {
                let dialog = rfd::FileDialog::new().set_title("Downloads folder");
                let dialog = match &self.browser.settings.downloads.directory {
                    Some(dir) => dialog.set_directory(dir),
                    None => dialog,
                };
                if let Some(dir) = dialog.pick_folder() {
                    self.run_commands(vec![Command::SetDownloadsDirectory(Some(dir))], event_loop);
                }
            }
            EngineAction::SetFullscreen { window, fullscreen } => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.set_fullscreen(fullscreen);
                }
            }
            EngineAction::SaveSettings => {
                self.save_settings();
                // The profile's name may have changed, and the title shows it.
                let title = self.window_title();
                for w in self.windows.values() {
                    w.set_title(&title);
                }
                self.redraw_all();
            }
            EngineAction::SaveBookmarks => {
                let path = self.browser.bookmarks_path();
                if let Err(e) = self.browser.bookmarks.save(&path) {
                    log::error!("cannot save bookmarks to {}: {e}", path.display());
                }
                self.favicons
                    .preload(bookmarked_hosts(&self.browser.bookmarks));
                self.redraw_all();
            }
            EngineAction::PickFile(which) => self.pick_bookmarks_file(which, event_loop),
            EngineAction::ExportBookmarks(path) => {
                let html = bookmarks_html::export(&self.browser.bookmarks);
                if let Err(e) = std::fs::write(&path, html) {
                    self.report_error(format!("cannot write {}: {e}", path.display()), event_loop);
                } else {
                    log::info!("bookmarks exported to {}", path.display());
                }
            }
            EngineAction::SwitchBookmarks { shared } => {
                if shared {
                    self.reload_bookmarks(event_loop);
                } else {
                    // What was shared stays, as this profile's own copy.
                    let path = self.browser.profile.bookmarks_path();
                    if let Err(e) = self.browser.bookmarks.save(&path) {
                        log::error!("cannot save bookmarks to {}: {e}", path.display());
                    }
                }
                self.redraw_all();
            }
            EngineAction::CopyExtensionsFromDefault => {
                self.copy_extensions_from_default(event_loop);
            }
            EngineAction::CreateProfile(form) => {
                let retired: Vec<String> = self.shared.retired.borrow().iter().cloned().collect();
                match profiles::create(&self.browser.profile, self.brand, &form, &retired) {
                    Ok(profile) => {
                        log::info!(
                            "created profile {:?} at {}",
                            profile.name,
                            profile.dir.display()
                        );
                        if let Err(e) = self.open_profile(&profile.name, &[]) {
                            self.report_error(
                                format!("cannot start a browser for the new profile: {e}"),
                                event_loop,
                            );
                        }
                    }
                    Err(e) => {
                        self.report_error(format!("cannot create the profile: {e}"), event_loop)
                    }
                }
                self.refresh_profiles();
                self.requests.push(Request::ProfilesChanged);
            }
            EngineAction::DeleteProfile(profile) => {
                if profile == self.browser.profile.name {
                    // This browser's own: hand over to the default profile
                    // (in one process, only when no other profile is
                    // open) and take the directory away once the engine
                    // is gone.
                    if self.engine_info.profiles_in_process {
                        self.requests.push(Request::HandOver);
                    } else if let Err(e) =
                        crate::process::open_in_profile(DEFAULT_PROFILE_NAME, &[])
                    {
                        log::error!("cannot start a browser for the default profile: {e}");
                    }
                    self.delete_on_exit = Some(self.browser.profile.clone());
                    self.perform(EngineAction::Quit, event_loop);
                    return;
                }
                let name = self
                    .browser
                    .profiles
                    .iter()
                    .find(|p| p.dir == profile)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| profile.clone());
                match self.browser.profile.sibling(&profile) {
                    Ok(other)
                        if self.shared.open.borrow().contains(&other.name)
                            || profiles::in_use(&other.pid_path()) =>
                    {
                        self.report_error(
                            format!(
                                "\u{201c}{name}\u{201d} is open in another window; close it first"
                            ),
                            event_loop,
                        )
                    }
                    Ok(other) => {
                        if let Err(e) = profiles::delete(&other) {
                            self.report_error(
                                format!("cannot delete \u{201c}{name}\u{201d}: {e}"),
                                event_loop,
                            );
                        } else {
                            log::info!(
                                "deleted profile {:?} at {}",
                                other.name,
                                other.dir.display()
                            );
                        }
                    }
                    Err(e) => {
                        self.report_error(format!("cannot delete the profile: {e}"), event_loop)
                    }
                }
                self.refresh_profiles();
                self.requests.push(Request::ProfilesChanged);
            }
            EngineAction::SaveHistory => {
                self.history_dirty.get_or_insert_with(Instant::now);
            }
            EngineAction::ShowPrompt { window, prompt } => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.open_prompt(prompt);
                }
            }
            EngineAction::ShowLauncher { window } => {
                // The most visited sites' icons, read from disk the way
                // the bookmarked sites' are.
                let hosts: Vec<String> = osmena_core::launch::top_sites(
                    &self.browser.history,
                    &self.browser.settings.launch.hidden_sites,
                    osmena_core::launch::TOP_SITES,
                )
                .into_iter()
                .map(|s| s.host)
                .collect();
                self.favicons.preload(hosts.into_iter());
                if let Some(w) = self.window_for_core_id(window) {
                    w.open_launcher();
                }
            }
            EngineAction::LookUpPlace(query) => self.launch.look_up_place(query),
            EngineAction::FetchLocation { units } => self.launch.fetch_location(units),
            EngineAction::FetchWeather { place, units } => self.launch.fetch_weather(place, units),
            EngineAction::BeginGoogleSignIn { window } => match self.launch.begin_sign_in() {
                Ok(url) => self.run_commands(
                    vec![Command::NewTab {
                        window,
                        url: Some(url),
                    }],
                    event_loop,
                ),
                Err(e) => {
                    log::warn!("Google sign-in: {e}");
                    self.launch_reported(osmena_core::launch::LaunchUpdate::GoogleConnected(Err(
                        e,
                    )));
                }
            },
            EngineAction::ForgetGoogle => self.launch.forget(),
            EngineAction::FetchDriveRecent => self.launch.fetch_drive_recent(),
            EngineAction::OpenExtensionAction { window, extension } => {
                let tab = self.browser.window(window).map(|w| w.active);
                if let (Some(tab), Some(w)) = (tab, self.window_for_core_id(window)) {
                    w.open_extension_action(tab, &extension);
                }
            }
            EngineAction::CloseView { window, view } => {
                let active = self.browser.window(window).map(|w| w.active);
                if let Some(w) = self.window_for_core_id(window) {
                    w.close_view(view, active);
                }
            }
            EngineAction::SetExtensionEnabled { extension, enabled } => {
                engine.set_extension_enabled(&extension, enabled);
            }
            EngineAction::UninstallExtension(extension) => engine.uninstall_extension(&extension),
            EngineAction::InstallExtension(source) => engine.install_extension(source),
            EngineAction::CopyText(text) => clipboard::set_text(&text),
            EngineAction::Page { tab, command } => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().page_command(tab, command);
                }
            }
            EngineAction::ContextMenuClosed(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().context_menu_closed(tab);
                }
            }
            EngineAction::AskMediaAccess {
                window,
                tab,
                request,
            } => {
                // The system's dialog, as a sheet on the tab's window.
                // It is answered before this returns, so the request
                // never waits across a change of focus.
                let parent = self.window_for_core_id(window).map(|w| w.native());
                let allow = media_prompt::ask(parent, &request);
                self.run_commands(
                    vec![Command::AnswerMediaAccess {
                        tab,
                        request,
                        allow,
                    }],
                    event_loop,
                );
            }
            EngineAction::AnswerMediaAccess {
                tab,
                request,
                allow,
            } => {
                log::info!(
                    "media request {request} of tab {}: {}",
                    tab.0,
                    if allow { "allowed" } else { "refused" }
                );
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().answer_media_access(tab, request, allow);
                }
            }
            EngineAction::OpenSettings => match &self.settings_window {
                Some(sw) => sw.focus(),
                None => match SettingsWindow::open(event_loop, engine, self.brand) {
                    Ok(sw) => self.settings_window = Some(sw),
                    Err(e) => log::error!("cannot open settings window: {e}"),
                },
            },
            EngineAction::MakeDefaultBrowser => {
                let proxy = self.default_browser_proxy.clone();
                default_browser::make_default(move || {
                    let proxy = proxy.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Err(e) = proxy.send_event(AppEvent::DefaultBrowserAnswered) {
                        log::warn!("failed to report the default-browser answer: {e}");
                    }
                });
                self.redraw_all();
            }
            EngineAction::CheckForUpdates => match self.shared.updater.as_ref() {
                Some(updater) => {
                    updater.check();
                    self.redraw_all();
                }
                None => {
                    // Asked by hand on a build that cannot: say why.
                    self.update_reported(UpdateStatus::Failed(format!(
                        "{} cannot update itself",
                        self.brand.describe_build()
                    )));
                }
            },
            EngineAction::InstallUpdate { update, file } => {
                // The new build comes back with what is open now: every
                // open profile when they share this process (the shell
                // writes the list on the way out), this one otherwise.
                let relaunch: Vec<String> = if self.engine_info.profiles_in_process {
                    vec!["--restore".to_string()]
                } else {
                    vec![
                        "--profile".to_string(),
                        self.browser.profile.name.clone(),
                        "--restore".to_string(),
                    ]
                };
                let outcome = match self.shared.updater.as_ref() {
                    Some(updater) => updater.install(&update, &file, &relaunch),
                    None => Err("this build cannot install updates".to_string()),
                };
                match outcome {
                    Ok(()) => {
                        log::info!("quitting to install {}", update.id);
                        self.requests.push(Request::QuitAll { force: true });
                        self.perform(EngineAction::Quit, event_loop);
                    }
                    Err(e) => {
                        self.browser.update = UpdateStatus::Idle;
                        self.update_reported(UpdateStatus::Failed(e));
                    }
                }
            }
            EngineAction::OpenGuestProfile => {
                let retired: Vec<String> = self.shared.retired.borrow().iter().cloned().collect();
                match profiles::create_guest(&self.browser.profile, self.brand, &retired) {
                    Ok(guest) => {
                        log::info!(
                            "made guest profile {:?} at {}",
                            guest.name,
                            guest.dir.display()
                        );
                        if let Err(e) = self.open_profile(&guest.name, &[]) {
                            // Nothing will claim it: take it back now.
                            if let Err(e) = profiles::delete(&guest) {
                                log::warn!("cannot remove the unused guest profile: {e}");
                            }
                            self.report_error(
                                format!("cannot start a browser for the guest profile: {e}"),
                                event_loop,
                            );
                        }
                    }
                    Err(e) => {
                        self.report_error(format!("cannot create a guest profile: {e}"), event_loop)
                    }
                }
            }
            EngineAction::OpenInProfile { profile, urls } => {
                if let Err(e) = self.open_profile(&profile, &urls) {
                    let urls: Vec<&str> = urls.iter().map(Url::as_str).collect();
                    let message = format!(
                        "cannot start a browser for profile {profile:?} with {}: {e}",
                        urls.join(" ")
                    );
                    log::error!("{message}");
                    let actions = apply(&mut self.browser, &EngineEvent::EngineError(message));
                    self.perform_all(actions, event_loop);
                }
            }
            EngineAction::ExtensionCommand {
                window,
                extension,
                name,
            } => {
                // An extension's shortcut is delivered with the window's
                // active tab, like a click on its action.
                let tab = self.browser.window(window).map(|w| w.active);
                if let (Some(tab), Some(w)) = (tab, self.window_for_core_id(window)) {
                    w.host_mut().extension_command(tab, &extension, &name);
                }
            }
            EngineAction::ExtensionReply { token, reply } => {
                engine.extension_reply(token, reply);
            }
            EngineAction::CreateView {
                window,
                view,
                extension,
                page,
            } => {
                // The view starts hidden; the window shows it in the
                // first frame that draws its panel.
                if let Some(w) = self.window_for_core_id(window) {
                    w.host_mut().create_view(view, &extension, &page);
                    w.request_redraw();
                }
            }
            EngineAction::Quit => {
                // This profile's part of it: the shell takes the profile
                // away after this turn, and ends the process when it was
                // the last one open.
                self.save_session_for_exit(None);
                self.exiting = true;
                self.close_settings_window();
                self.windows.clear();
            }
        }
    }

    /// Spin the engine and fold its events into the model.
    pub(crate) fn pump(&mut self, event_loop: &ActiveEventLoop) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        engine.spin();
        for event in engine.drain_events() {
            // A side panel's view that closed or crashed is gone from the
            // engine, so no window hosts it, and `apply` drops its panel:
            // the window is found through the model first, before that.
            if let Some(id) = window_losing_panel(&self.browser, &event)
                && let Some(w) = self.window_for_core_id(id)
            {
                w.request_redraw();
            }
            if let Some(tab) = tab_of(&event)
                && let Some(w) = self.window_for_tab(tab)
            {
                w.request_redraw();
            }
            // Commands the user picked from a page's context menu, run
            // after the event itself is applied.
            let mut menu_commands = Vec::new();
            match &event {
                EngineEvent::EngineError(message) => {
                    log::error!("engine error: {message}");
                    self.redraw_all();
                }
                EngineEvent::ExtensionsChanged(extensions) => {
                    // The snapshot another profile copies from.
                    let path = self.browser.profile.extensions_path();
                    if let Err(e) = InstalledExtensions::of(extensions).save(&path) {
                        log::warn!("cannot write {}: {e}", path.display());
                    }
                    self.redraw_all();
                }
                EngineEvent::CursorChanged { tab, cursor } => {
                    let browser = &self.browser;
                    if let Some(w) = self.windows.values_mut().find(|w| w.hosts_tab(*tab)) {
                        w.set_page_cursor(*tab, browser, *cursor);
                    }
                }
                EngineEvent::ContextMenuRequested {
                    tab,
                    position,
                    context,
                } => {
                    let (browser, keymap) = (&self.browser, &self.keymap);
                    if let Some(w) = self.windows.values_mut().find(|w| w.hosts_tab(*tab)) {
                        menu_commands = w.show_page_menu(browser, keymap, *tab, *position, context);
                    }
                }
                EngineEvent::ClipboardImage(image) => clipboard::set_image(image),
                // An extension changed the strip, a folder or a panel, in
                // whichever window: all of them show it.
                EngineEvent::ExtensionRequest { .. } => self.redraw_all(),
                _ => {}
            }
            let actions = apply(&mut self.browser, &event);
            if matches!(
                event,
                EngineEvent::UrlChanged { .. }
                    | EngineEvent::TitleChanged { .. }
                    | EngineEvent::TabClosed(_)
                    | EngineEvent::NavigationBlocked { .. }
                    | EngineEvent::NewTabRequested { .. }
                    | EngineEvent::ExtensionRequest { .. }
            ) {
                self.session_changed();
            }
            if let EngineEvent::FaviconChanged { tab, favicon } = &event
                && let Some(host) = self
                    .browser
                    .tab(*tab)
                    .and_then(|t| Bookmarks::site_key(&t.url))
            {
                self.favicons.remember(&host, favicon);
            }
            // What a warm waits for.
            match &event {
                EngineEvent::FaviconChanged { tab, .. } => {
                    if let Some(warm) = self.warms.get_mut(tab) {
                        warm.favicon = true;
                    }
                }
                EngineEvent::LoadStateChanged {
                    tab,
                    state: LoadState::Complete,
                } => {
                    if let Some(warm) = self.warms.get_mut(tab) {
                        warm.loaded.get_or_insert_with(Instant::now);
                    }
                }
                _ => {}
            }
            if matches!(event, EngineEvent::ExtensionsChanged(_))
                && self.browser.settings.profile.copy_extensions
            {
                // A profile made with "copy extensions": the engine is up
                // and has said what is installed, so the rest can be.
                self.browser.settings.profile.copy_extensions = false;
                self.save_settings();
                self.copy_extensions_from_default(event_loop);
            }
            if let EngineEvent::NewTabRequested { pending, .. } = &event
                && actions.is_empty()
                && let Some(engine) = self.engine.as_ref()
            {
                // The opener tab is gone from the model; nothing will adopt
                // the new tab, so let the engine forget it.
                engine.discard_pending(*pending);
            }
            self.perform_all(actions, event_loop);
            if self.exiting {
                return;
            }
            self.run_commands(menu_commands, event_loop);
            if self.exiting {
                return;
            }
        }
    }

    /// Every window shows something that changed outside it.
    pub(crate) fn redraw_all(&self) {
        for w in self.windows.values() {
            w.request_redraw();
        }
        if let Some(sw) = &self.settings_window {
            sw.request_redraw();
        }
    }

    fn save_settings(&self) {
        let path = self.browser.profile.settings_path();
        if let Err(e) = self.browser.settings.save(&path) {
            log::error!("cannot save settings to {}: {e}", path.display());
        }
    }

    /// Write the history if it changed, now.
    fn save_history(&mut self) {
        if self.history_dirty.take().is_none() {
            return;
        }
        let path = self.browser.profile.history_path();
        if let Err(e) = self.browser.history.save(&path) {
            log::error!("cannot save history to {}: {e}", path.display());
        }
    }

    fn save_keymap(&self) {
        let path = self.browser.profile.keymap_path();
        if let Err(e) = self.keymap.save(&path) {
            log::error!("cannot save keymap to {}: {e}", path.display());
        }
    }

    /// Settings changed in the settings window: persist them and apply
    /// what the open windows show.
    /// Where downloads go: the setting, else the system's Downloads
    /// folder, else a folder in the profile.
    fn download_policy(&self) -> DownloadPolicy {
        let settings = &self.browser.settings.downloads;
        let directory = settings
            .directory
            .clone()
            .or_else(osmena_core::system_downloads_dir)
            .unwrap_or_else(|| {
                self.browser
                    .profile
                    .downloads_path()
                    .parent()
                    .map(|p| p.join("downloads"))
                    .unwrap_or_else(std::env::temp_dir)
            });
        DownloadPolicy {
            directory,
            ask: settings.ask_where_to_save,
        }
    }

    fn save_downloads(&self) {
        let path = self.browser.profile.downloads_path();
        if let Err(e) = self.browser.downloads.save(&path) {
            log::error!("cannot save the downloads list to {}: {e}", path.display());
        }
    }

    fn settings_changed(&mut self) {
        self.save_settings();
        if self.engine_info.downloads
            && let Some(engine) = &self.engine
        {
            engine.configure_downloads(self.download_policy());
        }
        // A new default zoom reaches the pages without a level of their
        // own.
        let zoomed = osmena_core::command::apply_default_zoom(&mut self.browser);
        for action in zoomed {
            if let EngineAction::SetZoom { tab, percent } = action
                && let Some(w) = self.window_for_tab(tab)
            {
                w.set_zoom(tab, percent);
            }
        }
        let frameless = !self.browser.settings.system_title_bar;
        for w in self.windows.values_mut() {
            w.set_frameless(frameless);
            w.request_redraw();
        }
        // Automatic checks switched on: the first one runs soon; switched
        // off: the booked one is dropped.
        if self.checks_automatically() {
            self.schedule_update_check(update::FIRST_CHECK_DELAY);
        } else {
            self.update_check_due = None;
        }
    }

    /// Drop the settings window, saving an uncommitted edit first.
    fn close_settings_window(&mut self) {
        if self
            .settings_window
            .as_ref()
            .is_some_and(|sw| sw.needs_save())
        {
            self.save_settings();
        }
        self.settings_window = None;
    }

    /// A keymap chord pressed while the settings window had focus.
    fn settings_shortcut(&mut self, kind: CommandKind, event_loop: &ActiveEventLoop) {
        match kind {
            CommandKind::Quit => self.run_commands(vec![Command::Quit], event_loop),
            CommandKind::NewWindow => self.run_commands(vec![Command::NewWindow], event_loop),
            CommandKind::CloseWindow | CommandKind::CloseTab => self.close_settings_window(),
            CommandKind::OpenSettings => {
                if let Some(sw) = &self.settings_window {
                    sw.focus();
                }
            }
            // Tab and page commands need a browser window; ignore them here.
            _ => {}
        }
    }
}

/// The egui events an editing command stands for in a text field, or
/// `None` for a command that is the page's only.
fn edit_events(command: &PageCommand) -> Option<Vec<egui::Event>> {
    use egui::{Event, Key, Modifiers};
    let key = |key: Key, modifiers: Modifiers| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    Some(match command {
        PageCommand::Undo => vec![key(Key::Z, Modifiers::COMMAND)],
        PageCommand::Redo => vec![key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT)],
        PageCommand::Cut => vec![Event::Cut],
        PageCommand::Copy => vec![Event::Copy],
        PageCommand::Paste | PageCommand::PasteAsPlainText => clipboard::get_text()
            .map(Event::Paste)
            .into_iter()
            .collect(),
        PageCommand::Delete => vec![key(Key::Delete, Modifiers::NONE)],
        PageCommand::SelectAll => vec![key(Key::A, Modifiers::COMMAND)],
        _ => return None,
    })
}

/// The hosts of every bookmarked page, whose favicons the bar shows.
fn bookmarked_hosts(bookmarks: &Bookmarks) -> impl Iterator<Item = String> + '_ {
    bookmarks.all().filter_map(|b| Bookmarks::site_key(&b.url))
}

/// Open a file (or folder) with the system's default application.
fn open_path(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("explorer");
        c.arg(path);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    command.spawn().map(|_| ())
}

/// Show a file selected in the system's file manager.
fn reveal_path(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("explorer");
        c.arg(format!("/select,{}", path.display()));
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(path);
        c
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path.parent().unwrap_or(path));
        c
    };
    command.spawn().map(|_| ())
}

fn tab_of(event: &EngineEvent) -> Option<TabId> {
    match event {
        EngineEvent::UrlChanged { tab, .. }
        | EngineEvent::TitleChanged { tab, .. }
        | EngineEvent::LoadStateChanged { tab, .. }
        | EngineEvent::HistoryChanged { tab, .. }
        | EngineEvent::StatusChanged { tab, .. }
        | EngineEvent::FaviconChanged { tab, .. }
        | EngineEvent::FindResult { tab, .. }
        | EngineEvent::TabCrashed { tab, .. }
        | EngineEvent::NavigationBlocked { tab, .. }
        | EngineEvent::MediaAccessRequested { tab, .. }
        | EngineEvent::MediaCaptureChanged { tab, .. }
        | EngineEvent::FrameReady(tab)
        | EngineEvent::TabClosed(tab) => Some(*tab),
        EngineEvent::NewTabRequested { opener, .. }
        | EngineEvent::NewWindowRequested { opener, .. } => Some(*opener),
        EngineEvent::ContextMenuRequested { tab, .. } => Some(*tab),
        // The cursor and the clipboard are handled directly; no redraw.
        // An extension's request names no tab; `pump` redraws for it.
        EngineEvent::CursorChanged { .. }
        | EngineEvent::ClipboardImage(_)
        | EngineEvent::EngineError(_)
        | EngineEvent::ExtensionsChanged(_)
        | EngineEvent::DownloadUpdated { .. }
        | EngineEvent::ExtensionRequest { .. } => None,
    }
}

impl<E: Engine> ProfileApp<E> {
    /// An event of one of this profile's windows (`owns_window`).
    pub(crate) fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if self.exiting {
            return;
        }
        if self
            .settings_window
            .as_ref()
            .is_some_and(|sw| sw.winit_id() == window_id)
        {
            let sw = self.settings_window.as_mut().expect("checked above");
            let outcome = sw.handle_event(
                &event,
                &mut self.browser.settings,
                &mut self.keymap,
                &self.browser.extensions,
                self.engine_info.extensions.as_ref(),
                &self.browser.blocked,
                &self.browser.update,
                self.brand.can_update(),
                self.engine_info.downloads,
                self.browser.default_browser,
            );
            let commands = sw.take_commands();
            match outcome {
                SettingsOutcome::Nothing => {}
                SettingsOutcome::Changed { settings, keymap } => {
                    if settings {
                        self.settings_changed();
                    }
                    if keymap {
                        self.save_keymap();
                        for w in self.windows.values() {
                            w.request_redraw();
                        }
                    }
                }
                SettingsOutcome::Close { save } => {
                    if save {
                        self.save_settings();
                    }
                    self.settings_window = None;
                }
                SettingsOutcome::Shortcut(kind) => self.settings_shortcut(kind, event_loop),
            }
            self.run_commands(commands, event_loop);
        } else if self.windows.contains_key(&window_id) {
            if matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_)) {
                self.session_changed();
            }
            if let WindowEvent::Focused(focused) = &event {
                // The model's focused window: where an extension's
                // request that names no window lands, and the window
                // the strip flags. A window that loses the focus to
                // something that is no browser window (the settings
                // window, another application) leaves none focused.
                let id = self.windows[&window_id].core_id();
                if *focused {
                    note_focus(&mut self.focus_order, window_id);
                    self.browser.focused = Some(id);
                } else if self.browser.focused == Some(id) {
                    self.browser.focused = None;
                }
            }
            if matches!(event, WindowEvent::Focused(true)) {
                // Another browser may have made or deleted a profile, or
                // changed the shared bookmarks, meanwhile; the user may
                // have chosen another default browser.
                self.refresh_profiles();
                self.refresh_default_browser();
                if self.bookmarks_shared() {
                    self.reload_bookmarks(event_loop);
                }
            }
            let window = self
                .windows
                .get_mut(&window_id)
                .expect("checked by contains_key");
            let commands =
                window.handle_event(&self.browser, &self.keymap, self.favicons.icons(), &event);
            let drop = window.take_tab_drop();
            self.run_commands(commands, event_loop);
            if let Some(drop) = drop
                && let Some(command) = self.tab_drop_command(window_id, drop)
            {
                self.run_commands(vec![command], event_loop);
            }
        }
    }

    /// Text the system typed with no key event of its own, for whatever
    /// of this profile's has the keyboard.
    pub(crate) fn typed(&mut self, text: String) {
        if let Some(sw) = self.settings_window.as_mut()
            && sw.has_focus()
        {
            sw.inject_egui_events(vec![egui::Event::Text(text)]);
        } else if let Some(w) = self.windows.values_mut().find(|w| w.has_focus()) {
            w.typed_text(&self.browser, text);
        }
    }

    /// Everything pending is handled: ask each window what egui wants,
    /// and say when this profile next wants a turn: the earliest
    /// delayed repaint (a caret blink, a tooltip) or timer of its own.
    /// A window whose repaint is due now has asked for its redraw by the
    /// time this returns.
    pub(crate) fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) -> Option<Instant> {
        if self.exiting {
            return None;
        }
        let mut next = None;
        // The history is written a moment after it last changed.
        if let Some(since) = self.history_dirty {
            if since.elapsed() >= HISTORY_SAVE_DELAY {
                self.save_history();
            } else {
                next = repaint::earliest(next, Some(since + HISTORY_SAVE_DELAY));
            }
        }
        // The session likewise.
        if let Some(since) = self.session_dirty {
            if since.elapsed() >= SESSION_SAVE_DELAY {
                self.save_session();
            } else {
                next = repaint::earliest(next, Some(since + SESSION_SAVE_DELAY));
            }
        }
        if let Some(due) = self.update_check_due {
            if due <= Instant::now() {
                self.automatic_update_check(event_loop);
            } else {
                next = repaint::earliest(next, Some(due));
            }
        }
        next = repaint::earliest(next, self.tick_warms(event_loop));
        if self.exiting {
            return None;
        }
        // Last, so the strip carries the turn's changes, a warm's completion
        // among them: the update check and the warms run commands above, and
        // nothing below changes the model.
        self.sync_strip();
        for w in self.windows.values_mut() {
            next = repaint::earliest(next, w.redraw_deadline());
        }
        if let Some(sw) = self.settings_window.as_mut() {
            next = repaint::earliest(next, sw.redraw_deadline());
        }
        next
    }

    /// This profile is over: write what it keeps, then tear down in
    /// dependency order, windows (tab hosts, GL) before the engine. Runs
    /// when its last window closed and on every exit path of the
    /// process. Returns the profile if it was deleted with it.
    pub(crate) fn shutdown(&mut self) -> Option<Profile> {
        self.exiting = true;
        self.save_history();
        // The windows as they were, for the next start (not for a
        // profile on its way out).
        self.save_session_for_exit(None);
        // What was still downloading is cut short with the engine.
        if self
            .browser
            .downloads
            .interrupt_active("The browser closed", osmena_core::history::unix_now())
        {
            self.save_downloads();
        }
        self.close_settings_window();
        self.windows.clear();
        self.engine = None;
        self.shared
            .open
            .borrow_mut()
            .remove(&self.browser.profile.name);
        profiles::release(&self.browser.profile.pid_path());
        let deleted = self.delete_on_exit.take();
        if let Some(profile) = &deleted {
            self.shared
                .retired
                .borrow_mut()
                .insert(profile.name.clone());
            match profiles::delete_at_exit(profile) {
                Ok(profiles::Deletion::Removed) => {
                    log::info!("deleted profile {:?}", profile.name)
                }
                Ok(profiles::Deletion::Deferred) => log::info!(
                    "profile {:?} is still held by a file; a helper removes it once this process has exited",
                    profile.name
                ),
                Err(e) => log::error!(
                    "cannot delete profile {:?} at {}: {e}",
                    profile.name,
                    profile.dir.display()
                ),
            }
        }
        deleted
    }
}
