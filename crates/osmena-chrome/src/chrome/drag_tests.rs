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
            focused: true,
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
    assert_eq!(
        rig.state.take_tab_drop(),
        Some(TabDrop {
            tabs: vec![TabId(2)],
            pointer: to
        })
    );
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
#[ignore = "reproduces the tab-drag bug; un-ignore with the fix"]
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
#[ignore = "reproduces the tab-drag bug; un-ignore with the fix"]
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
    rig.release(to);
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
    // Three or four places on: the tab grew to the active tab's width
    // as the drag began, so its middle is right of the pointer's.
    let landed = rig.order().iter().position(|id| *id == 3);
    assert!(matches!(landed, Some(5 | 6)), "landed at {landed:?}");
    assert_eq!(rig.moves().len(), 1);
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
#[ignore = "reproduces the tab-drag bug; un-ignore with the fix"]
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
