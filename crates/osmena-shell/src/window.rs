//! A browser window: a winit window, the engine's tab host for it, a GL
//! surface from the engine, and an egui context that paints the chrome
//! and, under it, the active tab, whichever way the engine delivers it
//! (a blit from its own framebuffer, or a frame of pixels).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use egui::{Color32, LayerId, PaintCallback, Rect as EguiRect, pos2};
use egui_glow::{CallbackFn, EguiGlow};
use keyboard_types::{KeyState, Modifiers};
use osmena_chrome::chrome::{self, ChromeState, WindowRequest};
use osmena_chrome::routing::{
    KeyRouter, KeyStroke, KeyTarget, PointerCapture, PointerContext, PointerTarget, content_pixels,
    resize_edge, route_pointer,
};
use osmena_chrome::{icon, input, keyutils, menus, native_menu};
use osmena_core::{
    Brand, Browser, Command, Cursor, EngineInfo, EngineInput, KeyInput, Keymap, PageContext,
    Platform, Prompt, TabId, WindowId,
};
use osmena_engine::{
    BlitRect, Engine, EngineError, PagePaint, PhysicalSize, ScreenRect, Surface, TabHost,
};
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{CursorIcon, Fullscreen, ResizeDirection, Window, WindowAttributes};

use crate::page::PageTexture;
use crate::repaint::RepaintSchedule;

/// How close to a frameless window's edge (physical pixels at 1x) the
/// pointer resizes it instead of acting on what is under it.
const RESIZE_BORDER_PX: f32 = 6.0;

pub struct BrowserWindow<E: Engine> {
    id: WindowId,
    engine_info: &'static EngineInfo,
    chrome: ChromeState,
    modifiers: ModifiersState,
    cursor: Option<PhysicalPosition<f64>>,
    /// The window has no system title bar: the chrome's tab strip stands
    /// in for it, and the window handles the resize borders itself.
    frameless: bool,
    /// The edge the pointer is on, when a frameless window would resize
    /// from it on a press.
    resize_edge: Option<ResizeDirection>,
    /// Physical-pixel height of the chrome above the page, from the last
    /// frame. Events below it go to the page, above it to egui.
    content_top_px: f32,
    /// True while egui draws the page area itself (the crash panel).
    content_is_egui: bool,
    pointer_in_content: bool,
    /// The pointer shape the active page asked for.
    page_cursor: CursorIcon,
    /// Which side holds the pointer during a drag or click. See
    /// [`PointerCapture`].
    capture: PointerCapture,
    /// Which side each held key went to, and which presses are keymap
    /// commands. See [`KeyRouter`].
    keys: KeyRouter,
    /// The press the page got for each key it holds, so the key can be
    /// released there if the real release never arrives.
    page_presses: HashMap<keyboard_types::Code, KeyInput>,
    /// The uploaded frame of each tab that handed the shell pixels.
    textures: HashMap<TabId, PageTexture>,
    /// When egui wants the next frame.
    repaints: RepaintSchedule,
    egui: EguiGlow,
    // Dropped before the surface: the host may hold render targets on it.
    host: E::Host,
    surface: E::Surface,
    // Dropped last: the host, egui, and the surface are tied to it.
    window: Window,
}

impl<E: Engine> BrowserWindow<E> {
    pub fn open(
        event_loop: &ActiveEventLoop,
        engine: &E,
        brand: &Brand,
        engine_info: &'static EngineInfo,
        id: WindowId,
        frameless: bool,
    ) -> Result<BrowserWindow<E>, EngineError> {
        let attributes = Window::default_attributes()
            .with_title(brand.name)
            .with_inner_size(LogicalSize::new(1024.0, 768.0))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0))
            .with_window_icon(icon::window_icon(brand));
        let attributes = frameless_attributes(attributes, frameless);
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| EngineError(format!("creating window: {e}")))?;

        let size = non_zero(window.inner_size());
        let display_handle = event_loop
            .display_handle()
            .map_err(|e| EngineError(format!("display handle: {e}")))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| EngineError(format!("window handle: {e}")))?;
        let surface = engine.open_surface(display_handle, window_handle, size)?;
        surface.make_current()?;
        let host = engine.open_host(&surface, window_handle, window.scale_factor() as f32)?;
        let egui = EguiGlow::new(event_loop, surface.gl(), None, None, false);
        egui.egui_ctx.options_mut(|options| {
            options.zoom_with_keyboard = false;
        });
        let repaints = RepaintSchedule::install(&egui.egui_ctx);

        Ok(BrowserWindow {
            id,
            engine_info,
            chrome: ChromeState::default(),
            modifiers: ModifiersState::default(),
            cursor: None,
            frameless,
            resize_edge: None,
            content_top_px: 0.0,
            content_is_egui: false,
            pointer_in_content: false,
            page_cursor: CursorIcon::Default,
            capture: PointerCapture::default(),
            keys: KeyRouter::default(),
            page_presses: HashMap::new(),
            textures: HashMap::new(),
            repaints,
            egui,
            host,
            surface,
            window,
        })
    }

    pub fn core_id(&self) -> WindowId {
        self.id
    }

    pub fn winit_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    pub fn host(&self) -> &E::Host {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut E::Host {
        &mut self.host
    }

    pub fn hosts_tab(&self, tab: TabId) -> bool {
        self.host.has_tab(tab)
    }

    /// The native window has keyboard focus. A tab is told it has focus
    /// only while its window does; `Focused(true)` tells the active tab
    /// when the window gets it later.
    pub fn has_focus(&self) -> bool {
        self.window.has_focus()
    }

    /// Tell the engine `tab` has focus, if this window does.
    pub fn focus_tab(&self, tab: TabId) {
        if self.has_focus() {
            self.host.set_focused(tab, true);
        }
    }

    /// Close the tab in the engine and drop its texture, if it had one.
    pub fn close_tab(&mut self, tab: TabId) {
        self.host.close_tab(tab);
        if let Some(texture) = self.textures.remove(&tab) {
            let _ = self.surface.make_current();
            texture.destroy(&self.surface.gl(), &mut self.egui.painter);
        }
        self.window.request_redraw();
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Request a redraw if egui wants one now, and return when it wants
    /// the next one otherwise (a caret blink, a tooltip delay), for the
    /// event loop to wait until.
    pub fn redraw_deadline(&mut self) -> Option<Instant> {
        self.repaints.poll(&self.window)
    }

    pub fn focus_address_bar(&mut self) {
        self.chrome.focus_address = true;
        self.window.request_redraw();
    }

    /// Give the window the system title bar, or take it away. On macOS
    /// the title bar style is fixed when the window is created, so the
    /// change applies to windows opened from now on.
    pub fn set_frameless(&mut self, frameless: bool) {
        if self.frameless == frameless {
            return;
        }
        self.frameless = frameless;
        self.resize_edge = None;
        if !cfg!(target_os = "macos") {
            self.window.set_decorations(!frameless);
        }
        self.window.request_redraw();
    }

    /// Fill the screen (borderless) or go back to a normal window.
    /// Open the popup of `extension`'s action for `tab`, below its
    /// toolbar button (or below the extensions menu button when it is
    /// not pinned).
    pub fn open_extension_action(&self, tab: TabId, extension: &str) {
        let anchor = self
            .chrome
            .action_anchor(extension)
            .or_else(|| self.chrome.extensions_menu_anchor());
        let Some(anchor) = anchor else {
            return;
        };
        let Ok(origin) = self.window.inner_position() else {
            return;
        };
        let scale = self.window.scale_factor() as f32;
        let px = |v: f32| (v * scale).round() as i32;
        self.host.open_extension_action(
            tab,
            extension,
            ScreenRect {
                x: origin.x + px(anchor.min.x),
                y: origin.y + px(anchor.min.y),
                width: px(anchor.width()),
                height: px(anchor.height()),
            },
        );
    }

    /// Show the context menu of `tab`'s page, over `context`, at
    /// `position` (physical pixels from the page area's top left; the
    /// pointer when `None`), and return what to do: the chosen command,
    /// if any, then `ContextMenuClosed` so the engine lets go of what it
    /// held for the menu.
    pub fn show_page_menu(
        &self,
        browser: &Browser,
        keymap: &Keymap,
        tab: TabId,
        position: Option<(f32, f32)>,
        context: &PageContext,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        if let Some(model) = browser.window(self.id)
            && model.tab(tab).is_some()
        {
            let entries = menus::page_menu(
                model,
                tab,
                context,
                &self.engine_info.page_tools,
                keymap,
                Platform::current(),
                &browser.settings.search.name,
            );
            let scale = self.window.scale_factor();
            let position = match position {
                Some((x, y)) => Some(LogicalPosition::new(
                    f64::from(x) / scale,
                    (f64::from(y) + f64::from(self.content_top_px)) / scale,
                )),
                None => self
                    .cursor
                    .map(|p| LogicalPosition::new(p.x / scale, p.y / scale)),
            };
            let menu = native_menu::NativeMenu::build(&entries);
            if let Some(command) = menu.show(&self.window, position) {
                commands.push(command);
            }
        }
        commands.push(Command::ContextMenuClosed(tab));
        commands
    }

    /// Open the new-tab launcher over this window.
    pub fn open_launcher(&mut self) {
        self.chrome.open_launcher();
        self.window.request_redraw();
    }

    /// Ask the user something over this window's page.
    pub fn open_prompt(&mut self, prompt: Prompt) {
        self.chrome.open_prompt(prompt);
        self.window.request_redraw();
    }

    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.window
            .set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
        self.window.request_redraw();
    }

    /// The pointer is over an edge of this window that a press would
    /// resize from: a frameless window that is neither maximized nor
    /// full screen. On macOS the system keeps the resize borders.
    fn edge_under_pointer(&self, browser: &Browser) -> Option<ResizeDirection> {
        if !self.frameless
            || cfg!(target_os = "macos")
            || self.window.is_maximized()
            || browser.window(self.id).is_some_and(|w| w.fullscreen)
        {
            return None;
        }
        let p = self.cursor?;
        let border = RESIZE_BORDER_PX * self.window.scale_factor() as f32;
        resize_edge(p.x as f32, p.y as f32, self.window.inner_size(), border)
    }

    /// Hand the pointer to the system for a move or resize of the window.
    /// The system keeps it until the button is released, and that release
    /// never reaches us, so egui and the capture are told it happened now.
    fn start_system_drag(
        &mut self,
        drag: impl FnOnce(&Window) -> Result<(), winit::error::ExternalError>,
    ) {
        if let Err(e) = drag(&self.window) {
            log::warn!("window drag not supported: {e}");
        }
        self.capture.clear();
        let release = WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
        };
        let _ = self.egui.on_window_event(&self.window, &release);
    }

    /// The page in `tab` asked for a pointer shape.
    pub fn set_page_cursor(&mut self, tab: TabId, browser: &Browser, cursor: Cursor) {
        if browser.window(self.id).is_none_or(|w| w.active != tab) {
            return;
        }
        self.page_cursor = cursor_icon(cursor);
        if self.pointer_in_content {
            self.window.set_cursor(self.page_cursor);
        }
    }

    /// The tabs a drag that ended in this window carried, with the drop
    /// point in physical screen pixels (the pointer may have been outside
    /// this window). Taken once. `None` if the window's position is not
    /// known.
    pub fn take_tab_drop(&mut self) -> Option<(Vec<TabId>, PhysicalPosition<f64>)> {
        let drop = self.chrome.take_tab_drop()?;
        let origin = self.window.inner_position().ok()?;
        let scale = self.window.scale_factor();
        Some((
            drop.tabs,
            PhysicalPosition::new(
                f64::from(origin.x) + f64::from(drop.pointer.x) * scale,
                f64::from(origin.y) + f64::from(drop.pointer.y) * scale,
            ),
        ))
    }

    /// Whether `point` (physical screen pixels) is over this window, frame
    /// included. False if the window's position is not known.
    pub fn contains_screen_point(&self, point: PhysicalPosition<f64>) -> bool {
        let Ok(origin) = self.window.outer_position() else {
            return false;
        };
        let size = self.window.outer_size();
        point.x >= f64::from(origin.x)
            && point.y >= f64::from(origin.y)
            && point.x < f64::from(origin.x) + f64::from(size.width)
            && point.y < f64::from(origin.y) + f64::from(size.height)
    }

    /// Who the pointer belongs to right now: the side holding a capture,
    /// otherwise whatever [`route_pointer`] decides from the position and
    /// egui's state.
    fn pointer_target(&self) -> PointerTarget {
        let ctx = PointerContext {
            over_chrome: match self.cursor {
                Some(p) => (p.y as f32) < self.content_top_px,
                None => true,
            },
            content_is_egui: self.content_is_egui,
            over_egui: self.egui.egui_ctx.is_pointer_over_egui(),
            popup_open: egui::Popup::is_any_open(&self.egui.egui_ctx) || self.chrome.popup_open(),
        };
        route_pointer(self.capture.captured(), ctx)
    }

    /// The tab a pointer event for the page goes to: the tab holding the
    /// capture while one is held (even if another tab is active now),
    /// otherwise the active tab. Nothing while the capturing tab is gone:
    /// its release ends the capture without reaching anyone.
    fn pointer_tab(&self, browser: &Browser, active: Option<TabId>) -> Option<TabId> {
        match self.capture.tab() {
            Some(tab) => browser
                .window(self.id)
                .is_some_and(|w| w.tab(tab).is_some())
                .then_some(tab),
            None => active,
        }
    }

    /// Cursor position in physical pixels relative to the page area.
    fn content_point(&self) -> Option<(f32, f32)> {
        let p = self.cursor?;
        Some((p.x as f32, p.y as f32 - self.content_top_px))
    }

    /// The keyboard modifiers held now, for pointer events. winit keeps
    /// them from `ModifiersChanged`, which it also sends when the window
    /// loses focus (all up) and regains it (the real state), so they
    /// never go stale the way a modifier inferred from key events does.
    fn modifiers(&self) -> Modifiers {
        keyutils::keyboard_modifiers_from_winit_modifiers(self.modifiers)
    }

    fn enter_content(&mut self, active: Option<TabId>, x: f32, y: f32) {
        if !self.pointer_in_content {
            self.window.set_cursor(self.page_cursor);
        }
        self.pointer_in_content = true;
        if let Some(tab) = active {
            let modifiers = self.modifiers();
            self.host
                .input(tab, EngineInput::MouseMove { x, y, modifiers });
        }
    }

    fn leave_content(&mut self, active: Option<TabId>) {
        if self.pointer_in_content {
            self.pointer_in_content = false;
            self.window.set_cursor(CursorIcon::Default);
            if let Some(tab) = active {
                self.host.input(tab, EngineInput::MouseLeft);
            }
        }
    }

    /// Handle one winit event. Returns the commands the user issued.
    pub fn handle_event(
        &mut self,
        browser: &Browser,
        keymap: &Keymap,
        event: &WindowEvent,
    ) -> Vec<Command> {
        let active = browser.window(self.id).map(|w| w.active);
        let pointer_tab = self.pointer_tab(browser, active);
        let mut commands = Vec::new();
        let mut forward_to_egui = true;

        match event {
            WindowEvent::Resized(size) => {
                self.surface.resize(non_zero(*size));
                self.host.close_extension_popups();
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.host.set_hidpi(*scale_factor as f32);
                self.window.request_redraw();
            }
            WindowEvent::Moved(_) => {
                self.host.close_extension_popups();
                self.update_content_origin();
            }
            WindowEvent::RedrawRequested => {
                return self.frame(browser, keymap);
            }
            WindowEvent::CloseRequested => {
                return vec![Command::CloseWindow(self.id)];
            }
            WindowEvent::Focused(focused) => {
                if *focused {
                    // An extension popup is dismissed by clicking back
                    // into the window, like any popup.
                    self.host.close_extension_popups();
                } else {
                    self.release_held(browser);
                }
                if let Some(tab) = active {
                    self.host.set_focused(tab, *focused);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                // egui always sees cursor movement so it can hide tooltips
                // and hover state when the pointer leaves the chrome.
                self.cursor = Some(*position);
                // A frameless window's edges resize it; while the pointer
                // is on one it belongs to neither the chrome nor the page.
                let edge = if self.capture.captured().is_none() {
                    self.edge_under_pointer(browser)
                } else {
                    None
                };
                if edge != self.resize_edge {
                    self.resize_edge = edge;
                    if let Some(edge) = edge {
                        self.leave_content(active);
                        self.window.set_cursor(resize_cursor(edge));
                    } else {
                        self.window.set_cursor(CursorIcon::Default);
                    }
                }
                if edge.is_some() {
                    // Nothing else to do: the pointer is on the border.
                } else if self.pointer_target() == PointerTarget::Page {
                    if let Some((x, y)) = self.content_point() {
                        self.enter_content(pointer_tab, x, y);
                    }
                } else {
                    self.leave_content(pointer_tab);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.leave_content(pointer_tab);
                if self.resize_edge.take().is_some() {
                    self.window.set_cursor(CursorIcon::Default);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if self.resize_edge.is_some() => {
                let edge = self.resize_edge.expect("checked by the guard");
                forward_to_egui = false;
                self.start_system_drag(|w| w.drag_resize_window(edge));
            }
            // The back and forward buttons of a mouse step through the
            // active tab's history wherever the pointer is, as in other
            // browsers; neither side sees them.
            WindowEvent::MouseInput {
                state,
                button: button @ (MouseButton::Back | MouseButton::Forward),
                ..
            } => {
                forward_to_egui = false;
                if *state == ElementState::Pressed
                    && let Some(tab) = active
                {
                    commands.push(match button {
                        MouseButton::Back => Command::Back(tab),
                        _ => Command::Forward(tab),
                    });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let current = self.pointer_target();
                let target = match state {
                    ElementState::Pressed => self.capture.press(*button, current, pointer_tab),
                    ElementState::Released => self.capture.release(*button, current),
                };
                if target == PointerTarget::Page {
                    forward_to_egui = false;
                    self.surrender_egui_focus();
                    if let (Some(tab), Some((x, y))) = (pointer_tab, self.content_point()) {
                        self.host.input(
                            tab,
                            EngineInput::MouseButton {
                                action: input::button_action(*state),
                                button: input::mouse_button(*button),
                                x,
                                y,
                                modifiers: self.modifiers(),
                            },
                        );
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.pointer_target() == PointerTarget::Page {
                    forward_to_egui = false;
                    if let (Some(tab), Some((x, y))) = (pointer_tab, self.content_point()) {
                        let (dx, dy) = input::wheel_delta(*delta, self.engine_info.wheel_line_px);
                        let modifiers = self.modifiers();
                        self.host.input(
                            tab,
                            EngineInput::Wheel {
                                dx,
                                dy,
                                x,
                                y,
                                modifiers,
                            },
                        );
                    }
                }
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                let key_input = keyutils::key_input_from_winit(key, self.modifiers);
                let egui_wants_keyboard = self.egui.egui_ctx.egui_wants_keyboard_input();
                let stroke = KeyStroke {
                    code: key_input.code,
                    modifiers: key_input.modifiers,
                    pressed: key.state == ElementState::Pressed,
                    repeat: key.repeat,
                };
                let live = |tab| {
                    browser
                        .window(self.id)
                        .is_some_and(|w| w.tab(tab).is_some())
                };
                let target = self
                    .keys
                    .route(stroke, egui_wants_keyboard, active, live, keymap);
                match target {
                    Some(KeyTarget::Page(tab)) => {
                        forward_to_egui = false;
                        if stroke.pressed {
                            self.page_presses.insert(stroke.code, key_input.clone());
                        } else {
                            self.page_presses.remove(&stroke.code);
                        }
                        self.host.input(tab, EngineInput::Key(key_input));
                    }
                    Some(KeyTarget::Command(kind)) => {
                        forward_to_egui = false;
                        if let Some(tab) = active {
                            commands.push(kind.into_command(self.id, tab));
                        }
                    }
                    Some(KeyTarget::Egui) => {}
                    None => forward_to_egui = false,
                }
            }
            // egui-winit calls `set_ime_allowed(false)` whenever no egui
            // text field has focus, so winit sends no Ime events for page
            // content yet. The arm is ready for when it does.
            WindowEvent::Ime(ime) if !self.egui.egui_ctx.egui_wants_keyboard_input() => {
                forward_to_egui = false;
                if let (Some(tab), Some(input)) = (active, input::ime(ime)) {
                    self.host.input(tab, input);
                }
            }
            _ => {}
        }

        if forward_to_egui {
            let response = self.egui.on_window_event(&self.window, event);
            if response.repaint {
                self.window.request_redraw();
            }
        }
        commands
    }

    /// The window lost focus: the releases of the keys and buttons the
    /// page holds will arrive elsewhere (Alt+Tab takes the Alt release;
    /// the system does not synthesize one), so release them in the page
    /// now, each in the tab that got its press, with the modifiers as
    /// the window knows them (all up, on Windows, by this point). A page
    /// that tracks key or button state is then never stuck with one
    /// down; egui forgets its own on the same event.
    fn release_held(&mut self, browser: &Browser) {
        let live = |tab: TabId| {
            browser
                .window(self.id)
                .is_some_and(|w| w.tab(tab).is_some())
        };
        let modifiers = self.modifiers();
        for (code, tab) in self.keys.release_all() {
            if let Some(press) = self.page_presses.remove(&code)
                && live(tab)
            {
                let release = KeyInput {
                    state: KeyState::Up,
                    modifiers,
                    repeat: false,
                    ..press
                };
                self.host.input(tab, EngineInput::Key(release));
            }
        }
        self.page_presses.clear();
        if let Some((tab, buttons)) = self.capture.release_all()
            && live(tab)
        {
            let (x, y) = self.content_point().unwrap_or((0.0, 0.0));
            for button in buttons {
                self.host.input(
                    tab,
                    EngineInput::MouseButton {
                        action: osmena_core::ButtonAction::Up,
                        button: input::mouse_button(button),
                        x,
                        y,
                        modifiers,
                    },
                );
            }
        }
    }

    fn surrender_egui_focus(&self) {
        self.egui.egui_ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    /// Tell the engine where the page area sits on screen (logical
    /// pixels), for engines that place menus and widgets themselves.
    fn update_content_origin(&self) {
        let Ok(origin) = self.window.inner_position() else {
            return;
        };
        let scale = self.window.scale_factor();
        self.host.set_content_origin(
            (f64::from(origin.x) / scale).round() as i32,
            ((f64::from(origin.y) + f64::from(self.content_top_px)) / scale).round() as i32,
        );
    }

    /// Drop the textures of tabs that left this window.
    fn drop_stale_textures(&mut self) {
        let gone: Vec<TabId> = self
            .textures
            .keys()
            .copied()
            .filter(|tab| !self.host.has_tab(*tab))
            .collect();
        if gone.is_empty() {
            return;
        }
        let _ = self.surface.make_current();
        for tab in gone {
            if let Some(texture) = self.textures.remove(&tab) {
                texture.destroy(&self.surface.gl(), &mut self.egui.painter);
            }
        }
    }

    /// Draw the chrome and the active tab under it, and present.
    fn frame(&mut self, browser: &Browser, keymap: &Keymap) -> Vec<Command> {
        let Some(model) = browser.window(self.id) else {
            return Vec::new();
        };
        let active = model.active;
        let crashed = model.active_tab().crashed.clone();
        let engine_error = browser.engine_error.as_deref();
        let chrome_color = browser.settings.chrome_color;
        let engine_info = self.engine_info;
        let frameless = self.frameless;
        let maximized = self.window.is_maximized();
        let pinned = browser.pinned_extensions();
        let history = &browser.history;
        let mut commands = Vec::new();
        let mut content_top_px = 0.0;

        self.drop_stale_textures();
        // The GL context is current for the uploads and paints the host
        // does during the UI pass.
        self.surface.begin_frame();
        let gl = self.surface.gl();

        let Self {
            egui,
            host,
            chrome: chrome_state,
            textures,
            window,
            ..
        } = self;
        egui.run(window, |ui| {
            let context = chrome::ChromeContext {
                color: chrome_color,
                engine: engine_info,
                engine_error,
                frameless,
                maximized,
                pinned: &pinned,
                history,
            };
            commands = chrome::draw(ui, model, &context, chrome_state, keymap);
            let rect = ui.available_rect_before_wrap();
            let (top, size) = content_pixels(rect, ui.pixels_per_point());
            content_top_px = top;
            host.resize_content(active, size);
            match &crashed {
                Some(reason) => {
                    if chrome::draw_crash_panel(ui, rect, reason) {
                        commands.push(Command::Reload(active));
                    }
                }
                None => {
                    let ui: &egui::Ui = ui;
                    let mut painted = false;
                    host.paint_page(active, &mut |paint| match paint {
                        PagePaint::Blit(blit) => {
                            // Runs in egui's paint pass, after this frame's
                            // resize recreated the engine's framebuffer.
                            ui.ctx()
                                .layer_painter(LayerId::background())
                                .add(PaintCallback {
                                    rect,
                                    callback: Arc::new(CallbackFn::new(move |info, painter| {
                                        let v = info.viewport_in_pixels();
                                        blit(
                                            painter.gl(),
                                            BlitRect {
                                                x: v.left_px,
                                                y_from_bottom: v.from_bottom_px,
                                                width: v.width_px,
                                                height: v.height_px,
                                            },
                                        );
                                    })),
                                });
                            painted = true;
                        }
                        PagePaint::Frame(frame) => {
                            if let Some(texture) = textures.get_mut(&active) {
                                if frame.fresh || texture.size() != (frame.width, frame.height) {
                                    texture.update(
                                        &gl,
                                        frame.width,
                                        frame.height,
                                        frame.bgra,
                                        frame.dirty,
                                    );
                                }
                            } else if let Some(texture) =
                                PageTexture::new(&gl, frame.width, frame.height, frame.bgra)
                            {
                                textures.insert(active, texture);
                            }
                            // Registered with egui after this pass; until
                            // then the page stays blank for one frame.
                            let Some(texture) = textures.get(&active) else {
                                return;
                            };
                            let Some(id) = texture.id() else {
                                return;
                            };
                            let (width, height) = texture.size();
                            // Paint the frame at its own pixel size from the
                            // page area's top-left, so a frame from before a
                            // resize is neither stretched nor misplaced; the
                            // next paint fills the new size.
                            let ppp = ui.pixels_per_point();
                            let frame_rect = EguiRect::from_min_size(
                                rect.min,
                                egui::vec2(width as f32 / ppp, height as f32 / ppp),
                            );
                            ui.painter().with_clip_rect(rect).image(
                                id,
                                frame_rect,
                                EguiRect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                            painted = true;
                        }
                    });
                    if !painted {
                        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                    }
                }
            }
        });
        self.content_top_px = content_top_px;
        self.content_is_egui = crashed.is_some();
        self.update_content_origin();

        let mut registered = false;
        for texture in self.textures.values_mut() {
            registered |= texture.register(&mut self.egui.painter);
        }

        // Back to the window's framebuffer: the host may have painted into
        // its own. egui_glow paints over whatever the framebuffer holds, so
        // clear it first: a region egui does not draw (or a reused buffer)
        // would otherwise show stale pixels.
        self.surface.begin_frame();
        let size = self.window.inner_size();
        self.egui.painter.clear(
            [size.width, size.height],
            chrome::background(&self.egui.egui_ctx, chrome_color),
        );
        self.egui.paint(&self.window);
        self.surface.present();
        // A texture registered this frame shows on the next one; what egui
        // itself asked for, the application reads from `redraw_deadline`.
        if registered {
            self.window.request_redraw();
        }
        // After the frame is on screen: the strip used as a title bar.
        if let Some(request) = self.chrome.take_window_request() {
            match request {
                WindowRequest::Drag => self.start_system_drag(Window::drag_window),
                WindowRequest::Minimize => self.window.set_minimized(true),
                WindowRequest::ToggleMaximize => {
                    self.window.set_maximized(!self.window.is_maximized());
                }
                WindowRequest::SystemMenu { position } => {
                    self.window.show_window_menu(LogicalPosition::new(
                        f64::from(position.x),
                        f64::from(position.y),
                    ));
                }
            }
        }
        // A native menu blocks until it closes.
        if let Some(request) = self.chrome.take_menu_request()
            && let Some(command) = self.show_menu(browser, keymap, request)
        {
            commands.push(command);
        }
        commands
    }

    /// Show a native popup for `request` and return the command of the item
    /// the user picked, if any. The menu's entries come from `menus`; the
    /// tab menu lists the other windows and the other profiles.
    fn show_menu(
        &self,
        browser: &Browser,
        keymap: &Keymap,
        request: chrome::MenuRequest,
    ) -> Option<Command> {
        let model = browser.window(self.id)?;
        let (entries, position) = match request {
            chrome::MenuRequest::Window { position } => (
                menus::window_menu(
                    model,
                    keymap,
                    !browser.closed_tabs.is_empty(),
                    &browser.bookmarks,
                ),
                position,
            ),
            chrome::MenuRequest::Folder { folder, position } => {
                (menus::folder_menu(model, folder), position)
            }
            chrome::MenuRequest::Extensions { position } => {
                let support = self.engine_info.extensions.as_ref()?;
                (
                    menus::extensions_menu(
                        model,
                        &browser.extensions,
                        &browser.settings.pinned_extensions,
                        support,
                        &model.active_tab().url,
                    ),
                    position,
                )
            }
            chrome::MenuRequest::Tab { targets, position } => {
                let others: Vec<(WindowId, String)> = browser
                    .windows
                    .iter()
                    .filter(|w| w.id != self.id)
                    .map(|w| (w.id, w.active_tab().display_title().to_string()))
                    .collect();
                let profiles = browser.profile.others();
                (
                    menus::tab_menu(model, &targets, &others, &profiles),
                    position,
                )
            }
        };
        let menu = native_menu::NativeMenu::build(&entries);
        menu.show(
            &self.window,
            Some(LogicalPosition::new(
                f64::from(position.x),
                f64::from(position.y),
            )),
        )
    }
}

impl<E: Engine> Drop for BrowserWindow<E> {
    fn drop(&mut self) {
        let _ = self.surface.make_current();
        let gl = self.surface.gl();
        for (_, texture) in self.textures.drain() {
            texture.destroy(&gl, &mut self.egui.painter);
        }
        self.egui.destroy();
    }
}

/// GL surfaces reject zero-sized targets; a minimized window reports 0x0.
fn non_zero(size: winit::dpi::PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

/// The window attributes for a window without the system title bar. On
/// Windows and Linux the decorations go entirely (Windows keeps a shadow
/// and snap behaviour); on macOS the title bar stays but turns
/// transparent and the content extends under it, so the traffic lights
/// sit over the tab strip as they do in other browsers.
fn frameless_attributes(attributes: WindowAttributes, frameless: bool) -> WindowAttributes {
    #[cfg(windows)]
    let attributes = {
        use winit::platform::windows::WindowAttributesExtWindows;
        attributes.with_undecorated_shadow(true)
    };
    if !frameless {
        return attributes;
    }
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attributes
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        attributes.with_decorations(false)
    }
}

fn resize_cursor(edge: ResizeDirection) -> CursorIcon {
    match edge {
        ResizeDirection::North => CursorIcon::NResize,
        ResizeDirection::South => CursorIcon::SResize,
        ResizeDirection::East => CursorIcon::EResize,
        ResizeDirection::West => CursorIcon::WResize,
        ResizeDirection::NorthEast => CursorIcon::NeResize,
        ResizeDirection::NorthWest => CursorIcon::NwResize,
        ResizeDirection::SouthEast => CursorIcon::SeResize,
        ResizeDirection::SouthWest => CursorIcon::SwResize,
    }
}

fn cursor_icon(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::Default | Cursor::Hidden => CursorIcon::Default,
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::Text => CursorIcon::Text,
        Cursor::Crosshair => CursorIcon::Crosshair,
        Cursor::Wait => CursorIcon::Wait,
        Cursor::Progress => CursorIcon::Progress,
        Cursor::Help => CursorIcon::Help,
        Cursor::Move => CursorIcon::Move,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::NotAllowed => CursorIcon::NotAllowed,
        Cursor::ColResize => CursorIcon::ColResize,
        Cursor::RowResize => CursorIcon::RowResize,
        Cursor::NsResize => CursorIcon::NsResize,
        Cursor::EwResize => CursorIcon::EwResize,
        Cursor::NeswResize => CursorIcon::NeswResize,
        Cursor::NwseResize => CursorIcon::NwseResize,
        Cursor::ZoomIn => CursorIcon::ZoomIn,
        Cursor::ZoomOut => CursorIcon::ZoomOut,
    }
}
