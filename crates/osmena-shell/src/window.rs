//! A browser window: a winit window, the engine's tab host for it, a GL
//! surface from the engine, and an egui context that paints the chrome
//! and, under it, the active tab, whichever way the engine delivers it
//! (a blit from its own framebuffer, or a frame of pixels).

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, LayerId, PaintCallback, Rect as EguiRect, pos2};
use egui_glow::{CallbackFn, EguiGlow};
use osmena_chrome::chrome::{self, ChromeState};
use osmena_chrome::routing::{
    KeyRouter, KeyStroke, KeyTarget, PointerCapture, PointerContext, PointerTarget, content_pixels,
    route_pointer,
};
use osmena_chrome::{icon, input, keyutils, menus, native_menu};
use osmena_core::{
    Brand, Browser, Command, Cursor, EngineInfo, EngineInput, Keymap, TabId, WindowId,
};
use osmena_engine::{BlitRect, Engine, EngineError, PagePaint, PhysicalSize, Surface, TabHost};
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{CursorIcon, Window};

use crate::page::PageTexture;

pub struct BrowserWindow<E: Engine> {
    id: WindowId,
    engine_info: &'static EngineInfo,
    chrome: ChromeState,
    modifiers: ModifiersState,
    cursor: Option<PhysicalPosition<f64>>,
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
    /// The uploaded frame of each tab that handed the shell pixels.
    textures: HashMap<TabId, PageTexture>,
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
    ) -> Result<BrowserWindow<E>, EngineError> {
        let attributes = Window::default_attributes()
            .with_title(brand.name)
            .with_inner_size(LogicalSize::new(1024.0, 768.0))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0))
            .with_window_icon(icon::window_icon(brand));
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

        Ok(BrowserWindow {
            id,
            engine_info,
            chrome: ChromeState::default(),
            modifiers: ModifiersState::default(),
            cursor: None,
            content_top_px: 0.0,
            content_is_egui: false,
            pointer_in_content: false,
            page_cursor: CursorIcon::Default,
            capture: PointerCapture::default(),
            keys: KeyRouter::default(),
            textures: HashMap::new(),
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

    pub fn focus_address_bar(&mut self) {
        self.chrome.focus_address = true;
        self.window.request_redraw();
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
            popup_open: egui::Popup::is_any_open(&self.egui.egui_ctx),
        };
        route_pointer(self.capture.captured(), ctx)
    }

    /// Cursor position in physical pixels relative to the page area.
    fn content_point(&self) -> Option<(f32, f32)> {
        let p = self.cursor?;
        Some((p.x as f32, p.y as f32 - self.content_top_px))
    }

    fn enter_content(&mut self, active: Option<TabId>, x: f32, y: f32) {
        if !self.pointer_in_content {
            self.window.set_cursor(self.page_cursor);
        }
        self.pointer_in_content = true;
        if let Some(tab) = active {
            self.host.input(tab, EngineInput::MouseMove { x, y });
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
        let mut commands = Vec::new();
        let mut forward_to_egui = true;

        match event {
            WindowEvent::Resized(size) => {
                self.surface.resize(non_zero(*size));
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.host.set_hidpi(*scale_factor as f32);
                self.window.request_redraw();
            }
            WindowEvent::Moved(_) => {
                self.update_content_origin();
            }
            WindowEvent::RedrawRequested => {
                return self.frame(browser, keymap);
            }
            WindowEvent::CloseRequested => {
                return vec![Command::CloseWindow(self.id)];
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    // Releases of keys and buttons held while focus moved
                    // away arrive elsewhere; do not wait for them here.
                    self.keys.clear();
                    self.capture.clear();
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
                if self.pointer_target() == PointerTarget::Page {
                    if let Some((x, y)) = self.content_point() {
                        self.enter_content(active, x, y);
                    }
                } else {
                    self.leave_content(active);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.leave_content(active);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let current = self.pointer_target();
                let target = match state {
                    ElementState::Pressed => self.capture.press(*button, current),
                    ElementState::Released => self.capture.release(*button, current),
                };
                if target == PointerTarget::Page {
                    forward_to_egui = false;
                    self.surrender_egui_focus();
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        self.host.input(
                            tab,
                            EngineInput::MouseButton {
                                action: input::button_action(*state),
                                button: input::mouse_button(*button),
                                x,
                                y,
                            },
                        );
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.pointer_target() == PointerTarget::Page {
                    forward_to_egui = false;
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        let (dx, dy) = input::wheel_delta(*delta, self.engine_info.wheel_line_px);
                        self.host.input(tab, EngineInput::Wheel { dx, dy, x, y });
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
                                    texture.update(&gl, frame.width, frame.height, frame.bgra);
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
        if registered || self.egui.egui_ctx.has_requested_repaint() {
            self.window.request_redraw();
        }
        // After the frame is on screen: a native menu blocks until it closes.
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
            chrome::MenuRequest::Window { position } => {
                (menus::window_menu(model, keymap), position)
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
