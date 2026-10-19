//! The winit application: owns the engine, the browser model, and the
//! native windows, and runs the dispatch loop (see the crate docs).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use osmena_chrome::fatal::report_fatal;
use osmena_chrome::icon;
use osmena_core::{
    Brand, Browser, Command, CommandKind, EngineAction, EngineEvent, EngineInfo, Keymap, TabId,
    WindowId, apply, dispatch,
};
use osmena_engine::{Engine, TabHost, Waker};
use url::Url;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};

use crate::StartEngine;
use crate::settings_window::{SettingsOutcome, SettingsWindow};
use crate::window::BrowserWindow;

/// The only user event: the engine asked us to spin its message loop.
#[derive(Debug)]
pub enum AppEvent {
    Wake,
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
    exiting: bool,
    engine_failed: bool,
    /// The first window could not be opened, so the app has nothing to show.
    window_failed: bool,
}

impl<E: Engine> App<E> {
    /// `initial_actions` are the actions from building the initial model
    /// (opening the first window and its tabs). They run once the event
    /// loop is active and the engine exists.
    pub fn new(
        brand: &'static Brand,
        engine_info: &'static EngineInfo,
        browser: Browser,
        keymap: Keymap,
        initial_actions: Vec<EngineAction>,
        start_engine: StartEngine<E>,
        event_loop: &EventLoop<AppEvent>,
    ) -> App<E> {
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
            exiting: false,
            engine_failed: false,
            window_failed: false,
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
    /// (tabs are not reordered by dragging).
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
        for command in commands {
            let actions = dispatch(&mut self.browser, &command);
            self.perform_all(actions, event_loop);
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
            EngineAction::OpenWindow { window, tab, url } => {
                match BrowserWindow::open(event_loop, engine, self.brand, self.engine_info, window)
                {
                    Ok(mut bw) => {
                        bw.host_mut().create_tab(tab, url);
                        bw.host().set_focused(tab, true);
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
                    w.host().set_focused(tab, true);
                    w.request_redraw();
                }
            }
            EngineAction::AdoptTab {
                window,
                tab,
                pending,
            } => {
                let adopted = match self.window_for_core_id(window) {
                    Some(w) => {
                        let adopted = w.host_mut().adopt_tab(pending, tab);
                        if adopted {
                            w.host().set_focused(tab, true);
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
                    w.host().set_focused(tab, true);
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
            EngineAction::OpenSettings => match &self.settings_window {
                Some(sw) => sw.focus(),
                None => match SettingsWindow::open(event_loop, engine, self.brand) {
                    Ok(sw) => self.settings_window = Some(sw),
                    Err(e) => log::error!("cannot open settings window: {e}"),
                },
            },
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
            match &event {
                EngineEvent::EngineError(message) => {
                    log::error!("engine error: {message}");
                    for w in self.windows.values() {
                        w.request_redraw();
                    }
                }
                EngineEvent::CursorChanged { tab, cursor } => {
                    let browser = &self.browser;
                    if let Some(w) = self.windows.values_mut().find(|w| w.hosts_tab(*tab)) {
                        w.set_page_cursor(*tab, browser, *cursor);
                    }
                }
                _ => {}
            }
            let actions = apply(&mut self.browser, &event);
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
        }
    }

    fn save_settings(&self) {
        let path = self.browser.profile.settings_path();
        if let Err(e) = self.browser.settings.save(&path) {
            log::error!("cannot save settings to {}: {e}", path.display());
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

fn tab_of(event: &EngineEvent) -> Option<TabId> {
    match event {
        EngineEvent::UrlChanged { tab, .. }
        | EngineEvent::TitleChanged { tab, .. }
        | EngineEvent::LoadStateChanged { tab, .. }
        | EngineEvent::HistoryChanged { tab, .. }
        | EngineEvent::StatusChanged { tab, .. }
        | EngineEvent::FaviconChanged { tab, .. }
        | EngineEvent::TabCrashed { tab, .. }
        | EngineEvent::FrameReady(tab)
        | EngineEvent::TabClosed(tab) => Some(*tab),
        EngineEvent::NewTabRequested { opener, .. } => Some(*opener),
        // The cursor is applied directly; no redraw needed.
        EngineEvent::CursorChanged { .. } | EngineEvent::EngineError(_) => None,
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
                Ok(engine) => self.engine = Some(engine),
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
            let actions = std::mem::take(&mut self.pending_actions);
            self.perform_all(actions, event_loop);
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
            match sw.handle_event(&event, &mut self.browser.settings, &self.keymap) {
                SettingsOutcome::Nothing => {}
                SettingsOutcome::Changed => self.save_settings(),
                SettingsOutcome::Close { save } => {
                    if save {
                        self.save_settings();
                    }
                    self.settings_window = None;
                }
                SettingsOutcome::Shortcut(kind) => self.settings_shortcut(kind, event_loop),
            }
        } else if let Some(window) = self.windows.get_mut(&window_id) {
            let commands = window.handle_event(&self.browser, &self.keymap, &event);
            let drop = window.take_tab_drop();
            self.run_commands(commands, event_loop);
            if let Some((tabs, point)) = drop
                && let Some(command) = self.tab_drop_command(window_id, tabs, point)
            {
                self.run_commands(vec![command], event_loop);
            }
        }
        self.pump(event_loop);
        if !self.exiting {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Wake => self.pump(event_loop),
        }
        if !self.exiting {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    /// Runs on every exit path, including macOS `terminate:` from the Dock
    /// menu, which otherwise ends the process without dropping `App`. Tear
    /// down in dependency order: windows (tab hosts, GL) before the engine.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.exiting = true;
        self.close_settings_window();
        self.windows.clear();
        self.engine = None;
    }
}
