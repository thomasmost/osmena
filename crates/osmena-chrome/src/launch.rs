//! The Launch Panel: what `Ctrl+T` opens over the window. A wide modal
//! with the address-or-search field at its top (the matches drop down
//! over the rest), a mosaic of tiles under it (the recent Drive files,
//! the weather, a question for Claude or ChatGPT, the most visited
//! sites), and a gear that flips the panel to the Launch Settings: the
//! search engine, the question templates, the weather place, the Google
//! account, and the same tiles in edit mode, dragged to reorder and
//! closed to remove. Everything the user does is a [`Command`]; the data
//! on the tiles is the model's (`Browser::launch`), fetched by the shell.

use std::collections::HashMap;

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Key, Layout, Margin, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, TextStyle, Ui, UiBuilder, Vec2,
};
use osmena_core::launch::{
    DriveKind, DriveState, Fetched, TOP_SITES, TopSite, Units, WeatherCode, WidgetKind, top_sites,
};
use osmena_core::{Command, UrlTemplate, Window, suggestions};

use crate::chrome::{
    ChromeContext, ChromeState, LAUNCHER_FONT_SIZE, blend, caret_to_end, site_icon_texture,
    step_pick, suggestion_row, truncate,
};
use crate::theme;

/// The panel's widest; narrower windows get what is left of 48 px.
const PANEL_WIDTH: f32 = 960.0;
/// The space between tiles and the tiles' height.
const TILE_GAP: f32 = 10.0;
const TILE_HEIGHT: f32 = 190.0;
const TILE_PADDING: f32 = 12.0;
/// How long a tile takes to slide to a new place.
const TILE_SLIDE_SECS: f32 = 0.12;
/// A row on the Drive and Most visited tiles.
const ROW_HEIGHT: f32 = 26.0;
const ROW_ICON: f32 = 16.0;
const NAME_MAX_CHARS: usize = 40;
const GEAR_SIZE: f32 = 28.0;

/// The panel's view: the tiles, or the settings over them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Mosaic,
    Settings,
}

/// A tile being dragged in edit mode: which, where the pointer took
/// hold of it, and where it would land now (the index
/// `Command::MoveWidget` takes), as the last frame worked it out.
#[derive(Clone, Debug, PartialEq)]
struct WidgetDrag {
    kind: WidgetKind,
    grab: Vec2,
    index: usize,
    from: usize,
}

/// The settings view's text fields, edited here and sent as commands
/// when a field is left.
#[derive(Default)]
struct SettingsForm {
    search_name: String,
    search_url: String,
    /// "Custom" is picked although the fields still spell a preset.
    custom: bool,
    ask: HashMap<WidgetKind, String>,
    /// What the fields were last filled from, so a change in the model
    /// (a preset picked) refills a field that is not being edited.
    synced_search: Option<UrlTemplate>,
    synced_ask: HashMap<WidgetKind, String>,
}

/// The panel's state while it is open: what the user typed in the
/// field, the match picked with the arrow keys (`typed` keeps what they
/// typed, for going back up), the tiles' own fields, and the view.
#[derive(Default)]
pub(crate) struct Launcher {
    pub(crate) text: String,
    pub(crate) typed: String,
    pub(crate) picked: Option<usize>,
    /// The field takes focus on the first frame.
    pub(crate) focus: bool,
    view: View,
    /// The question fields' texts, by tile.
    ask: HashMap<WidgetKind, String>,
    /// The weather tile's place field.
    place_query: String,
    form: SettingsForm,
    drag: Option<WidgetDrag>,
    /// Where the matches dropped down last frame: a press there is not
    /// a press outside the panel.
    dropdown: Option<Rect>,
    /// The most visited sites, worked out once per opening (and again
    /// after one is hidden).
    top: Option<Vec<TopSite>>,
}

impl Launcher {
    pub(crate) fn new() -> Launcher {
        Launcher {
            focus: true,
            ..Launcher::default()
        }
    }
}

/// Draw the panel over the window, when open, and return the commands
/// it produced. Enter in the field opens what was typed in a new tab
/// (an address loads, anything else is a search) or the picked match;
/// Up and Down pick a match, whose address then stands in the field;
/// Escape, or a click anywhere outside the panel, opens nothing (in the
/// settings view, Escape goes back to the tiles first).
pub(crate) fn draw_launch_panel(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    state: &mut ChromeState,
) -> Vec<Command> {
    let Some(mut launcher) = state.launcher.take() else {
        return Vec::new();
    };
    let ctx = ui.ctx().clone();
    let content = ctx.content_rect();
    let mut commands = Vec::new();
    let mut close = false;

    // The backdrop is painted into the panel's own layer, before the
    // panel, so it dims the page (which the shell paints in a
    // middle-order layer of its own, above every middle-order area) and
    // stays under the panel whatever sequence egui keeps areas in. A
    // press anywhere outside the panel cancels; the page sees no pointer
    // events while the panel is open (`popup_open`).
    let area_id = Id::new(("launcher", window.id));
    ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, area_id))
        .rect_filled(content, 0.0, Color32::from_black_alpha(96));

    let width = (content.width() - 48.0).clamp(280.0, PANEL_WIDTH);
    let top = content.top() + content.height() * 0.08;
    let top_left = Pos2::new(content.center().x - width / 2.0, top);
    let max_height = (content.bottom() - top - 24.0).max(120.0);
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    let shown = egui::Area::new(area_id)
        .order(egui::Order::Foreground)
        .fixed_pos(top_left)
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style())
                .corner_radius(14)
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.set_width(width - 36.0);
                    // The scroll box gets an explicit height: an area sizes
                    // its ui by its own last frame, and a scroll area that
                    // shrank to that could never grow back.
                    let box_size = Vec2::new(width - 36.0, max_height - 36.0);
                    ui.allocate_ui_with_layout(box_size, Layout::top_down(Align::Min), |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(box_size.y)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                let field = address_field(ui, window, context, &mut launcher);
                                commands.extend(field.commands);
                                close |= field.close;
                                ui.add_space(14.0);
                                let edit = launcher.view == View::Settings;
                                if edit {
                                    settings_view(
                                        ui,
                                        window,
                                        context,
                                        &mut launcher,
                                        &mut commands,
                                    );
                                }
                                if let Some(c) = mosaic(
                                    ui,
                                    window,
                                    context,
                                    state,
                                    &mut launcher,
                                    edit,
                                    &mut commands,
                                ) {
                                    close |= c;
                                }
                                ui.add_space(10.0);
                                bottom_row(ui, &mut launcher, &mut commands);
                            });
                    });
                });
        });
    if escape {
        if launcher.view == View::Settings {
            launcher.view = View::Mosaic;
        } else {
            close = true;
        }
    }
    let pressed_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer.interact_pos().is_some_and(|p| {
                !shown.response.rect.contains(p)
                    && !launcher.dropdown.is_some_and(|d| d.contains(p))
            })
    });
    if pressed_outside {
        close = true;
    }
    if !close {
        state.launcher = Some(launcher);
    }
    commands
}

/// What the field did this frame.
struct FieldOutcome {
    commands: Vec<Command>,
    close: bool,
}

/// The address-or-search field at the panel's top, with the history and
/// bookmark matches dropping down over the tiles.
fn address_field(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    launcher: &mut Launcher,
) -> FieldOutcome {
    let mut commands = Vec::new();
    let mut close = false;
    let matches = suggestions(context.history, context.bookmarks, &launcher.typed);
    let id = Id::new(("launcher-field", window.id));
    let field = ui
        .scope(|ui| {
            theme::pill_fields(ui);
            ui.add(
                egui::TextEdit::singleline(&mut launcher.text)
                    .id(id)
                    .margin(Margin::symmetric(16, 9))
                    .font(FontId::proportional(LAUNCHER_FONT_SIZE))
                    // The hint takes the field's font: egui lays it out
                    // in the body size at the top-left otherwise.
                    .hint_text(
                        RichText::new("Search or enter address")
                            .font(FontId::proportional(LAUNCHER_FONT_SIZE)),
                    )
                    .desired_width(f32::INFINITY),
            )
        })
        .inner;
    if launcher.focus {
        launcher.focus = false;
        field.request_focus();
    }
    if field.changed() {
        launcher.typed = launcher.text.clone();
        launcher.picked = None;
    }
    if field.has_focus() && !matches.is_empty() {
        let (down, up) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            )
        });
        if down || up {
            launcher.picked = step_pick(launcher.picked, matches.len(), down);
            launcher.text = match launcher.picked {
                Some(i) => matches[i].url.to_string(),
                None => launcher.typed.clone(),
            };
            caret_to_end(ui, id, launcher.text.chars().count());
        }
    }
    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    if field.lost_focus() && enter {
        let text = launcher.text.trim();
        if let Some(i) = launcher.picked {
            commands.push(Command::NewTab {
                window: window.id,
                url: Some(matches[i].url.clone()),
            });
        } else if let Some(url) = crate::chrome::location_input_to_url(text) {
            commands.push(Command::NewTab {
                window: window.id,
                url: Some(url),
            });
        } else if !text.is_empty() {
            commands.push(Command::SearchInNewTab {
                window: window.id,
                query: text.to_string(),
            });
        }
        close = true;
    }
    // The matches drop down in their own area, above the tiles, so the
    // tiles never jump while the user types. The list stays while the
    // field has focus or the pointer is over where the list was last
    // frame (a click on a row takes the focus away first).
    let over_list = launcher
        .dropdown
        .zip(ui.ctx().pointer_latest_pos())
        .is_some_and(|(rect, p)| rect.contains(p));
    launcher.dropdown = None;
    let show_list = !matches.is_empty()
        && !launcher.typed.trim().is_empty()
        && (field.has_focus() || over_list);
    if show_list {
        let picked = launcher.picked;
        let list = egui::Area::new(Id::new(("launcher-matches", window.id)))
            .order(egui::Order::Tooltip)
            .fixed_pos(field.rect.left_bottom() + Vec2::new(0.0, 4.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .corner_radius(10)
                    .show(ui, |ui| {
                        ui.set_width(field.rect.width() - 16.0);
                        for (i, entry) in matches.iter().enumerate() {
                            if suggestion_row(ui, entry, picked == Some(i)).clicked() {
                                commands.push(Command::NewTab {
                                    window: window.id,
                                    url: Some(entry.url.clone()),
                                });
                                close = true;
                            }
                        }
                    });
            });
        launcher.dropdown = Some(list.response.rect);
    }
    FieldOutcome { commands, close }
}

/// The row under the tiles: the gear at the right (the settings view
/// has Done there and "Restore default widgets" at the left).
fn bottom_row(ui: &mut Ui, launcher: &mut Launcher, commands: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        if launcher.view == View::Settings
            && ui
                .add(egui::Button::new("Restore default widgets").frame(false))
                .clicked()
        {
            commands.push(Command::RestoreWidgets);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            match launcher.view {
                View::Mosaic => {
                    if gear_button(ui).on_hover_text("Launch settings").clicked() {
                        launcher.view = View::Settings;
                        launcher.form = SettingsForm::default();
                    }
                }
                View::Settings => {
                    if ui.button("Done").clicked() {
                        launcher.view = View::Mosaic;
                    }
                }
            }
        });
    });
}

/// A gear drawn with the painter, so it never depends on a font having
/// the glyph.
fn gear_button(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(GEAR_SIZE), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hovered = response.hovered() || response.is_pointer_button_down_on();
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let ink = if hovered {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let c = rect.center();
    let r = 6.0;
    let stroke = Stroke::new(2.2_f32, ink);
    painter.circle_stroke(c, r, stroke);
    painter.circle_filled(c, 1.8, ink);
    for i in 0..8 {
        let a = std::f32::consts::TAU * i as f32 / 8.0;
        let (sin, cos) = a.sin_cos();
        painter.line_segment(
            [
                Pos2::new(c.x + cos * (r + 0.5), c.y + sin * (r + 0.5)),
                Pos2::new(c.x + cos * (r + 3.4), c.y + sin * (r + 3.4)),
            ],
            stroke,
        );
    }
    response
}

/// One tile's place in the grid.
struct Slot {
    kind: WidgetKind,
    rect: Rect,
}

/// Lay the tiles out on `cols` columns from `origin`, each `kind` taking
/// its span (clamped to the grid), wrapping when a span does not fit.
fn layout(kinds: &[WidgetKind], cols: usize, tile_w: f32, origin: Pos2) -> Vec<Slot> {
    let mut slots = Vec::with_capacity(kinds.len());
    let (mut col, mut row) = (0usize, 0usize);
    for &kind in kinds {
        let span = kind.columns().clamp(1, cols);
        if col + span > cols {
            col = 0;
            row += 1;
        }
        let x = origin.x + col as f32 * (tile_w + TILE_GAP);
        let y = origin.y + row as f32 * (TILE_HEIGHT + TILE_GAP);
        let w = span as f32 * tile_w + (span - 1) as f32 * TILE_GAP;
        slots.push(Slot {
            kind,
            rect: Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, TILE_HEIGHT)),
        });
        col += span;
        if col >= cols {
            col = 0;
            row += 1;
        }
    }
    slots
}

/// How many columns fit in `width`.
fn columns_for(width: f32) -> usize {
    if width >= 720.0 {
        3
    } else if width >= 480.0 {
        2
    } else {
        1
    }
}

/// The index among the other tiles a tile dragged to `pointer` would
/// take: after every tile whose row is above the pointer, and every
/// tile on the pointer's row whose centre is left of it.
fn drop_index(others: &[Slot], pointer: Pos2) -> usize {
    others
        .iter()
        .filter(|s| {
            let c = s.rect.center();
            c.y + TILE_HEIGHT / 2.0 < pointer.y
                || ((c.y - pointer.y).abs() <= TILE_HEIGHT / 2.0 && c.x < pointer.x)
        })
        .count()
}

/// The mosaic. In edit mode each tile shows a handle and a close
/// control, drags to a new place (the others slide aside) and its
/// widgets are inert. Returns `Some(true)` when something on a tile
/// closes the panel (a question sent, a site opened).
fn mosaic(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    state: &mut ChromeState,
    launcher: &mut Launcher,
    edit: bool,
    commands: &mut Vec<Command>,
) -> Option<bool> {
    let drive_available = context.brand.google_client_id.is_some();
    let kinds = context.launch_settings.visible_widgets(drive_available);
    if kinds.is_empty() {
        ui.weak("No widgets. The gear below restores the defaults.");
        return None;
    }
    let avail = ui.available_width();
    let cols = columns_for(avail);
    let tile_w = (avail - TILE_GAP * (cols - 1) as f32) / cols as f32;
    let origin = ui.cursor().min;

    // The order as drawn: a dragged tile sits at the index it would
    // land on, so the others open a gap there.
    let mut order = kinds.clone();
    if let Some(drag) = &launcher.drag {
        if edit && kinds.contains(&drag.kind) {
            order.retain(|k| *k != drag.kind);
            order.insert(drag.index.min(order.len()), drag.kind);
        } else {
            launcher.drag = None;
        }
    }
    let slots = layout(&order, cols, tile_w, origin);
    let bottom = slots
        .iter()
        .map(|s| s.rect.bottom())
        .fold(origin.y, f32::max);
    ui.allocate_exact_size(Vec2::new(avail, bottom - origin.y), Sense::hover());

    let pointer = ui.ctx().pointer_latest_pos();
    let mut close = false;
    let dragged_kind = launcher.drag.as_ref().map(|d| d.kind);
    // The others first, the dragged tile last so it rides on top.
    let mut drawn: Vec<&Slot> = slots
        .iter()
        .filter(|s| Some(s.kind) != dragged_kind)
        .collect();
    if let Some(dragged) = slots.iter().find(|s| Some(s.kind) == dragged_kind) {
        drawn.push(dragged);
    }
    for slot in drawn {
        let ctx = ui.ctx().clone();
        let rect = if Some(slot.kind) == dragged_kind {
            let drag = launcher.drag.as_ref().expect("dragging");
            match pointer {
                Some(p) => Rect::from_min_size(p - drag.grab, slot.rect.size()),
                None => slot.rect,
            }
        } else {
            let x = ctx.animate_value_with_time(
                Id::new(("widget-x", slot.kind)),
                slot.rect.left(),
                TILE_SLIDE_SECS,
            );
            let y = ctx.animate_value_with_time(
                Id::new(("widget-y", slot.kind)),
                slot.rect.top(),
                TILE_SLIDE_SECS,
            );
            Rect::from_min_size(Pos2::new(x, y), slot.rect.size())
        };
        let tile_id = Id::new(("widget", slot.kind, window.id));
        let response = if edit {
            ui.interact(rect, tile_id, Sense::click_and_drag())
        } else {
            ui.interact(rect, tile_id, Sense::hover())
        };
        paint_tile_frame(ui, rect, edit && Some(slot.kind) == dragged_kind);
        let inner = rect.shrink(TILE_PADDING);
        // A salt per tile: the tiles are drawn in a different order while
        // one is dragged, and auto ids would move between frames.
        let builder = UiBuilder::new()
            .id_salt(("tile", slot.kind))
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min));
        let mut child = ui.new_child(builder);
        child.set_clip_rect(child.clip_rect().intersect(rect));
        if tile_header(&mut child, slot.kind, edit) {
            commands.push(Command::RemoveWidget(slot.kind));
        }
        child.add_space(4.0);
        if edit {
            child.disable();
        }
        let outcome = tile_body(&mut child, window, context, state, launcher, slot.kind);
        commands.extend(outcome.commands);
        close |= outcome.close;

        if edit {
            let others: Vec<Slot> = slots
                .iter()
                .filter(|s| s.kind != slot.kind)
                .map(|s| Slot {
                    kind: s.kind,
                    rect: s.rect,
                })
                .collect();
            if response.drag_started()
                && let Some(p) = response.interact_pointer_pos()
            {
                let from = kinds.iter().position(|k| *k == slot.kind).unwrap_or(0);
                launcher.drag = Some(WidgetDrag {
                    kind: slot.kind,
                    grab: p - rect.min,
                    index: from,
                    from,
                });
            }
            if response.dragged()
                && let Some(drag) = launcher.drag.as_mut()
                && drag.kind == slot.kind
                && let Some(p) = pointer
            {
                drag.index = drop_index(&others, p);
            }
            if response.drag_stopped()
                && let Some(drag) = launcher.drag.take()
                && drag.kind == slot.kind
            {
                if drag.index != drag.from {
                    commands.push(Command::MoveWidget {
                        kind: drag.kind,
                        index: drag.index,
                    });
                }
                // Snap: the slide starts from where the tile was let go.
                let ctx = ui.ctx();
                ctx.animate_value_with_time(Id::new(("widget-x", drag.kind)), rect.left(), 0.0);
                ctx.animate_value_with_time(Id::new(("widget-y", drag.kind)), rect.top(), 0.0);
            }
        }
    }
    if launcher.drag.is_some() {
        ui.ctx().request_repaint();
    }
    Some(close)
}

/// A tile's card: a faint fill and hairline, lifted while dragged.
fn paint_tile_frame(ui: &Ui, rect: Rect, lifted: bool) {
    let visuals = ui.visuals();
    let fill = if lifted {
        blend(visuals.faint_bg_color, visuals.text_color(), 0.06)
    } else {
        visuals.faint_bg_color
    };
    let painter = ui.painter();
    if lifted {
        painter.rect_filled(
            rect.translate(Vec2::new(0.0, 3.0)),
            CornerRadius::same(theme::CORNER + 2),
            Color32::from_black_alpha(40),
        );
    }
    painter.rect(
        rect,
        CornerRadius::same(theme::CORNER + 2),
        fill,
        visuals.widgets.noninteractive.bg_stroke,
        StrokeKind::Inside,
    );
}

/// The tile's heading; in edit mode a handle at its left and a close
/// control at its right. Returns whether the close control was clicked.
fn tile_header(ui: &mut Ui, kind: WidgetKind, edit: bool) -> bool {
    let mut remove = false;
    ui.horizontal(|ui| {
        if edit {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 16.0), Sense::hover());
            let ink = ui.visuals().weak_text_color();
            for row in 0..3 {
                for col in 0..2 {
                    ui.painter().circle_filled(
                        Pos2::new(
                            rect.left() + 3.0 + col as f32 * 5.0,
                            rect.top() + 3.0 + row as f32 * 5.0,
                        ),
                        1.3,
                        ink,
                    );
                }
            }
        }
        ui.label(RichText::new(kind.label()).font(FontId::new(
            13.5,
            egui::FontFamily::Name(theme::MEDIUM.into()),
        )));
        if edit {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                let hovered = response.hovered();
                if hovered {
                    ui.painter()
                        .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                }
                let ink = if hovered {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().weak_text_color()
                };
                let c = rect.center();
                let half = 4.0;
                let stroke = Stroke::new(1.5_f32, ink);
                ui.painter().line_segment(
                    [
                        Pos2::new(c.x - half, c.y - half),
                        Pos2::new(c.x + half, c.y + half),
                    ],
                    stroke,
                );
                ui.painter().line_segment(
                    [
                        Pos2::new(c.x - half, c.y + half),
                        Pos2::new(c.x + half, c.y - half),
                    ],
                    stroke,
                );
                remove = response.on_hover_text("Remove widget").clicked();
            });
        }
    });
    remove
}

/// What a tile's body did this frame.
#[derive(Default)]
struct TileOutcome {
    commands: Vec<Command>,
    close: bool,
}

fn tile_body(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    state: &mut ChromeState,
    launcher: &mut Launcher,
    kind: WidgetKind,
) -> TileOutcome {
    match kind {
        WidgetKind::DriveRecent => drive_tile(ui, window, context),
        WidgetKind::Weather => weather_tile(ui, context, launcher),
        WidgetKind::AskClaude | WidgetKind::AskChatGpt => {
            ask_tile(ui, window, context, launcher, kind)
        }
        WidgetKind::TopSites => top_sites_tile(ui, window, context, state, launcher),
    }
}

/// A question field. Enter opens the template with the question in a
/// new tab and closes the panel.
fn ask_tile(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    launcher: &mut Launcher,
    kind: WidgetKind,
) -> TileOutcome {
    let mut outcome = TileOutcome::default();
    let Some(template) = context.launch_settings.ask_template(kind) else {
        return outcome;
    };
    ui.weak(format!("Opens {} in a new tab.", template.name));
    ui.add_space(6.0);
    let text = launcher.ask.entry(kind).or_default();
    let field = ui
        .scope(|ui| {
            theme::pill_fields(ui);
            ui.add(
                egui::TextEdit::singleline(text)
                    .id(Id::new(("ask", kind, window.id)))
                    .margin(Margin::symmetric(12, 7))
                    .hint_text(format!("Ask {}\u{2026}", template.name))
                    .desired_width(f32::INFINITY),
            )
        })
        .inner;
    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    if field.lost_focus() && enter && !text.trim().is_empty() {
        outcome.commands.push(Command::AskInNewTab {
            window: window.id,
            kind,
            query: text.trim().to_string(),
        });
        outcome.close = true;
    }
    outcome
}

/// The most visited sites: favicon and host, a click opens the site's
/// most visited page; a close control on hover keeps the host off the
/// tile.
fn top_sites_tile(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    state: &mut ChromeState,
    launcher: &mut Launcher,
) -> TileOutcome {
    let mut outcome = TileOutcome::default();
    let sites = launcher.top.get_or_insert_with(|| {
        top_sites(
            context.history,
            &context.launch_settings.hidden_sites,
            TOP_SITES,
        )
    });
    if sites.is_empty() {
        ui.weak("The sites you visit most will appear here.");
        return outcome;
    }
    let mut hidden = None;
    for site in sites.iter() {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());
        let hovered = response.hovered();
        if hovered {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + 4.0 + ROW_ICON / 2.0, rect.center().y),
            Vec2::splat(ROW_ICON),
        );
        let texture = context
            .site_icons
            .get(&site.host)
            .and_then(|favicon| site_icon_texture(ui, state, &site.host, favicon).map(|t| t.id()));
        match texture {
            Some(texture) => {
                ui.painter().image(
                    texture,
                    icon_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => initial_disc(ui, icon_rect, &site.host),
        }
        let text_right = if hovered {
            rect.right() - 24.0
        } else {
            rect.right() - 4.0
        };
        let label = site.host.strip_prefix("www.").unwrap_or(&site.host);
        let galley = ui.painter().layout(
            label.to_string(),
            TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
            (text_right - icon_rect.right() - 8.0).max(10.0),
        );
        ui.painter().galley(
            Pos2::new(
                icon_rect.right() + 8.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            ui.visuals().text_color(),
        );
        if hovered {
            let x_rect = Rect::from_center_size(
                Pos2::new(rect.right() - 12.0, rect.center().y),
                Vec2::splat(16.0),
            );
            let x = ui.interact(x_rect, Id::new(("hide-site", &site.host)), Sense::click());
            let ink = if x.hovered() {
                ui.visuals().text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            let c = x_rect.center();
            let stroke = Stroke::new(1.4_f32, ink);
            ui.painter().line_segment(
                [
                    Pos2::new(c.x - 3.5, c.y - 3.5),
                    Pos2::new(c.x + 3.5, c.y + 3.5),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    Pos2::new(c.x - 3.5, c.y + 3.5),
                    Pos2::new(c.x + 3.5, c.y - 3.5),
                ],
                stroke,
            );
            if x.on_hover_text("Don't show this site").clicked() {
                hidden = Some(site.host.clone());
                continue;
            }
        }
        if response.clicked() {
            outcome.commands.push(Command::NewTab {
                window: window.id,
                url: Some(site.url.clone()),
            });
            outcome.close = true;
        }
        let title = if site.title.is_empty() {
            site.url.to_string()
        } else {
            format!("{}\n{}", site.title, site.url)
        };
        response.on_hover_text(title);
    }
    if let Some(host) = hidden {
        outcome.commands.push(Command::HideTopSite(host));
        launcher.top = None;
    }
    outcome
}

/// A disc with the host's first letter, for a site with no favicon.
fn initial_disc(ui: &Ui, rect: Rect, host: &str) {
    let letter = host
        .strip_prefix("www.")
        .unwrap_or(host)
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase().to_string())
        .unwrap_or_default();
    let fill = blend(ui.visuals().faint_bg_color, ui.visuals().text_color(), 0.25);
    ui.painter()
        .circle_filled(rect.center(), rect.width() / 2.0, fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        letter,
        FontId::proportional(10.0),
        ui.visuals().extreme_bg_color,
    );
}

/// The weather: the place's current temperature, conditions and today's
/// range; before a place is set, a field to name one and the places
/// found for it.
fn weather_tile(ui: &mut Ui, context: &ChromeContext<'_>, launcher: &mut Launcher) -> TileOutcome {
    let mut outcome = TileOutcome::default();
    let settings = &context.launch_settings.weather;
    let Some(place) = &settings.location else {
        ui.weak("Today's weather for a place you choose.");
        ui.add_space(6.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut launcher.place_query)
                .id(Id::new("weather-place-query"))
                .margin(theme::INPUT_MARGIN)
                .hint_text("City or town")
                .desired_width(f32::INFINITY),
        );
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        if field.lost_focus() && enter && !launcher.place_query.trim().is_empty() {
            outcome
                .commands
                .push(Command::LookUpPlace(launcher.place_query.clone()));
        }
        ui.add_space(4.0);
        match &context.launch.places {
            Fetched::Empty => {}
            Fetched::Loading => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.weak("Looking up\u{2026}");
                });
            }
            Fetched::Failed(e) => {
                ui.weak(e);
            }
            Fetched::Ready { value, .. } if value.is_empty() => {
                ui.weak("No place by that name.");
            }
            Fetched::Ready { value, .. } => {
                for candidate in value.iter().take(3) {
                    if ui.link(truncate(&candidate.name, 36)).clicked() {
                        outcome
                            .commands
                            .push(Command::SetWeatherPlace(Some(candidate.clone())));
                        launcher.place_query.clear();
                    }
                }
            }
        }
        return outcome;
    };
    ui.weak(truncate(&place.name, 36));
    match &context.launch.weather {
        Fetched::Empty | Fetched::Loading => {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.weak("Fetching\u{2026}");
            });
        }
        Fetched::Failed(e) => {
            ui.add_space(6.0);
            ui.label(RichText::new(e).small());
            if ui.small_button("Retry").clicked() {
                outcome.commands.push(Command::RefreshLaunch);
            }
        }
        Fetched::Ready { value: report, .. } => {
            ui.add_space(4.0);
            let avail = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(avail, 64.0), Sense::hover());
            let glyph_center = Pos2::new(rect.left() + 28.0, rect.center().y);
            paint_weather(
                ui.painter(),
                glyph_center,
                48.0,
                report.code,
                ui.visuals().text_color(),
                ui.visuals().weak_text_color(),
            );
            ui.painter().text(
                Pos2::new(rect.left() + 66.0, rect.center().y),
                Align2::LEFT_CENTER,
                format!("{}°", report.temperature.round()),
                FontId::new(34.0, egui::FontFamily::Name(theme::MEDIUM.into())),
                ui.visuals().text_color(),
            );
            ui.label(report.code.describe());
            ui.weak(format!(
                "H {}°  L {}°  {}",
                report.high.round(),
                report.low.round(),
                report.units.symbol()
            ));
        }
    }
    outcome
}

/// The conditions as a glyph: a sun, a cloud, or a cloud with rain,
/// snow or a bolt under it, drawn with the painter in `size` points.
fn paint_weather(
    painter: &egui::Painter,
    center: Pos2,
    size: f32,
    code: WeatherCode,
    ink: Color32,
    soft: Color32,
) {
    let s = size / 48.0;
    let stroke = Stroke::new(2.0 * s, ink);
    let sun = |c: Pos2, r: f32| {
        painter.circle_stroke(c, r, stroke);
        for i in 0..8 {
            let a = std::f32::consts::TAU * i as f32 / 8.0;
            let (sin, cos) = a.sin_cos();
            painter.line_segment(
                [
                    Pos2::new(c.x + cos * (r + 3.0 * s), c.y + sin * (r + 3.0 * s)),
                    Pos2::new(c.x + cos * (r + 7.0 * s), c.y + sin * (r + 7.0 * s)),
                ],
                stroke,
            );
        }
    };
    let cloud = |c: Pos2, w: f32| {
        // Three lobes over a flat base.
        let r = w * 0.22;
        painter.circle_filled(Pos2::new(c.x - w * 0.22, c.y + r * 0.2), r, soft);
        painter.circle_filled(Pos2::new(c.x + w * 0.05, c.y - r * 0.5), r * 1.25, soft);
        painter.circle_filled(Pos2::new(c.x + w * 0.3, c.y + r * 0.2), r * 0.95, soft);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(c.x - w * 0.22, c.y + r * 0.2),
                Pos2::new(c.x + w * 0.3, c.y + r * 1.2),
            ),
            CornerRadius::same((r * 0.6) as u8),
            soft,
        );
    };
    let drops = |c: Pos2, n: usize, dy: f32| {
        for i in 0..n {
            let x = c.x - 8.0 * s + i as f32 * 8.0 * s;
            painter.line_segment(
                [
                    Pos2::new(x + 1.5 * s, c.y + dy),
                    Pos2::new(x - 1.5 * s, c.y + dy + 7.0 * s),
                ],
                stroke,
            );
        }
    };
    match code {
        WeatherCode::Clear => sun(center, 9.0 * s),
        WeatherCode::PartlyCloudy => {
            sun(center + Vec2::new(-7.0 * s, -8.0 * s), 6.5 * s);
            cloud(center + Vec2::new(4.0 * s, 6.0 * s), 30.0 * s);
        }
        WeatherCode::Cloudy => cloud(center, 36.0 * s),
        WeatherCode::Fog => {
            cloud(center + Vec2::new(0.0, -6.0 * s), 32.0 * s);
            for i in 0..2 {
                let y = center.y + 10.0 * s + i as f32 * 5.0 * s;
                painter.line_segment(
                    [
                        Pos2::new(center.x - 12.0 * s, y),
                        Pos2::new(center.x + 12.0 * s, y),
                    ],
                    stroke,
                );
            }
        }
        WeatherCode::Drizzle => {
            cloud(center + Vec2::new(0.0, -7.0 * s), 32.0 * s);
            for i in 0..3 {
                let x = center.x - 8.0 * s + i as f32 * 8.0 * s;
                painter.circle_filled(Pos2::new(x, center.y + 12.0 * s), 1.6 * s, ink);
            }
        }
        WeatherCode::Rain => {
            cloud(center + Vec2::new(0.0, -7.0 * s), 32.0 * s);
            drops(center, 3, 8.0 * s);
        }
        WeatherCode::Showers => {
            cloud(center + Vec2::new(0.0, -7.0 * s), 32.0 * s);
            drops(center, 2, 8.0 * s);
        }
        WeatherCode::Snow => {
            cloud(center + Vec2::new(0.0, -7.0 * s), 32.0 * s);
            for i in 0..3 {
                let x = center.x - 8.0 * s + i as f32 * 8.0 * s;
                let c = Pos2::new(x, center.y + 12.0 * s);
                painter.circle_stroke(c, 2.2 * s, Stroke::new(1.4 * s, ink));
            }
        }
        WeatherCode::Thunderstorm => {
            cloud(center + Vec2::new(0.0, -8.0 * s), 32.0 * s);
            let bolt = [
                Pos2::new(center.x + 2.0 * s, center.y + 2.0 * s),
                Pos2::new(center.x - 3.0 * s, center.y + 10.0 * s),
                Pos2::new(center.x + 1.0 * s, center.y + 10.0 * s),
                Pos2::new(center.x - 2.0 * s, center.y + 18.0 * s),
            ];
            painter.add(egui::Shape::line(bolt.to_vec(), Stroke::new(2.4 * s, ink)));
        }
    }
}

/// The recent Drive files, or the way to connect an account.
fn drive_tile(ui: &mut Ui, window: &Window, context: &ChromeContext<'_>) -> TileOutcome {
    let mut outcome = TileOutcome::default();
    match &context.launch.drive {
        DriveState::Unavailable => {
            ui.weak("Not available in this build.");
        }
        DriveState::Disconnected => {
            ui.weak("See the Drive files you opened last.");
            ui.add_space(8.0);
            if ui.button("Connect Google Drive").clicked() {
                outcome.commands.push(Command::ConnectGoogle(window.id));
            }
        }
        DriveState::Connecting => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0));
                ui.weak("Finish signing in to Google in the new tab.");
            });
            ui.add_space(6.0);
            if ui.small_button("Cancel").clicked() {
                outcome.commands.push(Command::DisconnectGoogle);
            }
        }
        DriveState::Connected { files, .. } => match files {
            Fetched::Empty | Fetched::Loading => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.weak("Loading\u{2026}");
                });
            }
            Fetched::Failed(e) => {
                ui.label(RichText::new(e).small());
                if ui.small_button("Retry").clicked() {
                    outcome.commands.push(Command::RefreshLaunch);
                }
            }
            Fetched::Ready { value, .. } if value.is_empty() => {
                ui.weak("Nothing opened lately.");
            }
            Fetched::Ready { value, .. } => {
                let now = osmena_core::history::unix_now();
                for file in value.iter().take(5) {
                    if drive_row(ui, file, now).clicked() {
                        outcome.commands.push(Command::NewTab {
                            window: window.id,
                            url: Some(file.url.clone()),
                        });
                        outcome.close = true;
                    }
                }
            }
        },
    }
    outcome
}

/// One Drive file: a glyph in its kind's colour, the name, and when it
/// was opened, right-aligned and weak.
fn drive_row(ui: &mut Ui, file: &osmena_core::DriveFile, now: u64) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let glyph = Rect::from_center_size(
        Pos2::new(rect.left() + 4.0 + 9.0, rect.center().y),
        Vec2::splat(18.0),
    );
    let (fill, ink) = drive_colors(file.kind);
    ui.painter().rect_filled(glyph, 4.0, fill);
    ui.painter().text(
        glyph.center(),
        Align2::CENTER_CENTER,
        file.kind.initial(),
        FontId::new(
            if file.kind == DriveKind::Pdf {
                7.0
            } else {
                10.0
            },
            egui::FontFamily::Name(theme::MEDIUM.into()),
        ),
        ink,
    );
    let when = if file.viewed == 0 {
        String::new()
    } else {
        describe_ago(file.viewed, now)
    };
    let small = TextStyle::Small.resolve(ui.style());
    let when_galley = ui
        .painter()
        .layout_no_wrap(when, small, ui.visuals().weak_text_color());
    let when_left = rect.right() - 6.0 - when_galley.size().x;
    ui.painter().galley(
        Pos2::new(when_left, rect.center().y - when_galley.size().y / 2.0),
        when_galley,
        ui.visuals().weak_text_color(),
    );
    let name = ui.painter().layout(
        truncate(&file.name, NAME_MAX_CHARS),
        TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
        (when_left - 8.0 - glyph.right() - 8.0).max(10.0),
    );
    ui.painter().galley(
        Pos2::new(glyph.right() + 8.0, rect.center().y - name.size().y / 2.0),
        name,
        ui.visuals().text_color(),
    );
    response.on_hover_text(&file.name)
}

/// The fill and letter colour of a Drive kind's glyph: the colours
/// people know the apps by, muted enough for either theme.
fn drive_colors(kind: DriveKind) -> (Color32, Color32) {
    match kind {
        DriveKind::Document => (Color32::from_rgb(66, 133, 244), Color32::WHITE),
        DriveKind::Spreadsheet => (Color32::from_rgb(52, 168, 83), Color32::WHITE),
        DriveKind::Presentation => (Color32::from_rgb(251, 188, 5), Color32::from_gray(40)),
        DriveKind::Form => (Color32::from_rgb(123, 31, 162), Color32::WHITE),
        DriveKind::Folder => (Color32::from_gray(140), Color32::WHITE),
        DriveKind::Pdf => (Color32::from_rgb(234, 67, 53), Color32::WHITE),
        DriveKind::Image => (Color32::from_rgb(0, 150, 136), Color32::WHITE),
        DriveKind::Other => (Color32::from_gray(120), Color32::WHITE),
    }
}

/// "Just now", "5 min ago", "3 h ago", "Yesterday", "4 days ago",
/// "2 mo ago".
pub(crate) fn describe_ago(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..=59 => "Just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        86_400..=172_799 => "Yesterday".to_string(),
        172_800..=2_591_999 => format!("{} days ago", secs / 86_400),
        _ => format!("{} mo ago", secs / 2_592_000),
    }
}

/// The Launch Settings, drawn above the tiles in edit mode.
fn settings_view(
    ui: &mut Ui,
    window: &Window,
    context: &ChromeContext<'_>,
    launcher: &mut Launcher,
    commands: &mut Vec<Command>,
) {
    ui.label(RichText::new("Launch settings").font(FontId::new(
        16.0,
        egui::FontFamily::Name(theme::MEDIUM.into()),
    )));
    ui.add_space(6.0);
    let form = &mut launcher.form;
    egui::Grid::new(("launch-settings", window.id))
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            // The search engine: the presets as chips, or Custom with
            // its two fields.
            ui.label("Search engine");
            ui.vertical(|ui| {
                let current = context.search;
                let preset = current.preset_index();
                let custom = form.custom || preset.is_none();
                ui.horizontal_wrapped(|ui| {
                    for (i, (name, _)) in UrlTemplate::SEARCH_PRESETS.iter().enumerate() {
                        if ui
                            .selectable_label(!custom && preset == Some(i), *name)
                            .clicked()
                        {
                            form.custom = false;
                            commands.push(Command::SetSearch(UrlTemplate::preset(i)));
                        }
                    }
                    if ui.selectable_label(custom, "Custom").clicked() {
                        form.custom = true;
                    }
                });
                if custom {
                    let name_id = Id::new(("search-name", window.id));
                    let url_id = Id::new(("search-url", window.id));
                    let editing = ui.memory(|m| m.has_focus(name_id) || m.has_focus(url_id));
                    if !editing && form.synced_search.as_ref() != Some(current) {
                        form.search_name = current.name.clone();
                        form.search_url = current.url.clone();
                        form.synced_search = Some(current.clone());
                    }
                    let name = ui.add(
                        egui::TextEdit::singleline(&mut form.search_name)
                            .id(name_id)
                            .margin(theme::INPUT_MARGIN)
                            .hint_text("Name")
                            .desired_width(200.0),
                    );
                    let url = ui.add(
                        egui::TextEdit::singleline(&mut form.search_url)
                            .id(url_id)
                            .margin(theme::INPUT_MARGIN)
                            .hint_text("https://example.com/search?q={query}")
                            .desired_width(f32::INFINITY),
                    );
                    if (name.lost_focus() || url.lost_focus()) && !form.search_url.trim().is_empty()
                    {
                        let template =
                            UrlTemplate::new(form.search_name.trim(), form.search_url.trim());
                        if template != *current {
                            commands.push(Command::SetSearch(template));
                        }
                    }
                }
                ui.label(
                    RichText::new(
                        "Where the field above and the address bar send text that is not an \
                         address. {query} stands for the text.",
                    )
                    .small()
                    .weak(),
                );
            });
            ui.end_row();

            for kind in [WidgetKind::AskClaude, WidgetKind::AskChatGpt] {
                let Some(template) = context.launch_settings.ask_template(kind) else {
                    continue;
                };
                ui.label(kind.label());
                ui.vertical(|ui| {
                    let id = Id::new(("ask-url", kind, window.id));
                    let editing = ui.memory(|m| m.has_focus(id));
                    let synced = form.synced_ask.get(&kind);
                    if !editing && synced != Some(&template.url) {
                        form.ask.insert(kind, template.url.clone());
                        form.synced_ask.insert(kind, template.url.clone());
                    }
                    let text = form.ask.entry(kind).or_default();
                    let field = ui.add(
                        egui::TextEdit::singleline(text)
                            .id(id)
                            .margin(theme::INPUT_MARGIN)
                            .desired_width(f32::INFINITY),
                    );
                    if field.lost_focus() && !text.trim().is_empty() && text.trim() != template.url
                    {
                        commands.push(Command::SetAskTemplate {
                            kind,
                            template: UrlTemplate::new(&template.name, text.trim()),
                        });
                    }
                });
                ui.end_row();
            }

            ui.label("Weather");
            ui.vertical(|ui| {
                ui.horizontal(|ui| match &context.launch_settings.weather.location {
                    Some(place) => {
                        ui.label(truncate(&place.name, 40));
                        if ui.small_button("Change").clicked() {
                            commands.push(Command::SetWeatherPlace(None));
                        }
                    }
                    None => {
                        ui.weak("Set a place on the Weather tile.");
                    }
                });
                ui.horizontal(|ui| {
                    let units = context.launch_settings.weather.units;
                    for (choice, label) in [(Units::Fahrenheit, "°F"), (Units::Celsius, "°C")] {
                        if ui.selectable_label(units == choice, label).clicked() && units != choice
                        {
                            commands.push(Command::SetWeatherUnits(choice));
                        }
                    }
                });
                // The data is CC BY 4.0, which asks for this line.
                ui.label(
                    RichText::new("Weather data by Open-Meteo.com")
                        .small()
                        .weak(),
                );
            });
            ui.end_row();

            if context.brand.google_client_id.is_some() {
                ui.label("Google Drive");
                ui.horizontal(|ui| match &context.launch.drive {
                    DriveState::Unavailable => {
                        ui.weak("Not available in this build.");
                    }
                    DriveState::Disconnected => {
                        if ui.button("Connect Google Drive").clicked() {
                            commands.push(Command::ConnectGoogle(window.id));
                        }
                    }
                    DriveState::Connecting => {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.weak("Waiting for Google\u{2026}");
                        if ui.small_button("Cancel").clicked() {
                            commands.push(Command::DisconnectGoogle);
                        }
                    }
                    DriveState::Connected { account, .. } => {
                        ui.label(account);
                        if ui.small_button("Disconnect").clicked() {
                            commands.push(Command::DisconnectGoogle);
                        }
                    }
                });
                ui.end_row();
            }
        });
    ui.add_space(10.0);
    ui.label(RichText::new("Widgets").font(FontId::new(
        16.0,
        egui::FontFamily::Name(theme::MEDIUM.into()),
    )));
    ui.weak("Drag a tile to move it; its close control removes it.");
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(slots: &[Slot]) -> Vec<(WidgetKind, f32, f32, f32)> {
        slots
            .iter()
            .map(|s| (s.kind, s.rect.left(), s.rect.top(), s.rect.width()))
            .collect()
    }

    #[test]
    fn the_grid_wraps_spans_and_clamps_them() {
        let origin = Pos2::new(0.0, 0.0);
        // Three columns of 100: Drive takes two, Weather the third, the
        // next three the second row.
        let three = layout(&WidgetKind::ALL, 3, 100.0, origin);
        assert_eq!(
            kinds(&three),
            vec![
                (WidgetKind::DriveRecent, 0.0, 0.0, 210.0),
                (WidgetKind::Weather, 220.0, 0.0, 100.0),
                (WidgetKind::AskClaude, 0.0, 200.0, 100.0),
                (WidgetKind::AskChatGpt, 110.0, 200.0, 100.0),
                (WidgetKind::TopSites, 220.0, 200.0, 100.0),
            ]
        );
        // Two columns: Drive alone on its row's two columns.
        let two = layout(&WidgetKind::ALL, 2, 100.0, origin);
        assert_eq!(two[1].rect.min, Pos2::new(0.0, 200.0));
        assert_eq!(two[2].rect.min, Pos2::new(110.0, 200.0));
        // One column: every tile one wide, one per row.
        let one = layout(&WidgetKind::ALL, 1, 100.0, origin);
        assert!(one.iter().all(|s| s.rect.width() == 100.0));
        assert_eq!(one[4].rect.top(), 800.0);
        // A span that does not fit wraps: Weather, then Drive on its own
        // row.
        let wrapped = layout(
            &[
                WidgetKind::Weather,
                WidgetKind::DriveRecent,
                WidgetKind::TopSites,
            ],
            3,
            100.0,
            origin,
        );
        assert_eq!(wrapped[1].rect.min, Pos2::new(110.0, 0.0));
        assert_eq!(wrapped[2].rect.min, Pos2::new(0.0, 200.0));
    }

    #[test]
    fn columns_follow_the_width() {
        assert_eq!(columns_for(900.0), 3);
        assert_eq!(columns_for(720.0), 3);
        assert_eq!(columns_for(600.0), 2);
        assert_eq!(columns_for(300.0), 1);
    }

    #[test]
    fn a_drop_lands_after_the_tiles_above_and_left_of_the_pointer() {
        let origin = Pos2::new(0.0, 0.0);
        let others = layout(
            &[
                WidgetKind::Weather,
                WidgetKind::AskClaude,
                WidgetKind::AskChatGpt,
                WidgetKind::TopSites,
            ],
            3,
            100.0,
            origin,
        );
        // Over the first tile's left half: first.
        assert_eq!(drop_index(&others, Pos2::new(20.0, 50.0)), 0);
        // Right of the second tile's centre, same row: after two.
        assert_eq!(drop_index(&others, Pos2::new(180.0, 50.0)), 2);
        // On the second row, left: after the whole first row.
        assert_eq!(drop_index(&others, Pos2::new(10.0, 250.0)), 3);
        // Below everything: last.
        assert_eq!(drop_index(&others, Pos2::new(500.0, 900.0)), 4);
    }

    #[test]
    fn ago_reads_naturally() {
        assert_eq!(describe_ago(1000, 1030), "Just now");
        assert_eq!(describe_ago(1000, 1000 + 5 * 60), "5 min ago");
        assert_eq!(describe_ago(1000, 1000 + 3 * 3600), "3 h ago");
        assert_eq!(describe_ago(1000, 1000 + 30 * 3600), "Yesterday");
        assert_eq!(describe_ago(1000, 1000 + 4 * 86_400), "4 days ago");
        assert_eq!(describe_ago(1000, 1000 + 70 * 86_400), "2 mo ago");
        assert_eq!(describe_ago(2000, 1000), "Just now");
    }
}
