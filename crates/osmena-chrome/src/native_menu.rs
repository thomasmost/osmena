//! Native menus (`muda`) built from a [`MenuEntry`] list: the popups (the
//! "☰" window menu, the tab, folder, extensions and page context menus),
//! shown synchronously for a winit window with the clicked entry coming
//! back as its `Command`, and on macOS the application's [`MenuBar`],
//! whose picks arrive later through [`take_menu_ids`]. The rest of the
//! chrome is egui.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use muda::accelerator::Accelerator;
use muda::dpi::{LogicalPosition, Position};
use muda::{
    CheckMenuItem, ContextMenu, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem,
    Submenu,
};
use osmena_core::Command;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::menus::{MenuEntry, SystemItem};

/// The items picked and not yet taken. muda delivers every pick to one
/// process-wide handler; this is where it puts them.
static PICKED: Mutex<Vec<MenuId>> = Mutex::new(Vec::new());

/// Route every menu pick through [`take_menu_ids`], calling `wake` for
/// each so an event loop asleep in `Wait` runs and takes it. A popup
/// finds its pick there when it returns; the menu bar's picks arrive
/// whenever the user makes them. Call once; later calls do nothing.
pub fn install_event_handler(wake: impl Fn() + Send + Sync + 'static) {
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        PICKED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.id);
        wake();
    }));
}

/// The ids of the items picked since the last call, oldest first. Without
/// a handler installed, muda's own channel is drained instead.
pub fn take_menu_ids() -> Vec<MenuId> {
    let mut picked: Vec<MenuId> = PICKED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .drain(..)
        .collect();
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        picked.push(event.id);
    }
    picked
}

/// A native popup menu and the command behind each of its items.
pub struct NativeMenu {
    menu: Menu,
    commands: HashMap<MenuId, Command>,
}

impl NativeMenu {
    pub fn build(entries: &[MenuEntry]) -> NativeMenu {
        let mut commands = HashMap::new();
        let menu = Menu::new();
        let mut next_id = 0;
        for item in items(entries, &mut commands, &mut next_id) {
            // Appending only fails for an item already in another menu.
            let _ = menu.append(item.as_ref());
        }
        NativeMenu { menu, commands }
    }

    /// Show the menu at `position` (logical pixels from the window's
    /// top-left; `None` for the pointer) and block until it closes. Returns
    /// the command of the clicked item, if any.
    pub fn show(&self, window: &Window, position: Option<LogicalPosition<f64>>) -> Option<Command> {
        // Anything still queued from an earlier menu is stale.
        take_menu_ids();
        // muda mirrors y for a view whose origin is at the bottom (AppKit's
        // default), but winit's view is flipped, its origin at the top, so
        // the two mirrors cancel and a menu asked for at the tab strip
        // opened along the window's bottom edge. Mirror it here first.
        #[cfg(target_os = "macos")]
        let position = position.map(|p| {
            let height = window
                .inner_size()
                .to_logical::<f64>(window.scale_factor())
                .height;
            LogicalPosition::new(p.x, height - p.y)
        });
        let position = position.map(Position::Logical);
        let Ok(handle) = window.window_handle() else {
            return None;
        };
        match handle.as_raw() {
            #[cfg(target_os = "windows")]
            RawWindowHandle::Win32(h) => {
                // SAFETY: the handle comes from a live winit window.
                unsafe {
                    self.menu.show_context_menu_for_hwnd(h.hwnd.get(), position);
                }
            }
            #[cfg(target_os = "macos")]
            RawWindowHandle::AppKit(h) => {
                // SAFETY: the view pointer comes from a live winit window.
                unsafe {
                    self.menu
                        .show_context_menu_for_nsview(h.ns_view.as_ptr(), position);
                }
            }
            _ => return None,
        }
        let mut chosen = None;
        for id in take_menu_ids() {
            if let Some(command) = self.commands.get(&id) {
                chosen = Some(command.clone());
            }
        }
        chosen
    }
}

/// The application's menu bar, kept in step with a [`MenuEntry`] list of
/// top-level submenus (see `menus::menu_bar`). Only macOS has one; on
/// the other platforms [`MenuBar::sync`] keeps the entries and does
/// nothing else. A pick arrives through [`take_menu_ids`]; [`MenuBar::
/// command`] says what it means.
#[derive(Default)]
pub struct MenuBar {
    entries: Vec<MenuEntry>,
    menu: Option<Menu>,
    commands: HashMap<MenuId, Command>,
}

impl MenuBar {
    /// Make the menu bar show `entries`, rebuilding it only if they differ
    /// from what it shows. Cheap to call after every event.
    pub fn sync(&mut self, entries: Vec<MenuEntry>) {
        if self.menu.is_some() && entries == self.entries {
            return;
        }
        let mut commands = HashMap::new();
        let mut next_id = 0;
        let menu = Menu::new();
        for item in items(&entries, &mut commands, &mut next_id) {
            let _ = menu.append(item.as_ref());
        }
        #[cfg(target_os = "macos")]
        {
            use crate::menus::WINDOW_MENU_LABEL;
            menu.init_for_nsapp();
            for (entry, item) in entries.iter().zip(menu.items()) {
                if let (MenuEntry::Submenu { label, .. }, muda::MenuItemKind::Submenu(submenu)) =
                    (entry, item)
                    && label == WINDOW_MENU_LABEL
                {
                    submenu.set_as_windows_menu_for_nsapp();
                }
            }
        }
        self.entries = entries;
        self.menu = Some(menu);
        self.commands = commands;
    }

    /// The command of the item with this id, if it is the menu bar's.
    pub fn command(&self, id: &MenuId) -> Option<Command> {
        self.commands.get(id).cloned()
    }

    /// What the menu bar shows.
    pub fn entries(&self) -> &[MenuEntry] {
        &self.entries
    }
}

fn items(
    entries: &[MenuEntry],
    commands: &mut HashMap<MenuId, Command>,
    next_id: &mut usize,
) -> Vec<Box<dyn IsMenuItem>> {
    entries
        .iter()
        .map(|entry| -> Box<dyn IsMenuItem> {
            match entry {
                MenuEntry::Item {
                    label,
                    enabled,
                    chord,
                    command,
                } => {
                    let id = MenuId::new(format!("osmena-menu-{}", *next_id));
                    *next_id += 1;
                    commands.insert(id.clone(), command.clone());
                    let accelerator = chord.map(|c| Accelerator::new(c.modifiers, c.code));
                    Box::new(MenuItem::with_id(id, literal(label), *enabled, accelerator))
                }
                MenuEntry::Submenu {
                    label,
                    enabled,
                    entries,
                } => {
                    let submenu = Submenu::new(literal(label), *enabled);
                    for item in items(entries, commands, next_id) {
                        let _ = submenu.append(item.as_ref());
                    }
                    Box::new(submenu)
                }
                MenuEntry::Check {
                    label,
                    enabled,
                    checked,
                    command,
                } => {
                    let id = MenuId::new(format!("osmena-menu-{}", *next_id));
                    *next_id += 1;
                    commands.insert(id.clone(), command.clone());
                    Box::new(CheckMenuItem::with_id(
                        id,
                        literal(label),
                        *enabled,
                        *checked,
                        None,
                    ))
                }
                MenuEntry::Disabled { label, chord } => {
                    let accelerator = chord.map(|c| Accelerator::new(c.modifiers, c.code));
                    Box::new(MenuItem::new(literal(label), false, accelerator))
                }
                MenuEntry::System { label, item } => {
                    let text = Some(literal(label));
                    let text = text.as_deref();
                    Box::new(match item {
                        SystemItem::About => PredefinedMenuItem::about(text, None),
                        SystemItem::Services => PredefinedMenuItem::services(text),
                        SystemItem::Hide => PredefinedMenuItem::hide(text),
                        SystemItem::HideOthers => PredefinedMenuItem::hide_others(text),
                        SystemItem::ShowAll => PredefinedMenuItem::show_all(text),
                        SystemItem::Minimize => PredefinedMenuItem::minimize(text),
                        SystemItem::Zoom => PredefinedMenuItem::zoom(text),
                        SystemItem::BringAllToFront => PredefinedMenuItem::bring_all_to_front(text),
                    })
                }
                MenuEntry::Separator => Box::new(PredefinedMenuItem::separator()),
            }
        })
        .collect()
}

/// `label` as muda shows it unchanged: a lone `&` marks a mnemonic
/// (underlined on Windows, dropped on macOS), and a page title or a
/// search text may well hold one.
fn literal(label: &str) -> String {
    label.replace('&', "&&")
}

#[cfg(test)]
mod tests {
    use super::literal;

    #[test]
    fn ampersands_are_doubled_so_they_show() {
        assert_eq!(literal("Fish & Chips"), "Fish && Chips");
        assert_eq!(literal("Plain"), "Plain");
    }
}
