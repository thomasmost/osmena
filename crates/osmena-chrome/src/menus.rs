//! The browser's menus as data: which entries they hold, which are enabled,
//! and the `Command` each one issues. Pure and unit-tested; `native_menu`
//! turns an entry list into a native menu and maps the click back.

use osmena_core::{
    Command, CommandKind, Extension, ExtensionSource, ExtensionSupport, KeyChord, Keymap, TabId,
    Window, WindowId,
};
use url::Url;

/// One entry of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        label: String,
        enabled: bool,
        /// Shown beside the label as the shortcut, when the command has one.
        chord: Option<KeyChord>,
        command: Command,
    },
    Submenu {
        label: String,
        enabled: bool,
        entries: Vec<MenuEntry>,
    },
    /// An item with a check mark; the command is what clicking it does
    /// (it carries the new state).
    Check {
        label: String,
        enabled: bool,
        checked: bool,
        command: Command,
    },
    Separator,
}

impl MenuEntry {
    fn item(label: impl Into<String>, enabled: bool, command: Command) -> MenuEntry {
        MenuEntry::Item {
            label: label.into(),
            enabled,
            chord: None,
            command,
        }
    }

    fn bound(
        keymap: &Keymap,
        kind: CommandKind,
        label: &str,
        window: WindowId,
        tab: TabId,
    ) -> MenuEntry {
        MenuEntry::Item {
            label: label.to_string(),
            enabled: true,
            chord: keymap.chord_for(kind),
            command: kind.into_command(window, tab),
        }
    }
}

/// The window menu behind the "☰" button. `can_reopen` says whether a
/// closed tab is there to reopen.
pub fn window_menu(window: &Window, keymap: &Keymap, can_reopen: bool) -> Vec<MenuEntry> {
    let tab = window.active;
    let bound = |kind: CommandKind| MenuEntry::bound(keymap, kind, &kind.label(), window.id, tab);
    let mut reopen = bound(CommandKind::ReopenClosedTab);
    if let MenuEntry::Item { enabled, .. } = &mut reopen {
        *enabled = can_reopen;
    }
    vec![
        bound(CommandKind::NewTab),
        bound(CommandKind::NewWindow),
        reopen,
        MenuEntry::Separator,
        bound(CommandKind::Home),
        bound(CommandKind::ToggleFullscreen),
        bound(CommandKind::OpenSettings),
        MenuEntry::Separator,
        bound(CommandKind::Quit),
    ]
}

/// The menu behind the toolbar's puzzle-piece button. Every installed
/// extension is listed: clicking one opens its action's popup (disabled
/// when it has none, or the extension is off). "Pin to toolbar" holds a
/// check item per extension. Then the ways to add one: load an unpacked
/// directory, install the extension the active tab's store page shows
/// (`store_url` is the page the user is on), visit the store, and manage
/// them in the settings window.
pub fn extensions_menu(
    window: &Window,
    extensions: &[Extension],
    pinned: &[String],
    support: &ExtensionSupport,
    active_url: &Url,
) -> Vec<MenuEntry> {
    let mut entries = Vec::new();
    if extensions.is_empty() {
        entries.push(MenuEntry::item(
            "No extensions installed",
            false,
            Command::OpenSettings,
        ));
    }
    for extension in extensions {
        entries.push(MenuEntry::item(
            extension.name.clone(),
            extension.has_action(),
            Command::OpenExtensionAction {
                window: window.id,
                extension: extension.id.clone(),
            },
        ));
    }
    entries.push(MenuEntry::Submenu {
        label: "Pin to toolbar".to_string(),
        enabled: !extensions.is_empty(),
        entries: extensions
            .iter()
            .map(|extension| {
                let checked = pinned.contains(&extension.id);
                MenuEntry::Check {
                    label: extension.name.clone(),
                    enabled: extension.enabled,
                    checked,
                    command: Command::PinExtension {
                        extension: extension.id.clone(),
                        pinned: !checked,
                    },
                }
            })
            .collect(),
    });
    entries.push(MenuEntry::Separator);
    let store_item = (support.store_item_id)(active_url);
    entries.push(MenuEntry::item(
        "Install the extension on this page",
        store_item.is_some(),
        Command::InstallExtension(ExtensionSource::Store {
            id: store_item.unwrap_or_default(),
        }),
    ));
    entries.push(MenuEntry::item(
        format!("Visit the {}", support.store_name),
        true,
        Command::NewTab {
            window: window.id,
            url: Url::parse(support.store_url).ok(),
        },
    ));
    entries.push(MenuEntry::item(
        "Load unpacked extension…",
        true,
        Command::InstallExtension(ExtensionSource::AskForDirectory),
    ));
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::item(
        "Manage extensions…",
        true,
        Command::OpenSettings,
    ));
    entries
}

/// The context menu of a tab, acting on `targets` (the selection, or the
/// right-clicked tab alone). `others` are the other windows (id and label)
/// and `profiles` the other profiles on this machine. Moves recreate the
/// tabs at their URLs: an engine's page is bound to the native window
/// it was created for.
pub fn tab_menu(
    window: &Window,
    targets: &[TabId],
    others: &[(WindowId, String)],
    profiles: &[String],
) -> Vec<MenuEntry> {
    let count = targets.len();
    let plural = |one: &str, many: &str| {
        if count == 1 {
            one.to_string()
        } else {
            many.replace("{n}", &count.to_string())
        }
    };
    let rest: Vec<TabId> = window
        .tabs
        .iter()
        .map(|t| t.id)
        .filter(|t| !targets.contains(t))
        .collect();
    let has_rest = !rest.is_empty();

    vec![
        MenuEntry::item(
            plural("Close tab", "Close {n} tabs"),
            true,
            Command::CloseTabs(targets.to_vec()),
        ),
        MenuEntry::item("Close other tabs", has_rest, Command::CloseTabs(rest)),
        MenuEntry::Separator,
        MenuEntry::item(
            plural("Move tab to new window", "Move {n} tabs to new window"),
            has_rest,
            Command::MoveTabsToNewWindow(targets.to_vec()),
        ),
        MenuEntry::Submenu {
            label: plural("Move tab to window", "Move {n} tabs to window"),
            enabled: !others.is_empty(),
            entries: others
                .iter()
                .map(|(id, label)| {
                    MenuEntry::item(
                        label.clone(),
                        true,
                        Command::MoveTabsToWindow {
                            tabs: targets.to_vec(),
                            window: *id,
                        },
                    )
                })
                .collect(),
        },
        MenuEntry::Submenu {
            label: plural("Open tab in profile", "Open {n} tabs in profile"),
            enabled: !profiles.is_empty(),
            entries: profiles
                .iter()
                .map(|name| {
                    MenuEntry::item(
                        name.clone(),
                        true,
                        Command::OpenTabsInProfile {
                            tabs: targets.to_vec(),
                            profile: name.clone(),
                        },
                    )
                })
                .collect(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use osmena_core::{Platform, Tab};
    use url::Url;

    fn window_with(n: u64) -> Window {
        let mut w = Window::with_tab(
            WindowId(1),
            Tab::new(TabId(1), Url::parse("https://a.example/").unwrap()),
        );
        for i in 2..=n {
            w.tabs.push(Tab::new(
                TabId(i),
                Url::parse(&format!("https://{i}.example/")).unwrap(),
            ));
        }
        w
    }

    fn labels(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                MenuEntry::Item { label, .. }
                | MenuEntry::Submenu { label, .. }
                | MenuEntry::Check { label, .. } => label.clone(),
                MenuEntry::Separator => "-".to_string(),
            })
            .collect()
    }

    fn enabled(entries: &[MenuEntry]) -> Vec<bool> {
        entries
            .iter()
            .map(|e| match e {
                MenuEntry::Item { enabled, .. }
                | MenuEntry::Submenu { enabled, .. }
                | MenuEntry::Check { enabled, .. } => *enabled,
                MenuEntry::Separator => true,
            })
            .collect()
    }

    fn store_id(url: &Url) -> Option<String> {
        (url.host_str() == Some("store.example"))
            .then(|| url.path().trim_start_matches('/').to_string())
    }

    const SUPPORT: ExtensionSupport = ExtensionSupport {
        store_name: "Test Store",
        store_url: "https://store.example/",
        store_item_id: store_id,
    };

    fn extension(id: &str, name: &str, enabled: bool, popup: bool) -> Extension {
        Extension {
            id: id.to_string(),
            name: name.to_string(),
            version: "1".to_string(),
            description: String::new(),
            enabled,
            icon: None,
            action_title: popup.then(|| "Act".to_string()),
        }
    }

    #[test]
    fn extensions_menu_lists_actions_pins_and_the_ways_to_install() {
        let w = window_with(1);
        let installed = [
            extension("a", "Alpha", true, true),
            extension("b", "Beta", true, false),
            extension("c", "Gamma", false, true),
        ];
        let pinned = vec!["b".to_string()];
        let url = Url::parse("https://store.example/detail-id").unwrap();
        let entries = extensions_menu(&w, &installed, &pinned, &SUPPORT, &url);
        assert_eq!(
            labels(&entries),
            vec![
                "Alpha",
                "Beta",
                "Gamma",
                "Pin to toolbar",
                "-",
                "Install the extension on this page",
                "Visit the Test Store",
                "Load unpacked extension…",
                "-",
                "Manage extensions…",
            ]
        );
        // Only an enabled extension with a popup opens; the store page
        // on the active tab makes the install entry live.
        assert_eq!(
            enabled(&entries),
            vec![true, false, false, true, true, true, true, true, true, true]
        );
        assert!(matches!(
            &entries[0],
            MenuEntry::Item { command: Command::OpenExtensionAction { window: WindowId(1), extension }, .. }
                if extension == "a"
        ));
        let MenuEntry::Submenu { entries: pins, .. } = &entries[3] else {
            panic!("pin is a submenu");
        };
        assert_eq!(labels(pins), vec!["Alpha", "Beta", "Gamma"]);
        assert!(matches!(
            &pins[0],
            MenuEntry::Check { checked: false, command: Command::PinExtension { extension, pinned: true }, .. }
                if extension == "a"
        ));
        assert!(matches!(
            &pins[1],
            MenuEntry::Check { checked: true, command: Command::PinExtension { extension, pinned: false }, .. }
                if extension == "b"
        ));
        assert!(matches!(&pins[2], MenuEntry::Check { enabled: false, .. }));
        assert!(matches!(
            &entries[5],
            MenuEntry::Item { command: Command::InstallExtension(ExtensionSource::Store { id }), .. }
                if id == "detail-id"
        ));
        assert!(matches!(
            &entries[6],
            MenuEntry::Item { command: Command::NewTab { window: WindowId(1), url: Some(u) }, .. }
                if u.as_str() == "https://store.example/"
        ));
        assert!(matches!(
            &entries[7],
            MenuEntry::Item {
                command: Command::InstallExtension(ExtensionSource::AskForDirectory),
                ..
            }
        ));
        assert!(matches!(
            &entries[9],
            MenuEntry::Item {
                command: Command::OpenSettings,
                ..
            }
        ));
    }

    #[test]
    fn extensions_menu_with_nothing_installed_says_so_and_cannot_install_from_a_plain_page() {
        let w = window_with(1);
        let url = Url::parse("https://a.example/").unwrap();
        let entries = extensions_menu(&w, &[], &[], &SUPPORT, &url);
        assert_eq!(labels(&entries)[0], "No extensions installed");
        assert_eq!(
            enabled(&entries),
            vec![false, false, true, false, true, true, true, true]
        );
    }

    #[test]
    fn window_menu_carries_the_keymap_chords_and_commands() {
        let w = window_with(1);
        let keymap = Keymap::defaults(Platform::Windows);
        let entries = window_menu(&w, &keymap, true);
        assert_eq!(
            labels(&entries),
            vec![
                "New tab",
                "New window",
                "Reopen closed tab",
                "-",
                "Open homepage",
                "Full screen",
                "Settings",
                "-",
                "Quit"
            ]
        );
        assert_eq!(enabled(&entries), vec![true; 9]);
        let MenuEntry::Item { chord, command, .. } = &entries[1] else {
            panic!("second entry is an item");
        };
        assert_eq!(*chord, keymap.chord_for(CommandKind::NewWindow));
        assert_eq!(*command, Command::NewWindow);
        assert!(matches!(
            &entries[2],
            MenuEntry::Item {
                command: Command::ReopenClosedTab(WindowId(1)),
                ..
            }
        ));
        assert!(matches!(
            &entries[5],
            MenuEntry::Item {
                command: Command::ToggleFullscreen(WindowId(1)),
                ..
            }
        ));
        assert!(matches!(
            &entries[8],
            MenuEntry::Item {
                command: Command::Quit,
                ..
            }
        ));
    }

    #[test]
    fn reopen_is_disabled_with_nothing_to_reopen() {
        let w = window_with(1);
        let entries = window_menu(&w, &Keymap::defaults(Platform::Windows), false);
        assert!(!enabled(&entries)[2]);
    }

    #[test]
    fn tab_menu_for_one_tab_of_three() {
        let w = window_with(3);
        let entries = tab_menu(
            &w,
            &[TabId(2)],
            &[(WindowId(7), "Other".to_string())],
            &["work".to_string()],
        );
        assert_eq!(
            labels(&entries),
            vec![
                "Close tab",
                "Close other tabs",
                "-",
                "Move tab to new window",
                "Move tab to window",
                "Open tab in profile",
            ]
        );
        assert_eq!(enabled(&entries), vec![true; 6]);
        assert!(matches!(
            &entries[1],
            MenuEntry::Item { command: Command::CloseTabs(rest), .. } if *rest == vec![TabId(1), TabId(3)]
        ));
        let MenuEntry::Submenu {
            entries: windows, ..
        } = &entries[4]
        else {
            panic!("move to window is a submenu");
        };
        assert_eq!(labels(windows), vec!["Other"]);
        assert!(matches!(
            &windows[0],
            MenuEntry::Item { command: Command::MoveTabsToWindow { tabs, window: WindowId(7) }, .. }
                if *tabs == vec![TabId(2)]
        ));
        let MenuEntry::Submenu {
            entries: profiles, ..
        } = &entries[5]
        else {
            panic!("open in profile is a submenu");
        };
        assert!(matches!(
            &profiles[0],
            MenuEntry::Item { command: Command::OpenTabsInProfile { profile, .. }, .. } if profile == "work"
        ));
    }

    #[test]
    fn tab_menu_counts_the_selection_and_disables_what_cannot_apply() {
        let w = window_with(3);
        let all = [TabId(1), TabId(2), TabId(3)];
        let entries = tab_menu(&w, &all, &[], &[]);
        assert_eq!(labels(&entries)[0], "Close 3 tabs");
        assert_eq!(labels(&entries)[3], "Move 3 tabs to new window");
        // Close others, move to new window: nothing left; no other windows
        // or profiles: the submenus are disabled.
        assert_eq!(
            enabled(&entries),
            vec![true, false, true, false, false, false]
        );
    }
}
