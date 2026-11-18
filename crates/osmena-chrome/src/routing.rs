//! The window's routing decisions as pure functions and small state
//! machines: which side gets a pointer event, whether a key event is a
//! keymap command, an extension's command, or goes to egui or a view, and
//! where the page area and the side panel sit. The window reads egui
//! and winit state and feeds it in here, so every rule has a unit test.

use std::collections::{HashMap, HashSet};

use dpi::PhysicalSize;
use egui::{Rect, pos2};
use keyboard_types::{Code, Modifiers};
use osmena_core::{CommandKind, ExtensionSupport, Keymap, TabId, Window};
use winit::event::MouseButton;
use winit::window::ResizeDirection;

use crate::input::{self, ExtensionChord, KeyRoute};
use crate::theme;

/// Who receives a pointer event: the chrome (egui), the active tab's
/// page, or the side panel's view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerTarget {
    Egui,
    Page,
    Panel,
}

/// What the window knows about the pointer's position and egui's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerContext {
    /// The pointer is above the page area or over the side panel's
    /// header, or its position is unknown.
    pub over_chrome: bool,
    /// The pointer is over the side panel's view ([`ContentRegion::Panel`]).
    pub over_panel: bool,
    /// egui draws the page area itself (the crash panel, a block page).
    /// The side panel beside it is still the engine's.
    pub content_is_egui: bool,
    /// The pointer is over an interactable egui layer (menus, the error
    /// overlay).
    pub over_egui: bool,
    /// A popup is open, so a click outside it must reach egui to close it.
    pub popup_open: bool,
}

/// The side that gets a pointer event. A capture (set by a press) wins over
/// everything else; otherwise egui owns the pointer over the chrome and over
/// any egui layer or open popup, the side panel's view owns it over its own
/// rectangle, and the page owns the rest unless egui draws the page area.
pub fn route_pointer(captured: Option<PointerTarget>, ctx: PointerContext) -> PointerTarget {
    if let Some(captured) = captured {
        return captured;
    }
    if ctx.over_chrome || ctx.over_egui || ctx.popup_open {
        PointerTarget::Egui
    } else if ctx.over_panel {
        PointerTarget::Panel
    } else if ctx.content_is_egui {
        PointerTarget::Egui
    } else {
        PointerTarget::Page
    }
}

/// Pointer capture: the side that took the first button press gets every
/// pointer event until the last pressed button is released, so a drag that
/// crosses the toolbar edge, or the edge between the page and the side
/// panel, ends where it started, and holding a second button does not hand
/// the pointer over in the middle.
///
/// A capture held by the page or the panel also remembers the view that
/// got the press (a tab, or the panel's view): its moves and release go to
/// that view even if another tab became active or another panel is shown
/// meanwhile (a press in tab A, then Ctrl+Tab makes B active, then the
/// release: A gets it, so A never keeps a button down and B never sees a
/// release it did not start).
#[derive(Debug, Default)]
pub struct PointerCapture {
    pressed: HashSet<MouseButton>,
    target: Option<PointerTarget>,
    /// The view holding a page or panel capture.
    tab: Option<TabId>,
}

impl PointerCapture {
    /// The side holding the capture, if any.
    pub fn captured(&self) -> Option<PointerTarget> {
        self.target
    }

    /// The view holding the capture, when the page or the panel holds it:
    /// the tab, or the panel's view.
    pub fn tab(&self) -> Option<TabId> {
        self.tab
    }

    /// A button went down while `current` was the pointer's side and
    /// `tab` was the view a press there would reach (the pointer's tab
    /// for the page, the shown panel's view for the panel). Returns the
    /// side that gets this press.
    pub fn press(
        &mut self,
        button: MouseButton,
        current: PointerTarget,
        tab: Option<TabId>,
    ) -> PointerTarget {
        if self.pressed.is_empty() {
            self.target = Some(current);
            self.tab = (current != PointerTarget::Egui).then_some(tab).flatten();
        }
        self.pressed.insert(button);
        self.target.unwrap_or(current)
    }

    /// A button went up while `current` was the pointer's side. Returns the
    /// side that gets this release. The capture ends with the last button.
    pub fn release(&mut self, button: MouseButton, current: PointerTarget) -> PointerTarget {
        let target = self.target.unwrap_or(current);
        self.pressed.remove(&button);
        if self.pressed.is_empty() {
            self.target = None;
            self.tab = None;
        }
        target
    }

    /// Forget all buttons, for example when the system took the pointer
    /// for a window drag and the release will never arrive.
    pub fn clear(&mut self) {
        self.pressed.clear();
        self.target = None;
        self.tab = None;
    }

    /// End the capture because the window lost focus: the releases will
    /// arrive elsewhere. Returns the view and the buttons it holds down
    /// when the page or the panel holds the capture, so the window can
    /// release them there; egui forgets its own on focus loss.
    pub fn release_all(&mut self) -> Option<(TabId, Vec<MouseButton>)> {
        let held = self
            .tab
            .map(|tab| (tab, self.pressed.iter().copied().collect()));
        self.clear();
        held
    }
}

/// Where a key event goes, as decided by [`KeyRouter::route`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyTarget {
    /// egui gets the key.
    Egui,
    /// The key fires a keymap command.
    Command(CommandKind),
    /// The key is an enabled extension's shortcut, and no keymap chord.
    ExtensionCommand { extension: String, name: String },
    /// The page gets the key in this view: the view that received its
    /// press, a tab or the side panel's.
    Page(TabId),
}

/// One key event as the window reads it from winit.
#[derive(Clone, Copy, Debug)]
pub struct KeyStroke {
    pub code: Code,
    pub modifiers: Modifiers,
    pub pressed: bool,
    pub repeat: bool,
}

/// Decides where key events go and remembers which side received each
/// press, so that side also receives the repeats and the release. The page
/// never sees half a key stroke (for example a key pressed in another window
/// before focus moved here, or a chord that fired a command), and egui never
/// misses the release of a key it saw go down (after Enter submits the
/// address bar, the field loses focus before the Enter release arrives).
///
/// A key the page holds remembers the view that received its press: the
/// active tab, or the side panel's view while that has the keyboard. Its
/// repeats and release go to that view even if another became active or
/// took the keyboard meanwhile (Meta pressed in tab A, then Cmd+Shift+]
/// makes B active, then Meta is released: A gets the release, so A never
/// keeps a key down; Shift pressed in the panel, then a click in the page:
/// the panel gets the release). If that view has gone, the hold is dropped
/// and nothing is delivered.
///
/// The full rule, in order:
/// 1. A release goes to the side that got the press, whoever has focus now;
///    a release nobody here saw pressed goes to egui while it has focus and
///    nowhere otherwise.
/// 2. A press or repeat is first put to [`input::route_key`]: the initial
///    press of a chord bound in the keymap is a command, and failing that
///    the initial press of an enabled extension's chord is that extension's
///    command (while egui has focus, only chords with Cmd or Ctrl, for
///    both). A command takes its key away from the page, so the chord's
///    repeats and release never reach it.
/// 3. A press or repeat of a key the page holds goes to the view that holds
///    it, or is dropped if that view is gone.
/// 4. egui gets the key while it has focus, or if it got the key's press.
/// 5. A repeat nobody here saw pressed is dropped.
/// 6. Otherwise the view that has the keyboard gets the press and holds the
///    key until its release.
#[derive(Debug, Default)]
pub struct KeyRouter {
    /// Keys the page holds, each with the view that received its press.
    page_keys: HashMap<Code, TabId>,
    egui_keys: HashSet<Code>,
}

impl KeyRouter {
    /// Route one key event. `active` is the view that has the keyboard,
    /// which gets new page presses: the window's active tab, or the shown
    /// side panel's view after a click in it ([`keyboard_view`]). `live`
    /// says whether a view still exists (a tab of the window, or a side
    /// panel's view). `extensions` are the enabled extensions' shortcuts.
    /// `None` means nobody sees the event.
    pub fn route(
        &mut self,
        stroke: KeyStroke,
        egui_wants_keyboard: bool,
        active: Option<TabId>,
        live: impl Fn(TabId) -> bool,
        keymap: &Keymap,
        extensions: &[ExtensionChord<'_>],
    ) -> Option<KeyTarget> {
        let KeyStroke {
            code,
            modifiers,
            pressed,
            repeat,
        } = stroke;
        if !pressed {
            if let Some(tab) = self.page_keys.remove(&code) {
                return live(tab).then_some(KeyTarget::Page(tab));
            }
            if self.egui_keys.remove(&code) {
                return Some(KeyTarget::Egui);
            }
            return egui_wants_keyboard.then_some(KeyTarget::Egui);
        }
        let first_cut = input::route_key(
            egui_wants_keyboard,
            true,
            repeat,
            modifiers,
            code,
            keymap,
            extensions,
        );
        match first_cut {
            KeyRoute::Command(kind) => {
                self.page_keys.remove(&code);
                return Some(KeyTarget::Command(kind));
            }
            KeyRoute::Extension(index) => {
                self.page_keys.remove(&code);
                let chord = &extensions[index];
                return Some(KeyTarget::ExtensionCommand {
                    extension: chord.extension.to_string(),
                    name: chord.name.to_string(),
                });
            }
            KeyRoute::Egui | KeyRoute::Page => {}
        }
        if let Some(&tab) = self.page_keys.get(&code) {
            if live(tab) {
                return Some(KeyTarget::Page(tab));
            }
            // The tab closed while the key was held, so the hold goes too.
            self.page_keys.remove(&code);
            if repeat {
                return None;
            }
        }
        if first_cut == KeyRoute::Egui || self.egui_keys.contains(&code) {
            self.egui_keys.insert(code);
            return Some(KeyTarget::Egui);
        }
        if repeat {
            return None;
        }
        let tab = active?;
        self.page_keys.insert(code, tab);
        Some(KeyTarget::Page(tab))
    }

    /// Forget every key because the window lost focus: the releases will
    /// arrive elsewhere. Returns the keys the page holds, with their views,
    /// so the window can release them there; egui forgets its own held
    /// keys on `WindowFocused(false)`.
    pub fn release_all(&mut self) -> Vec<(Code, TabId)> {
        self.egui_keys.clear();
        self.page_keys.drain().collect()
    }
}

/// The view that has the keyboard: the side panel's view `in_panel` after
/// a click in it, for as long as that same view is the panel `shown`;
/// otherwise the window's `active` tab. A panel that is closed, replaced
/// by another tab's, or not drawn (full screen, a narrow window) never
/// keeps the keyboard.
pub fn keyboard_view(
    active: Option<TabId>,
    in_panel: Option<TabId>,
    shown: Option<TabId>,
) -> Option<TabId> {
    match in_panel {
        Some(view) if shown == Some(view) => Some(view),
        _ => active,
    }
}

/// What to tell the engine when the side panel a window draws changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanelTransition {
    /// The view that was drawn and no longer is: `TabHost::hide` it, if
    /// the host still has it.
    pub hide: Option<TabId>,
    /// The view that is drawn from now on: `TabHost::show` it.
    pub show: Option<TabId>,
}

/// From the panel view the last frame drew to the one this frame draws
/// (each `None` when no panel is drawn: none open, full screen, a window
/// too narrow). The same view both times is nothing to do.
pub fn panel_transition(previous: Option<TabId>, now: Option<TabId>) -> PanelTransition {
    if previous == now {
        return PanelTransition::default();
    }
    PanelTransition {
        hide: previous,
        show: now,
    }
}

/// The page area and the side panel beside it, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentSplit {
    /// The active tab's page: the whole content area while no panel is
    /// drawn.
    pub page: Rect,
    /// The side panel, when one is drawn.
    pub panel: Option<PanelRects>,
}

/// The two parts of a drawn side panel, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelRects {
    /// The strip the chrome draws: the extension's name and the close
    /// control.
    pub header: Rect,
    /// Where the engine's view of the panel's page is painted.
    pub view: Rect,
}

/// Whether `window` draws a side panel: the engine shows them
/// (`support` is `EngineInfo::extensions`), the window is not in full
/// screen, which shows the page alone, and the model has a panel for
/// the active tab or the window. This is [`split_content`]'s
/// `panel_shown`; whether the panel also fits is `split_content`'s to
/// say. The model keeps the panel through full screen, so leaving it
/// brings the panel back.
pub fn panel_shown(window: &Window, support: Option<&ExtensionSupport>) -> bool {
    support.is_some_and(|support| support.side_panels)
        && !window.fullscreen
        && window.shown_panel().is_some()
}

/// Divide the content area (what is left under the toolbar) between the
/// page and the side panel. The panel is [`theme::SIDE_PANEL_WIDTH`] wide
/// on the right, its header [`theme::SIDE_PANEL_HEADER`] tall, and the
/// page keeps its left edge, so the page still starts where it always
/// did. There is no panel when `panel_shown` is false (the window shows
/// none, the engine has none, or the window is full screen: the caller
/// says, with [`panel_shown`]) or when the content area is narrower than the panel plus
/// [`theme::SIDE_PANEL_MIN_PAGE`].
///
/// The rectangles are in points and share their edges exactly; put each
/// through [`view_pixels`] (or [`content_pixels`]) and neighbours land on
/// the same pixel column and row, with no gap and no overlap. The
/// chrome's drawing, the shell's sizing and the pointer routing all read
/// this one function.
pub fn split_content(content: Rect, panel_shown: bool) -> ContentSplit {
    if !panel_shown || content.width() < theme::SIDE_PANEL_WIDTH + theme::SIDE_PANEL_MIN_PAGE {
        return ContentSplit {
            page: content,
            panel: None,
        };
    }
    let boundary = content.max.x - theme::SIDE_PANEL_WIDTH;
    let header_foot = (content.min.y + theme::SIDE_PANEL_HEADER).min(content.max.y);
    ContentSplit {
        page: Rect::from_min_max(content.min, pos2(boundary, content.max.y)),
        panel: Some(PanelRects {
            header: Rect::from_min_max(
                pos2(boundary, content.min.y),
                pos2(content.max.x, header_foot),
            ),
            view: Rect::from_min_max(pos2(boundary, header_foot), content.max),
        }),
    }
}

/// A view's rectangle in physical pixels from the window's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPixels {
    pub left: f32,
    pub top: f32,
    /// Never zero in either direction: an engine's surface cannot be.
    pub size: PhysicalSize<u32>,
}

impl ViewPixels {
    /// The window point `(x, y)` (physical pixels) from this view's own
    /// top left, which is how the view's engine wants its pointer events.
    /// Negative, or beyond the size, while a captured drag is outside it.
    pub fn local(&self, x: f32, y: f32) -> (f32, f32) {
        (x - self.left, y - self.top)
    }
}

/// `rect` on the pixel grid. Every edge is rounded to a whole pixel first
/// and the size is the difference, so the size covers exactly the pixel
/// rows and columns the paint fills, and two rectangles that share an
/// edge in points share it in pixels.
pub fn view_pixels(rect: Rect, pixels_per_point: f32) -> ViewPixels {
    let left = (rect.min.x * pixels_per_point).round();
    let right = (rect.max.x * pixels_per_point).round();
    let top = (rect.min.y * pixels_per_point).round();
    let bottom = (rect.max.y * pixels_per_point).round();
    ViewPixels {
        left,
        top,
        size: PhysicalSize::new(
            (right - left).max(1.0) as u32,
            (bottom - top).max(1.0) as u32,
        ),
    }
}

/// The page area in physical pixels: its top edge, which pointer routing
/// compares against, and its size. See [`view_pixels`].
pub fn content_pixels(rect: Rect, pixels_per_point: f32) -> (f32, PhysicalSize<u32>) {
    let pixels = view_pixels(rect, pixels_per_point);
    (pixels.top, pixels.size)
}

/// What the pointer is over, by position alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentRegion {
    /// Above the content area: the strip and the toolbar.
    Chrome,
    /// The active tab's page.
    Page,
    /// The side panel's header, which the chrome draws.
    PanelHeader,
    /// The side panel's view.
    Panel,
}

/// Where the window point `(x, y)` (physical pixels) is, given the
/// content area's top edge and the drawn panel's view, both from the last
/// frame. The panel takes everything from its left edge rightwards; its
/// header is what lies above the view's top.
pub fn content_region(
    x: f32,
    y: f32,
    content_top: f32,
    panel: Option<ViewPixels>,
) -> ContentRegion {
    if y < content_top {
        return ContentRegion::Chrome;
    }
    match panel {
        Some(view) if x >= view.left => {
            if y < view.top {
                ContentRegion::PanelHeader
            } else {
                ContentRegion::Panel
            }
        }
        _ => ContentRegion::Page,
    }
}

/// Which edge of a frameless window the pointer at `(x, y)` (physical
/// pixels from the window's top-left) is on, within `border` pixels of
/// the edge; `None` inside. The corners take precedence where two edges
/// meet. A maximized or full-screen window has no edges to drag; the
/// caller checks that.
pub fn resize_edge(
    x: f32,
    y: f32,
    size: PhysicalSize<u32>,
    border: f32,
) -> Option<ResizeDirection> {
    let width = size.width as f32;
    let height = size.height as f32;
    if x < 0.0 || y < 0.0 || x >= width || y >= height {
        return None;
    }
    let west = x < border;
    let east = x >= width - border;
    let north = y < border;
    let south = y >= height - border;
    match (north, south, west, east) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (true, _, _, true) => Some(ResizeDirection::NorthEast),
        (_, true, true, _) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::North),
        (_, true, _, _) => Some(ResizeDirection::South),
        (_, _, true, _) => Some(ResizeDirection::West),
        (_, _, _, true) => Some(ResizeDirection::East),
        _ => None,
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;

    const SIZE: PhysicalSize<u32> = PhysicalSize::new(800, 600);

    #[test]
    fn edges_and_corners_within_the_border() {
        let edge = |x, y| resize_edge(x, y, SIZE, 6.0);
        assert_eq!(edge(0.0, 0.0), Some(ResizeDirection::NorthWest));
        assert_eq!(edge(799.0, 5.9), Some(ResizeDirection::NorthEast));
        assert_eq!(edge(2.0, 599.0), Some(ResizeDirection::SouthWest));
        assert_eq!(edge(795.0, 595.0), Some(ResizeDirection::SouthEast));
        assert_eq!(edge(400.0, 3.0), Some(ResizeDirection::North));
        assert_eq!(edge(400.0, 594.0), Some(ResizeDirection::South));
        assert_eq!(edge(1.0, 300.0), Some(ResizeDirection::West));
        assert_eq!(edge(794.0, 300.0), Some(ResizeDirection::East));
    }

    #[test]
    fn the_interior_and_the_outside_are_not_edges() {
        let edge = |x, y| resize_edge(x, y, SIZE, 6.0);
        assert_eq!(edge(6.0, 6.0), None);
        assert_eq!(edge(400.0, 300.0), None);
        assert_eq!(edge(793.0, 593.0), None);
        assert_eq!(edge(-1.0, 300.0), None);
        assert_eq!(edge(400.0, 600.0), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;
    use osmena_core::{KeyChord, Platform};

    use crate::input::ExtensionChord;

    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;
    const TAB_A: TabId = TabId(1);
    const TAB_B: TabId = TabId(2);
    /// A side panel's view.
    const PANEL: TabId = TabId(9);

    fn ctx() -> PointerContext {
        PointerContext {
            over_chrome: false,
            over_panel: false,
            content_is_egui: false,
            over_egui: false,
            popup_open: false,
        }
    }

    #[test]
    fn pointer_over_page_goes_to_page() {
        assert_eq!(route_pointer(None, ctx()), PointerTarget::Page);
    }

    #[test]
    fn pointer_over_chrome_goes_to_egui() {
        let c = PointerContext {
            over_chrome: true,
            ..ctx()
        };
        assert_eq!(route_pointer(None, c), PointerTarget::Egui);
    }

    #[test]
    fn egui_layers_popups_and_crash_panel_take_the_pointer() {
        for c in [
            PointerContext {
                over_egui: true,
                ..ctx()
            },
            PointerContext {
                popup_open: true,
                ..ctx()
            },
            PointerContext {
                content_is_egui: true,
                ..ctx()
            },
        ] {
            assert_eq!(route_pointer(None, c), PointerTarget::Egui, "{c:?}");
        }
    }

    #[test]
    fn a_capture_overrides_the_position() {
        let over_chrome = PointerContext {
            over_chrome: true,
            ..ctx()
        };
        assert_eq!(
            route_pointer(Some(PointerTarget::Page), over_chrome),
            PointerTarget::Page
        );
        assert_eq!(
            route_pointer(Some(PointerTarget::Egui), ctx()),
            PointerTarget::Egui
        );
    }

    #[test]
    fn press_and_release_keep_one_side_for_the_whole_click() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.press(LEFT, PointerTarget::Page, Some(TAB_A)),
            PointerTarget::Page
        );
        assert_eq!(capture.tab(), Some(TAB_A));
        // Dragging onto the toolbar: the pointer is now egui's, but the
        // capture keeps the release with the page.
        let now = route_pointer(
            capture.captured(),
            PointerContext {
                over_chrome: true,
                ..ctx()
            },
        );
        assert_eq!(now, PointerTarget::Page);
        assert_eq!(capture.release(LEFT, now), PointerTarget::Page);
        assert_eq!(capture.captured(), None);
        assert_eq!(capture.tab(), None);
    }

    /// A press in tab A, then a keyboard tab switch makes B active: the
    /// capture still names A, so the window sends A the moves and the
    /// release. A press on the chrome names no tab.
    #[test]
    fn a_page_capture_remembers_the_tab_that_got_the_press() {
        let mut capture = PointerCapture::default();
        capture.press(LEFT, PointerTarget::Page, Some(TAB_A));
        // B is active now; a second button joins A's capture.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Page, Some(TAB_B)),
            PointerTarget::Page
        );
        assert_eq!(capture.tab(), Some(TAB_A));
        capture.release(RIGHT, PointerTarget::Page);
        assert_eq!(capture.tab(), Some(TAB_A));
        capture.release(LEFT, PointerTarget::Page);
        assert_eq!(capture.tab(), None);

        capture.press(LEFT, PointerTarget::Egui, Some(TAB_A));
        assert_eq!(capture.tab(), None);
    }

    /// The reviewer's scenario: left pressed over the page, right pressed
    /// and released, pointer moved over the toolbar, left released over the
    /// toolbar. The page must see the left release, so right's release must
    /// not end the capture.
    #[test]
    fn releasing_one_button_does_not_end_a_capture_held_by_another() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.press(LEFT, PointerTarget::Page, Some(TAB_A)),
            PointerTarget::Page
        );

        // Right press and release while the pointer is still over the page.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Page, Some(TAB_A)),
            PointerTarget::Page
        );
        assert_eq!(
            capture.release(RIGHT, PointerTarget::Page),
            PointerTarget::Page
        );
        assert_eq!(capture.captured(), Some(PointerTarget::Page));

        // The pointer moves over the toolbar with left still held.
        let over_toolbar = PointerContext {
            over_chrome: true,
            ..ctx()
        };
        assert_eq!(
            route_pointer(capture.captured(), over_toolbar),
            PointerTarget::Page
        );

        // Left released over the toolbar still goes to the page.
        let now = route_pointer(capture.captured(), over_toolbar);
        assert_eq!(capture.release(LEFT, now), PointerTarget::Page);
        assert_eq!(capture.captured(), None);
        // After the last release the toolbar owns the pointer again.
        assert_eq!(
            route_pointer(capture.captured(), over_toolbar),
            PointerTarget::Egui
        );
    }

    #[test]
    fn a_second_button_pressed_over_the_chrome_joins_the_page_capture() {
        let mut capture = PointerCapture::default();
        capture.press(LEFT, PointerTarget::Page, Some(TAB_A));
        // A right press while the pointer is over the chrome still belongs
        // to the page capture held by the left button.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Egui, Some(TAB_A)),
            PointerTarget::Page
        );
        assert_eq!(
            capture.release(RIGHT, PointerTarget::Egui),
            PointerTarget::Page
        );
        assert_eq!(
            capture.release(LEFT, PointerTarget::Egui),
            PointerTarget::Page
        );
        assert_eq!(capture.captured(), None);
    }

    #[test]
    fn a_release_without_a_press_here_uses_the_current_side() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.release(LEFT, PointerTarget::Egui),
            PointerTarget::Egui
        );
        assert_eq!(capture.captured(), None);
    }

    #[test]
    fn clearing_forgets_every_button() {
        let mut capture = PointerCapture::default();
        capture.press(LEFT, PointerTarget::Page, Some(TAB_A));
        capture.clear();
        assert_eq!(capture.captured(), None);
        assert_eq!(capture.tab(), None);
        // A later press starts a fresh capture on whichever side it lands.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Egui, Some(TAB_A)),
            PointerTarget::Egui
        );
    }

    /// Focus lost with buttons down in the page: the window gets them
    /// back, with the tab, to release there. A capture held by egui has
    /// nothing to release in a page.
    #[test]
    fn losing_focus_hands_back_the_buttons_the_page_holds() {
        let mut capture = PointerCapture::default();
        capture.press(LEFT, PointerTarget::Page, Some(TAB_A));
        capture.press(RIGHT, PointerTarget::Egui, Some(TAB_B));
        let (tab, mut buttons) = capture.release_all().expect("the page held two buttons");
        buttons.sort_by_key(|b| format!("{b:?}"));
        assert_eq!(tab, TAB_A);
        assert_eq!(buttons, [LEFT, RIGHT]);
        assert_eq!(capture.captured(), None);

        capture.press(LEFT, PointerTarget::Egui, Some(TAB_A));
        assert_eq!(capture.release_all(), None);
        assert_eq!(capture.captured(), None);
    }

    #[test]
    fn pointer_over_the_panel_goes_to_the_panel() {
        let c = PointerContext {
            over_panel: true,
            ..ctx()
        };
        assert_eq!(route_pointer(None, c), PointerTarget::Panel);
    }

    /// The crash panel and the block page cover the page's rectangle
    /// only: the side panel beside them still gets its pointer.
    #[test]
    fn the_chromes_own_page_keeps_the_page_area_but_not_the_panel() {
        let over_page = PointerContext {
            content_is_egui: true,
            ..ctx()
        };
        assert_eq!(route_pointer(None, over_page), PointerTarget::Egui);
        let over_panel = PointerContext {
            content_is_egui: true,
            over_panel: true,
            ..ctx()
        };
        assert_eq!(route_pointer(None, over_panel), PointerTarget::Panel);
    }

    #[test]
    fn egui_layers_and_popups_take_the_pointer_over_the_panel_too() {
        for c in [
            PointerContext {
                over_panel: true,
                over_egui: true,
                ..ctx()
            },
            PointerContext {
                over_panel: true,
                popup_open: true,
                ..ctx()
            },
        ] {
            assert_eq!(route_pointer(None, c), PointerTarget::Egui, "{c:?}");
        }
    }

    /// A text selection dragged from the page across the boundary: the
    /// page keeps every move and the release, in the tab that got the
    /// press, and the panel sees nothing of it.
    #[test]
    fn a_press_in_the_page_dragged_over_the_panel_stays_with_the_page() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.press(LEFT, PointerTarget::Page, Some(TAB_A)),
            PointerTarget::Page
        );
        let over_panel = PointerContext {
            over_panel: true,
            ..ctx()
        };
        let now = route_pointer(capture.captured(), over_panel);
        assert_eq!(now, PointerTarget::Page);
        assert_eq!(capture.tab(), Some(TAB_A));
        assert_eq!(capture.release(LEFT, now), PointerTarget::Page);
        // The capture is over: the panel owns the pointer where it rests.
        assert_eq!(
            route_pointer(capture.captured(), over_panel),
            PointerTarget::Panel
        );
    }

    /// And the other way: a press in the panel names the panel's view,
    /// and a drag over the page, or up over the toolbar, stays the
    /// panel's until the release.
    #[test]
    fn a_press_in_the_panel_dragged_over_the_page_stays_with_the_panel() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.press(LEFT, PointerTarget::Panel, Some(PANEL)),
            PointerTarget::Panel
        );
        assert_eq!(capture.tab(), Some(PANEL));
        assert_eq!(
            route_pointer(capture.captured(), ctx()),
            PointerTarget::Panel
        );
        let over_chrome = PointerContext {
            over_chrome: true,
            ..ctx()
        };
        assert_eq!(
            route_pointer(capture.captured(), over_chrome),
            PointerTarget::Panel
        );
        // A second button pressed over the page joins the panel's capture.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Page, Some(TAB_A)),
            PointerTarget::Panel
        );
        assert_eq!(capture.tab(), Some(PANEL));
        assert_eq!(
            capture.release(RIGHT, PointerTarget::Page),
            PointerTarget::Panel
        );
        assert_eq!(
            capture.release(LEFT, PointerTarget::Page),
            PointerTarget::Panel
        );
        assert_eq!(capture.captured(), None);
        assert_eq!(capture.tab(), None);
    }

    #[test]
    fn losing_focus_hands_back_the_buttons_the_panel_holds() {
        let mut capture = PointerCapture::default();
        capture.press(LEFT, PointerTarget::Panel, Some(PANEL));
        assert_eq!(capture.release_all(), Some((PANEL, vec![LEFT])));
        assert_eq!(capture.captured(), None);
    }

    fn mac() -> Keymap {
        Keymap::defaults(Platform::MacOs)
    }

    /// Every tab exists.
    fn all_live(_: TabId) -> bool {
        true
    }

    fn stroke(code: Code, modifiers: Modifiers, pressed: bool, repeat: bool) -> KeyStroke {
        KeyStroke {
            code,
            modifiers,
            pressed,
            repeat,
        }
    }

    /// An unmodified key event through `keys`, with TAB_A active.
    fn plain(
        keys: &mut KeyRouter,
        code: Code,
        pressed: bool,
        repeat: bool,
        egui_wants_keyboard: bool,
    ) -> Option<KeyTarget> {
        keys.route(
            stroke(code, Modifiers::empty(), pressed, repeat),
            egui_wants_keyboard,
            Some(TAB_A),
            all_live,
            &mac(),
            &[],
        )
    }

    /// A key event with Cmd held, as macOS shortcuts arrive, with TAB_A
    /// active.
    fn cmd(
        keys: &mut KeyRouter,
        code: Code,
        pressed: bool,
        repeat: bool,
        egui_wants_keyboard: bool,
    ) -> Option<KeyTarget> {
        keys.route(
            stroke(code, Modifiers::META, pressed, repeat),
            egui_wants_keyboard,
            Some(TAB_A),
            all_live,
            &mac(),
            &[],
        )
    }

    #[test]
    fn page_key_gets_its_repeats_and_release() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            plain(&mut keys, Code::KeyA, true, false, false),
            Some(KeyTarget::Page(TAB_A))
        );
        assert_eq!(
            plain(&mut keys, Code::KeyA, true, true, false),
            Some(KeyTarget::Page(TAB_A))
        );
        assert_eq!(
            plain(&mut keys, Code::KeyA, false, false, false),
            Some(KeyTarget::Page(TAB_A))
        );
        // Released: a later release is no longer the page's.
        assert_eq!(plain(&mut keys, Code::KeyA, false, false, false), None);
    }

    #[test]
    fn key_pressed_while_egui_has_focus_goes_to_egui() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            plain(&mut keys, Code::KeyA, true, false, true),
            Some(KeyTarget::Egui)
        );
        assert_eq!(
            plain(&mut keys, Code::KeyA, true, true, true),
            Some(KeyTarget::Egui)
        );
        assert_eq!(
            plain(&mut keys, Code::KeyA, false, false, true),
            Some(KeyTarget::Egui)
        );
    }

    /// The reviewer's Enter case: egui gets Enter, the address bar loses
    /// focus on that press, and the release must still reach egui.
    #[test]
    fn egui_gets_the_release_after_it_lost_focus() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            plain(&mut keys, Code::Enter, true, false, true),
            Some(KeyTarget::Egui)
        );
        assert_eq!(
            plain(&mut keys, Code::Enter, false, false, false),
            Some(KeyTarget::Egui)
        );
        // Nothing left to deliver.
        assert_eq!(plain(&mut keys, Code::Enter, false, false, false), None);
    }

    #[test]
    fn repeat_of_an_egui_key_still_goes_to_egui_after_focus_moves() {
        let mut keys = KeyRouter::default();
        plain(&mut keys, Code::Backspace, true, false, true);
        assert_eq!(
            plain(&mut keys, Code::Backspace, true, true, false),
            Some(KeyTarget::Egui)
        );
    }

    #[test]
    fn page_never_sees_a_repeat_or_release_it_did_not_start() {
        let mut keys = KeyRouter::default();
        // Repeat with no press from this window.
        assert_eq!(plain(&mut keys, Code::KeyB, true, true, false), None);
        // A release for a key pressed elsewhere, with no egui focus.
        assert_eq!(plain(&mut keys, Code::KeyB, false, false, false), None);
    }

    #[test]
    fn a_key_held_by_the_page_stays_with_the_page_even_if_egui_gets_focus() {
        let mut keys = KeyRouter::default();
        plain(&mut keys, Code::KeyC, true, false, false);
        // Focus moved to egui while C is held: the repeat and release stay
        // with the page so it never sees half a stroke.
        assert_eq!(
            plain(&mut keys, Code::KeyC, true, true, true),
            Some(KeyTarget::Page(TAB_A))
        );
        assert_eq!(
            plain(&mut keys, Code::KeyC, false, false, true),
            Some(KeyTarget::Page(TAB_A))
        );
    }

    /// Meta pressed in tab A, then Cmd+Shift+] makes B active: the release
    /// still targets A, the tab that saw the press. B gets no lone keyup.
    #[test]
    fn a_held_key_releases_in_the_tab_that_got_its_press() {
        let mut keys = KeyRouter::default();
        let meta = Code::MetaLeft;
        assert_eq!(
            keys.route(
                stroke(meta, Modifiers::empty(), true, false),
                false,
                Some(TAB_A),
                all_live,
                &mac(),
                &[],
            ),
            Some(KeyTarget::Page(TAB_A))
        );
        // B is now active; a repeat of the held key still goes to A.
        assert_eq!(
            keys.route(
                stroke(meta, Modifiers::empty(), true, true),
                false,
                Some(TAB_B),
                all_live,
                &mac(),
                &[],
            ),
            Some(KeyTarget::Page(TAB_A))
        );
        assert_eq!(
            keys.route(
                stroke(meta, Modifiers::empty(), false, false),
                false,
                Some(TAB_B),
                all_live,
                &mac(),
                &[],
            ),
            Some(KeyTarget::Page(TAB_A))
        );
        // The release ended the hold: nothing more goes anywhere.
        assert_eq!(
            keys.route(
                stroke(meta, Modifiers::empty(), false, false),
                false,
                Some(TAB_B),
                all_live,
                &mac(),
                &[],
            ),
            None
        );
    }

    /// A held key whose tab closed is dropped: its release goes nowhere and
    /// a fresh press goes to the active tab.
    #[test]
    fn a_held_key_is_dropped_when_its_tab_closes() {
        let mut keys = KeyRouter::default();
        plain(&mut keys, Code::KeyQ, true, false, false);
        let tab_a_gone = |tab: TabId| tab != TAB_A;
        assert_eq!(
            keys.route(
                stroke(Code::KeyQ, Modifiers::empty(), false, false),
                false,
                Some(TAB_B),
                tab_a_gone,
                &mac(),
                &[],
            ),
            None
        );

        plain(&mut keys, Code::KeyQ, true, false, false);
        assert_eq!(
            keys.route(
                stroke(Code::KeyQ, Modifiers::empty(), true, true),
                false,
                Some(TAB_B),
                tab_a_gone,
                &mac(),
                &[],
            ),
            None,
            "a repeat of a key whose tab closed is not delivered"
        );
        assert_eq!(
            keys.route(
                stroke(Code::KeyQ, Modifiers::empty(), true, false),
                false,
                Some(TAB_B),
                tab_a_gone,
                &mac(),
                &[],
            ),
            Some(KeyTarget::Page(TAB_B)),
            "a fresh press goes to the active tab"
        );
    }

    /// Focus lost with A held in the page and Enter held in egui: the
    /// window gets A back, with its tab, to release there; Enter is
    /// egui's to forget. Afterwards neither release is anyone's.
    #[test]
    fn losing_focus_hands_back_the_keys_the_page_holds() {
        let mut keys = KeyRouter::default();
        plain(&mut keys, Code::KeyA, true, false, false);
        plain(&mut keys, Code::Enter, true, false, true);
        assert_eq!(keys.release_all(), [(Code::KeyA, TAB_A)]);
        assert_eq!(plain(&mut keys, Code::KeyA, false, false, false), None);
        assert_eq!(plain(&mut keys, Code::Enter, false, false, false), None);
        assert!(keys.release_all().is_empty());
    }

    /// Holding Cmd+T over the page opens one tab: the press is a command,
    /// and its repeats and release reach neither the page nor egui.
    #[test]
    fn a_command_chord_never_reaches_the_page() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            cmd(&mut keys, Code::KeyT, true, false, false),
            Some(KeyTarget::Command(CommandKind::NewTab))
        );
        assert_eq!(cmd(&mut keys, Code::KeyT, true, true, false), None);
        assert_eq!(cmd(&mut keys, Code::KeyT, false, false, false), None);
    }

    /// A lost release left T with the page; a fresh Cmd+T press still fires
    /// the command and takes T away from the page, so the page never sees
    /// the chord's repeats or release.
    #[test]
    fn a_command_takes_its_key_back_from_the_page() {
        let mut keys = KeyRouter::default();
        plain(&mut keys, Code::KeyT, true, false, false);
        assert_eq!(
            cmd(&mut keys, Code::KeyT, true, false, false),
            Some(KeyTarget::Command(CommandKind::NewTab))
        );
        assert_eq!(cmd(&mut keys, Code::KeyT, true, true, false), None);
        assert_eq!(cmd(&mut keys, Code::KeyT, false, false, false), None);
    }

    /// Cmd+T in the address bar opens a tab; Cmd+A stays egui's select-all.
    #[test]
    fn primary_chords_fire_while_egui_has_focus() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            cmd(&mut keys, Code::KeyT, true, false, true),
            Some(KeyTarget::Command(CommandKind::NewTab))
        );
        assert_eq!(
            cmd(&mut keys, Code::KeyA, true, false, true),
            Some(KeyTarget::Egui)
        );
    }

    fn claude() -> [ExtensionChord<'static>; 1] {
        [ExtensionChord {
            chord: KeyChord::new(Modifiers::META, Code::KeyE),
            extension: "claude",
            name: "toggle-side-panel",
        }]
    }

    /// Cmd+E is an extension's: the press is its command, and neither the
    /// page nor egui sees the press, the repeats or the release.
    #[test]
    fn an_extensions_chord_is_its_command_and_never_reaches_the_page() {
        let mut keys = KeyRouter::default();
        let mut route = |pressed, repeat| {
            keys.route(
                stroke(Code::KeyE, Modifiers::META, pressed, repeat),
                false,
                Some(TAB_A),
                all_live,
                &mac(),
                &claude(),
            )
        };
        assert_eq!(
            route(true, false),
            Some(KeyTarget::ExtensionCommand {
                extension: "claude".to_string(),
                name: "toggle-side-panel".to_string(),
            })
        );
        assert_eq!(route(true, true), None);
        assert_eq!(route(false, false), None);
    }

    /// An extension that declares Cmd+T gets nothing: the keymap's New
    /// tab runs.
    #[test]
    fn an_extension_chord_loses_to_the_keymap() {
        let mut keys = KeyRouter::default();
        let greedy = [ExtensionChord {
            chord: KeyChord::new(Modifiers::META, Code::KeyT),
            extension: "greedy",
            name: "grab",
        }];
        assert_eq!(
            keys.route(
                stroke(Code::KeyT, Modifiers::META, true, false),
                false,
                Some(TAB_A),
                all_live,
                &mac(),
                &greedy,
            ),
            Some(KeyTarget::Command(CommandKind::NewTab))
        );
    }

    /// The keyboard is the panel's only after a click in it and only
    /// while that same panel is the one drawn; otherwise the active tab
    /// has it.
    #[test]
    fn the_keyboard_is_the_panels_only_while_that_panel_is_shown() {
        // No click in a panel yet.
        assert_eq!(keyboard_view(Some(TAB_A), None, Some(PANEL)), Some(TAB_A));
        // Clicked in, still shown.
        assert_eq!(
            keyboard_view(Some(TAB_A), Some(PANEL), Some(PANEL)),
            Some(PANEL)
        );
        // Another panel is shown now, or none (closed, full screen).
        assert_eq!(
            keyboard_view(Some(TAB_A), Some(PANEL), Some(TabId(10))),
            Some(TAB_A)
        );
        assert_eq!(keyboard_view(Some(TAB_A), Some(PANEL), None), Some(TAB_A));
        // A window with no model has nobody to type to.
        assert_eq!(keyboard_view(None, None, None), None);
    }

    #[test]
    fn keys_go_to_the_panel_while_it_has_the_keyboard() {
        let mut keys = KeyRouter::default();
        let keyboard = keyboard_view(Some(TAB_A), Some(PANEL), Some(PANEL));
        let mut route = |pressed, repeat| {
            keys.route(
                stroke(Code::KeyA, Modifiers::empty(), pressed, repeat),
                false,
                keyboard,
                all_live,
                &mac(),
                &[],
            )
        };
        assert_eq!(route(true, false), Some(KeyTarget::Page(PANEL)));
        assert_eq!(route(true, true), Some(KeyTarget::Page(PANEL)));
        assert_eq!(route(false, false), Some(KeyTarget::Page(PANEL)));
    }

    /// Cmd+T with the keyboard in the panel still opens a tab, and the
    /// panel's page never sees the T.
    #[test]
    fn a_keymap_chord_fires_whichever_view_has_the_keyboard() {
        let mut keys = KeyRouter::default();
        let mut route = |pressed, repeat| {
            keys.route(
                stroke(Code::KeyT, Modifiers::META, pressed, repeat),
                false,
                Some(PANEL),
                all_live,
                &mac(),
                &claude(),
            )
        };
        assert_eq!(
            route(true, false),
            Some(KeyTarget::Command(CommandKind::NewTab))
        );
        assert_eq!(route(true, true), None);
        assert_eq!(route(false, false), None);
    }

    /// A key pressed in the panel, then a click in the page: the release
    /// goes to the panel's view, which saw the press, and the next press
    /// to the tab.
    #[test]
    fn a_key_held_in_the_panel_releases_there_after_the_page_takes_the_keyboard() {
        let mut keys = KeyRouter::default();
        let mut route = |code, pressed, keyboard| {
            keys.route(
                stroke(code, Modifiers::empty(), pressed, false),
                false,
                keyboard,
                all_live,
                &mac(),
                &[],
            )
        };
        assert_eq!(
            route(Code::ShiftLeft, true, Some(PANEL)),
            Some(KeyTarget::Page(PANEL))
        );
        assert_eq!(
            route(Code::KeyB, true, Some(TAB_A)),
            Some(KeyTarget::Page(TAB_A))
        );
        assert_eq!(
            route(Code::ShiftLeft, false, Some(TAB_A)),
            Some(KeyTarget::Page(PANEL))
        );
    }

    /// The panel closed with a key down in it: the liveness test says the
    /// view is gone, so the release goes nowhere.
    #[test]
    fn a_key_held_in_a_panel_that_closed_is_dropped() {
        let mut keys = KeyRouter::default();
        keys.route(
            stroke(Code::KeyA, Modifiers::empty(), true, false),
            false,
            Some(PANEL),
            all_live,
            &mac(),
            &[],
        );
        let panel_gone = |view: TabId| view != PANEL;
        assert_eq!(
            keys.route(
                stroke(Code::KeyA, Modifiers::empty(), false, false),
                false,
                Some(TAB_A),
                panel_gone,
                &mac(),
                &[],
            ),
            None
        );
    }

    #[test]
    fn nothing_is_shown_or_hidden_while_the_same_panel_stays() {
        assert_eq!(panel_transition(None, None), PanelTransition::default());
        assert_eq!(
            panel_transition(Some(PANEL), Some(PANEL)),
            PanelTransition::default()
        );
    }

    #[test]
    fn a_panel_that_appears_is_shown_and_one_that_goes_is_hidden() {
        assert_eq!(
            panel_transition(None, Some(PANEL)),
            PanelTransition {
                hide: None,
                show: Some(PANEL),
            }
        );
        // Closed, or the window went full screen, or got too narrow.
        assert_eq!(
            panel_transition(Some(PANEL), None),
            PanelTransition {
                hide: Some(PANEL),
                show: None,
            }
        );
    }

    /// A tab switch from a tab with its own panel to another with its
    /// own: one view out, the other in.
    #[test]
    fn switching_between_two_panels_hides_one_and_shows_the_other() {
        assert_eq!(
            panel_transition(Some(PANEL), Some(TabId(10))),
            PanelTransition {
                hide: Some(PANEL),
                show: Some(TabId(10)),
            }
        );
    }

    #[test]
    fn content_size_covers_the_same_pixel_edges_as_the_blit() {
        // Edges 20.4 and 30.6 round to 20 and 31: eleven rows, starting at 20.
        let rect = Rect::from_min_max(pos2(0.5, 20.4), pos2(10.5, 30.6));
        let (top, size) = content_pixels(rect, 1.0);
        assert_eq!(top, 20.0);
        assert_eq!(size, PhysicalSize::new(10, 11));
    }

    #[test]
    fn content_scales_with_pixels_per_point() {
        // 40.2 * 2 = 80.4 -> 80; 300.6 * 2 = 601.2 -> 601.
        let rect = Rect::from_min_max(pos2(0.0, 40.2), pos2(100.0, 300.6));
        let (top, size) = content_pixels(rect, 2.0);
        assert_eq!(top, 80.0);
        assert_eq!(size, PhysicalSize::new(200, 521));
    }

    #[test]
    fn content_is_never_zero_sized() {
        let rect = Rect::from_min_max(pos2(5.0, 5.0), pos2(5.0, 5.0));
        let (_, size) = content_pixels(rect, 1.0);
        assert_eq!(size, PhysicalSize::new(1, 1));
    }

    /// A content area under an 80-point chrome in a 1024 by 768 window.
    fn content() -> Rect {
        Rect::from_min_max(pos2(0.0, 80.0), pos2(1024.0, 768.0))
    }

    fn no_store_item(_: &url::Url) -> Option<String> {
        None
    }

    /// An engine with extensions and side panels, and one without the
    /// panels.
    const PANELS: ExtensionSupport = ExtensionSupport {
        store_name: "Test Store",
        store_url: "https://store.example/",
        store_item_id: no_store_item,
        side_panels: true,
    };
    const NO_PANELS: ExtensionSupport = ExtensionSupport {
        side_panels: false,
        ..PANELS
    };

    /// `ext`'s panel, rendered by view 90.
    fn side_panel() -> osmena_core::extension::SidePanel {
        osmena_core::extension::SidePanel {
            extension: "ext".to_string(),
            page: "panel.html".to_string(),
            view: TabId(90),
        }
    }

    #[test]
    fn a_panel_is_drawn_only_out_of_full_screen_on_an_engine_with_side_panels() {
        let address = url::Url::parse("https://a.example/").expect("test URL parses");
        let mut w = Window::with_tab(
            osmena_core::WindowId(1),
            osmena_core::Tab::new(TabId(1), address),
        );
        // No panel in the model: nothing to draw on any engine.
        assert!(!panel_shown(&w, Some(&PANELS)));
        w.panel = Some(side_panel());
        assert!(panel_shown(&w, Some(&PANELS)));
        // An engine without side panels, or without extensions at all.
        assert!(!panel_shown(&w, Some(&NO_PANELS)));
        assert!(!panel_shown(&w, None));
        // Full screen shows the page alone; leaving it brings the panel
        // back, since the model kept it.
        w.fullscreen = true;
        assert!(!panel_shown(&w, Some(&PANELS)));
        w.fullscreen = false;
        assert!(panel_shown(&w, Some(&PANELS)));
        // The active tab's own panel counts as much as the window's.
        w.panel = None;
        w.tabs[0].panel = Some(side_panel());
        assert!(panel_shown(&w, Some(&PANELS)));
        // What the predicate feeds: no panel rectangle without it.
        assert!(split_content(content(), false).panel.is_none());
        assert!(split_content(content(), true).panel.is_some());
    }

    #[test]
    fn a_window_without_a_shown_panel_gives_the_page_everything() {
        assert_eq!(
            split_content(content(), false),
            ContentSplit {
                page: content(),
                panel: None,
            }
        );
    }

    #[test]
    fn a_shown_panel_takes_a_fixed_width_on_the_right_under_its_header() {
        assert_eq!(
            split_content(content(), true),
            ContentSplit {
                page: Rect::from_min_max(pos2(0.0, 80.0), pos2(664.0, 768.0)),
                panel: Some(PanelRects {
                    header: Rect::from_min_max(pos2(664.0, 80.0), pos2(1024.0, 116.0)),
                    view: Rect::from_min_max(pos2(664.0, 116.0), pos2(1024.0, 768.0)),
                }),
            }
        );
    }

    #[test]
    fn a_content_area_too_narrow_for_page_and_panel_shows_no_panel() {
        // 360 for the panel and 320 for the page: 680 is the least.
        let narrow = Rect::from_min_max(pos2(0.0, 80.0), pos2(679.0, 768.0));
        assert_eq!(
            split_content(narrow, true),
            ContentSplit {
                page: narrow,
                panel: None,
            }
        );
        let just_enough = Rect::from_min_max(pos2(0.0, 80.0), pos2(680.0, 768.0));
        let split = split_content(just_enough, true);
        assert_eq!(split.page.width(), 320.0);
        assert!(split.panel.is_some());
    }

    #[test]
    fn a_content_area_shorter_than_the_header_gives_the_view_no_height() {
        let short = Rect::from_min_max(pos2(0.0, 80.0), pos2(1024.0, 100.0));
        let panel = split_content(short, true).panel.expect("wide enough");
        assert_eq!(panel.header.max.y, 100.0);
        assert_eq!(panel.view.height(), 0.0);
    }

    /// The page's right edge and the panel's left are one pixel column,
    /// and the header's foot and the view's top one pixel row, whatever
    /// the scale: no gap and no overlap between the three rectangles.
    #[test]
    fn the_page_and_the_panel_share_their_pixel_edges_at_fractional_scales() {
        let content = Rect::from_min_max(pos2(0.0, 80.4), pos2(1023.6, 767.2));
        for ppp in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let split = split_content(content, true);
            let panel = split.panel.expect("wide enough");
            let page = view_pixels(split.page, ppp);
            let header = view_pixels(panel.header, ppp);
            let view = view_pixels(panel.view, ppp);
            assert_eq!(page.left, 0.0, "at {ppp}");
            assert_eq!(page.left + page.size.width as f32, view.left, "at {ppp}");
            assert_eq!(header.left, view.left, "at {ppp}");
            assert_eq!(header.top, page.top, "at {ppp}");
            assert_eq!(header.top + header.size.height as f32, view.top, "at {ppp}");
            assert_eq!(
                view.left + view.size.width as f32,
                (content.max.x * ppp).round(),
                "at {ppp}"
            );
            assert_eq!(
                view.top + view.size.height as f32,
                (content.max.y * ppp).round(),
                "at {ppp}"
            );
        }
    }

    #[test]
    fn view_pixels_gives_the_top_left_and_the_size_on_the_pixel_grid() {
        // 664 * 1.5 = 996; 116 * 1.5 = 174; 1024 * 1.5 = 1536; 768 * 1.5 = 1152.
        let rect = Rect::from_min_max(pos2(664.0, 116.0), pos2(1024.0, 768.0));
        assert_eq!(
            view_pixels(rect, 1.5),
            ViewPixels {
                left: 996.0,
                top: 174.0,
                size: PhysicalSize::new(540, 978),
            }
        );
        // The page area's top and size are the same arithmetic.
        assert_eq!(
            content_pixels(rect, 1.5),
            (174.0, PhysicalSize::new(540, 978))
        );
    }

    /// A pointer position in the window, seen from a view: from the
    /// view's own top left, and negative once a drag has left it to the
    /// left or upward.
    #[test]
    fn a_point_in_the_window_becomes_the_views_own_point() {
        let view = ViewPixels {
            left: 664.0,
            top: 116.0,
            size: PhysicalSize::new(360, 652),
        };
        assert_eq!(view.local(664.0, 116.0), (0.0, 0.0));
        assert_eq!(view.local(700.0, 200.0), (36.0, 84.0));
        assert_eq!(view.local(600.0, 100.0), (-64.0, -16.0));
    }

    #[test]
    fn the_pointer_is_classified_by_the_rectangle_it_is_over() {
        let panel = Some(ViewPixels {
            left: 664.0,
            top: 116.0,
            size: PhysicalSize::new(360, 652),
        });
        // Above the content area it is the chrome, panel or not.
        assert_eq!(
            content_region(300.0, 79.0, 80.0, panel),
            ContentRegion::Chrome
        );
        assert_eq!(
            content_region(700.0, 79.0, 80.0, panel),
            ContentRegion::Chrome
        );
        // Left of the panel, the page; the boundary column is the panel's.
        assert_eq!(
            content_region(663.0, 300.0, 80.0, panel),
            ContentRegion::Page
        );
        assert_eq!(
            content_region(664.0, 300.0, 80.0, panel),
            ContentRegion::Panel
        );
        // The header strip above the view.
        assert_eq!(
            content_region(700.0, 80.0, 80.0, panel),
            ContentRegion::PanelHeader
        );
        assert_eq!(
            content_region(700.0, 115.0, 80.0, panel),
            ContentRegion::PanelHeader
        );
        assert_eq!(
            content_region(700.0, 116.0, 80.0, panel),
            ContentRegion::Panel
        );
        // With no panel drawn the whole width is the page's.
        assert_eq!(
            content_region(700.0, 300.0, 80.0, None),
            ContentRegion::Page
        );
    }
}
