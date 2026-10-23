//! The winit application: owns the engine, the browser model, and the
//! native windows, and runs the dispatch loop (see the crate docs).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
use crate::clipboard;
use crate::repaint;
use crate::settings_window::{SettingsOutcome, SettingsWindow};
use crate::window::BrowserWindow;

/// How long after a history change the file is written, so a run of
/// page loads is saved once.
const HISTORY_SAVE_DELAY: Duration = Duration::from_secs(3);

/// The user events: the engine asked us to spin its message loop, or
/// the `--exit-after` timer ran out.
#[derive(Debug)]
pub enum AppEvent {
    Wake,
    ExitAfter(std::time::Duration),
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
}

impl<E: Engine> App<E> {
    /// `initial_actions` are the actions from building the initial model
    /// (opening the first window and its tabs). They run once the event
    /// loop is active and the engine exists.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        brand: &'static Brand,
        engine_info: &'static EngineInfo,
        browser: Browser,
        keymap: Keymap,
        initial_actions: Vec<EngineAction>,
        start_engine: StartEngine<E>,
        event_loop: &EventLoop<AppEvent>,
        exit_after: Option<std::time::Duration>,
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
            exit_after,
            exit_proxy: event_loop.create_proxy(),
            exiting: false,
            engine_failed: false,
            window_failed: false,
            history_dirty: None,
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
                let frameless = !self.browser.settings.system_title_bar;
                match BrowserWindow::open(
                    event_loop,
                    engine,
                    self.brand,
                    self.engine_info,
                    window,
                    frameless,
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
            EngineAction::SetFullscreen { window, fullscreen } => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.set_fullscreen(fullscreen);
                }
            }
            EngineAction::SaveSettings => {
                self.save_settings();
                self.redraw_all();
            }
            EngineAction::SaveBookmarks => {
                let path = self.browser.profile.bookmarks_path();
                if let Err(e) = self.browser.bookmarks.save(&path) {
                    log::error!("cannot save bookmarks to {}: {e}", path.display());
                }
            }
            EngineAction::SaveHistory => {
                self.history_dirty.get_or_insert_with(Instant::now);
            }
            EngineAction::ShowPrompt { window, prompt } => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.open_prompt(prompt);
                }
            }
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
            // Commands the user picked from a page's context menu, run
            // after the event itself is applied.
            let mut menu_commands = Vec::new();
            match &event {
                EngineEvent::EngineError(message) => {
                    log::error!("engine error: {message}");
                    self.redraw_all();
                }
                EngineEvent::ExtensionsChanged(_) => self.redraw_all(),
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
    fn settings_changed(&mut self) {
        self.save_settings();
        let frameless = !self.browser.settings.system_title_bar;
        for w in self.windows.values_mut() {
            w.set_frameless(frameless);
            w.request_redraw();
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
        EngineEvent::NewTabRequested { opener, .. }
        | EngineEvent::NewWindowRequested { opener, .. } => Some(*opener),
        EngineEvent::ContextMenuRequested { tab, .. } => Some(*tab),
        // The cursor and the clipboard are handled directly; no redraw.
        EngineEvent::CursorChanged { .. }
        | EngineEvent::ClipboardImage(_)
        | EngineEvent::EngineError(_)
        | EngineEvent::ExtensionsChanged(_) => None,
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
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Wake => self.pump(event_loop),
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
        let mut next = None;
        // The history is written a moment after it last changed.
        if let Some(since) = self.history_dirty {
            if since.elapsed() >= HISTORY_SAVE_DELAY {
                self.save_history();
            } else {
                next = repaint::earliest(next, Some(since + HISTORY_SAVE_DELAY));
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
        self.close_settings_window();
        self.windows.clear();
        self.engine = None;
    }
}
