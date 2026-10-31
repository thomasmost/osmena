//! The winit application: owns the engine, the browser model, and the
//! native windows, and runs the dispatch loop (see the crate docs).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use osmena_chrome::fatal::report_fatal;
use osmena_chrome::{icon, menus, native_menu};
use osmena_core::profile::DEFAULT_PROFILE_NAME;
use osmena_core::{
    Bookmarks, BookmarksFile, Brand, Browser, Command, CommandKind, DownloadPolicy, EngineAction,
    EngineEvent, EngineInfo, ExtensionSource, InstalledExtensions, Keymap, PageCommand, Profile,
    Session, TabId, UpdateStatus, WindowGeometry, WindowId, apply, bookmarks_html, dispatch,
};
use osmena_engine::{Engine, TabHost, Waker};
use url::Url;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};

use crate::StartEngine;
use crate::clipboard;
use crate::favicons::FaviconCache;
use crate::launch::LaunchFetcher;
use crate::profiles;
use crate::repaint;
use crate::settings_window::{SettingsOutcome, SettingsWindow};
use crate::update::{self, Updater};
use crate::window::BrowserWindow;

/// How long after a history change the file is written, so a run of
/// page loads is saved once.
const HISTORY_SAVE_DELAY: Duration = Duration::from_secs(3);
/// How long after the windows or tabs last changed the session is written.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);

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
    /// A Launch Panel fetcher's answer (`crate::launch`).
    Launch(osmena_core::launch::LaunchUpdate),
}

struct WinitWaker(Mutex<EventLoopProxy<AppEvent>>);

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

pub struct App<E: Engine> {
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
    /// Builds the engine on `resumed`; taken once.
    start_engine: Option<StartEngine<E>>,
    waker: Arc<WinitWaker>,
    /// `--exit-after`: quit this long after the engine started.
    exit_after: Option<std::time::Duration>,
    exit_proxy: EventLoopProxy<AppEvent>,
    exiting: bool,
    engine_failed: bool,
    /// The first window could not be opened, so the app has nothing to show.
    window_failed: bool,
    /// When the history last changed without being saved since.
    history_dirty: Option<Instant>,
    /// The application's menu bar (macOS), rebuilt from the model after
    /// every event when its entries change.
    menu_bar: native_menu::MenuBar,
    /// The user deleted the profile this browser runs: its directory
    /// goes once the engine has let go of it, on the way out.
    delete_on_exit: Option<Profile>,
    /// Checks for and installs newer builds; `None` for a build that
    /// cannot update itself.
    updater: Option<Updater>,
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
    /// Where a window about to open goes (`EngineAction::PlaceWindow`
    /// arrives before its `OpenWindow`).
    pending_geometry: HashMap<WindowId, WindowGeometry>,
    /// The session was written for this exit already (before the
    /// windows went), so `exiting` leaves it alone.
    session_final: bool,
}

impl<E: Engine> App<E> {
    /// `initial_actions` are the actions from building the initial model
    /// (opening the first window and its tabs). They run once the event
    /// loop is active and the engine exists.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        brand: &'static Brand,
        engine_info: &'static EngineInfo,
        mut browser: Browser,
        keymap: Keymap,
        initial_actions: Vec<EngineAction>,
        start_engine: StartEngine<E>,
        event_loop: &EventLoop<AppEvent>,
        exit_after: Option<std::time::Duration>,
        update_manifest: Option<&str>,
    ) -> App<E> {
        let menu_proxy = Mutex::new(event_loop.create_proxy());
        native_menu::install_event_handler(move || {
            let proxy = menu_proxy.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(e) = proxy.send_event(AppEvent::Menu) {
                log::warn!("failed to report a menu pick to the event loop: {e}");
            }
        });
        browser.profiles = browser.profile.siblings(brand);
        profiles::claim(&browser.profile.pid_path());
        let mut favicons = FaviconCache::new(browser.profile.favicons_dir());
        favicons.preload(bookmarked_hosts(&browser.bookmarks));
        let launch = LaunchFetcher::new(
            brand,
            browser.profile.google_path(),
            event_loop.create_proxy(),
        );
        browser.launch.drive = launch.initial_drive_state();
        let updater = Updater::new(
            brand,
            update_manifest,
            browser.profile.updates_dir(),
            event_loop.create_proxy(),
        );
        match &updater {
            Some(updater) => log::info!("running {}", updater.current()),
            None => log::info!(
                "{}: no build number or no update source; updates are off",
                brand.describe_build()
            ),
        }
        App {
            brand,
            engine_info,
            browser,
            keymap,
            windows: HashMap::new(),
            settings_window: None,
            pending_actions: initial_actions,
            engine: None,
            start_engine: Some(start_engine),
            waker: Arc::new(WinitWaker(Mutex::new(event_loop.create_proxy()))),
            exit_after,
            exit_proxy: event_loop.create_proxy(),
            exiting: false,
            engine_failed: false,
            window_failed: false,
            history_dirty: None,
            menu_bar: native_menu::MenuBar::default(),
            delete_on_exit: None,
            updater,
            update_check_due: None,
            favicons,
            launch,
            session_dirty: None,
            pending_geometry: HashMap::new(),
            session_final: false,
        }
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
        self.updater.is_some() && self.browser.settings.check_for_updates
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
    fn update_reported(&mut self, status: UpdateStatus) {
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
    fn launch_reported(&mut self, update: osmena_core::launch::LaunchUpdate) {
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
    fn report_error(&mut self, message: String, event_loop: &ActiveEventLoop) {
        log::error!("{message}");
        let actions = apply(&mut self.browser, &EngineEvent::EngineError(message));
        self.perform_all(actions, event_loop);
    }

    /// List the other profiles again (one may have been made or deleted,
    /// here or in another browser).
    fn refresh_profiles(&mut self) {
        self.browser.profiles = self.browser.profile.siblings(self.brand);
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

    /// False if the engine or the first window could not be started. Either
    /// way the app exits at once, and the process must report failure.
    pub fn started_cleanly(&self) -> bool {
        !self.engine_failed && !self.window_failed
    }

    fn window_for_tab(&mut self, tab: TabId) -> Option<&mut BrowserWindow<E>> {
        self.windows.values_mut().find(|w| w.hosts_tab(tab))
    }

    fn window_for_core_id(&mut self, id: WindowId) -> Option<&mut BrowserWindow<E>> {
        self.windows.values_mut().find(|w| w.core_id() == id)
    }

    /// The command for tabs dragged out of `source` and dropped at `point`
    /// (physical screen pixels): move them to the other browser window
    /// under the pointer, to a new window when the pointer is outside every
    /// browser window, and nothing when it is still over the source window
    /// (a drop inside the strip's own window reorders the tabs there, and
    /// the chrome handles that itself).
    fn tab_drop_command(
        &self,
        source: winit::window::WindowId,
        tabs: Vec<TabId>,
        point: PhysicalPosition<f64>,
    ) -> Option<Command> {
        if let Some(target) = self
            .windows
            .iter()
            .find(|(id, w)| **id != source && w.contains_screen_point(point))
        {
            return Some(Command::MoveTabsToWindow {
                tabs,
                window: target.1.core_id(),
            });
        }
        if self
            .windows
            .get(&source)
            .is_some_and(|w| w.contains_screen_point(point))
        {
            return None;
        }
        Some(Command::MoveTabsToNewWindow(tabs))
    }

    fn run_commands(&mut self, commands: Vec<Command>, event_loop: &ActiveEventLoop) {
        if !commands.is_empty() {
            self.session_changed();
        }
        for command in commands {
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

    /// Rebuild the menu bar if the model it shows changed.
    fn sync_menu_bar(&mut self) {
        let window = self.menu_window().and_then(|id| self.browser.window(id));
        let profile_name = self.browser.profile_name();
        let updates = self.update_menu();
        let entries = menus::menu_bar(&menus::MenuBarInput {
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
            },
            profiles: &self.browser.profiles,
            updates,
        });
        self.menu_bar.sync(entries);
    }

    /// Run the commands of the menu bar items picked since the last
    /// time. An editing command goes to whatever has the keyboard: a
    /// text field of the chrome or of the settings window gets it as the
    /// egui events its key chord would have been, the page otherwise.
    fn menu_picks(&mut self, event_loop: &ActiveEventLoop) {
        for id in native_menu::take_menu_ids() {
            let Some(command) = self.menu_bar.command(&id) else {
                continue;
            };
            if let Command::Page { command: page, .. } = &command
                && let Some(events) = edit_events(page)
            {
                if let Some(sw) = self.settings_window.as_mut()
                    && sw.has_focus()
                {
                    sw.inject_egui_events(events);
                    continue;
                }
                if let Some(w) = self.windows.values_mut().find(|w| w.has_focus())
                    && w.egui_wants_keyboard()
                {
                    w.inject_egui_events(events);
                    continue;
                }
            }
            self.run_commands(vec![command], event_loop);
            if self.exiting {
                return;
            }
        }
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
                match profiles::create(&self.browser.profile, self.brand, &form) {
                    Ok(profile) => {
                        log::info!(
                            "created profile {:?} at {}",
                            profile.name,
                            profile.dir.display()
                        );
                        if let Err(e) = crate::process::open_in_profile(&profile.name, &[]) {
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
                self.redraw_all();
            }
            EngineAction::DeleteProfile(profile) => {
                if profile == self.browser.profile.name {
                    // This browser's own: hand over to the default profile
                    // and take the directory away once the engine is gone.
                    if let Err(e) = crate::process::open_in_profile(DEFAULT_PROFILE_NAME, &[]) {
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
                    Ok(other) if profiles::in_use(&other.pid_path()) => self.report_error(
                        format!("\u{201c}{name}\u{201d} is open in another window; close it first"),
                        event_loop,
                    ),
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
                self.redraw_all();
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
            EngineAction::OpenSettings => match &self.settings_window {
                Some(sw) => sw.focus(),
                None => match SettingsWindow::open(event_loop, engine, self.brand) {
                    Ok(sw) => self.settings_window = Some(sw),
                    Err(e) => log::error!("cannot open settings window: {e}"),
                },
            },
            EngineAction::CheckForUpdates => match &self.updater {
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
                let outcome = match &self.updater {
                    Some(updater) => updater.install(&update, &file, &self.browser.profile.name),
                    None => Err("this build cannot install updates".to_string()),
                };
                match outcome {
                    Ok(()) => {
                        log::info!("quitting to install {}", update.id);
                        self.perform(EngineAction::Quit, event_loop);
                    }
                    Err(e) => {
                        self.browser.update = UpdateStatus::Idle;
                        self.update_reported(UpdateStatus::Failed(e));
                    }
                }
            }
            EngineAction::OpenInProfile { profile, urls } => {
                if let Err(e) = crate::process::open_in_profile(&profile, &urls) {
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
            EngineAction::Quit => {
                self.save_session_for_exit(None);
                self.exiting = true;
                self.close_settings_window();
                self.windows.clear();
                event_loop.exit();
            }
        }
    }

    /// Spin the engine and fold its events into the model.
    fn pump(&mut self, event_loop: &ActiveEventLoop) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        engine.spin();
        for event in engine.drain_events() {
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
                _ => {}
            }
            let actions = apply(&mut self.browser, &event);
            if matches!(
                event,
                EngineEvent::UrlChanged { .. }
                    | EngineEvent::TitleChanged { .. }
                    | EngineEvent::TabClosed(_)
                    | EngineEvent::NewTabRequested { .. }
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
    fn redraw_all(&self) {
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
        | EngineEvent::FrameReady(tab)
        | EngineEvent::TabClosed(tab) => Some(*tab),
        EngineEvent::NewTabRequested { opener, .. }
        | EngineEvent::NewWindowRequested { opener, .. } => Some(*opener),
        EngineEvent::ContextMenuRequested { tab, .. } => Some(*tab),
        // The cursor and the clipboard are handled directly; no redraw.
        EngineEvent::CursorChanged { .. }
        | EngineEvent::ClipboardImage(_)
        | EngineEvent::EngineError(_)
        | EngineEvent::ExtensionsChanged(_)
        | EngineEvent::DownloadUpdated { .. } => None,
    }
}

impl<E: Engine> ApplicationHandler<AppEvent> for App<E> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.engine.is_none() && !self.exiting {
            let Some(start_engine) = self.start_engine.take() else {
                return;
            };
            let waker: Arc<dyn Waker> = self.waker.clone();
            match start_engine(&self.browser.profile.engine_dir, waker) {
                Ok(engine) => {
                    if self.engine_info.downloads {
                        engine.configure_downloads(self.download_policy());
                    }
                    self.engine = Some(engine);
                }
                Err(e) => {
                    report_fatal(
                        self.brand,
                        &format!(
                            "{}: the browser engine failed to start for profile {:?} \
                             (directory {}): {e}",
                            self.brand.binary,
                            self.browser.profile.name,
                            self.browser.profile.dir.display()
                        ),
                    );
                    self.engine_failed = true;
                    self.exiting = true;
                    event_loop.exit();
                    return;
                }
            }
            log::info!(
                "profile {:?} at {}",
                self.browser.profile.name,
                self.browser.profile.dir.display()
            );
            crate::process::bind_children_to_this_process();
            icon::set_dock_icon(self.brand);
            if let Some(delay) = self.exit_after {
                let proxy = self.exit_proxy.clone();
                let _ = std::thread::Builder::new()
                    .name("ExitAfter".to_string())
                    .spawn(move || {
                        std::thread::sleep(delay);
                        if let Err(e) = proxy.send_event(AppEvent::ExitAfter(delay)) {
                            log::warn!("cannot ask the event loop to exit: {e}");
                        }
                    });
            }
            let actions = std::mem::take(&mut self.pending_actions);
            self.perform_all(actions, event_loop);
            self.schedule_update_check(update::FIRST_CHECK_DELAY);
        }
    }

    fn window_event(
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
                &self.browser.update,
                self.brand.can_update(),
                self.engine_info.downloads,
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
            if matches!(event, WindowEvent::Focused(true)) {
                // Another browser may have made or deleted a profile, or
                // changed the shared bookmarks, meanwhile.
                self.refresh_profiles();
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
            if let Some((tabs, point)) = drop
                && let Some(command) = self.tab_drop_command(window_id, tabs, point)
            {
                self.run_commands(vec![command], event_loop);
            }
        }
        self.pump(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Wake => self.pump(event_loop),
            AppEvent::Menu => {
                self.menu_picks(event_loop);
                self.pump(event_loop);
            }
            AppEvent::Update(status) => self.update_reported(status),
            AppEvent::Launch(update) => self.launch_reported(update),
            AppEvent::ExitAfter(delay) => {
                // What a smoke test reads: did the pages load.
                let tabs: Vec<String> = self
                    .browser
                    .windows
                    .iter()
                    .flat_map(|w| w.tabs.iter())
                    .map(|t| format!("{:?} at {}", t.title, t.url))
                    .collect();
                log::info!(
                    "--exit-after {:.1}s: quitting with {} tab(s): {}",
                    delay.as_secs_f64(),
                    tabs.len(),
                    tabs.join(", ")
                );
                self.run_commands(vec![Command::Quit], event_loop);
            }
        }
    }

    /// Everything pending is handled: ask each window what egui wants,
    /// and sleep until the earliest delayed repaint (a caret blink, a
    /// tooltip) or the next event, whichever comes first. A window whose
    /// repaint is due now has asked for its redraw by the time this
    /// returns.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exiting {
            return;
        }
        if cfg!(target_os = "macos") {
            self.sync_menu_bar();
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
        for w in self.windows.values_mut() {
            next = repaint::earliest(next, w.redraw_deadline());
        }
        if let Some(sw) = self.settings_window.as_mut() {
            next = repaint::earliest(next, sw.redraw_deadline());
        }
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    /// Runs on every exit path, including macOS `terminate:` from the Dock
    /// menu, which otherwise ends the process without dropping `App`. Tear
    /// down in dependency order: windows (tab hosts, GL) before the engine.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
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
        profiles::release(&self.browser.profile.pid_path());
        if let Some(profile) = self.delete_on_exit.take() {
            match profiles::delete(&profile) {
                Ok(()) => log::info!("deleted profile {:?}", profile.name),
                Err(e) => log::error!(
                    "cannot delete profile {:?} at {}: {e}",
                    profile.name,
                    profile.dir.display()
                ),
            }
        }
    }
}
