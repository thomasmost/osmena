//! The winit application: the process's side of the browser. It owns
//! the event loop's handler, the open profiles (`crate::app`), and what
//! they share: the engine's start closure, the menu bar, the updater,
//! the lock that makes this the one browser process.
//!
//! With an engine that runs every profile in one process
//! (`EngineInfo::profiles_in_process`) several profiles are open here at
//! once, each with its own engine, model and windows, and the windows
//! of all of them are this application's. With any other engine exactly
//! one profile is ever open, and another profile is another process.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use osmena_chrome::fatal::report_fatal;
use osmena_chrome::{icon, native_menu};
use osmena_core::profile::DEFAULT_PROFILE_NAME;
use osmena_core::{Brand, Command, EngineInfo, Profile, UpdateStatus};
use osmena_engine::{Engine, Waker};
use url::Url;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

use crate::StartEngine;
use crate::app::{AppEvent, ProfileApp, Request, Shared, WinitWaker};
use crate::default_browser;
use crate::instance::{Primary, Relaunch};
use crate::load::{self, Opened};
use crate::profiles;
use crate::repaint;
use crate::update::Updater;

/// How many rounds of the open profiles' requests one turn carries out:
/// a request may raise another (a profile opened for a deleted one), but
/// never without end.
const REQUEST_ROUNDS: usize = 8;

/// What a start hands the shell beside the first profile.
pub(crate) struct Startup {
    /// The profiles to open beside the first one: those that were open
    /// when the browser last quit.
    pub others: Vec<String>,
    /// `--restore`: every profile opens its last windows.
    pub restore: bool,
    /// `--data-dir`.
    pub data_dir: Option<PathBuf>,
    /// `--exit-after`.
    pub exit_after: Option<Duration>,
    /// `--update-manifest`.
    pub update_manifest: Option<String>,
    /// This process is the one browser (`crate::instance`).
    pub primary: Option<Primary>,
}

/// The addresses to open a profile with.
enum Urls {
    Parsed(Vec<Url>),
    /// As they were typed on a command line: read with the profile's own
    /// search engine.
    Typed(Vec<String>),
}

pub(crate) struct Shell<E: Engine> {
    brand: &'static Brand,
    engine_info: &'static EngineInfo,
    // The profiles are declared before `start_engine` so they drop
    // first: every engine must be gone before what the closure holds for
    // all of them.
    profiles: Vec<ProfileApp<E>>,
    start_engine: Option<StartEngine<E>>,
    shared: Shared,
    waker: Arc<WinitWaker>,
    /// The profile the start named, opened on `resumed`; taken once.
    first: Option<Opened>,
    startup: Startup,
    /// The application's menu bar (macOS), rebuilt from the profile in
    /// front after every event when its entries change, and the profile
    /// its commands are for.
    menu_bar: native_menu::MenuBar,
    menu_profile: Option<String>,
    /// The profile whose window last had the keyboard: where what names
    /// no profile lands (a link from another application, a menu pick).
    active: Option<String>,
    /// Where the browser is with updates: every open profile shows the
    /// same.
    update: UpdateStatus,
    /// URLs the system handed over before a profile was there to open
    /// them; opened once one is.
    pending_urls: Vec<Url>,
    /// The first profile, to find the brand's directories by.
    home: Profile,
    /// A Quit wrote the list of open profiles this turn.
    quit_listed: bool,
    /// The profiles deleted in this run whose engine was running: an
    /// engine may write a profile's data once more as it lets go of it,
    /// so what is left goes again once every engine is gone.
    deleted: Vec<Profile>,
    exiting: bool,
    failed: bool,
}

impl<E: Engine> Shell<E> {
    pub(crate) fn new(
        brand: &'static Brand,
        engine_info: &'static EngineInfo,
        first: Opened,
        startup: Startup,
        start_engine: StartEngine<E>,
        event_loop: &EventLoop<AppEvent>,
    ) -> Shell<E> {
        let menu_proxy = Mutex::new(event_loop.create_proxy());
        native_menu::install_event_handler(move || {
            let proxy = menu_proxy.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(e) = proxy.send_event(AppEvent::Menu) {
                log::warn!("failed to report a menu pick to the event loop: {e}");
            }
        });
        let open_proxy = Mutex::new(event_loop.create_proxy());
        default_browser::install_open_handler(move |urls| {
            let proxy = open_proxy.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(e) = proxy.send_event(AppEvent::OpenUrls(urls)) {
                log::warn!("failed to hand the system's URLs to the event loop: {e}");
            }
        });
        let home = first.browser.profile.clone();
        profiles::sweep_guests(&home);
        profiles::sweep_engine_data(&home);
        let updater = Updater::new(
            brand,
            startup.update_manifest.as_deref(),
            home.updates_dir(),
            event_loop.create_proxy(),
        );
        match &updater {
            Some(updater) => log::info!("running {}", updater.current()),
            None => log::info!(
                "{}: no build number or no update source; updates are off",
                brand.describe_build()
            ),
        }
        Shell {
            brand,
            engine_info,
            profiles: Vec::new(),
            start_engine: Some(start_engine),
            shared: Shared {
                proxy: event_loop.create_proxy(),
                updater: Rc::new(updater),
                open: Rc::new(RefCell::new(BTreeSet::new())),
                retired: Rc::new(RefCell::new(BTreeSet::new())),
            },
            waker: Arc::new(WinitWaker(Mutex::new(event_loop.create_proxy()))),
            first: Some(first),
            startup,
            menu_bar: native_menu::MenuBar::default(),
            menu_profile: None,
            active: None,
            update: UpdateStatus::Idle,
            pending_urls: Vec::new(),
            home,
            quit_listed: false,
            deleted: Vec::new(),
            exiting: false,
            failed: false,
        }
    }

    /// False if the engine or the first window could not be started.
    /// Either way the app exits at once, and the process must report
    /// failure.
    pub(crate) fn started_cleanly(&self) -> bool {
        !self.failed
    }

    fn index_of(&self, profile: &str) -> Option<usize> {
        self.profiles.iter().position(|p| p.name() == profile)
    }

    /// The profile what names none is for: the one with the keyboard,
    /// else the one that last had it, else any.
    fn front(&self) -> Option<usize> {
        self.profiles
            .iter()
            .position(|p| p.has_focus() && !p.is_exiting())
            .or_else(|| {
                self.active
                    .as_deref()
                    .and_then(|name| self.index_of(name))
                    .filter(|i| !self.profiles[*i].is_exiting())
            })
            .or_else(|| self.profiles.iter().position(|p| !p.is_exiting()))
    }

    /// Start an engine for the profile `opened` holds and put its first
    /// windows on screen.
    fn start_profile(
        &mut self,
        opened: Opened,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), String> {
        let Some(start_engine) = self.start_engine.as_mut() else {
            return Err("the browser is closing".to_string());
        };
        let waker: Arc<dyn Waker> = self.waker.clone();
        let engine = start_engine(&opened.browser.profile, waker).map_err(|e| e.to_string())?;
        let mut app = ProfileApp::new(
            self.brand,
            self.engine_info,
            opened,
            engine,
            self.shared.clone(),
        );
        app.update_reported(self.update.clone());
        let name = app.name().to_string();
        self.shared.open.borrow_mut().insert(name.clone());
        self.profiles.push(app);
        self.active = Some(name);
        let app = self.profiles.last_mut().expect("just pushed");
        app.start(event_loop);
        Ok(())
    }

    /// Open the profile `name` in this process with `urls`: its last
    /// windows when its settings (or `restore`) say so. A profile that
    /// is open already gets the URLs as tabs and comes to the front.
    fn open_profile(
        &mut self,
        name: &str,
        urls: Urls,
        restore: bool,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), String> {
        if let Some(index) = self.index_of(name) {
            let app = &mut self.profiles[index];
            if app.is_exiting() {
                return Err("it is closing; try again in a moment".to_string());
            }
            let urls = match urls {
                Urls::Parsed(urls) => urls,
                Urls::Typed(typed) => app.typed_urls(&typed),
            };
            if !urls.is_empty() {
                app.open_urls(urls, event_loop);
            }
            app.bring_to_front();
            return Ok(());
        }
        let profile = load::resolve(
            self.brand,
            self.engine_info,
            self.startup.data_dir.as_deref(),
            name,
        )?;
        if profiles::in_use(&profile.pid_path()) {
            return Err(format!(
                "profile {name:?} is open in another {} process; quit that one first",
                self.brand.name
            ));
        }
        let loaded = load::load(self.brand, profile)?;
        let urls = match urls {
            Urls::Parsed(urls) => urls,
            Urls::Typed(typed) => loaded.urls(&typed)?,
        };
        self.start_profile(loaded.open(urls, restore), event_loop)
    }

    /// Another start of the browser handed over its command line.
    fn relaunched(&mut self, relaunch: Relaunch, event_loop: &ActiveEventLoop) {
        log::info!("another start handed over: {relaunch:?}");
        let name = relaunch
            .profile
            .clone()
            .or_else(|| self.front().map(|i| self.profiles[i].name().to_string()))
            .unwrap_or_else(|| DEFAULT_PROFILE_NAME.to_string());
        // Starting the browser again with nothing to open asks for a
        // window, as it does everywhere.
        if relaunch.urls.is_empty()
            && let Some(index) = self.index_of(&name)
            && !self.profiles[index].is_exiting()
        {
            let app = &mut self.profiles[index];
            app.run_commands(vec![Command::NewWindow], event_loop);
            app.bring_to_front();
            return;
        }
        if let Err(e) = self.open_profile(
            &name,
            Urls::Typed(relaunch.urls),
            relaunch.restore,
            event_loop,
        ) {
            self.report(
                None,
                format!("cannot open profile {name:?}: {e}"),
                event_loop,
            );
        }
    }

    /// Say `message` in the status area of the profile at `index`, or
    /// of the one in front.
    fn report(&mut self, index: Option<usize>, message: String, event_loop: &ActiveEventLoop) {
        match index.or_else(|| self.front()) {
            Some(index) => self.profiles[index].report_error(message, event_loop),
            None => log::error!("{message}"),
        }
    }

    /// The names of the open profiles that a later start should bring
    /// back: `first` (the profile at that index) ahead of the rest, or
    /// the one in front when none is named. No guests.
    fn open_names(&self, first: Option<usize>) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let ordered = first
            .or_else(|| self.front())
            .into_iter()
            .chain(0..self.profiles.len());
        for index in ordered {
            let app = &self.profiles[index];
            let name = app.name().to_string();
            if !app.profile().guest && !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Write down which profiles are open, for the next start. Only an
    /// engine that runs them together reads it.
    fn remember_open(&self, names: &[String]) {
        if self.engine_info.profiles_in_process && !names.is_empty() {
            profiles::write_open(&self.home, names);
        }
    }

    /// Carry out what the open profiles asked of the process, keep what
    /// they share the same in all of them, and take away the ones that
    /// are over. Called after every turn any of them had.
    fn after_turn(&mut self, event_loop: &ActiveEventLoop) {
        if self.exiting {
            return;
        }
        for _ in 0..REQUEST_ROUNDS {
            let requests: Vec<(usize, Request)> = self
                .profiles
                .iter_mut()
                .enumerate()
                .flat_map(|(i, app)| app.take_requests().into_iter().map(move |r| (i, r)))
                .collect();
            if requests.is_empty() {
                break;
            }
            for (from, request) in requests {
                self.carry_out(from, request, event_loop);
            }
        }
        // Where the browser is with updates is one thing: a profile that
        // moved it (a check asked for, a restart) moves it for all.
        if let Some(status) = self
            .profiles
            .iter()
            .map(ProfileApp::update_status)
            .find(|status| **status != self.update)
            .cloned()
        {
            self.update = status;
            for app in &mut self.profiles {
                if *app.update_status() != self.update {
                    app.set_update_status(self.update.clone());
                }
            }
        }
        self.reap(event_loop);
    }

    fn carry_out(&mut self, from: usize, request: Request, event_loop: &ActiveEventLoop) {
        match request {
            Request::OpenProfile { profile, urls } => {
                if let Err(e) = self.open_profile(&profile, Urls::Parsed(urls), false, event_loop) {
                    // A guest nothing will run is taken back now.
                    if let Ok(unused) = self.home.sibling(&profile)
                        && unused.guest
                        && let Err(e) = profiles::delete(&unused)
                    {
                        log::warn!("cannot remove the unused guest profile: {e}");
                    }
                    self.report(
                        Some(from),
                        format!("cannot open profile {profile:?}: {e}"),
                        event_loop,
                    );
                }
            }
            Request::HandOver => {
                if self.profiles.iter().all(ProfileApp::is_exiting)
                    && let Err(e) = self.open_profile(
                        DEFAULT_PROFILE_NAME,
                        Urls::Parsed(Vec::new()),
                        false,
                        event_loop,
                    )
                {
                    log::error!("cannot open the default profile: {e}");
                }
            }
            Request::QuitAll { force } => {
                // What is open now is what the next start brings back,
                // whatever each profile's own guard then does.
                self.remember_open(&self.open_names(Some(from)));
                self.quit_listed = true;
                for (index, app) in self.profiles.iter_mut().enumerate() {
                    if index != from {
                        app.quit_relayed(force, event_loop);
                    }
                }
            }
            Request::ProfilesChanged => {
                for (index, app) in self.profiles.iter_mut().enumerate() {
                    if index != from {
                        app.refresh_profiles();
                    }
                }
            }
        }
    }

    /// Take away the profiles whose windows are gone; with the last one
    /// goes the process.
    fn reap(&mut self, event_loop: &ActiveEventLoop) {
        if !self.profiles.iter().any(ProfileApp::is_exiting) {
            return;
        }
        let last = self.profiles.iter().all(ProfileApp::is_exiting);
        if last {
            // Closing the last window quits: the profile it was in is
            // the one to come back to. A Quit wrote the list already,
            // with every profile it found open.
            if !self.quit_listed {
                self.remember_open(&self.open_names(None));
            }
        } else {
            // Some profile outlived a Quit (its guard asked first): the
            // list is written again when the browser does go.
            self.quit_listed = false;
        }
        let mut index = 0;
        while index < self.profiles.len() {
            if self.profiles[index].is_exiting() {
                let mut app = self.profiles.remove(index);
                if app.window_failed() && self.profiles.is_empty() {
                    self.failed = true;
                }
                log::info!("closing profile {:?}", app.name());
                self.deleted.extend(app.shutdown());
            } else {
                index += 1;
            }
        }
        if self.profiles.is_empty() {
            self.exiting = true;
            // Nothing holds an engine any more: what they shared goes
            // now, inside the event loop, not when the process is torn
            // down around it.
            self.end_engines();
            event_loop.exit();
        }
    }

    /// Every profile is closed: drop what their engines shared, and take
    /// away what a deleted profile's engine wrote after the deletion.
    fn end_engines(&mut self) {
        self.start_engine = None;
        for profile in self.deleted.drain(..) {
            if let Err(e) = profiles::delete(&profile) {
                log::warn!(
                    "cannot remove what was left of profile {:?}: {e}",
                    profile.name
                );
            }
        }
    }

    /// Spin every engine and fold its events into its profile's model.
    fn pump(&mut self, event_loop: &ActiveEventLoop) {
        for app in &mut self.profiles {
            if !app.is_exiting() {
                app.pump(event_loop);
            }
        }
        self.after_turn(event_loop);
    }

    /// Rebuild the menu bar if the profile in front, or what it shows of
    /// it, changed.
    fn sync_menu_bar(&mut self) {
        let Some(index) = self.front() else {
            return;
        };
        let app = &self.profiles[index];
        self.menu_bar.sync(app.menu_bar_entries());
        self.menu_profile = Some(app.name().to_string());
    }

    /// Run the commands of the menu bar items picked since the last
    /// time, in the profile the bar was showing.
    fn menu_picks(&mut self, event_loop: &ActiveEventLoop) {
        for id in native_menu::take_menu_ids() {
            let Some(command) = self.menu_bar.command(&id) else {
                continue;
            };
            let index = self
                .menu_profile
                .as_deref()
                .and_then(|name| self.index_of(name))
                .or_else(|| self.front());
            let Some(index) = index else {
                continue;
            };
            self.profiles[index].menu_pick(command, event_loop);
            self.after_turn(event_loop);
            if self.exiting {
                return;
            }
        }
    }
}

impl<E: Engine> ApplicationHandler<AppEvent> for Shell<E> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(first) = self.first.take() else {
            return;
        };
        let (name, dir) = (
            first.browser.profile.name.clone(),
            first.browser.profile.dir.clone(),
        );
        if let Err(e) = self.start_profile(first, event_loop) {
            report_fatal(
                self.brand,
                &format!(
                    "{}: the browser engine failed to start for profile {name:?} \
                     (directory {}): {e}",
                    self.brand.binary,
                    dir.display()
                ),
            );
            self.failed = true;
            self.exiting = true;
            self.end_engines();
            event_loop.exit();
            return;
        }
        crate::process::bind_children_to_this_process();
        icon::set_dock_icon(self.brand);
        if let Some(delay) = self.startup.exit_after {
            let proxy = self.shared.proxy.clone();
            let _ = std::thread::Builder::new()
                .name("ExitAfter".to_string())
                .spawn(move || {
                    std::thread::sleep(delay);
                    if let Err(e) = proxy.send_event(AppEvent::ExitAfter(delay)) {
                        log::warn!("cannot ask the event loop to exit: {e}");
                    }
                });
        }
        if let Some(primary) = self.startup.primary.take() {
            let proxy = Mutex::new(self.shared.proxy.clone());
            primary.serve(move |relaunch| {
                let proxy = proxy.lock().unwrap_or_else(PoisonError::into_inner);
                if let Err(e) = proxy.send_event(AppEvent::Relaunch(relaunch)) {
                    log::warn!("failed to hand another start to the event loop: {e}");
                }
            });
        }
        // The other profiles that were open at the last quit, behind the
        // first: each with its own last windows, so only those that
        // bring them back (or all, with `--restore`).
        let restore = self.startup.restore;
        for other in std::mem::take(&mut self.startup.others) {
            let restores = restore
                || load::resolve(
                    self.brand,
                    self.engine_info,
                    self.startup.data_dir.as_deref(),
                    &other,
                )
                .ok()
                .and_then(|profile| {
                    osmena_core::Settings::load(&profile.settings_path(), self.brand).ok()
                })
                .is_some_and(|settings| settings.restore_windows_on_launch);
            if !restores {
                continue;
            }
            if let Err(e) = self.open_profile(&other, Urls::Parsed(Vec::new()), restore, event_loop)
            {
                log::error!("cannot open profile {other:?} again: {e}");
            }
        }
        // The first profile stays in front of the ones opened after it.
        if let Some(index) = self.index_of(&name) {
            self.active = Some(name);
            if self.profiles.len() > 1 {
                self.profiles[index].bring_to_front();
            }
        }
        let urls = std::mem::take(&mut self.pending_urls);
        if !urls.is_empty()
            && let Some(index) = self.front()
        {
            self.profiles[index].open_urls(urls, event_loop);
        }
        self.after_turn(event_loop);
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
        if let Some(index) = self.profiles.iter().position(|p| p.owns_window(window_id)) {
            if matches!(event, WindowEvent::Focused(true)) {
                self.active = Some(self.profiles[index].name().to_string());
            }
            self.profiles[index].window_event(event_loop, window_id, event);
        }
        self.pump(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        if self.exiting {
            return;
        }
        match event {
            AppEvent::Wake => self.pump(event_loop),
            AppEvent::Menu => {
                self.menu_picks(event_loop);
                self.pump(event_loop);
            }
            AppEvent::Update(status) => {
                for app in &mut self.profiles {
                    app.update_reported(status.clone());
                }
                if let Some(app) = self.profiles.first() {
                    self.update = app.update_status().clone();
                }
            }
            AppEvent::Launch(profile, update) => {
                if let Some(index) = self.index_of(&profile) {
                    self.profiles[index].launch_reported(update);
                }
            }
            AppEvent::Relaunch(relaunch) => {
                self.relaunched(relaunch, event_loop);
                self.pump(event_loop);
            }
            AppEvent::OpenUrls(urls) => {
                match self.front() {
                    Some(index) => self.profiles[index].open_urls(urls, event_loop),
                    None => self.pending_urls.extend(urls),
                }
                self.pump(event_loop);
            }
            AppEvent::DefaultBrowserAnswered => {
                for app in &mut self.profiles {
                    app.refresh_default_browser();
                    // An answer that left things as they were still
                    // needs the bar back in the settings window's terms.
                    app.redraw_all();
                }
            }
            AppEvent::Typed(text) => {
                if let Some(app) = self.profiles.iter_mut().find(|p| p.has_focus()) {
                    app.typed(text);
                }
                self.pump(event_loop);
            }
            AppEvent::ExitAfter(delay) => {
                // What a smoke test reads: did the pages load.
                let tabs: Vec<String> = self
                    .profiles
                    .iter()
                    .flat_map(ProfileApp::describe_tabs)
                    .collect();
                log::info!(
                    "--exit-after {:.1}s: quitting with {} tab(s): {}",
                    delay.as_secs_f64(),
                    tabs.len(),
                    tabs.join(", ")
                );
                if let Some(index) = self.front() {
                    self.profiles[index].run_commands(vec![Command::Quit], event_loop);
                }
                self.after_turn(event_loop);
            }
        }
    }

    /// Everything pending is handled: give every profile its timers, and
    /// sleep until the earliest of what they wait for (a delayed
    /// repaint, a file to write) or the next event, whichever comes
    /// first.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exiting {
            return;
        }
        // One profile runs the automatic update checks: the first open.
        for (index, app) in self.profiles.iter_mut().enumerate() {
            app.set_update_lead(index == 0);
        }
        if cfg!(target_os = "macos") {
            self.sync_menu_bar();
        }
        let mut next = None;
        for app in &mut self.profiles {
            next = repaint::earliest(next, app.about_to_wait(event_loop));
        }
        // A timer may have run a command (an update check, a warm).
        self.after_turn(event_loop);
        if self.exiting {
            return;
        }
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    /// Runs on every exit path, including macOS `terminate:` from the
    /// Dock menu, which otherwise ends the process without dropping the
    /// shell. Tear down in dependency order: each profile's windows
    /// before its engine, every engine before what they share.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if !self.profiles.is_empty() {
            // Still open: the system ended the application around them.
            self.remember_open(&self.open_names(None));
        }
        self.exiting = true;
        let closing: Vec<ProfileApp<E>> = self.profiles.drain(..).collect();
        for mut app in closing {
            self.deleted.extend(app.shutdown());
        }
        self.end_engines();
    }
}
