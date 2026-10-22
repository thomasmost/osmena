//! The window chrome: tab strip, toolbar, status overlay, and crash panel.
//! Pure functions of the model that return the commands the user issued.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

use egui::{
    Align2, Button, Color32, ColorImage, CornerRadius, Frame, Id, Key, Margin, Panel, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, TextStyle, TextureHandle, TextureOptions, Ui, Vec2,
};
use keyboard_types::Code;
use osmena_core::{
    ChromeColor, Command, CommandKind, EngineInfo, Extension, Favicon, FolderId, FolderTarget,
    KeyChord, Keymap, LoadState, Prompt, StripItem, Tab, TabId, Window,
};
use url::{ParseError, Url};

/// The widest a title is drawn in a folder's drop-down list.
const FOLDER_LIST_TITLE_MAX_CHARS: usize = 36;
const FAVICON_SIZE: f32 = 16.0;
/// Tab geometry, in points.
const TAB_HEIGHT: f32 = 32.0;
/// Every tab in the strip is `TAB_WIDTH` wide until the strip runs out
/// of room; then all of them shrink alike, down to `TAB_MIN_WIDTH`,
/// after which the strip scrolls ([`tab_width_for`]).
const TAB_WIDTH: f32 = 200.0;
const TAB_MIN_WIDTH: f32 = 64.0;
/// Narrower than this, an inactive tab shows its close control only
/// while hovered, so its title keeps the room.
const TAB_CLOSE_MIN_WIDTH: f32 = 110.0;
const TAB_PADDING_X: f32 = 12.0;
const TAB_CORNER: u8 = 8;
const CLOSE_SIZE: f32 = 18.0;
/// The "+" and "☰" buttons on the strip.
const MENU_WIDTH: f32 = 28.0;
/// One window control (minimize, maximize, close) on a frameless window.
const CONTROL_WIDTH: f32 = 40.0;
/// Room for macOS's traffic lights at the left of a frameless strip.
const TRAFFIC_LIGHTS_INSET: f32 = 72.0;
/// A toolbar button for an extension's action, and the icon inside it.
const ACTION_SIZE: f32 = 26.0;
const ACTION_ICON: f32 = 16.0;
/// The widest a folder's name is drawn in the strip.
const FOLDER_NAME_MAX_CHARS: usize = 18;

/// The shades the chrome paints with, all derived from the profile's
/// [`ChromeColor`]: the tab strip is the swatch at its top and fades to
/// the toolbar's fill at its bottom edge ([`Palette::strip_at`]), tabs
/// are veils of white (or black, on a light swatch) over it, and the
/// active tab shares the toolbar's fill, which is the swatch washed most
/// of the way to the page background, so it reads as attached to the
/// page with no edge between strip and toolbar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub strip: Color32,
    pub toolbar: Color32,
    pub text_on_strip: Color32,
    pub tab_idle: Color32,
    pub tab_hover: Color32,
    pub tab_selected: Color32,
    pub tab_outline: Color32,
}

impl Palette {
    pub fn new(color: ChromeColor, dark_mode: bool) -> Palette {
        let [r, g, b] = color.rgb();
        let strip = Color32::from_rgb(r, g, b);
        let page = if dark_mode {
            Color32::from_gray(32)
        } else {
            Color32::WHITE
        };
        let light_strip = luminance(strip) > 0.55;
        let veil = if light_strip {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        let veil_alpha = |a: u8| Color32::from_rgba_unmultiplied(veil.r(), veil.g(), veil.b(), a);
        Palette {
            strip,
            toolbar: blend(strip, page, 0.84),
            text_on_strip: if light_strip {
                Color32::from_gray(24)
            } else {
                Color32::WHITE
            },
            tab_idle: veil_alpha(26),
            tab_hover: veil_alpha(56),
            tab_selected: veil_alpha(96),
            tab_outline: veil_alpha(180),
        }
    }

    /// The strip's fill `t` of the way down it (0 at the top, 1 at the
    /// bottom edge): the swatch, easing into the toolbar's fill so the
    /// two meet without a line. The ease is quadratic: the top half
    /// stays close to the swatch, which keeps the tab titles legible on
    /// it, and the fade happens under the tabs' lower half.
    pub fn strip_at(&self, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        blend(self.strip, self.toolbar, t * t)
    }
}

/// The strip's background: `rect` filled with [`Palette::strip_at`] from
/// top to bottom, as a mesh of horizontal bands whose vertex colours
/// egui interpolates (the bands keep the quadratic ease from flattening
/// into one linear ramp).
fn strip_background(rect: Rect, palette: &Palette) -> egui::Shape {
    const BANDS: u32 = 8;
    let mut mesh = egui::Mesh::default();
    for band in 0..BANDS {
        let t0 = band as f32 / BANDS as f32;
        let t1 = (band + 1) as f32 / BANDS as f32;
        let y0 = rect.top() + rect.height() * t0;
        let y1 = rect.top() + rect.height() * t1;
        let (c0, c1) = (palette.strip_at(t0), palette.strip_at(t1));
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(rect.left(), y0), c0);
        mesh.colored_vertex(Pos2::new(rect.right(), y0), c0);
        mesh.colored_vertex(Pos2::new(rect.right(), y1), c1);
        mesh.colored_vertex(Pos2::new(rect.left(), y1), c1);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    egui::Shape::mesh(mesh)
}

/// `a` moved `t` of the way to `b`, per channel.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Relative luminance in 0..1, from gamma-space channels (close enough to
/// pick a text colour).
fn luminance(c: Color32) -> f32 {
    (0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b())) / 255.0
}

/// Per-window chrome state that is not part of the browser model: the
/// address bar text the user may be editing, and favicon textures.
#[derive(Default)]
pub struct ChromeState {
    pub address_text: String,
    /// True while the user has edited the address bar since the last
    /// navigation; engine URL changes then leave the text alone.
    pub address_dirty: bool,
    /// Set by `Command::FocusAddressBar`; consumed by the next frame.
    pub focus_address: bool,
    shown_tab: Option<TabId>,
    shown_url: Option<Url>,
    favicons: HashMap<TabId, CachedFavicon>,
    /// The uploaded icons of the pinned extensions, by extension id.
    extension_icons: HashMap<String, CachedFavicon>,
    /// Where each pinned extension's toolbar button was in the last
    /// frame, so the window can anchor the action's popup to it.
    action_anchors: HashMap<String, Rect>,
    /// Where the extensions menu button was in the last frame: the anchor
    /// for the popup of an action opened from the menu.
    menu_anchor: Option<Rect>,
    /// Set when a drag of tabs ends; the window takes it and works out
    /// where the tabs were dropped.
    tab_drop: Option<TabDrop>,
    /// Set when the user asks for a menu; the window shows it as a native
    /// popup (`native_menu`) once the frame is painted.
    menu_request: Option<MenuRequest>,
    /// Set when the user used the strip as a title bar; the window acts
    /// on it once the frame is painted.
    window_request: Option<WindowRequest>,
    /// The folder whose tab list is dropped down under its chip, if any.
    open_folder: Option<FolderId>,
    /// The question being asked over the page, if any.
    prompt: Option<PromptState>,
}

/// A [`Prompt`] being answered: the typed text, and which existing
/// folder or group was picked instead (`None` for a new one).
struct PromptState {
    prompt: Prompt,
    text: String,
    existing: Option<usize>,
    /// The text field takes focus on the first frame.
    focus: bool,
}

impl ChromeState {
    /// The tab drag that ended since the last call, if any.
    pub fn take_tab_drop(&mut self) -> Option<TabDrop> {
        self.tab_drop.take()
    }

    /// The menu the user asked for since the last call, if any.
    pub fn take_menu_request(&mut self) -> Option<MenuRequest> {
        self.menu_request.take()
    }

    /// What the user asked of the native window since the last call.
    pub fn take_window_request(&mut self) -> Option<WindowRequest> {
        self.window_request.take()
    }

    /// The toolbar button of `extension`'s action, as last drawn (egui
    /// points of this window), if it is pinned.
    pub fn action_anchor(&self, extension: &str) -> Option<Rect> {
        self.action_anchors.get(extension).copied()
    }

    /// The extensions menu button, as last drawn, if the engine has
    /// extensions.
    pub fn extensions_menu_anchor(&self) -> Option<Rect> {
        self.menu_anchor
    }

    /// Ask the user `prompt`; the answer comes back as a command from
    /// [`draw`]. A prompt already open is replaced.
    pub fn open_prompt(&mut self, prompt: Prompt) {
        let text = match &prompt {
            Prompt::MoveToFolder { suggested, .. } => suggested.clone(),
            Prompt::BookmarkTabs { .. } => String::new(),
            Prompt::RenameFolder { name, .. } => name.clone(),
        };
        self.prompt = Some(PromptState {
            prompt,
            text,
            existing: None,
            focus: true,
        });
        self.open_folder = None;
    }

    pub fn has_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// Something the chrome drew over the page wants every click: a
    /// folder's drop-down list (a click anywhere else closes it, which
    /// it can only see if egui gets the click) or a prompt (modal: the
    /// page waits for the answer). egui's own popups report themselves
    /// through `egui::Popup::is_any_open`; this covers the rest.
    pub fn popup_open(&self) -> bool {
        self.open_folder.is_some() || self.prompt.is_some()
    }
}

/// A menu the chrome wants shown, with where to put it (egui points of
/// this window).
#[derive(Clone, Debug, PartialEq)]
pub enum MenuRequest {
    /// The "☰" window menu, below its button.
    Window { position: Pos2 },
    /// The extensions menu, below the puzzle-piece button.
    Extensions { position: Pos2 },
    /// A tab's context menu, acting on `targets`, at the pointer.
    Tab { targets: Vec<TabId>, position: Pos2 },
    /// A folder's context menu, at the pointer.
    Folder { folder: FolderId, position: Pos2 },
}

/// A drag of tabs that ended: the tabs to move and where the pointer was
/// when the button was released, in egui points of this window (which may
/// be outside the window while the drag held the pointer).
#[derive(Clone, Debug, PartialEq)]
pub struct TabDrop {
    pub tabs: Vec<TabId>,
    pub pointer: Pos2,
}

/// An uploaded favicon texture. The `Arc` is a clone of the model's pixels:
/// holding it keeps that allocation alive, so pointer equality with a new
/// favicon's pixels means the same icon, never a reused address.
struct CachedFavicon {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
    texture: TextureHandle,
}

/// What the chrome needs besides the window it draws.
#[derive(Clone, Copy)]
pub struct ChromeContext<'a> {
    /// The profile's chrome colour.
    pub color: ChromeColor,
    /// The engine, for what the chrome may promise (Stop).
    pub engine: &'a EngineInfo,
    /// The browser-wide engine error to show in the status area, if any.
    pub engine_error: Option<&'a str>,
    /// The window has no system title bar, so the tab strip stands in for
    /// it: its empty space drags the window and, except on macOS, it ends
    /// in the minimize, maximize, and close controls.
    pub frameless: bool,
    /// The window is maximized (the maximize control shows "restore").
    pub maximized: bool,
    /// The extensions whose actions sit in the toolbar, in order
    /// (`Browser::pinned_extensions`). Only drawn when the engine runs
    /// extensions (`EngineInfo::extensions`).
    pub pinned: &'a [&'a Extension],
}

/// Something the chrome asks of the native window, recorded while drawing
/// and taken by the window after the frame (see
/// [`ChromeState::take_window_request`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowRequest {
    /// Start moving the window with the pointer (the title bar drag).
    Drag,
    Minimize,
    /// Maximize, or restore if already maximized.
    ToggleMaximize,
    /// Show the system's window menu at this position (egui points).
    SystemMenu {
        position: Pos2,
    },
}

/// Draw the chrome for `window` and return the commands it produced.
pub fn draw(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    state: &mut ChromeState,
    keymap: &Keymap,
) -> Vec<Command> {
    let ChromeContext {
        color,
        engine,
        engine_error,
        frameless,
        maximized,
        pinned,
    } = *context;
    let mut commands = Vec::new();
    let active = window.active_tab();
    sync_address_text(state, active.id, &active.url);
    state
        .favicons
        .retain(|tab, _| window.index_of(*tab).is_some());
    state
        .extension_icons
        .retain(|id, _| pinned.iter().any(|e| e.id == *id));
    state.action_anchors.clear();
    let palette = Palette::new(color, ui.visuals().dark_mode);

    // Full screen shows the page alone; every shortcut still works.
    if window.fullscreen {
        return commands;
    }

    // The strip's fill is painted behind the panel once its rect is
    // known: a gradient, which a `Frame` cannot fill with.
    let strip_background_slot = ui.painter().add(egui::Shape::Noop);
    let strip_frame = Frame::new()
        .fill(Color32::TRANSPARENT)
        .inner_margin(Margin {
            left: 8,
            right: 8,
            top: 6,
            bottom: 0,
        });
    let strip = Panel::top("tabs").frame(strip_frame).show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if frameless && cfg!(target_os = "macos") {
                // The system's traffic lights sit over the strip's left end.
                ui.add_space(TRAFFIC_LIGHTS_INSET);
            }
            let on_strip = |text: &str, size: f32| {
                Button::new(RichText::new(text).color(palette.text_on_strip).size(size))
                    .frame(false)
                    .min_size(Vec2::new(MENU_WIDTH, TAB_HEIGHT))
            };
            // The strip's right end holds the menu button and, on a
            // frameless window (except on macOS, where the system keeps
            // its own), the window controls. The tabs scroll within what
            // is left of them, and the gap between is the title bar's
            // drag area.
            let controls = frameless && !cfg!(target_os = "macos");
            let right_width = MENU_WIDTH
                + if controls { 3.0 * CONTROL_WIDTH } else { 0.0 }
                + 2.0 * ui.spacing().item_spacing.x;
            let tabs_width = (ui.available_width() - right_width).max(0.0);
            // What is left for the tabs after the "+" button, shared
            // out evenly. A folder's chip takes its own width from the
            // same room, so a strip with folders may scroll a little
            // sooner than the count alone predicts.
            let items = window.strip_items();
            let tab_count = items
                .iter()
                .map(|item| match item {
                    StripItem::Tab(_) => 1,
                    StripItem::Folder { folder, tabs } => {
                        if window.folder(*folder).is_some_and(|f| f.expanded) {
                            tabs.len()
                        } else {
                            0
                        }
                    }
                })
                .sum();
            let tab_spacing = 2.0;
            let strip_tabs = StripTabs {
                palette: &palette,
                width: tab_width_for(
                    tabs_width - (4.0 + MENU_WIDTH + tab_spacing),
                    tab_count,
                    tab_spacing,
                ),
            };
            egui::ScrollArea::horizontal()
                .max_width(tabs_width)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = tab_spacing;
                        // How many tabs a drag in progress carries, for the overlay.
                        let mut dragging: Option<usize> = None;
                        for item in items {
                            match item {
                                StripItem::Tab(id) => {
                                    if let Some(tab) = window.tab(id) {
                                        strip_tab(
                                            ui,
                                            window,
                                            tab,
                                            &strip_tabs,
                                            state,
                                            &mut commands,
                                            &mut dragging,
                                        );
                                    }
                                }
                                StripItem::Folder { folder, tabs } => {
                                    strip_folder(
                                        ui,
                                        window,
                                        folder,
                                        &tabs,
                                        &palette,
                                        state,
                                        &mut commands,
                                    );
                                    if window.folder(folder).is_some_and(|f| f.expanded) {
                                        for id in tabs {
                                            if let Some(tab) = window.tab(id) {
                                                strip_tab(
                                                    ui,
                                                    window,
                                                    tab,
                                                    &strip_tabs,
                                                    state,
                                                    &mut commands,
                                                    &mut dragging,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let (Some(count), Some(pointer)) =
                            (dragging, ui.ctx().pointer_latest_pos())
                        {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            egui::Area::new(Id::new("tab-drag-overlay"))
                                .order(egui::Order::Tooltip)
                                .fixed_pos(pointer + Vec2::new(12.0, 12.0))
                                .interactable(false)
                                .show(ui.ctx(), |ui| {
                                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                                        ui.label(if count == 1 {
                                            "Move 1 tab".to_string()
                                        } else {
                                            format!("Move {count} tabs")
                                        });
                                    });
                                });
                        }
                        ui.add_space(4.0);
                        if ui
                            .add(on_strip("+", 20.0))
                            .on_hover_text(hint(keymap, CommandKind::NewTab, "New tab"))
                            .clicked()
                        {
                            commands.push(Command::NewTab {
                                window: window.id,
                                url: None,
                            });
                        }
                    });
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if controls {
                    if control_button(ui, &palette, ControlGlyph::Close)
                        .on_hover_text(hint(keymap, CommandKind::CloseWindow, "Close window"))
                        .clicked()
                    {
                        commands.push(Command::CloseWindow(window.id));
                    }
                    let (glyph, label) = if maximized {
                        (ControlGlyph::Restore, "Restore")
                    } else {
                        (ControlGlyph::Maximize, "Maximize")
                    };
                    if control_button(ui, &palette, glyph)
                        .on_hover_text(label)
                        .clicked()
                    {
                        state.window_request = Some(WindowRequest::ToggleMaximize);
                    }
                    if control_button(ui, &palette, ControlGlyph::Minimize)
                        .on_hover_text("Minimize")
                        .clicked()
                    {
                        state.window_request = Some(WindowRequest::Minimize);
                    }
                }
                let menu = ui.add(on_strip("☰", 16.0)).on_hover_text("Menu");
                if menu.clicked() {
                    state.menu_request = Some(MenuRequest::Window {
                        position: menu.rect.left_bottom(),
                    });
                }
                // Whatever is left between the tabs and the buttons moves
                // the window, as a title bar would.
                let sense = if frameless {
                    Sense::click_and_drag()
                } else {
                    Sense::hover()
                };
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width().max(0.0), TAB_HEIGHT),
                    sense,
                );
                if frameless {
                    if response.double_clicked() {
                        state.window_request = Some(WindowRequest::ToggleMaximize);
                    } else if response.drag_started_by(egui::PointerButton::Primary) {
                        state.window_request = Some(WindowRequest::Drag);
                    } else if response.secondary_clicked() {
                        state.window_request = Some(WindowRequest::SystemMenu {
                            position: response
                                .interact_pointer_pos()
                                .unwrap_or_else(|| rect.left_bottom()),
                        });
                    }
                }
            });
        });
    });
    ui.painter().set(
        strip_background_slot,
        strip_background(strip.response.rect, &palette),
    );

    let toolbar_frame = Frame::new()
        .fill(palette.toolbar)
        .inner_margin(Margin::symmetric(8, 6));
    Panel::top("toolbar")
        .frame(toolbar_frame)
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(active.can_go_back, Button::new("◀"))
                    .on_hover_text(hint(keymap, CommandKind::Back, "Back"))
                    .clicked()
                {
                    commands.push(Command::Back(active.id));
                }
                if ui
                    .add_enabled(active.can_go_forward, Button::new("▶"))
                    .on_hover_text(hint(keymap, CommandKind::Forward, "Forward"))
                    .clicked()
                {
                    commands.push(Command::Forward(active.id));
                }
                // Reload is always available (a hung load must be recoverable);
                // Stop appears beside it while loading.
                if ui
                    .button("⟳")
                    .on_hover_text(hint(keymap, CommandKind::Reload, "Reload"))
                    .clicked()
                {
                    state.address_dirty = false;
                    commands.push(Command::Reload(active.id));
                }
                if active.load == LoadState::Loading {
                    if engine.supports_stop {
                        if ui
                            .button("✕")
                            .on_hover_text(hint(keymap, CommandKind::Stop, "Stop"))
                            .clicked()
                        {
                            commands.push(Command::Stop(active.id));
                        }
                    } else {
                        // The button shows the state but stays disabled so
                        // the UI does not promise what the engine lacks.
                        ui.add_enabled(false, Button::new("✕"))
                            .on_disabled_hover_text(format!(
                                "{} (not supported by {} {})",
                                hint(keymap, CommandKind::Stop, "Stop"),
                                engine.name,
                                engine.version
                            ));
                    }
                }

                // From the right: the extensions menu, the pinned actions
                // (in their order), then the address bar takes the rest.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if engine.extensions.is_some() {
                        let puzzle =
                            puzzle_button(ui, &palette, window).on_hover_text("Extensions");
                        state.menu_anchor = Some(puzzle.rect);
                        if puzzle.clicked() {
                            state.menu_request = Some(MenuRequest::Extensions {
                                position: puzzle.rect.left_bottom(),
                            });
                        }
                        for extension in pinned.iter().rev() {
                            if action_button(ui, state, &palette, window, extension).clicked() {
                                commands.push(Command::OpenExtensionAction {
                                    window: window.id,
                                    extension: extension.id.clone(),
                                });
                            }
                        }
                    }
                    let address_id = Id::new(("address", window.id));
                    let field = ui.add_sized(
                        ui.available_size(),
                        egui::TextEdit::singleline(&mut state.address_text)
                            .id(address_id)
                            .hint_text("Enter address"),
                    );
                    if field.changed() {
                        state.address_dirty = true;
                    }
                    // Select the text on focus, and again on Cmd+L while
                    // already focused, so the shortcut always means
                    // "replace the address".
                    let focus_requested = state.focus_address;
                    if state.focus_address {
                        state.focus_address = false;
                        field.request_focus();
                    }
                    if field.gained_focus() || focus_requested {
                        select_all(ui, address_id, state.address_text.chars().count());
                    }
                    if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        // A URL loads; anything else is a search.
                        let text = state.address_text.trim();
                        if let Some(url) = location_input_to_url(text) {
                            state.address_dirty = false;
                            commands.push(Command::Navigate {
                                tab: active.id,
                                url,
                            });
                        } else if !text.is_empty() {
                            state.address_dirty = false;
                            commands.push(Command::Search {
                                tab: active.id,
                                query: text.to_string(),
                            });
                        }
                    }
                });
            });
        });

    commands.extend(draw_prompt(ui, state));

    // The status text floats over the bottom-left corner of the page as an
    // overlay, so showing or hiding it never resizes the page.
    let status = match engine_error {
        Some(error) => Some(format!("Engine error: {error}")),
        None if !active.status.is_empty() => Some(active.status.clone()),
        None => None,
    };
    if let Some(text) = status {
        egui::Area::new(Id::new(("status", window.id)))
            .anchor(egui::Align2::LEFT_BOTTOM, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(engine_error.is_some())
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(text).small());
                        if engine_error.is_some()
                            && ui.small_button("×").on_hover_text("Dismiss").clicked()
                        {
                            commands.push(Command::DismissEngineError);
                        }
                    });
                });
            });
    }

    commands
}

/// The colour the framebuffer is cleared to before egui paints: the panel
/// fill, in the gamma-space components egui_glow's painter expects.
/// The colour the window is cleared to before the chrome paints: the
/// toolbar fill for `color`, so nothing flashes a different shade.
pub fn background(ctx: &egui::Context, color: ChromeColor) -> [f32; 4] {
    Palette::new(color, ctx.global_style().visuals.dark_mode)
        .toolbar
        .to_normalized_gamma_f32()
}

/// What every tab of one strip is drawn with: the palette and the width
/// they share ([`tab_width_for`]).
struct StripTabs<'a> {
    palette: &'a Palette,
    width: f32,
}

/// One tab of the strip, with everything a tab answers to: click,
/// Ctrl/Shift click, middle click, the close control, the context menu,
/// and dragging.
fn strip_tab(
    ui: &mut Ui,
    window: &Window,
    tab: &Tab,
    look: &StripTabs<'_>,
    state: &mut ChromeState,
    commands: &mut Vec<Command>,
    dragging: &mut Option<usize>,
) {
    let is_active = tab.id == window.active;
    let is_selected = window.is_selected(tab.id);
    let favicon = favicon_texture(ui, state, tab.id, tab.favicon.as_ref()).map(|t| t.id());
    let (response, close_clicked) = tab_widget(
        ui,
        look.palette,
        is_active,
        is_selected,
        favicon,
        tab.display_title(),
        look.width,
    );
    if close_clicked {
        commands.push(Command::CloseTab(tab.id));
    }
    // A plain click activates; Ctrl (Cmd on macOS) adds or removes; Shift
    // extends from the active tab.
    let modifiers = ui.input(|i| i.modifiers);
    if response.clicked() {
        if modifiers.shift {
            commands.push(Command::SelectTabRange(tab.id));
        } else if modifiers.command {
            commands.push(Command::ToggleTabSelection(tab.id));
        } else if !is_active || window.selected.len() > 1 {
            commands.push(Command::ActivateTab(tab.id));
        }
    }
    if response.middle_clicked() {
        commands.push(Command::CloseTab(tab.id));
    }
    // The context menu and a drag act on the whole selection when this
    // tab is part of it; otherwise on this tab alone, which becomes the
    // active one.
    let targets = if is_selected {
        window.selection()
    } else {
        vec![tab.id]
    };
    if response.secondary_clicked() {
        if !is_selected {
            commands.push(Command::ActivateTab(tab.id));
        }
        state.menu_request = Some(MenuRequest::Tab {
            targets: targets.clone(),
            position: response
                .interact_pointer_pos()
                .unwrap_or_else(|| response.rect.left_bottom()),
        });
    }
    if response.drag_started() && !is_selected {
        commands.push(Command::ActivateTab(tab.id));
    }
    if response.dragged() {
        *dragging = Some(targets.len());
    }
    if response.drag_stopped()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        state.tab_drop = Some(TabDrop {
            tabs: targets,
            pointer,
        });
    }
}

/// A folder's chip in the strip: its name with a disclosure glyph and,
/// collapsed, its tab count. A click drops the list of its tabs down
/// under it (another click, a click elsewhere, or Escape takes it away);
/// a right click opens the folder's menu. Expanded, its tabs follow it in
/// the strip, drawn by the caller.
fn strip_folder(
    ui: &mut Ui,
    window: &Window,
    folder: FolderId,
    tabs: &[TabId],
    palette: &Palette,
    state: &mut ChromeState,
    commands: &mut Vec<Command>,
) {
    let Some(f) = window.folder(folder) else {
        return;
    };
    let holds_active = tabs.contains(&window.active);
    // The active tab's page shows through a collapsed chip: it takes the
    // active tab's look.
    let as_active = holds_active && !f.expanded;
    let label = if f.expanded {
        truncate(&f.name, FOLDER_NAME_MAX_CHARS)
    } else {
        format!(
            "{} ({})",
            truncate(&f.name, FOLDER_NAME_MAX_CHARS),
            tabs.len()
        )
    };
    let font = TextStyle::Body.resolve(ui.style());
    let text_color = if as_active {
        ui.visuals().text_color()
    } else {
        palette.text_on_strip
    };
    let galley = ui.painter().layout_no_wrap(label, font, text_color);
    // Room for the disclosure triangle before the name.
    let glyph_width = 12.0;
    let width = TAB_PADDING_X + glyph_width + galley.size().x + TAB_PADDING_X;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, TAB_HEIGHT), Sense::click());
    let response = response.on_hover_text(if f.expanded {
        format!("{}: {} tabs", f.name, tabs.len())
    } else {
        format!("{}: click to list its tabs", f.name)
    });
    if ui.is_rect_visible(rect) {
        let fill = if as_active {
            palette.toolbar
        } else if response.hovered() || state.open_folder == Some(folder) {
            palette.tab_hover
        } else {
            palette.tab_selected
        };
        let corners = CornerRadius {
            nw: TAB_CORNER,
            ne: TAB_CORNER,
            sw: 0,
            se: 0,
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            corners,
            fill,
            Stroke::new(1.0_f32, palette.tab_outline),
            StrokeKind::Inside,
        );
        // The disclosure triangle, drawn (the fonts lack the glyphs):
        // pointing right when collapsed, down when expanded.
        let c = Pos2::new(rect.left() + TAB_PADDING_X + 4.0, rect.center().y);
        let h = 4.0;
        let points = if f.expanded {
            vec![
                Pos2::new(c.x - h, c.y - h / 2.0),
                Pos2::new(c.x + h, c.y - h / 2.0),
                Pos2::new(c.x, c.y + h / 2.0 + 1.0),
            ]
        } else {
            vec![
                Pos2::new(c.x - h / 2.0, c.y - h),
                Pos2::new(c.x - h / 2.0, c.y + h),
                Pos2::new(c.x + h / 2.0 + 1.0, c.y),
            ]
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            text_color,
            Stroke::NONE,
        ));
        painter.galley(
            Pos2::new(
                rect.left() + TAB_PADDING_X + glyph_width,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            text_color,
        );
    }
    if response.clicked() {
        state.open_folder = if state.open_folder == Some(folder) {
            None
        } else {
            Some(folder)
        };
    }
    if response.secondary_clicked() {
        state.open_folder = None;
        state.menu_request = Some(MenuRequest::Folder {
            folder,
            position: response
                .interact_pointer_pos()
                .unwrap_or_else(|| rect.left_bottom()),
        });
    }
    if state.open_folder != Some(folder) {
        return;
    }
    // The list of the folder's tabs, under the chip.
    let area = egui::Area::new(Id::new(("folder-list", folder)))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.left_bottom())
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(180.0);
                for id in tabs {
                    let Some(tab) = window.tab(*id) else {
                        continue;
                    };
                    let favicon =
                        favicon_texture(ui, state, tab.id, tab.favicon.as_ref()).map(|t| t.id());
                    let title = truncate(tab.display_title(), FOLDER_LIST_TITLE_MAX_CHARS);
                    let row = ui.horizontal(|ui| {
                        if let Some(texture) = favicon {
                            ui.add(
                                egui::Image::new((texture, Vec2::splat(FAVICON_SIZE)))
                                    .fit_to_exact_size(Vec2::splat(FAVICON_SIZE)),
                            );
                        }
                        ui.selectable_label(tab.id == window.active, title)
                    });
                    if row.inner.clicked() {
                        commands.push(Command::ActivateTab(tab.id));
                        state.open_folder = None;
                    }
                }
            });
        });
    let clicked_elsewhere = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !rect.contains(pos))
    });
    if clicked_elsewhere || ui.input(|i| i.key_pressed(Key::Escape)) {
        state.open_folder = None;
    }
}

/// The question the chrome is asking, as a small window over the page:
/// a text field (and, when there is something to choose from, a choice
/// of the existing folders or groups), OK and Cancel. Enter answers,
/// Escape cancels. Returns the command the answer is.
fn draw_prompt(ui: &mut Ui, state: &mut ChromeState) -> Vec<Command> {
    let Some(prompt) = state.prompt.as_mut() else {
        return Vec::new();
    };
    let (title, field_label, existing_label, existing): (&str, &str, &str, Vec<String>) =
        match &prompt.prompt {
            Prompt::MoveToFolder { folders, .. } => (
                "Move to folder",
                "New folder",
                "Or an existing folder",
                folders.iter().map(|(_, name)| name.clone()).collect(),
            ),
            Prompt::BookmarkTabs { groups, .. } => (
                "Bookmark all selected",
                "New bookmark group",
                "Or an existing group",
                groups.clone(),
            ),
            Prompt::RenameFolder { .. } => ("Rename folder", "Name", "", Vec::new()),
        };
    let mut answered = false;
    let mut cancelled = false;
    egui::Window::new(title)
        .id(Id::new("chrome-prompt"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
        .show(ui.ctx(), |ui| {
            ui.set_min_width(320.0);
            ui.label(field_label);
            let field =
                ui.add(egui::TextEdit::singleline(&mut prompt.text).desired_width(f32::INFINITY));
            if prompt.focus {
                prompt.focus = false;
                field.request_focus();
                select_all(ui, field.id, prompt.text.chars().count());
            }
            if field.changed() {
                prompt.existing = None;
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                answered = true;
            }
            if !existing.is_empty() {
                ui.add_space(6.0);
                ui.label(existing_label);
                let current = prompt
                    .existing
                    .and_then(|i| existing.get(i))
                    .cloned()
                    .unwrap_or_else(|| "(new)".to_string());
                egui::ComboBox::from_id_salt("prompt-existing")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(prompt.existing.is_none(), "(new)")
                            .clicked()
                        {
                            prompt.existing = None;
                        }
                        for (i, name) in existing.iter().enumerate() {
                            if ui
                                .selectable_label(prompt.existing == Some(i), name)
                                .clicked()
                            {
                                prompt.existing = Some(i);
                            }
                        }
                    });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can_answer = prompt.existing.is_some() || !prompt.text.trim().is_empty();
                if ui.add_enabled(can_answer, Button::new("OK")).clicked() {
                    answered = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                cancelled = true;
            }
        });
    if cancelled {
        state.prompt = None;
        return Vec::new();
    }
    if !answered {
        return Vec::new();
    }
    let Some(PromptState {
        prompt,
        text,
        existing: chosen,
        ..
    }) = state.prompt.take()
    else {
        return Vec::new();
    };
    let name = text.trim().to_string();
    match prompt {
        Prompt::MoveToFolder { tabs, folders, .. } => {
            let folder = match chosen.and_then(|i| folders.get(i)) {
                Some((id, _)) => FolderTarget::Existing(*id),
                None if name.is_empty() => return Vec::new(),
                None => FolderTarget::New { name },
            };
            vec![Command::MoveTabsToFolder { tabs, folder }]
        }
        Prompt::BookmarkTabs { tabs, groups } => {
            let group = match chosen.and_then(|i| groups.get(i)) {
                Some(group) => group.clone(),
                None if name.is_empty() => return Vec::new(),
                None => name,
            };
            vec![Command::BookmarkTabs { tabs, group }]
        }
        Prompt::RenameFolder { folder, .. } => {
            if name.is_empty() {
                return Vec::new();
            }
            vec![Command::RenameFolder { folder, name }]
        }
    }
}

/// The width of each tab in a strip with `room` points for `tabs` tabs
/// and `spacing` between them: `TAB_WIDTH` while they fit, else the
/// even share, never under `TAB_MIN_WIDTH`.
fn tab_width_for(room: f32, tabs: usize, spacing: f32) -> f32 {
    if tabs == 0 {
        return TAB_WIDTH;
    }
    let share = (room - spacing * (tabs as f32 - 1.0)) / tabs as f32;
    share.clamp(TAB_MIN_WIDTH, TAB_WIDTH)
}

/// One tab: a rounded-top block `width` wide with the favicon, the
/// title elided to the room left, and a close control (hidden on a
/// narrow inactive tab until hovered), filled per its state. Returns the
/// tab's response (click, drag, context menu) and whether the close
/// control was clicked.
fn tab_widget(
    ui: &mut Ui,
    palette: &Palette,
    is_active: bool,
    is_selected: bool,
    favicon: Option<egui::TextureId>,
    title: &str,
    width: f32,
) -> (egui::Response, bool) {
    let font = TextStyle::Body.resolve(ui.style());
    let text_color = if is_active {
        ui.visuals().text_color()
    } else {
        palette.text_on_strip
    };
    let icon_width = if favicon.is_some() {
        FAVICON_SIZE + 6.0
    } else {
        0.0
    };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, TAB_HEIGHT), Sense::click_and_drag());
    let show_close = is_active || width >= TAB_CLOSE_MIN_WIDTH || response.hovered();
    let close_rect = Rect::from_center_size(
        Pos2::new(
            rect.right() - TAB_PADDING_X - CLOSE_SIZE / 2.0 + 4.0,
            rect.center().y,
        ),
        Vec2::splat(CLOSE_SIZE),
    );
    // Added after the tab, so it is on top and takes the pointer over it.
    let close =
        show_close.then(|| ui.interact(close_rect, response.id.with("close"), Sense::click()));
    let close_room = if show_close { 8.0 + CLOSE_SIZE } else { 0.0 };
    let title_room = (width - 2.0 * TAB_PADDING_X - icon_width - close_room).max(0.0);
    let mut job =
        egui::text::LayoutJob::simple_singleline(title.to_string(), font.clone(), text_color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(title_room);
    let galley = ui.painter().layout_job(job);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if is_active {
            palette.toolbar
        } else if is_selected {
            palette.tab_selected
        } else if response.hovered() || response.dragged() {
            palette.tab_hover
        } else {
            palette.tab_idle
        };
        let stroke = if is_selected && !is_active {
            Stroke::new(1.0_f32, palette.tab_outline)
        } else {
            Stroke::NONE
        };
        let corners = CornerRadius {
            nw: TAB_CORNER,
            ne: TAB_CORNER,
            sw: 0,
            se: 0,
        };
        painter.rect(rect, corners, fill, stroke, StrokeKind::Inside);

        let mut x = rect.left() + TAB_PADDING_X;
        if let Some(texture) = favicon {
            let icon = Rect::from_center_size(
                Pos2::new(x + FAVICON_SIZE / 2.0, rect.center().y),
                Vec2::splat(FAVICON_SIZE),
            );
            painter.image(
                texture,
                icon,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            x += icon_width;
        }
        painter.galley(
            Pos2::new(x, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );

        if let Some(close) = &close {
            if close.hovered() {
                let ring = if is_active {
                    Color32::from_black_alpha(28)
                } else {
                    palette.tab_hover
                };
                painter.circle_filled(close_rect.center(), CLOSE_SIZE / 2.0, ring);
            }
            painter.text(
                close_rect.center(),
                Align2::CENTER_CENTER,
                "×",
                font,
                text_color,
            );
        }
    }
    let close_clicked = close.is_some_and(|close| close.on_hover_text("Close tab").clicked());
    (response, close_clicked)
}

/// The glyph of a window control, drawn as lines so it never depends on
/// a font having the symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// One window control at the strip's right end: a flat button that fills
/// on hover (red for close, as the system's does) with its glyph drawn as
/// one-pixel strokes.
fn control_button(ui: &mut Ui, palette: &Palette, glyph: ControlGlyph) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(CONTROL_WIDTH, TAB_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let active = response.hovered() || response.is_pointer_button_down_on();
        let (fill, ink) = match (active, glyph) {
            (true, ControlGlyph::Close) => (Color32::from_rgb(196, 43, 28), Color32::WHITE),
            (true, _) => (palette.tab_hover, palette.text_on_strip),
            (false, _) => (Color32::TRANSPARENT, palette.text_on_strip),
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, fill);
        let stroke = Stroke::new(1.0_f32, ink);
        let c = rect.center();
        let half = 5.0;
        match glyph {
            ControlGlyph::Minimize => {
                painter.line_segment(
                    [Pos2::new(c.x - half, c.y), Pos2::new(c.x + half, c.y)],
                    stroke,
                );
            }
            ControlGlyph::Maximize => {
                painter.rect_stroke(
                    Rect::from_center_size(c, Vec2::splat(2.0 * half)),
                    0.0,
                    stroke,
                    StrokeKind::Inside,
                );
            }
            ControlGlyph::Restore => {
                let front = Rect::from_min_size(
                    Pos2::new(c.x - half, c.y - half + 3.0),
                    Vec2::splat(2.0 * half - 3.0),
                );
                // The back window peeks out above and to the right.
                painter.line_segment(
                    [
                        Pos2::new(front.min.x + 3.0, front.min.y),
                        Pos2::new(front.min.x + 3.0, front.min.y - 3.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(front.min.x + 3.0, front.min.y - 3.0),
                        Pos2::new(front.max.x + 3.0, front.min.y - 3.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(front.max.x + 3.0, front.min.y - 3.0),
                        Pos2::new(front.max.x + 3.0, front.max.y - 3.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(front.max.x + 3.0, front.max.y - 3.0),
                        Pos2::new(front.max.x, front.max.y - 3.0),
                    ],
                    stroke,
                );
                painter.rect_stroke(front, 0.0, stroke, StrokeKind::Inside);
            }
            ControlGlyph::Close => {
                painter.line_segment(
                    [
                        Pos2::new(c.x - half, c.y - half),
                        Pos2::new(c.x + half, c.y + half),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(c.x - half, c.y + half),
                        Pos2::new(c.x + half, c.y - half),
                    ],
                    stroke,
                );
            }
        }
    }
    response
}

/// The flat square behind a toolbar icon button, filled on hover. The
/// id is explicit: the toolbar's widget count changes with the Stop
/// button, which would otherwise move the auto ids between frames.
fn icon_button_rect(ui: &mut Ui, palette: &Palette, id: Id) -> (Rect, egui::Response, Color32) {
    let (_, rect) = ui.allocate_space(Vec2::splat(ACTION_SIZE));
    let response = ui.interact(rect, id, Sense::click());
    let fill = if response.hovered() || response.is_pointer_button_down_on() {
        blend(palette.toolbar, ui.visuals().text_color(), 0.12)
    } else {
        palette.toolbar
    };
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    (rect, response, fill)
}

/// The extensions menu button: a puzzle piece drawn with the painter, so
/// it never depends on a font having the glyph.
fn puzzle_button(ui: &mut Ui, palette: &Palette, window: &Window) -> egui::Response {
    let id = Id::new(("extensions-menu", window.id));
    let (rect, response, fill) = icon_button_rect(ui, palette, id);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let ink = ui.visuals().text_color();
        let c = rect.center();
        // The body sits a little low and left so the knobs on top and on
        // the right keep the whole glyph centred.
        let body = Rect::from_center_size(Pos2::new(c.x - 1.5, c.y + 1.5), Vec2::splat(11.0));
        painter.rect_filled(body, 2.0, ink);
        let knob = 2.6;
        painter.circle_filled(Pos2::new(body.center().x, body.min.y - 0.5), knob, ink);
        painter.circle_filled(Pos2::new(body.max.x + 0.5, body.center().y), knob, ink);
        // The socket on the left is a bite of the background.
        painter.circle_filled(Pos2::new(body.min.x - 0.5, body.center().y), knob, fill);
    }
    response
}

/// One pinned extension's action button: its icon, or its initial when it
/// has none, with the action's title as the tooltip. The button's rect is
/// recorded so the window can anchor the popup to it.
fn action_button(
    ui: &mut Ui,
    state: &mut ChromeState,
    palette: &Palette,
    window: &Window,
    extension: &Extension,
) -> egui::Response {
    let id = Id::new(("extension-action", window.id, &extension.id));
    let (rect, response, _) = icon_button_rect(ui, palette, id);
    state.action_anchors.insert(extension.id.clone(), rect);
    if ui.is_rect_visible(rect) {
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(ACTION_ICON));
        match extension_icon_texture(ui, state, extension) {
            Some(texture) => {
                let id = texture.id();
                ui.painter().image(
                    id,
                    icon_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                let initial: String = extension.name.chars().take(1).collect();
                let painter = ui.painter();
                painter.circle_filled(icon_rect.center(), ACTION_ICON / 2.0, palette.strip);
                painter.text(
                    icon_rect.center(),
                    Align2::CENTER_CENTER,
                    initial.to_uppercase(),
                    TextStyle::Small.resolve(ui.style()),
                    palette.text_on_strip,
                );
            }
        }
    }
    let title = extension
        .action_title
        .clone()
        .unwrap_or_else(|| extension.name.clone());
    response.on_hover_text(title)
}

/// Replace the page area with an error panel. Returns true if the user
/// asked to reload.
pub fn draw_crash_panel(ui: &mut Ui, rect: Rect, reason: &str) -> bool {
    let mut reload = false;
    // The page is not blitted while crashed, so the panel fills its own
    // area; nothing else paints there.
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("This tab crashed");
            ui.label(reason);
            ui.add_space(12.0);
            if ui.button("Reload").clicked() {
                reload = true;
            }
        });
    });
    reload
}

/// Turn address-bar text into a URL. Bare domains get `https://`. `localhost`,
/// bare IPv4 addresses, and `host:port` get `http://`. `javascript:` URLs
/// are refused. Returns `None` for text that is not a URL (Phase 1 has no
/// search engine).
pub fn location_input_to_url(input: &str) -> Option<Url> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    match Url::parse(text) {
        Ok(url) if url.scheme() == "javascript" => None,
        // "localhost:8080" and "example.com:8080" parse as scheme + path.
        Ok(url)
            if url.host_str().is_none()
                && (url.scheme() == "localhost" || url.scheme().contains('.')) =>
        {
            with_default_scheme(text)
        }
        Ok(url) => Some(url),
        Err(ParseError::RelativeUrlWithoutBase) if looks_like_host(text) => {
            with_default_scheme(text)
        }
        Err(_) => None,
    }
}

/// `text` with the scheme `default_scheme` picks for it, parsed.
fn with_default_scheme(text: &str) -> Option<Url> {
    Url::parse(&format!("{}://{text}", default_scheme(text))).ok()
}

/// `http` for `localhost`, bare IPv4 addresses, and `host:port`; `https` for
/// everything else.
fn default_scheme(text: &str) -> &'static str {
    let (host, port) = split_port(authority(text));
    if host == "localhost" || host.parse::<Ipv4Addr>().is_ok() || port.is_some() {
        "http"
    } else {
        "https"
    }
}

fn looks_like_host(text: &str) -> bool {
    let (host, _) = split_port(authority(text));
    !text.contains(char::is_whitespace)
        && !text.starts_with('.')
        && (host == "localhost" || host.contains('.'))
}

/// The part of address-bar text before any path, query, or fragment, such as
/// `host` or `host:port`.
fn authority(text: &str) -> &str {
    text.split(['/', '?', '#']).next().unwrap_or(text)
}

/// Splits `host:port` when the part after the last colon is all digits.
fn split_port(authority: &str) -> (&str, Option<&str>) {
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (authority, None),
    }
}

fn sync_address_text(state: &mut ChromeState, tab: TabId, url: &Url) {
    let tab_changed = state.shown_tab != Some(tab);
    let url_changed = state.shown_url.as_ref() != Some(url);
    if tab_changed || (url_changed && !state.address_dirty) {
        state.address_text = url.to_string();
        state.address_dirty = false;
    }
    state.shown_tab = Some(tab);
    state.shown_url = Some(url.clone());
}

fn select_all(ui: &Ui, id: Id, len: usize) {
    use egui::text::{CCursor, CCursorRange};
    use egui::text_edit::TextEditState;
    if let Some(mut edit) = TextEditState::load(ui.ctx(), id) {
        edit.cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
        edit.store(ui.ctx(), id);
    }
}

fn favicon_texture<'a>(
    ui: &Ui,
    state: &'a mut ChromeState,
    tab: TabId,
    favicon: Option<&Favicon>,
) -> Option<&'a TextureHandle> {
    let favicon = favicon?;
    let fresh = state
        .favicons
        .get(&tab)
        .is_some_and(|cached| cached.is_for(favicon));
    if !fresh {
        let image = ColorImage::from_rgba_unmultiplied(
            [favicon.width as usize, favicon.height as usize],
            &favicon.rgba,
        );
        let texture =
            ui.ctx()
                .load_texture(format!("favicon-{}", tab.0), image, TextureOptions::LINEAR);
        state.favicons.insert(
            tab,
            CachedFavicon {
                width: favicon.width,
                height: favicon.height,
                rgba: favicon.rgba.clone(),
                texture,
            },
        );
    }
    state.favicons.get(&tab).map(|cached| &cached.texture)
}

/// The texture of a pinned extension's icon, uploaded on first use and
/// again when the engine reports a different icon.
fn extension_icon_texture<'a>(
    ui: &Ui,
    state: &'a mut ChromeState,
    extension: &Extension,
) -> Option<&'a TextureHandle> {
    let icon = extension.icon.as_ref()?;
    let fresh = state
        .extension_icons
        .get(&extension.id)
        .is_some_and(|cached| cached.is_for(icon));
    if !fresh {
        let image = ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        );
        let texture = ui.ctx().load_texture(
            format!("extension-{}", extension.id),
            image,
            TextureOptions::LINEAR,
        );
        state.extension_icons.insert(
            extension.id.clone(),
            CachedFavicon {
                width: icon.width,
                height: icon.height,
                rgba: icon.rgba.clone(),
                texture,
            },
        );
    }
    state
        .extension_icons
        .get(&extension.id)
        .map(|cached| &cached.texture)
}

impl CachedFavicon {
    /// True if this texture was uploaded from `favicon`'s pixels. The model
    /// replaces the `Arc` whenever the engine reports a new icon, so the
    /// pointer comparison is exact and costs nothing per frame.
    fn is_for(&self, favicon: &Favicon) -> bool {
        self.width == favicon.width
            && self.height == favicon.height
            && Arc::ptr_eq(&self.rgba, &favicon.rgba)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// A label with its shortcut, like "New tab (Cmd+T)".
fn hint(keymap: &Keymap, kind: CommandKind, label: &str) -> String {
    match keymap.chord_for(kind) {
        Some(chord) => format!("{label} ({})", describe_chord(chord)),
        None => label.to_string(),
    }
}

/// A readable name for a chord: modifiers in Ctrl, Alt, Shift, Cmd order,
/// then the key, like "Ctrl+Shift+]" (the stored form is
/// `KeyChord`'s `Display`, which names the key code instead).
pub fn describe_chord(chord: KeyChord) -> String {
    use keyboard_types::Modifiers;
    let mut parts: Vec<String> = Vec::new();
    if chord.modifiers.contains(Modifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if chord.modifiers.contains(Modifiers::ALT) {
        parts.push(
            if cfg!(target_os = "macos") {
                "Option"
            } else {
                "Alt"
            }
            .to_string(),
        );
    }
    if chord.modifiers.contains(Modifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    if chord.modifiers.contains(Modifiers::META) {
        parts.push(
            if cfg!(target_os = "macos") {
                "Cmd"
            } else {
                "Win"
            }
            .to_string(),
        );
    }
    parts.push(key_name(chord.code));
    parts.join("+")
}

fn key_name(code: Code) -> String {
    let name = match code {
        Code::BracketLeft => "[",
        Code::BracketRight => "]",
        Code::Comma => ",",
        Code::Period => ".",
        Code::ArrowLeft => "Left",
        Code::ArrowRight => "Right",
        Code::ArrowUp => "Up",
        Code::ArrowDown => "Down",
        Code::Escape => "Esc",
        Code::PageUp => "PgUp",
        Code::PageDown => "PgDn",
        Code::Backspace => "Backspace",
        Code::Delete => "Del",
        Code::Insert => "Ins",
        Code::Space => "Space",
        Code::Enter => "Enter",
        Code::Minus => "-",
        Code::Equal => "=",
        Code::Semicolon => ";",
        Code::Quote => "'",
        Code::Backquote => "`",
        Code::Backslash => "\\",
        Code::Slash => "/",
        other => {
            let debug = format!("{other:?}");
            return debug
                .strip_prefix("Key")
                .or_else(|| debug.strip_prefix("Digit"))
                .map(str::to_string)
                .or_else(|| {
                    debug
                        .strip_prefix("Numpad")
                        .map(|rest| format!("Num {rest}"))
                })
                .unwrap_or(debug);
        }
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_fades_from_the_swatch_to_the_toolbar() {
        let palette = Palette::new(ChromeColor::default(), false);
        assert_eq!(palette.strip_at(0.0), palette.strip);
        assert_eq!(palette.strip_at(1.0), palette.toolbar);
        // Mostly the swatch still halfway down.
        assert_eq!(
            palette.strip_at(0.5),
            blend(palette.strip, palette.toolbar, 0.25)
        );
        assert_eq!(palette.strip_at(2.0), palette.toolbar);
    }
    use keyboard_types::Modifiers;

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(
            location_input_to_url("https://example.com/a?b=c")
                .unwrap()
                .as_str(),
            "https://example.com/a?b=c"
        );
        assert_eq!(
            location_input_to_url("about:blank").unwrap().as_str(),
            "about:blank"
        );
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(
            location_input_to_url("example.com").unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            location_input_to_url("  www.example.com/path ")
                .unwrap()
                .as_str(),
            "https://www.example.com/path"
        );
    }

    #[test]
    fn localhost_and_host_port_get_http() {
        assert_eq!(
            location_input_to_url("localhost").unwrap().as_str(),
            "http://localhost/"
        );
        assert_eq!(
            location_input_to_url("localhost:8080").unwrap().as_str(),
            "http://localhost:8080/"
        );
        assert_eq!(
            location_input_to_url("example.com:8080").unwrap().as_str(),
            "http://example.com:8080/"
        );
    }

    #[test]
    fn non_urls_are_rejected() {
        assert_eq!(location_input_to_url(""), None);
        assert_eq!(location_input_to_url("hello world"), None);
        assert_eq!(location_input_to_url("justaword"), None);
        assert_eq!(location_input_to_url(".hidden"), None);
    }

    #[test]
    fn localhost_with_a_path_gets_http() {
        assert_eq!(
            location_input_to_url("localhost/path").unwrap().as_str(),
            "http://localhost/path"
        );
        assert_eq!(
            location_input_to_url("localhost:8080/path?q=1")
                .unwrap()
                .as_str(),
            "http://localhost:8080/path?q=1"
        );
    }

    #[test]
    fn a_domain_that_starts_with_localhost_gets_https() {
        assert_eq!(
            location_input_to_url("localhostfoo.com").unwrap().as_str(),
            "https://localhostfoo.com/"
        );
    }

    #[test]
    fn bare_ipv4_addresses_get_http_with_or_without_port_and_path() {
        assert_eq!(
            location_input_to_url("192.168.1.1").unwrap().as_str(),
            "http://192.168.1.1/"
        );
        assert_eq!(
            location_input_to_url("192.168.1.1:8080").unwrap().as_str(),
            "http://192.168.1.1:8080/"
        );
        assert_eq!(
            location_input_to_url("10.0.0.1/admin").unwrap().as_str(),
            "http://10.0.0.1/admin"
        );
    }

    #[test]
    fn javascript_urls_are_refused() {
        assert_eq!(location_input_to_url("javascript:alert(1)"), None);
        assert_eq!(location_input_to_url("  JavaScript:void(0) "), None);
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn edited_address_text_survives_an_engine_url_change() {
        let mut state = ChromeState::default();
        sync_address_text(&mut state, TabId(1), &url("https://a.test/"));
        state.address_text = "typed by the user".into();
        state.address_dirty = true;
        sync_address_text(&mut state, TabId(1), &url("https://a.test/moved"));
        assert_eq!(state.address_text, "typed by the user");
        assert!(state.address_dirty);
    }

    #[test]
    fn switching_tabs_resets_edited_text_to_the_new_tab_url() {
        let mut state = ChromeState::default();
        sync_address_text(&mut state, TabId(1), &url("https://a.test/"));
        state.address_text = "typed by the user".into();
        state.address_dirty = true;
        sync_address_text(&mut state, TabId(2), &url("https://b.test/"));
        assert_eq!(state.address_text, "https://b.test/");
        assert!(!state.address_dirty);
    }

    #[test]
    fn clean_address_text_follows_the_url() {
        let mut state = ChromeState::default();
        sync_address_text(&mut state, TabId(1), &url("https://a.test/"));
        assert!(!state.address_dirty);
        sync_address_text(&mut state, TabId(1), &url("https://a.test/next"));
        assert_eq!(state.address_text, "https://a.test/next");
        assert!(!state.address_dirty);
    }

    #[test]
    fn tabs_share_the_strip_evenly_between_a_standard_and_a_minimum_width() {
        assert_eq!(tab_width_for(1000.0, 0, 2.0), TAB_WIDTH);
        assert_eq!(tab_width_for(1000.0, 1, 2.0), TAB_WIDTH);
        assert_eq!(tab_width_for(1000.0, 4, 2.0), TAB_WIDTH);
        // Ten tabs in 1000 points: the even share, spacing taken out.
        assert_eq!(tab_width_for(1000.0, 10, 2.0), (1000.0 - 18.0) / 10.0);
        assert_eq!(tab_width_for(100.0, 10, 2.0), TAB_MIN_WIDTH);
        assert_eq!(tab_width_for(-50.0, 3, 2.0), TAB_MIN_WIDTH);
    }

    #[test]
    fn truncate_adds_ellipsis_only_when_cut() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a longer title here", 8), "a longer…");
    }

    #[test]
    fn chord_labels_are_readable() {
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::CONTROL, Code::KeyT)),
            "Ctrl+T"
        );
        assert_eq!(
            describe_chord(KeyChord::new(
                Modifiers::CONTROL | Modifiers::SHIFT,
                Code::BracketRight
            )),
            "Ctrl+Shift+]"
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::CONTROL, Code::Comma)),
            "Ctrl+,"
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::CONTROL, Code::Digit1)),
            "Ctrl+1"
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::empty(), Code::F5)),
            "F5"
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::CONTROL, Code::PageDown)),
            "Ctrl+PgDn"
        );
        // Alt reads as Option on macOS.
        let alt = if cfg!(target_os = "macos") {
            "Option"
        } else {
            "Alt"
        };
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::ALT, Code::Home)),
            format!("{alt}+Home")
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::empty(), Code::Numpad5)),
            "Num 5"
        );
    }
}
