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
use osmena_chrome::input::extension_chords;
use osmena_chrome::routing::{
    ContentRegion, KeyRouter, KeyStroke, KeyTarget, PointerCapture, PointerContext, PointerTarget,
    ViewPixels, content_pixels, content_region, drawn_panel_view, keyboard_view, panel_shown,
    panel_transition, resize_edge, route_pointer, split_content, view_pixels,
};
use osmena_chrome::{icon, input, keyutils, menus, native_menu, theme};
use osmena_core::{
    Brand, Browser, Command, Cursor, EngineInfo, EngineInput, Favicon, KeyInput, Keymap,
    PageContext, Platform, Prompt, TabId, WindowGeometry, WindowId,
};
use osmena_engine::{
    BlitRect, Engine, EngineError, PagePaint, PhysicalSize, ScreenRect, Surface, TabHost,
};
use url::Url;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{DeviceId, ElementState, MouseButton, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{CursorIcon, Fullscreen, ResizeDirection, Window, WindowAttributes};

use crate::page::PageTexture;
use crate::pinch::ZoomSteps;
use crate::repaint::RepaintSchedule;
use crate::tab_drop::torn_off_place;

/// How close to a frameless window's edge (physical pixels at 1x) the
/// pointer resizes it instead of acting on what is under it.
const RESIZE_BORDER_PX: f32 = 6.0;

/// The side panel as the last frame drew it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ShownPanel {
    /// The engine view that renders the panel's page.
    view: TabId,
    /// The view's rectangle, in physical pixels from the window's top
    /// left.
    px: ViewPixels,
}

/// Tabs a drag let go off its strip: see [`BrowserWindow::take_tab_drop`].
pub struct DroppedTabs {
    pub tabs: Vec<TabId>,
    /// Where the pointer was, in physical screen pixels (it may have
    /// been outside the window).
    pub point: PhysicalPosition<f64>,
    /// Where a window made for the tabs goes: the size of the one they
    /// left, placed so they stay under the pointer.
    pub place: Option<WindowGeometry>,
    /// The folder the tabs were dragged as, by its chip.
    pub folder: Option<osmena_core::FolderId>,
}

pub struct BrowserWindow<E: Engine> {
    id: WindowId,
    brand: &'static Brand,
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
    /// True while egui draws the page's rectangle itself (the crash panel,
    /// a block page). The side panel is not part of it.
    content_is_egui: bool,
    /// The side panel drawn in the last frame, if one was: `None` while
    /// the window shows no panel, is full screen, or is too narrow for
    /// one. The view in it is the one the engine was last told to show.
    panel: Option<ShownPanel>,
    /// The side panel view that has the keyboard, after a button press in
    /// it; `None` while the page has it. Cleared by a press in the page,
    /// by any tab becoming the focused tab (`focus_tab`), when the view
    /// closes, and by the first frame that no longer draws that view.
    keyboard_in_panel: Option<TabId>,
    pointer_in_content: bool,
    /// The view the pointer is in, which got its last move: a tab's page
    /// or the side panel's view. It hears the pointer leave.
    hovered: Option<TabId>,
    /// The pointer shape the active page asked for.
    page_cursor: CursorIcon,
    /// The pointer shape the drawn side panel's page asked for.
    panel_cursor: CursorIcon,
    /// Which side holds the pointer during a drag or click. See
    /// [`PointerCapture`].
    capture: PointerCapture,
    /// Which side each held key went to, and which presses are keymap
    /// commands. See [`KeyRouter`].
    keys: KeyRouter,
    /// The press the page got for each key it holds, so the key can be
    /// released there if the real release never arrives.
    page_presses: HashMap<keyboard_types::Code, KeyInput>,
    /// The uploaded frame of each view (a tab, or a side panel's) that handed the shell pixels.
    textures: HashMap<TabId, PageTexture>,
    /// Wheel travel with the primary modifier held (or from a
    /// touchpad's pinch), in pixels, until it adds up to a zoom step.
    zoom_wheel: ZoomSteps,
    /// Magnification from a touchpad's pinch gesture, until it adds up
    /// to a zoom step.
    zoom_pinch: ZoomSteps,
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
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        event_loop: &ActiveEventLoop,
        engine: &E,
        brand: &'static Brand,
        engine_info: &'static EngineInfo,
        id: WindowId,
        title: &str,
        frameless: bool,
        geometry: Option<WindowGeometry>,
    ) -> Result<BrowserWindow<E>, EngineError> {
        let mut attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(1024.0, 768.0))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0))
            .with_window_icon(icon::window_icon(brand));
        // Where the window was last time, when the session says; a
        // position off every monitor is dropped by the caller.
        if let Some(g) = geometry {
            attributes = attributes
                .with_inner_size(PhysicalSize::new(g.width.max(400), g.height.max(300)))
                .with_maximized(g.maximized);
            if g.x != i32::MIN {
                attributes = attributes.with_position(PhysicalPosition::new(g.x, g.y));
            }
        }
        let attributes = frameless_attributes(attributes, frameless);
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| EngineError(format!("creating window: {e}")))?;
        // The title bar macOS keeps over the strip must not take a press
        // on a tab for a move of the window.
        if crate::titlebar::content_refuses_window_moves(frameless, cfg!(target_os = "macos")) {
            #[cfg(target_os = "macos")]
            crate::titlebar::keep_presses(&window);
        }

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
        theme::install(&egui.egui_ctx);
        egui.egui_ctx.options_mut(|options| {
            options.zoom_with_keyboard = false;
        });
        let repaints = RepaintSchedule::install(&egui.egui_ctx);

        Ok(BrowserWindow {
            id,
            brand,
            engine_info,
            chrome: ChromeState::default(),
            zoom_wheel: ZoomSteps::default(),
            zoom_pinch: ZoomSteps::default(),
            modifiers: ModifiersState::default(),
            cursor: None,
            frameless,
            resize_edge: None,
            content_top_px: 0.0,
            content_is_egui: false,
            panel: None,
            keyboard_in_panel: None,
            pointer_in_content: false,
            hovered: None,
            page_cursor: CursorIcon::Default,
            panel_cursor: CursorIcon::Default,
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

    /// The native window, as the parent of a dialog of the system's.
    pub fn native(&self) -> &Window {
        &self.window
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

    /// Bring the native window to the front with the keyboard (a link
    /// another app handed over opened in it).
    pub fn bring_to_front(&self) {
        self.window.focus_window();
    }

    /// Whether a text field of the chrome (the address bar, the launcher,
    /// a prompt) has the keyboard, so an editing command from the menu
    /// bar is its rather than the page's.
    pub fn egui_wants_keyboard(&self) -> bool {
        self.egui.egui_ctx.egui_wants_keyboard_input()
    }

    /// Hand egui input events it did not get from the window (an editing
    /// command from the menu bar, as the key presses it stands for).
    pub fn inject_egui_events(&mut self, events: Vec<egui::Event>) {
        self.egui.egui_winit.egui_input_mut().events.extend(events);
        self.window.request_redraw();
    }

    /// Text typed with no key event of its own (an Alt code's character
    /// on Windows), for whatever has the keyboard: a field of the chrome
    /// as egui's text event, the active page otherwise as committed
    /// text. Nothing while the chrome owns the content area.
    pub fn typed_text(&mut self, browser: &Browser, text: String) {
        if self.egui.egui_ctx.egui_wants_keyboard_input() || self.chrome.takes_every_key() {
            self.inject_egui_events(vec![egui::Event::Text(text)]);
        } else if let Some(view) = self.keyboard_view(browser)
            && !(self.content_is_egui && browser.window(self.id).is_some_and(|w| w.active == view))
        {
            self.host.input(view, EngineInput::ImeCommit(text));
        }
    }

    /// `tab` became the tab this window shows (created, adopted, or
    /// switched to): the keyboard is its page's, so a side panel that had
    /// it gives it up, and the engine is told `tab` has focus if this
    /// window does.
    pub fn focus_tab(&mut self, tab: TabId) {
        self.keyboard_to_page();
        if self.has_focus() {
            self.host.set_focused(tab, true);
        }
    }

    /// Take the keyboard back from the side panel, if it has it, telling
    /// its view so. The caller gives the focus to the active tab.
    fn keyboard_to_page(&mut self) {
        if let Some(view) = self.keyboard_in_panel.take()
            && self.host.has_tab(view)
        {
            self.host.set_focused(view, false);
        }
    }

    /// The side panel's view drawn in the last frame, unless the model has
    /// dropped it since (its page closed): input never goes to a dead view.
    fn drawn_panel(&self, browser: &Browser) -> Option<TabId> {
        drawn_panel_view(self.panel.map(|panel| panel.view), |view| {
            view_is_live(browser, self.id, view)
        })
    }

    /// The view that has the keyboard: the drawn side panel's after a
    /// click in it, else the active tab.
    fn keyboard_view(&self, browser: &Browser) -> Option<TabId> {
        let active = browser.window(self.id).map(|w| w.active);
        keyboard_view(active, self.keyboard_in_panel, self.drawn_panel(browser))
    }

    /// A button press landed on `target`, in `view`: the keyboard goes
    /// where the click went. A press in the chrome moves nothing.
    fn keyboard_follows(
        &mut self,
        target: PointerTarget,
        active: Option<TabId>,
        view: Option<TabId>,
    ) {
        match (target, view) {
            (PointerTarget::Panel, Some(view)) if self.keyboard_in_panel != Some(view) => {
                self.keyboard_to_page();
                if let Some(tab) = active {
                    self.host.set_focused(tab, false);
                }
                self.host.set_focused(view, true);
                self.keyboard_in_panel = Some(view);
            }
            (PointerTarget::Page, _) if self.keyboard_in_panel.is_some() => {
                self.keyboard_to_page();
                if let Some(tab) = active {
                    self.host.set_focused(tab, true);
                }
            }
            _ => {}
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

    /// Close a side panel's view in the engine and drop its texture: its
    /// panel left the model. `active` is the window's active tab, which
    /// gets the keyboard back if the view had it.
    pub fn close_view(&mut self, view: TabId, active: Option<TabId>) {
        if self.keyboard_in_panel == Some(view) {
            self.keyboard_in_panel = None;
            if let Some(tab) = active
                && self.has_focus()
            {
                self.host.set_focused(tab, true);
            }
        }
        if self.hovered == Some(view) {
            self.hovered = None;
        }
        self.host.close_view(view);
        if let Some(texture) = self.textures.remove(&view) {
            let _ = self.surface.make_current();
            texture.destroy(&self.surface.gl(), &mut self.egui.painter);
        }
        // Not drawn any more, so nothing is routed to it before the next
        // frame either.
        if self.panel.is_some_and(|panel| panel.view == view) {
            self.panel = None;
        }
        self.window.request_redraw();
    }

    /// The native window's title (the product, and the profile when it
    /// is not the default one; see `window_title`).
    pub fn set_title(&self, title: &str) {
        self.window.set_title(title);
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

    pub fn focus_find(&mut self) {
        self.chrome.focus_find = true;
        self.window.request_redraw();
    }

    pub fn find(&self, tab: TabId, text: &str, forward: bool, match_case: bool, find_next: bool) {
        self.host.find(tab, text, forward, match_case, find_next);
    }

    pub fn set_zoom(&self, tab: TabId, percent: u16) {
        self.host.set_zoom(tab, percent);
    }

    pub fn toggle_downloads(&mut self) {
        self.chrome.downloads_open = !self.chrome.downloads_open;
        self.window.request_redraw();
    }

    /// Where the window is, for the session: the outer position and the
    /// inner size in physical pixels, and whether it is maximized.
    pub fn geometry(&self) -> Option<WindowGeometry> {
        let position = self.window.outer_position().ok()?;
        let size = self.window.inner_size();
        Some(WindowGeometry {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized: self.window.is_maximized(),
        })
    }

    pub fn start_download(&self, tab: TabId, url: Url) {
        self.host.start_download(tab, url);
    }

    pub fn stop_finding(&self, tab: TabId, clear_selection: bool) {
        self.host.stop_finding(tab, clear_selection);
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

    /// The page in `tab` asked for a pointer shape: the active tab's
    /// page, or the drawn side panel's. Any other view is ignored.
    pub fn set_page_cursor(&mut self, tab: TabId, browser: &Browser, cursor: Cursor) {
        let shape = cursor_icon(cursor);
        if self.panel.is_some_and(|panel| panel.view == tab) {
            self.panel_cursor = shape;
        } else if browser.window(self.id).is_some_and(|w| w.active == tab) {
            self.page_cursor = shape;
        } else {
            return;
        }
        if self.pointer_in_content && self.hovered == Some(tab) {
            self.window.set_cursor(shape);
        }
    }

    /// The tabs a drag that began in this window let go off its strip,
    /// with where. Taken once. `None` if the window's position is not
    /// known.
    pub fn take_tab_drop(&mut self) -> Option<DroppedTabs> {
        let drop = self.chrome.take_tab_drop()?;
        let origin = self.window.inner_position().ok()?;
        let scale = self.window.scale_factor();
        Some(DroppedTabs {
            tabs: drop.tabs,
            folder: drop.folder,
            point: PhysicalPosition::new(
                f64::from(origin.x) + f64::from(drop.pointer.x) * scale,
                f64::from(origin.y) + f64::from(drop.pointer.y) * scale,
            ),
            place: self
                .geometry()
                .map(|source| torn_off_place(source, (drop.origin.x, drop.origin.y), scale)),
        })
    }

    /// Where tabs dropped at `point` (physical screen pixels) would go
    /// among this window's tabs: the gap of its strip under the point,
    /// or `None` when the point is not over the strip.
    pub fn strip_drop_index(&self, point: PhysicalPosition<f64>) -> Option<usize> {
        let origin = self.window.inner_position().ok()?;
        let scale = self.window.scale_factor();
        self.chrome.strip_drop_index(pos2(
            ((point.x - f64::from(origin.x)) / scale) as f32,
            ((point.y - f64::from(origin.y)) / scale) as f32,
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
    /// egui's state. The side panel's header is the chrome's.
    fn pointer_target(&self) -> PointerTarget {
        let region = self.cursor.map(|p| {
            content_region(
                p.x as f32,
                p.y as f32,
                self.content_top_px,
                self.panel.map(|panel| panel.px),
            )
        });
        let ctx = PointerContext {
            over_chrome: matches!(
                region,
                None | Some(ContentRegion::Chrome | ContentRegion::PanelHeader)
            ),
            over_panel: region == Some(ContentRegion::Panel),
            content_is_egui: self.content_is_egui,
            over_egui: self.egui.egui_ctx.is_pointer_over_egui(),
            popup_open: egui::Popup::is_any_open(&self.egui.egui_ctx) || self.chrome.popup_open(),
        };
        route_pointer(self.capture.captured(), ctx)
    }

    /// The view a pointer event for `side` goes to: the view holding the
    /// capture while one is held (even if another tab is active or
    /// another panel shown now), otherwise the active tab for the page
    /// and the drawn panel's view for the panel, unless the model has
    /// dropped that view. Nothing while the capturing view is gone: its
    /// release ends the capture without reaching anyone.
    fn pointer_view(
        &self,
        browser: &Browser,
        active: Option<TabId>,
        side: PointerTarget,
    ) -> Option<TabId> {
        match self.capture.tab() {
            Some(view) => view_is_live(browser, self.id, view).then_some(view),
            None => match side {
                PointerTarget::Page => active,
                PointerTarget::Panel => self.drawn_panel(browser),
                PointerTarget::Egui => None,
            },
        }
    }

    /// Cursor position in physical pixels as the view on `side` wants
    /// it: from the page area's top left, or from the panel view's own.
    /// A panel capture that outlived the drawn panel (its tab went into
    /// the background mid-drag) is given the view's corner, as a release
    /// on focus loss is.
    fn view_point(&self, side: PointerTarget) -> Option<(f32, f32)> {
        let p = self.cursor?;
        let (x, y) = (p.x as f32, p.y as f32);
        Some(match (side, self.panel) {
            (PointerTarget::Panel, Some(panel)) => panel.px.local(x, y),
            (PointerTarget::Panel, None) => (0.0, 0.0),
            _ => (x, y - self.content_top_px),
        })
    }

    /// The keyboard modifiers held now, for pointer events. winit keeps
    /// them from `ModifiersChanged`, which it also sends when the window
    /// loses focus (all up) and regains it (the real state), so they
    /// never go stale the way a modifier inferred from key events does.
    fn modifiers(&self) -> Modifiers {
        keyutils::keyboard_modifiers_from_winit_modifiers(self.modifiers)
    }

    /// The pointer is over `view` (a tab's page or the panel's) at
    /// `(x, y)` in that view's own pixels.
    fn enter_content(&mut self, view: Option<TabId>, x: f32, y: f32) {
        if self.pointer_in_content && self.hovered != view {
            // From one view straight into another (across the boundary,
            // or the tab under a resting pointer changed): the one left
            // behind hears the pointer go.
            self.leave_content();
        }
        if !self.pointer_in_content {
            let shape = if view.is_some() && self.panel.map(|panel| panel.view) == view {
                self.panel_cursor
            } else {
                self.page_cursor
            };
            self.window.set_cursor(shape);
        }
        self.pointer_in_content = true;
        self.hovered = view;
        if let Some(view) = view {
            let modifiers = self.modifiers();
            self.host
                .input(view, EngineInput::MouseMove { x, y, modifiers });
        }
    }

    /// The pointer left the view it was in, which is told if the host
    /// still has it.
    fn leave_content(&mut self) {
        if self.pointer_in_content {
            self.pointer_in_content = false;
            self.window.set_cursor(CursorIcon::Default);
            if let Some(view) = self.hovered.take()
                && self.host.has_tab(view)
            {
                self.host.input(view, EngineInput::MouseLeft);
            }
        }
    }

    /// Handle one winit event. Returns the commands the user issued.
    pub fn handle_event(
        &mut self,
        browser: &Browser,
        keymap: &Keymap,
        site_icons: &HashMap<String, Favicon>,
        event: &WindowEvent,
    ) -> Vec<Command> {
        let active = browser.window(self.id).map(|w| w.active);
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
                return self.frame(browser, keymap, site_icons);
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
                if let Some(view) = self.keyboard_view(browser) {
                    self.host.set_focused(view, *focused);
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
                        self.leave_content();
                        self.window.set_cursor(resize_cursor(edge));
                    } else {
                        self.window.set_cursor(CursorIcon::Default);
                    }
                }
                if edge.is_none() {
                    let target = self.pointer_target();
                    match self.view_point(target) {
                        Some((x, y)) if target != PointerTarget::Egui => {
                            let view = self.pointer_view(browser, active, target);
                            self.enter_content(view, x, y);
                        }
                        _ => self.leave_content(),
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.leave_content();
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
                // The side this event belongs to is the capture's while
                // one is held; its view and the point in it are read
                // before a release ends the capture.
                let side = self.capture.captured().unwrap_or(current);
                let view = self.pointer_view(browser, active, side);
                let point = self.view_point(side);
                let target = match state {
                    ElementState::Pressed => self.capture.press(*button, current, view),
                    ElementState::Released => self.capture.release(*button, current),
                };
                if target != PointerTarget::Egui {
                    forward_to_egui = false;
                    self.surrender_egui_focus();
                    if *state == ElementState::Pressed {
                        self.keyboard_follows(target, active, view);
                    }
                    if let (Some(view), Some((x, y))) = (view, point) {
                        self.host.input(
                            view,
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
            // Two fingers spread or pinched on a macOS touchpad: the
            // page under the pointer zooms a step for every
            // `PINCH_STEP` of magnification. Never egui's: it would
            // scale the whole chrome.
            WindowEvent::PinchGesture { delta, phase, .. } => {
                forward_to_egui = false;
                if matches!(phase, TouchPhase::Started) {
                    self.zoom_pinch.reset();
                }
                let target = self.pointer_target();
                if target == PointerTarget::Page
                    && let Some(tab) = self.pointer_view(browser, active, target)
                {
                    let steps = self.zoom_pinch.add(*delta, crate::pinch::PINCH_STEP);
                    push_zoom(&mut commands, tab, steps);
                }
                if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                    self.zoom_pinch.reset();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let target = self.pointer_target();
                if target != PointerTarget::Egui {
                    forward_to_egui = false;
                    if let (Some(tab), Some((x, y))) = (
                        self.pointer_view(browser, active, target),
                        self.view_point(target),
                    ) {
                        let (dx, dy) = input::wheel_delta(*delta, self.engine_info.wheel_line_px);
                        let modifiers = self.modifiers();
                        // Ctrl (Cmd) and the wheel zoom the page a step per
                        // notch instead of scrolling it. A side panel has no
                        // zoom of its own: there the wheel scrolls. A pinch
                        // on a Windows touchpad is the wheel with Ctrl in
                        // the message alone, the key not held (pinch.rs).
                        let primary = if cfg!(target_os = "macos") {
                            Modifiers::META
                        } else {
                            Modifiers::CONTROL
                        };
                        let zooming =
                            modifiers.contains(primary) || crate::pinch::wheel_has_control();
                        if target == PointerTarget::Page && zooming {
                            let notch = f64::from(self.engine_info.wheel_line_px).max(1.0);
                            push_zoom(&mut commands, tab, self.zoom_wheel.add(dy, notch));
                            return commands;
                        }
                        self.zoom_wheel.reset();
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
                // Whatever the chrome has open takes every key while it is
                // open (Escape closes it), focused or not: the downloads
                // popover, the Launch Panel and a prompt. A prompt of buttons
                // has no field to hold the focus, so without this its Escape
                // would go to the page.
                let egui_wants_keyboard =
                    self.egui.egui_ctx.egui_wants_keyboard_input() || self.chrome.takes_every_key();
                let stroke = KeyStroke {
                    code: key_input.code,
                    modifiers: key_input.modifiers,
                    pressed: key.state == ElementState::Pressed,
                    repeat: key.repeat,
                };
                let live = |view| view_is_live(browser, self.id, view);
                let chords = extension_chords(&browser.extensions);
                let keyboard = self.keyboard_view(browser);
                let target =
                    self.keys
                        .route(stroke, egui_wants_keyboard, keyboard, live, keymap, &chords);
                match target {
                    Some(KeyTarget::Page(tab)) => {
                        forward_to_egui = false;
                        // The chrome owns the content while it shows a block
                        // page or the crash panel, so no new press reaches
                        // the page then. A key the page already holds still
                        // gets its release, or the page would keep it down;
                        // a release for a key the page never saw is dropped.
                        let chrome_owns_content = self.content_is_egui && active == Some(tab);
                        if stroke.pressed {
                            if !chrome_owns_content {
                                self.page_presses.insert(stroke.code, key_input.clone());
                                self.host.input(tab, EngineInput::Key(key_input));
                            }
                        } else if self.page_presses.remove(&stroke.code).is_some() {
                            self.host.input(tab, EngineInput::Key(key_input));
                        }
                    }
                    Some(KeyTarget::Command(kind)) => {
                        forward_to_egui = false;
                        if let Some(tab) = active {
                            commands.push(kind.into_command(self.id, tab));
                        }
                    }
                    Some(KeyTarget::ExtensionCommand { extension, name }) => {
                        forward_to_egui = false;
                        commands.push(Command::ExtensionCommand {
                            window: self.id,
                            extension,
                            name,
                        });
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
                if let (Some(view), Some(input)) = (self.keyboard_view(browser), input::ime(ime)) {
                    self.host.input(view, input);
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
    /// now, each in the view that got its press, with the modifiers as
    /// the window knows them (all up, on Windows, by this point). A page
    /// that tracks key or button state is then never stuck with one
    /// down; egui forgets its own on the same event.
    fn release_held(&mut self, browser: &Browser) {
        let live = |view: TabId| view_is_live(browser, self.id, view);
        let modifiers = self.modifiers();
        for (code, view) in self.keys.release_all() {
            if let Some(press) = self.page_presses.remove(&code)
                && live(view)
            {
                let release = KeyInput {
                    state: KeyState::Up,
                    modifiers,
                    repeat: false,
                    ..press
                };
                self.host.input(view, EngineInput::Key(release));
            }
        }
        self.page_presses.clear();
        // The side is read before the capture is let go of: it says whose
        // pixels the release is in.
        let side = self.capture.captured();
        if let Some((view, buttons)) = self.capture.release_all()
            && live(view)
        {
            let (x, y) = side
                .and_then(|side| self.view_point(side))
                .unwrap_or((0.0, 0.0));
            for button in buttons {
                self.host.input(
                    view,
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

    /// Tell the engine where the page area, and the side panel's view
    /// when one is drawn, sit on screen (logical pixels), for engines
    /// that place menus and widgets themselves.
    fn update_content_origin(&mut self) {
        let Ok(origin) = self.window.inner_position() else {
            return;
        };
        let scale = self.window.scale_factor();
        let logical = |px: f64| (px / scale).round() as i32;
        self.host.set_content_origin(
            logical(f64::from(origin.x)),
            logical(f64::from(origin.y) + f64::from(self.content_top_px)),
        );
        if let Some(panel) = self.panel {
            self.host.set_view_origin(
                panel.view,
                logical(f64::from(origin.x) + f64::from(panel.px.left)),
                logical(f64::from(origin.y) + f64::from(panel.px.top)),
            );
        }
    }

    /// Drop the textures of tabs that left this window and of side panel
    /// views that closed: the host answers `has_tab` for both.
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

    /// Draw the chrome, the active tab under it and the side panel beside it, and present.
    fn frame(
        &mut self,
        browser: &Browser,
        keymap: &Keymap,
        site_icons: &HashMap<String, Favicon>,
    ) -> Vec<Command> {
        let Some(model) = browser.window(self.id) else {
            return Vec::new();
        };
        let active = model.active;
        let crashed = model.active_tab().crashed.clone();
        let blocked = model.active_tab().blocked.clone();
        let engine_error = browser.engine_error.as_deref();
        let chrome_color = browser.settings.chrome_color;
        let profile_name = browser.profile_name();
        let avatar = browser.settings.profile.avatar;
        let brand = self.brand;
        let engine_info = self.engine_info;
        let frameless = self.frameless;
        let maximized = self.window.is_maximized();
        let pinned = browser.pinned_extensions();
        let history = &browser.history;
        let mut commands = Vec::new();
        let mut content_top_px = 0.0;
        // The panel's view, when one is to be drawn: never for an engine
        // without side panels, never in full screen. `panel_shown` is the
        // one predicate; `shown` is `Some` exactly when it is true.
        let shown = model
            .shown_panel()
            .filter(|_| panel_shown(model, engine_info.extensions.as_ref()))
            .map(|panel| panel.view);
        let previous = self.panel.map(|panel| panel.view);
        let mut panel_now: Option<ShownPanel> = None;

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
                brand,
                profile_dir: &browser.profile.name,
                profile_name: &profile_name,
                avatar,
                engine: engine_info,
                engine_error,
                frameless,
                maximized,
                pinned: &pinned,
                history,
                update: &browser.update,
                bookmarks: &browser.bookmarks,
                show_bookmarks_bar: browser.settings.show_bookmarks_bar,
                site_icons,
                page_zoom: browser.settings.page_zoom,
                downloads: &browser.downloads,
                restore_offer: browser.crashed_session.is_some(),
                default_browser_offer: browser.offers_default_browser(),
                launch: &browser.launch,
                launch_settings: &browser.settings.launch,
                search: &browser.settings.search,
            };
            commands = chrome::draw(ui, model, &context, chrome_state, keymap);
            let ppp = ui.pixels_per_point();
            let split = split_content(ui.available_rect_before_wrap(), shown.is_some());
            let rect = split.page;
            let (top, size) = content_pixels(rect, ppp);
            content_top_px = top;
            host.resize_content(active, size);
            // The panel, when the window is wide enough for one. The
            // engine hears of a view that came or went before it is
            // sized and painted.
            let panel = split.panel.zip(shown);
            let transition = panel_transition(previous, panel.map(|(_, view)| view));
            if let Some(view) = transition.hide
                && host.has_tab(view)
            {
                host.hide(view);
            }
            if let Some(view) = transition.show {
                host.show(view);
            }
            if let Some((rects, view)) = panel {
                let px = view_pixels(rects.view, ppp);
                host.resize_content(view, px.size);
                panel_now = Some(ShownPanel { view, px });
            }
            match (&crashed, &blocked) {
                (Some(reason), _) => {
                    if chrome::draw_crash_panel(ui, rect, reason) {
                        commands.push(Command::Reload(active));
                    }
                }
                // The site is blocked: the engine shows a blank page
                // behind, the chrome says why. Nothing on it is a
                // control.
                (None, Some(page)) => {
                    chrome::draw_blocked_page(ui, rect, chrome_color, page);
                }
                (None, None) => paint_view(ui, &*host, textures, &gl, active, rect),
            }
            // The panel's view, after the page, and its header after the
            // view: what is drawn over the view's edge lies on top.
            if let Some((rects, view)) = panel {
                paint_view(ui, &*host, textures, &gl, view, rects.view);
            }
            // The header over the panel's view, drawn after the view so
            // its hairline lies over the view's first pixel column. No
            // rectangles, no header: full screen, an engine without
            // side panels, a window too narrow for both.
            if let (Some(rects), Some(panel)) = (&split.panel, model.shown_panel()) {
                commands.extend(chrome::draw_side_panel_header(
                    ui,
                    chrome_state,
                    rects,
                    chrome_color,
                    model.id,
                    panel,
                    &browser.extensions,
                ));
            }
        });
        self.content_top_px = content_top_px;
        self.content_is_egui = crashed.is_some() || blocked.is_some();
        let drawn = panel_now.map(|panel| panel.view);
        if self.keyboard_in_panel.is_some() && self.keyboard_in_panel != drawn {
            // The panel that had the keyboard is not drawn any more
            // (closed by its extension, full screen, a narrower window):
            // the page has it again.
            self.keyboard_to_page();
            if self.has_focus() {
                self.host.set_focused(active, true);
            }
        }
        if drawn != previous {
            self.panel_cursor = CursorIcon::Default;
        }
        self.panel = panel_now;
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
                    self.engine_info.find,
                    self.engine_info.downloads,
                    menus::BookmarksMenu {
                        bookmarks: &browser.bookmarks,
                        bar_shown: browser.settings.show_bookmarks_bar,
                    },
                    menus::UpdateMenu {
                        brand: self.brand.name,
                        status: &browser.update,
                        can_update: self.brand.can_update(),
                    },
                ),
                position,
            ),
            chrome::MenuRequest::Folder { folder, position } => {
                (menus::folder_menu(model, folder), position)
            }
            chrome::MenuRequest::Bookmark { url, position } => {
                (menus::bookmark_menu(self.id, &url), position)
            }
            chrome::MenuRequest::BookmarkGroup { group, position } => {
                let group = browser.bookmarks.group(&group)?;
                (menus::bookmark_group_menu(self.id, group), position)
            }
            chrome::MenuRequest::BookmarkOverflow { from, position } => {
                let bar = browser.bookmarks.bar()?;
                let rest = bar.bookmarks.get(from..).unwrap_or(&[]);
                (menus::bookmark_overflow_menu(model.active, rest), position)
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
                (
                    menus::tab_menu(
                        model,
                        &targets,
                        &others,
                        &browser.profiles,
                        &browser.blocked,
                    ),
                    position,
                )
            }
            chrome::MenuRequest::Profiles { position } => {
                let name = browser.profile_name();
                (
                    menus::profiles_menu(
                        self.brand.name,
                        Some(self.id),
                        menus::CurrentProfile {
                            dir: &browser.profile.name,
                            name: &name,
                            guest: browser.profile.guest,
                        },
                        &browser.profiles,
                    ),
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

/// Whether `view` can still be sent input in `window`: a tab of the
/// window, or the view of one of its side panels. Held keys, a pointer
/// capture and the hover all name a view and are dropped once this is
/// false.
/// `steps` zoom steps for `tab`: in for a positive count, out for a
/// negative one.
fn push_zoom(commands: &mut Vec<Command>, tab: TabId, steps: i32) {
    for _ in 0..steps.unsigned_abs() {
        commands.push(if steps > 0 {
            Command::ZoomIn(tab)
        } else {
            Command::ZoomOut(tab)
        });
    }
}

fn view_is_live(browser: &Browser, window: WindowId, view: TabId) -> bool {
    browser
        .window(window)
        .is_some_and(|w| w.tab(view).is_some() || w.has_panel_view(view))
}

/// Paint `view` (a tab's page, or a side panel's) into `rect`, whichever
/// way the engine hands it over: a blit from its own framebuffer, or a
/// frame of pixels kept in `textures`. White while the engine has nothing
/// for it yet. Called inside the UI pass, after `resize_content` for the
/// same view.
fn paint_view<H: TabHost>(
    ui: &egui::Ui,
    host: &H,
    textures: &mut HashMap<TabId, PageTexture>,
    gl: &glow::Context,
    view: TabId,
    rect: EguiRect,
) {
    let mut painted = false;
    host.paint_page(view, &mut |paint| match paint {
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
            if let Some(texture) = textures.get_mut(&view) {
                if frame.fresh || texture.size() != (frame.width, frame.height) {
                    texture.update(gl, frame.width, frame.height, frame.bgra, frame.dirty);
                }
            } else if let Some(texture) =
                PageTexture::new(gl, frame.width, frame.height, frame.bgra)
            {
                textures.insert(view, texture);
            }
            // Registered with egui after this pass; until
            // then the view stays blank for one frame.
            let Some(texture) = textures.get(&view) else {
                return;
            };
            let Some(id) = texture.id() else {
                return;
            };
            let (width, height) = texture.size();
            // Paint the frame at its own pixel size from the
            // rectangle's top-left, so a frame from before a
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
