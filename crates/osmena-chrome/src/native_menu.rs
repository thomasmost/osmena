//! Native popup menus (`muda`) built from a [`MenuEntry`] list. The menu is
//! shown synchronously for a winit window; the clicked entry comes back as
//! its `Command`. Only the browser's menus (the "☰" window menu, the tab
//! context menu, and the extensions menu) go through here; the rest of the
//! chrome is egui.

use std::collections::HashMap;

use muda::accelerator::Accelerator;
use muda::dpi::{LogicalPosition, Position};
use muda::{
    CheckMenuItem, ContextMenu, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem,
    Submenu,
};
use osmena_core::Command;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::menus::MenuEntry;

/// A native menu and the command behind each of its items.
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
        while MenuEvent::receiver().try_recv().is_ok() {}
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
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(command) = self.commands.get(event.id()) {
                chosen = Some(command.clone());
            }
        }
        chosen
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
