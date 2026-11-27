//! Tab drags, driven the way a pointer drives them: the real
//! `chrome::draw` run headlessly, frame after frame, with the pointer
//! events egui-winit would hand egui for a press, a move and a release,
//! and every command the chrome answers put through `dispatch` before
//! the next frame, as the shell does.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Event, Modifiers, PointerButton, RawInput};
use osmena_core::{
    Browser, EngineInfo, ExtensionSupport, PageTools, Platform, Profile, Settings, dispatch,
};

use super::*;

static BRAND: Brand = Brand {
    name: "Osmena",
    binary: "osmena",
    data_dir: "Osmena",
    homepage: "https://home.example/",
    chrome_color: ChromeColor::Shell,
    icon_png: &[],
    version: "0.1.0",
    build: None,
    updates: None,
    google_client_id: None,
};

fn no_item(_: &Url) -> Option<String> {
    None
}

static ENGINE: EngineInfo = EngineInfo {
    name: "TestEngine",
    version: "0.0",
    data_dir: "engine",
    supports_stop: true,
    find: true,
    downloads: true,
    wheel_line_px: 40.0,
    extensions: Some(ExtensionSupport {
        store_name: "Store",
        store_url: "https://store.example/",
        store_item_id: no_item,
        side_panels: false,
    }),
    page_tools: PageTools::ALL,
    profiles_in_process: false,
};

const SIZE: Vec2 = Vec2::new(900.0, 600.0);

/// A window of the chrome with a pointer over it.
struct Rig {
    ctx: egui::Context,
    browser: Browser,
    state: ChromeState,
    keymap: Keymap,
    time: f64,
    frameless: bool,
    /// The modifier keys held, as egui-winit reports them with every
    /// frame's input.
    modifiers: Modifiers,
    /// Every command the chrome has answered so far.
    answered: Vec<Command>,
    /// Whether the window has the keyboard, as each frame's input says.
    focused: bool,
}

impl Rig {
    /// One window of `tabs` tabs (ids 1 to `tabs`), the first active.
    fn new(tabs: u64) -> Rig {
        let profile = Profile {
            name: "default".to_string(),
            dir: PathBuf::from("profile"),
            engine_dir: PathBuf::from("profile/engine"),
            guest: false,
        };
        let mut browser = Browser::new(profile, Settings::for_brand(&BRAND));
        let page = |n: u64| {
            let mut tab = Tab::new(
                TabId(n),
                Url::parse(&format!("https://site{n}.example/")).expect("a test address"),
            );
            tab.title = format!("Page {n}");
            tab
        };
        let mut window = Window::with_tab(WindowId(1), page(1));
        for n in 2..=tabs {
            window.tabs.push(page(n));
        }
        browser.windows.push(window);
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut rig = Rig {
            ctx,
            browser,
            state: ChromeState::default(),
            keymap: Keymap::defaults(Platform::MacOs),
            time: 0.0,
            frameless: true,
            modifiers: Modifiers::NONE,
            answered: Vec::new(),
            focused: true,
        };
        // Until the widths and the offsets have settled.
        for _ in 0..30 {
            rig.frame(Vec::new());
        }
        rig
    }

    /// Tabs `tabs` (by id) in a folder, expanded or not.
    fn folder(&mut self, id: u64, tabs: &[u64], expanded: bool) {
        let window = &mut self.browser.windows[0];
        window.folders.push(Folder {
            id: FolderId(id),
            name: "Reading".to_string(),
            expanded,
        });
        for tab in &mut window.tabs {
            if tabs.contains(&tab.id.0) {
                tab.folder = Some(FolderId(id));
            }
        }
        for _ in 0..30 {
            self.frame(Vec::new());
        }
    }

    /// One frame, a sixtieth of a second after the last, with `events`
    /// arrived since. The commands go through `dispatch`, as in the shell.
    fn frame(&mut self, events: Vec<Event>) -> Vec<Command> {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SIZE)),
            time: Some(self.time),
            events,
            modifiers: self.modifiers,
            focused: self.focused,
            ..RawInput::default()
        };
        let site_icons = HashMap::new();
        let mut commands = Vec::new();
        let browser = &self.browser;
        let state = &mut self.state;
        let keymap = &self.keymap;
        let frameless = self.frameless;
        let _ = self.ctx.run_ui(input, |ui| {
            let context = ChromeContext {
                color: browser.settings.chrome_color,
                brand: &BRAND,
                profile_dir: &browser.profile.name,
                profile_name: "Thomas",
                avatar: browser.settings.profile.avatar,
                engine: &ENGINE,
                engine_error: None,
                frameless,
                maximized: false,
                pinned: &[],
                history: &browser.history,
                update: &browser.update,
                bookmarks: &browser.bookmarks,
                show_bookmarks_bar: false,
                site_icons: &site_icons,
                page_zoom: browser.settings.page_zoom,
                downloads: &browser.downloads,
                restore_offer: false,
                default_browser_offer: false,
                launch: &browser.launch,
                launch_settings: &browser.settings.launch,
                search: &browser.settings.search,
            };
            commands = draw(ui, &browser.windows[0], &context, state, keymap);
        });
        for command in &commands {
            let _ = dispatch(&mut self.browser, command);
        }
        self.answered.extend(commands.iter().cloned());
        commands
    }

    /// Where tab `id` was drawn in the last frame.
    fn tab_rect(&self, id: u64) -> Rect {
        self.ctx
            .read_response(Id::new(("tab", TabId(id))))
            .unwrap_or_else(|| panic!("tab {id} is drawn"))
            .rect
    }

    /// Where folder `id`'s chip was drawn in the last frame.
    fn chip_rect(&self, id: u64) -> Rect {
        self.ctx
            .read_response(Id::new(("chip", FolderId(id))))
            .unwrap_or_else(|| panic!("the chip of folder {id} is drawn"))
            .rect
    }

    fn order(&self) -> Vec<u64> {
        self.browser.windows[0]
            .tabs
            .iter()
            .map(|t| t.id.0)
            .collect()
    }

    fn press(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: self.modifiers,
        }]);
    }

    /// The pointer goes from `from` to `to` in steps of at most eight
    /// points, a frame each, the button held.
    fn drag(&mut self, from: Pos2, to: Pos2) {
        let steps = ((to - from).length() / 8.0).ceil().max(1.0) as usize;
        for step in 1..=steps {
            let at = from + (to - from) * (step as f32 / steps as f32);
            self.frame(vec![Event::PointerMoved(at)]);
        }
        // The pointer rests a moment where it arrived.
        for _ in 0..12 {
            self.frame(Vec::new());
        }
    }

    fn release(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: self.modifiers,
        }]);
        for _ in 0..30 {
            self.frame(Vec::new());
        }
    }

    /// The window loses the keyboard mid-drag, as on Alt+Tab or when a
    /// modal dialog opens: egui hears `WindowFocused(false)` and no
    /// button release follows.
    fn lose_focus(&mut self) {
        self.focused = false;
        // The shell's `Focused(false)` handler calls the cancel before
        // egui hears the event, as here.
        self.state.cancel_drag(&self.ctx);
        self.frame(vec![Event::WindowFocused(false)]);
    }

    fn regain_focus(&mut self) {
        self.focused = true;
        self.frame(vec![Event::WindowFocused(true)]);
    }

    fn moves(&self) -> Vec<Command> {
        self.answered
            .iter()
            .filter(|c| matches!(c, Command::MoveTabs { .. }))
            .cloned()
            .collect()
    }
}

#[test]
fn a_tab_dragged_along_the_strip_is_moved_to_where_it_is_dropped() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(2).center();
    let to = Pos2::new(rig.tab_rect(4).right() - 4.0, from.y);
    rig.press(from);
    assert_eq!(rig.state.tab_drag, None, "a press alone is no drag");
    rig.drag(from, to);
    let drag = rig.state.tab_drag.clone().expect("the drag is on");
    assert_eq!(drag.tabs, vec![TabId(2)]);
    assert_eq!(drag.index, 3, "past the last of the tabs that stay");
    rig.release(to);
    assert_eq!(rig.state.tab_drag, None);
    assert_eq!(
        rig.moves(),
        vec![Command::MoveTabs {
            tabs: vec![TabId(2)],
            index: 3
        }]
    );
    assert_eq!(rig.order(), vec![1, 3, 4, 2]);
    assert_eq!(rig.state.take_tab_drop(), None);
}

#[test]
fn a_tab_dragged_left_lands_before_the_tab_it_passed() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(4).center();
    let to = Pos2::new(rig.tab_rect(1).left() + 4.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    rig.release(to);
    assert_eq!(rig.order(), vec![4, 1, 2, 3]);
}

#[test]
fn the_active_tab_is_dragged_like_any_other() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(1).center();
    // Its middle just short of the last tab's: it lands before it.
    let to = Pos2::new(rig.tab_rect(3).center().x - 10.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    rig.release(to);
    assert_eq!(rig.order(), vec![2, 3, 1, 4]);
}

#[test]
fn a_tab_dropped_outside_the_window_is_a_tab_drop() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(2).center();
    let to = Pos2::new(from.x + 60.0, SIZE.y + 80.0);
    rig.press(from);
    rig.drag(from, to);
    assert!(
        rig.state.tab_drag.is_some(),
        "the drag outlives the window's edge"
    );
    rig.release(to);
    assert_eq!(rig.moves(), Vec::new());
    let drop = rig.state.take_tab_drop().expect("a drop for the shell");
    assert_eq!(drop.tabs, vec![TabId(2)]);
    assert_eq!(drop.pointer, to);
    assert_eq!(rig.order(), vec![1, 2, 3, 4]);
}

#[test]
fn the_selected_tabs_are_dragged_together_in_strip_order() {
    let mut rig = Rig::new(5);
    // Tab 1 is active; Cmd-click adds tab 3.
    let third = rig.tab_rect(3).center();
    rig.modifiers = Modifiers::COMMAND | Modifiers::MAC_CMD;
    rig.press(third);
    rig.release(third);
    rig.modifiers = Modifiers::NONE;
    assert_eq!(
        rig.browser.windows[0].selection(),
        vec![TabId(1), TabId(3)],
        "Cmd-click adds to the selection"
    );
    let from = rig.tab_rect(3).center();
    let to = Pos2::new(rig.tab_rect(5).right() - 2.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    rig.release(to);
    assert_eq!(rig.order(), vec![2, 4, 5, 1, 3]);
}

/// winit says the pointer left when a drag crosses the window's edge
/// (`CursorLeft`, which egui-winit turns into `PointerGone`), and goes
/// on reporting where it is while the button is held.
#[test]
fn a_drag_outlives_the_pointer_leaving_the_window() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(2).center();
    let edge = Pos2::new(from.x + 30.0, SIZE.y - 1.0);
    let to = Pos2::new(from.x + 60.0, SIZE.y + 80.0);
    rig.press(from);
    rig.drag(from, edge);
    // The frame that sees the pointer gone and nothing after it.
    rig.frame(vec![Event::PointerGone]);
    rig.drag(edge, to);
    assert!(rig.state.tab_drag.is_some(), "the drag is still on");
    rig.release(to);
    assert_eq!(
        rig.state.take_tab_drop().map(|drop| drop.tabs),
        Some(vec![TabId(2)])
    );
}

#[test]
fn a_folder_dragged_by_its_chip_moves_its_tabs_together() {
    let mut rig = Rig::new(5);
    rig.folder(7, &[2, 3], true);
    let from = rig.chip_rect(7).center();
    let to = Pos2::new(rig.tab_rect(5).right() - 2.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    assert_eq!(
        rig.state.tab_drag.as_ref().map(|drag| drag.tabs.clone()),
        Some(vec![TabId(2), TabId(3)]),
        "a press on the chip that moves is a drag of the folder's tabs"
    );
    let chip = rig.chip_rect(7);
    let second = rig.tab_rect(2);
    assert!(
        (second.left() - chip.right()).abs() <= 3.0 && chip.right() <= to.x + chip.width(),
        "the chip rides with its tabs after it: {chip:?}, {second:?}"
    );
    rig.release(to);
    assert_eq!(
        rig.answered.last(),
        Some(&Command::MoveFolder {
            folder: FolderId(7),
            index: 3
        })
    );
    assert_eq!(rig.order(), vec![1, 4, 5, 2, 3]);
    let window = &rig.browser.windows[0];
    assert_eq!(
        window.tab(TabId(2)).and_then(|t| t.folder),
        Some(FolderId(7))
    );
    assert_eq!(
        window.tab(TabId(3)).and_then(|t| t.folder),
        Some(FolderId(7))
    );
}

/// More tabs than fit: the row scrolls, and the scroll area takes a
/// drag of its background for itself. A drag that starts on a tab is
/// still the tab's.
#[test]
fn a_tab_is_dragged_in_a_strip_that_scrolls() {
    let mut rig = Rig::new(30);
    let from = rig.tab_rect(3).center();
    let to = Pos2::new(rig.tab_rect(6).center().x + 4.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    assert_eq!(
        rig.state.tab_drag.as_ref().map(|drag| drag.tabs.clone()),
        Some(vec![TabId(3)])
    );
    rig.release(to);
    // Past tab 7: the tab became the active one as its drag began, the
    // first tab gave up the active tab's width, and the others closed
    // up under the pointer.
    let landed = rig.order().iter().position(|id| *id == 3);
    assert_eq!(landed, Some(6));
    assert_eq!(rig.moves().len(), 1);
}

/// A crowded strip: every tab at its least width, the active one
/// wider. An inactive tab that is dragged becomes the active one and
/// grows under the pointer; the point held stays the point held, so a
/// tab taken by its middle rides with its middle under the pointer and
/// lands where the pointer is.
#[test]
fn a_tab_that_grows_as_its_drag_begins_stays_under_the_pointer() {
    const LEAST: f32 = 40.0;
    const PITCH: f32 = LEAST + 2.0;
    let mut rig = Rig::new(30);
    let pressed = rig.tab_rect(4);
    assert_eq!(pressed.width(), LEAST, "the strip is crowded");
    let from = pressed.center();
    // Six points short of the middle of the eighth place among the
    // tabs that stay, once they are all at their least.
    let left = rig.tab_rect(1).left();
    let to = Pos2::new(left + LEAST / 2.0 + 7.0 * PITCH - 6.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    let riding = rig.tab_rect(4);
    assert!(riding.width() > LEAST, "the dragged tab is the active one");
    assert!(
        (riding.center().x - to.x).abs() <= 1.0,
        "its middle is at {}, the pointer at {}",
        riding.center().x,
        to.x
    );
    rig.release(to);
    assert_eq!(
        rig.order().iter().position(|id| *id == 4),
        Some(7),
        "before the tab whose middle is right of the pointer"
    );
}

/// No key is the page's while a drag is on: the shell hands every key
/// to the chrome, which is how Escape reaches the strip.
#[test]
fn a_drag_takes_every_key() {
    let mut rig = Rig::new(4);
    assert!(!rig.state.takes_every_key());
    let from = rig.tab_rect(2).center();
    let to = Pos2::new(rig.tab_rect(3).center().x - 6.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    assert!(rig.state.takes_every_key());
    rig.release(to);
    assert!(!rig.state.takes_every_key());
}

/// A drag whose window loses the keyboard mid-drag has lost its release
/// for good: the drag must end there, and no key may stay the drag's,
/// and the next click elsewhere must not be taken for the drop.
#[test]
fn a_drag_stuck_by_a_focus_loss_is_cancelled() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(2).center();
    let to = Pos2::new(rig.tab_rect(4).right() - 4.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    assert!(rig.state.takes_every_key(), "the drag is on");
    rig.lose_focus();
    // The pointer goes on reporting where it is, with no release.
    rig.frame(vec![Event::PointerGone]);
    for _ in 0..30 {
        rig.frame(Vec::new());
    }
    assert_eq!(rig.state.tab_drag, None, "losing focus ends the drag");
    assert!(!rig.state.takes_every_key(), "no key is the drag's");
    rig.regain_focus();
    // Egui was never told the button came up, so a move now must not
    // resume the drag.
    let elsewhere = Pos2::new(300.0, 300.0);
    rig.drag(from, elsewhere);
    assert_eq!(rig.state.tab_drag, None, "a move does not resume the drag");
    rig.press(elsewhere);
    rig.release(elsewhere);
    assert_eq!(rig.state.take_tab_drop(), None, "no drop for a click");
    assert_eq!(rig.moves(), Vec::new());
    assert_eq!(rig.order(), vec![1, 2, 3, 4]);
}

/// Full screen starts mid-drag: no strip is drawn there, so nothing sees
/// the release. The drag ends when full screen does, and no key stays the
/// drag's for the rest of it.
#[test]
fn a_drag_ends_when_the_window_goes_full_screen() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(2).center();
    let to = Pos2::new(rig.tab_rect(4).right() - 4.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    assert!(rig.state.takes_every_key(), "the drag is on");
    rig.browser.windows[0].fullscreen = true;
    rig.frame(Vec::new());
    assert_eq!(rig.state.tab_drag, None, "full screen ends the drag");
    assert!(!rig.state.takes_every_key(), "no key is the drag's");
    rig.release(to);
    assert_eq!(rig.moves(), Vec::new());
    assert_eq!(rig.state.take_tab_drop(), None);
    assert_eq!(rig.order(), vec![1, 2, 3, 4]);
}

/// A press that does not travel is a click: it activates and drags
/// nothing.
#[test]
fn a_press_that_stays_put_activates_the_tab_and_moves_nothing() {
    let mut rig = Rig::new(4);
    let at = rig.tab_rect(3).center();
    rig.press(at);
    rig.drag(at, at + Vec2::new(3.0, 1.0));
    assert_eq!(rig.state.tab_drag, None);
    rig.release(at + Vec2::new(3.0, 1.0));
    assert_eq!(rig.moves(), Vec::new());
    assert_eq!(rig.browser.windows[0].active, TabId(3));
    assert_eq!(rig.order(), vec![1, 2, 3, 4]);
}

/// Escape while a drag is on puts the tabs back where they were.
#[test]
fn escape_cancels_a_drag() {
    let mut rig = Rig::new(4);
    let from = rig.tab_rect(2).center();
    let to = Pos2::new(rig.tab_rect(4).right() - 4.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    rig.frame(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]);
    assert_eq!(rig.state.tab_drag, None, "Escape ends the drag");
    rig.release(to);
    assert_eq!(rig.moves(), Vec::new());
    assert_eq!(rig.state.take_tab_drop(), None);
    assert_eq!(rig.order(), vec![1, 2, 3, 4]);
}

/// A drag pulled more than a tab's height off the strip is detached,
/// as in other browsers: the gap closes behind it, the tab rides with the
/// pointer wherever that goes, and moving back over the strip puts it
/// in the row again.
#[test]
fn a_drag_pulled_off_the_strip_detaches_and_comes_back() {
    let mut rig = Rig::new(4);
    let second = rig.tab_rect(2);
    let fourth = rig.tab_rect(4);
    let from = second.center();
    let along = Pos2::new(from.x + 40.0, from.y);
    let below = Pos2::new(along.x, second.bottom() + TAB_HEIGHT + 30.0);
    rig.press(from);
    rig.drag(from, along);
    let drag = rig.state.tab_drag.clone().expect("the drag is on");
    assert!(!drag.detached, "along the strip the tab is in the row");
    assert_eq!(drag.index, 2, "between the third tab and the fourth");
    assert!(
        (rig.tab_rect(4).left() - fourth.left()).abs() < 1.0,
        "the gap is open before the fourth tab"
    );

    rig.drag(along, below);
    assert!(rig.state.tab_drag.as_ref().is_some_and(|d| d.detached));
    assert!(
        (rig.tab_rect(4).right() - fourth.left()).abs() < 3.0,
        "the gap has closed: the fourth tab stands where the third did, at {:?}",
        rig.tab_rect(4)
    );
    let riding = rig.tab_rect(2);
    assert!(
        riding.contains(below),
        "the tab rides with the pointer off the strip: {riding:?}"
    );
    assert!((riding.center().x - below.x).abs() <= 1.0);
    assert_eq!(rig.moves(), Vec::new());

    rig.drag(below, along);
    assert!(rig.state.tab_drag.as_ref().is_some_and(|d| !d.detached));
    assert!(
        (rig.tab_rect(4).left() - fourth.left()).abs() < 1.0,
        "the gap is open again"
    );
    assert!((rig.tab_rect(2).top() - second.top()).abs() < 1.0);
    rig.release(along);
    assert_eq!(rig.order(), vec![1, 3, 2, 4]);
    assert_eq!(rig.state.take_tab_drop(), None);
}

/// Let go while detached, the tabs are the shell's to place, even with
/// the pointer still inside the window: over the page is where a new
/// window is asked for. The drop says where that window's corner goes
/// for the tab to stay under the pointer.
#[test]
fn a_detached_drag_dropped_inside_the_window_is_a_tab_drop() {
    let mut rig = Rig::new(4);
    let first = rig.tab_rect(1);
    let second = rig.tab_rect(2);
    let from = second.center();
    let to = Pos2::new(from.x + 60.0, 300.0);
    rig.press(from);
    rig.drag(from, to);
    rig.release(to);
    assert_eq!(rig.moves(), Vec::new(), "nothing is reordered");
    assert_eq!(rig.order(), vec![1, 2, 3, 4]);
    let drop = rig.state.take_tab_drop().expect("a drop for the shell");
    assert_eq!(drop.tabs, vec![TabId(2)]);
    assert_eq!(drop.pointer, to);
    // A window with its corner at `origin` has its first tab where the
    // dragged one was held: the same point of the tab under the pointer.
    let held = from - second.min;
    let first_tab_at = drop.origin + first.min.to_vec2();
    assert!(
        (to - first_tab_at - held).length() <= 1.5,
        "the corner {:?} puts the first tab at {first_tab_at:?}, held at {held:?}",
        drop.origin
    );
}

/// Tabs dropped on another window's strip land between the tabs there:
/// that window says where, from how its strip stood in its last frame.
#[test]
fn a_point_over_the_strip_names_the_gap_there() {
    let rig = Rig::new(4);
    let y = rig.tab_rect(1).center().y;
    let at = |x: f32| rig.state.strip_drop_index(Pos2::new(x, y));
    assert_eq!(at(rig.tab_rect(1).left() + 2.0), Some(0));
    assert_eq!(at(rig.tab_rect(2).right() - 4.0), Some(2));
    assert_eq!(at(rig.tab_rect(3).left() + 4.0), Some(2));
    assert_eq!(at(rig.tab_rect(4).center().x + 1.0), Some(4));
    assert_eq!(at(SIZE.x - 1.0), Some(4), "right of every tab is the end");
    // Over the top margin of the strip counts; over the page does not.
    let x = rig.tab_rect(3).left();
    assert_eq!(rig.state.strip_drop_index(Pos2::new(x, 1.0)), Some(2));
    let under = rig.tab_rect(1).bottom() + 4.0;
    assert_eq!(rig.state.strip_drop_index(Pos2::new(x, under)), None);
    assert_eq!(rig.state.strip_drop_index(Pos2::new(x, 300.0)), None);
}

/// A collapsed folder's chip stands for its tabs: a drop past it lands
/// after all of them.
#[test]
fn a_point_past_a_collapsed_folder_counts_its_hidden_tabs() {
    let mut rig = Rig::new(5);
    rig.folder(7, &[2, 3], false);
    let y = rig.tab_rect(1).center().y;
    let chip = rig.chip_rect(7);
    let at = |x: f32| rig.state.strip_drop_index(Pos2::new(x, y));
    assert_eq!(at(chip.left() + 2.0), Some(1));
    assert_eq!(at(chip.right() - 2.0), Some(3));
}

/// A collapsed folder is its chip alone, and is dragged as that; it is
/// still collapsed, and still a folder, where it lands.
#[test]
fn a_collapsed_folder_is_dragged_by_its_chip_alone() {
    let mut rig = Rig::new(5);
    rig.folder(7, &[2, 3], false);
    let from = rig.chip_rect(7).center();
    let to = Pos2::new(rig.tab_rect(5).right() - 2.0, from.y);
    rig.press(from);
    rig.drag(from, to);
    let drag = rig.state.tab_drag.clone().expect("the drag is on");
    assert_eq!(drag.folder, Some(FolderId(7)));
    assert_eq!(drag.index, 3);
    assert!(
        rig.ctx.read_response(Id::new(("tab", TabId(2)))).is_none(),
        "its tabs stay hidden"
    );
    rig.release(to);
    assert_eq!(rig.order(), vec![1, 4, 5, 2, 3]);
    let window = &rig.browser.windows[0];
    assert_eq!(window.folder_tabs(FolderId(7)), [TabId(2), TabId(3)]);
    assert!(!window.folder(FolderId(7)).expect("still a folder").expanded);
}

/// A folder dragged over another does not open a gap inside it: it
/// lands before or after.
#[test]
fn a_folder_dragged_over_another_lands_beside_it() {
    let mut rig = Rig::new(6);
    rig.folder(7, &[2, 3], true);
    rig.folder(8, &[4, 5], true);
    let from = rig.chip_rect(7).center();
    // Its own second tab's place is, once the folder is out of the
    // row and the rest has closed up, in the middle of the other
    // folder (the two chips are as wide as each other).
    let to = Pos2::new(rig.tab_rect(3).center().x, from.y);
    rig.press(from);
    rig.drag(from, to);
    assert_eq!(
        rig.state.tab_drag.as_ref().map(|drag| drag.index),
        Some(3),
        "the gap is after the other folder's tabs"
    );
    rig.release(to);
    let window = &rig.browser.windows[0];
    assert_eq!(window.folder_tabs(FolderId(7)), [TabId(2), TabId(3)]);
    assert_eq!(window.folder_tabs(FolderId(8)), [TabId(4), TabId(5)]);
    assert_eq!(
        rig.order(),
        vec![1, 4, 5, 2, 3, 6],
        "after the other folder, not in it"
    );
}

/// A press on a chip that does not travel is still a click: it drops
/// the list of the folder's tabs down.
#[test]
fn a_click_on_a_chip_still_lists_its_tabs() {
    let mut rig = Rig::new(5);
    rig.folder(7, &[2, 3], false);
    let at = rig.chip_rect(7).center();
    rig.press(at);
    rig.release(at);
    assert_eq!(rig.state.open_folder, Some(FolderId(7)));
    assert_eq!(rig.state.tab_drag, None);
    assert_eq!(rig.order(), vec![1, 2, 3, 4, 5]);
}

/// A folder pulled off the strip and let go is the shell's to place,
/// as a folder: the drop names it.
#[test]
fn a_detached_folder_drag_is_dropped_as_a_folder() {
    let mut rig = Rig::new(5);
    rig.folder(7, &[2, 3], true);
    let from = rig.chip_rect(7).center();
    let to = Pos2::new(from.x + 80.0, 300.0);
    rig.press(from);
    rig.drag(from, to);
    assert!(rig.state.tab_drag.as_ref().is_some_and(|d| d.detached));
    assert!(
        rig.chip_rect(7).contains(to),
        "the chip rides with the pointer off the strip"
    );
    rig.release(to);
    let drop = rig.state.take_tab_drop().expect("a drop for the shell");
    assert_eq!(drop.tabs, vec![TabId(2), TabId(3)]);
    assert_eq!(drop.folder, Some(FolderId(7)));
    assert_eq!(rig.order(), vec![1, 2, 3, 4, 5]);
    assert!(
        rig.answered
            .iter()
            .all(|c| !matches!(c, Command::MoveFolder { .. } | Command::MoveTabs { .. }))
    );
}

/// More tabs than fit: a drag held near an end of the row scrolls the
/// row that way, so a tab can be taken to a place that was out of
/// sight when the drag began.
#[test]
fn a_drag_held_at_the_rows_end_scrolls_it() {
    let mut rig = Rig::new(30);
    let from = rig.tab_rect(3).center();
    let last = rig.tab_rect(30);
    // The right end of what shows: the "+" stands just past it.
    let plus = rig
        .ctx
        .read_response(Id::new(("new-tab", WindowId(1))))
        .map(|r| r.rect);
    let end = plus.map_or(SIZE.x - 120.0, |r| r.left()) - 10.0;
    assert!(last.left() > end, "the last tab is out of sight");
    let to = Pos2::new(end, from.y);
    rig.press(from);
    rig.drag(from, to);
    // Held there, the pointer still: two seconds of frames.
    for _ in 0..120 {
        rig.frame(Vec::new());
    }
    assert!(
        rig.tab_rect(30).right() <= end + 60.0,
        "the row has scrolled to its end: the last tab is at {:?}",
        rig.tab_rect(30)
    );
    rig.release(to);
    assert_eq!(
        rig.order().iter().position(|id| *id == 3),
        Some(29),
        "dropped past the tab that was out of sight"
    );

    // And back: held at the left end it scrolls to the start.
    let from = rig.tab_rect(3).center();
    let start = Pos2::new(rig.tab_rect(1).left().max(0.0) + 90.0, from.y);
    let from = Pos2::new(from.x.min(end), from.y);
    rig.press(from);
    rig.drag(from, start);
    for _ in 0..120 {
        rig.frame(Vec::new());
    }
    rig.release(start);
    // Scrolled to its start, the first tab is under the pointer: the drag
    // lands in the gap beside it, after tab 1 (index 1 of the row).
    assert_eq!(
        rig.order().iter().position(|id| *id == 3),
        Some(1),
        "back at the front, beside the first tab"
    );
}

#[test]
fn a_drag_scrolls_only_towards_tabs_that_are_out_of_sight() {
    let view = (100.0, 500.0);
    // Everything shows: nothing to scroll to, wherever the pointer is.
    assert_eq!(drag_scroll(495.0, view, (100.0, 500.0)), 0.0);
    assert_eq!(drag_scroll(105.0, view, (100.0, 400.0)), 0.0);
    // More past the right end only.
    let row = (100.0, 900.0);
    assert_eq!(drag_scroll(300.0, view, row), 0.0);
    assert_eq!(drag_scroll(500.0 - DRAG_SCROLL_EDGE, view, row), 1.0);
    assert_eq!(drag_scroll(640.0, view, row), 1.0, "past the end too");
    assert_eq!(drag_scroll(105.0, view, row), 0.0, "nothing to the left");
    // Scrolled to the middle: both ways.
    let row = (-200.0, 900.0);
    assert_eq!(drag_scroll(110.0, view, row), -1.0);
    assert_eq!(drag_scroll(490.0, view, row), 1.0);
    assert_eq!(drag_scroll(300.0, view, row), 0.0);
}
