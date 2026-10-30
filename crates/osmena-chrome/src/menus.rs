//! The browser's menus as data: which entries they hold, which are enabled,
//! and the `Command` each one issues. Pure and unit-tested; `native_menu`
//! turns an entry list into a native menu and maps the click back.

use keyboard_types::{Code, Modifiers};
use osmena_core::profile::DEFAULT_PROFILE_NAME;
use osmena_core::{
    Bookmark, BookmarkGroup, Bookmarks, BookmarksFile, Command, CommandKind, Extension,
    ExtensionSource, ExtensionSupport, FolderId, History, HistoryEntry, KeyChord, Keymap,
    LoadState, MediaKind, PageCommand, PageContext, PageTools, Platform, ProfileEntry, TabId,
    UpdateStatus, Window, WindowId,
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
    /// An item that cannot be picked and has nothing to do: a window
    /// action in a menu bar while no browser window is open.
    Disabled {
        label: String,
        chord: Option<KeyChord>,
    },
    /// An item the system provides and carries out itself (the
    /// application and Window menus of a macOS menu bar).
    System {
        label: String,
        item: SystemItem,
    },
    Separator,
}

/// The standard items a macOS menu bar takes from the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemItem {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    BringAllToFront,
}

/// The label of the menu bar's Window menu. The system appends the open
/// windows to the submenu with this label.
pub const WINDOW_MENU_LABEL: &str = "Window";

/// How many pages the menu bar's History menu lists, most recent first.
pub const RECENT_HISTORY: usize = 10;

/// What the menu bar is built from.
pub struct MenuBarInput<'a> {
    /// The product name, for "About …", "Hide …" and "Quit …".
    pub brand: &'a str,
    /// The browser window the menus act on (the focused one). `None`
    /// when no browser window is open: the entries that need one are
    /// disabled.
    pub window: Option<&'a Window>,
    pub keymap: &'a Keymap,
    /// Whether a closed tab is there to reopen.
    pub can_reopen: bool,
    pub bookmarks: BookmarksMenu<'a>,
    pub history: &'a History,
    /// The current profile: its directory name and its display name.
    pub profile: CurrentProfile<'a>,
    /// The other profiles on this machine.
    pub profiles: &'a [ProfileEntry],
    /// Where the browser is with updates, for the application menu's
    /// update entry.
    pub updates: UpdateMenu<'a>,
}

/// What the update entry of a menu is built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateMenu<'a> {
    /// The product name, for "Restart to update …".
    pub brand: &'a str,
    pub status: &'a UpdateStatus,
    /// Whether this build can update itself at all (`Brand::can_update`);
    /// the entry is disabled otherwise.
    pub can_update: bool,
}

impl UpdateMenu<'_> {
    /// The one entry about updates: "Check for updates…" when nothing is
    /// going on, "Restart to update <brand>" when a build is ready, and
    /// a disabled line saying what is under way in between. Disabled
    /// outright for a build that cannot update itself.
    pub fn entry(&self) -> MenuEntry {
        let disabled = |label: String| MenuEntry::Disabled { label, chord: None };
        if !self.can_update {
            return disabled("Check for updates…".to_string());
        }
        match self.status {
            UpdateStatus::Idle | UpdateStatus::UpToDate | UpdateStatus::Failed(_) => {
                MenuEntry::item("Check for updates…", true, Command::CheckForUpdates)
            }
            UpdateStatus::Checking => disabled("Checking for updates…".to_string()),
            UpdateStatus::Downloading { .. } | UpdateStatus::Installing(_) => {
                disabled(self.status.describe(self.brand))
            }
            UpdateStatus::Ready { .. } => MenuEntry::item(
                format!("Restart to update {}", self.brand),
                true,
                Command::InstallUpdate,
            ),
        }
    }
}

/// What the Bookmarks menus are built from.
#[derive(Clone, Copy, Debug)]
pub struct BookmarksMenu<'a> {
    pub bookmarks: &'a Bookmarks,
    /// Whether the bookmarks bar is shown (the check item).
    pub bar_shown: bool,
}

/// The profile this browser runs, as the Profiles menu names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentProfile<'a> {
    /// The directory name (`--profile <dir>`).
    pub dir: &'a str,
    /// The display name.
    pub name: &'a str,
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
/// closed tab is there to reopen; the Bookmarks submenu holds the
/// actions and a submenu per group ([`bookmarks_entries`]); `updates`
/// is the entry about updates, above Settings.
pub fn window_menu(
    window: &Window,
    keymap: &Keymap,
    can_reopen: bool,
    bookmarks: BookmarksMenu<'_>,
    updates: UpdateMenu<'_>,
) -> Vec<MenuEntry> {
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
        MenuEntry::Submenu {
            label: "Bookmarks".to_string(),
            enabled: true,
            entries: bookmarks_entries(Some(window), keymap, bookmarks, false),
        },
        bound(CommandKind::ToggleFullscreen),
        MenuEntry::Separator,
        updates.entry(),
        bound(CommandKind::OpenSettings),
        MenuEntry::Separator,
        bound(CommandKind::Quit),
    ]
}

/// The entries of a Bookmarks menu (the "☰" menu's submenu, or the
/// menu bar's menu, in title case): bookmark this tab (the star's
/// command, with its chord), bookmark all tabs, the bookmarks bar
/// switch, import and export, then a submenu per group. Entries that
/// need a browser window are disabled without one.
pub fn bookmarks_entries(
    window: Option<&Window>,
    keymap: &Keymap,
    bookmarks: BookmarksMenu<'_>,
    title_case: bool,
) -> Vec<MenuEntry> {
    let BookmarksMenu {
        bookmarks,
        bar_shown,
    } = bookmarks;
    let label = |sentence: &str, title: &str| {
        if title_case {
            title.to_string()
        } else {
            sentence.to_string()
        }
    };
    let this_tab = label("Bookmark this tab…", "Bookmark This Tab…");
    let all_tabs = label("Bookmark all tabs…", "Bookmark All Tabs…");
    let mut entries = match window {
        Some(w) => {
            let tab = w.active_tab();
            vec![
                MenuEntry::Item {
                    label: this_tab,
                    enabled: Bookmarks::bookmarkable(&tab.url),
                    chord: keymap.chord_for(CommandKind::BookmarkPage),
                    command: Command::ToggleBookmark(tab.id),
                },
                MenuEntry::item(
                    all_tabs,
                    w.tabs.iter().any(|t| Bookmarks::bookmarkable(&t.url)),
                    Command::PromptBookmarkTabs(w.tabs.iter().map(|t| t.id).collect()),
                ),
            ]
        }
        None => vec![
            MenuEntry::Disabled {
                label: this_tab,
                chord: keymap.chord_for(CommandKind::BookmarkPage),
            },
            MenuEntry::Disabled {
                label: all_tabs,
                chord: None,
            },
        ],
    };
    entries.push(MenuEntry::Check {
        label: label("Show bookmarks bar", "Show Bookmarks Bar"),
        enabled: true,
        checked: bar_shown,
        command: Command::ToggleBookmarksBar,
    });
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::item(
        label("Import bookmarks…", "Import Bookmarks…"),
        true,
        Command::PickBookmarksFile(BookmarksFile::Import),
    ));
    entries.push(MenuEntry::item(
        label("Export bookmarks…", "Export Bookmarks…"),
        !bookmarks.is_empty(),
        Command::PickBookmarksFile(BookmarksFile::Export),
    ));
    entries.push(MenuEntry::Separator);
    match window {
        Some(w) if !bookmarks.groups.is_empty() => {
            entries.extend(bookmarks.groups.iter().map(|group| MenuEntry::Submenu {
                label: group.name.clone(),
                enabled: true,
                entries: group_entries(w.id, group, title_case),
            }));
        }
        _ => entries.push(MenuEntry::Disabled {
            label: label("No bookmarks", "No Bookmarks"),
            chord: None,
        }),
    }
    entries
}

/// A group's entries: "Open all", one entry per page (a new tab of
/// `window`), then rename and delete.
fn group_entries(window: WindowId, group: &BookmarkGroup, title_case: bool) -> Vec<MenuEntry> {
    let label = |sentence: &str, title: &str| {
        if title_case {
            title.to_string()
        } else {
            sentence.to_string()
        }
    };
    let mut entries = vec![
        MenuEntry::item(
            format!(
                "{} ({})",
                label("Open all", "Open All"),
                group.bookmarks.len()
            ),
            !group.bookmarks.is_empty(),
            Command::OpenBookmarkGroup {
                window,
                group: group.name.clone(),
            },
        ),
        MenuEntry::Separator,
    ];
    entries.extend(group.bookmarks.iter().map(|b| {
        MenuEntry::item(
            elide(bookmark_label(b), 60),
            true,
            Command::NewTab {
                window,
                url: Some(b.url.clone()),
            },
        )
    }));
    if !group.bookmarks.is_empty() {
        entries.push(MenuEntry::Separator);
    }
    entries.push(MenuEntry::item(
        label("Rename group…", "Rename Group…"),
        true,
        Command::PromptRenameBookmarkGroup {
            window,
            group: group.name.clone(),
        },
    ));
    entries.push(MenuEntry::item(
        label("Delete group", "Delete Group"),
        true,
        Command::PromptDeleteBookmarkGroup {
            window,
            group: group.name.clone(),
        },
    ));
    entries
}

/// A group chip's menu in the bookmarks bar: the group's entries.
pub fn bookmark_group_menu(window: WindowId, group: &BookmarkGroup) -> Vec<MenuEntry> {
    group_entries(window, group, false)
}

/// A bookmark chip's context menu in the bookmarks bar.
pub fn bookmark_menu(window: WindowId, url: &Url) -> Vec<MenuEntry> {
    vec![
        MenuEntry::item(
            "Open in new tab",
            true,
            Command::NewTab {
                window,
                url: Some(url.clone()),
            },
        ),
        MenuEntry::item(
            "Open in new window",
            true,
            Command::OpenInNewWindow(url.clone()),
        ),
        MenuEntry::item(
            "Copy link address",
            true,
            Command::CopyText(url.to_string()),
        ),
        MenuEntry::Separator,
        MenuEntry::item(
            "Edit…",
            true,
            Command::PromptEditBookmark {
                window,
                url: url.clone(),
            },
        ),
        MenuEntry::item("Delete", true, Command::RemoveBookmark(url.clone())),
    ]
}

/// The bar's "»" menu: the bookmarks that did not fit, each loading in
/// `tab`.
pub fn bookmark_overflow_menu(tab: TabId, rest: &[Bookmark]) -> Vec<MenuEntry> {
    rest.iter()
        .map(|b| {
            MenuEntry::item(
                elide(bookmark_label(b), 60),
                true,
                Command::Navigate {
                    tab,
                    url: b.url.clone(),
                },
            )
        })
        .collect()
}

/// A bookmark's title, or its address when it has none.
pub fn bookmark_label(bookmark: &Bookmark) -> &str {
    if bookmark.title.is_empty() {
        bookmark.url.as_str()
    } else {
        &bookmark.title
    }
}

/// The application's menu bar, as macOS shows it: the application menu
/// (named by the system after the bundle), then File, Edit, View,
/// History, Bookmarks, Profiles, Tab and Window. Every entry is a
/// `Command` for the focused window or one the system carries out
/// itself; the shortcuts shown are the keymap's, so the menu bar and the
/// keyboard agree, and the Edit entries carry the standard editing
/// chords. Without a browser window only the window-free entries are
/// enabled.
pub fn menu_bar(input: &MenuBarInput<'_>) -> Vec<MenuEntry> {
    let MenuBarInput {
        brand,
        window,
        keymap,
        can_reopen,
        bookmarks,
        history,
        profile,
        profiles,
        updates,
    } = *input;
    let ids = window.map(|w| (w.id, w.active));
    let active = window.map(Window::active_tab);
    let system = |label: String, item: SystemItem| MenuEntry::System { label, item };
    // A keymap action with its chord: for the whole application, or
    // for the focused window (disabled without one).
    let global = |kind: CommandKind, label: &str, command: Command| MenuEntry::Item {
        label: label.to_string(),
        enabled: true,
        chord: keymap.chord_for(kind),
        command,
    };
    let bound = |kind: CommandKind, label: &str, enabled: bool| match ids {
        Some((window, tab)) => MenuEntry::Item {
            label: label.to_string(),
            enabled,
            chord: keymap.chord_for(kind),
            command: kind.into_command(window, tab),
        },
        None => MenuEntry::Disabled {
            label: label.to_string(),
            chord: keymap.chord_for(kind),
        },
    };
    // A command on the active tab, without a chord.
    let on_tab = |label: &str, enabled: bool, command: fn(TabId) -> Command| match ids {
        Some((_, tab)) => MenuEntry::Item {
            label: label.to_string(),
            enabled,
            chord: None,
            command: command(tab),
        },
        None => MenuEntry::Disabled {
            label: label.to_string(),
            chord: None,
        },
    };
    let edit = |label: &str, modifiers: Modifiers, code: Code, command: PageCommand| match ids {
        Some((_, tab)) => MenuEntry::Item {
            label: label.to_string(),
            enabled: true,
            chord: Some(KeyChord::new(modifiers, code)),
            command: Command::Page { tab, command },
        },
        None => MenuEntry::Disabled {
            label: label.to_string(),
            chord: Some(KeyChord::new(modifiers, code)),
        },
    };
    let menu = |label: &str, entries: Vec<MenuEntry>| MenuEntry::Submenu {
        label: label.to_string(),
        enabled: true,
        entries,
    };
    let loading = active.is_some_and(|t| t.load == LoadState::Loading);
    let several_tabs = window.is_some_and(|w| w.tabs.len() > 1);
    let cmd = Modifiers::META;
    let cmd_shift = Modifiers::META | Modifiers::SHIFT;
    let cmd_alt_shift = Modifiers::META | Modifiers::ALT | Modifiers::SHIFT;

    let mut recent: Vec<&HistoryEntry> = history.entries().iter().collect();
    recent.sort_by_key(|entry| std::cmp::Reverse(entry.last_visit));
    recent.truncate(RECENT_HISTORY);
    let mut history_menu = vec![
        bound(
            CommandKind::Back,
            "Back",
            active.is_some_and(|t| t.can_go_back),
        ),
        bound(
            CommandKind::Forward,
            "Forward",
            active.is_some_and(|t| t.can_go_forward),
        ),
        bound(CommandKind::Home, "Home", true),
        MenuEntry::Separator,
        bound(
            CommandKind::ReopenClosedTab,
            "Reopen Closed Tab",
            can_reopen,
        ),
    ];
    if !recent.is_empty() {
        history_menu.push(MenuEntry::Separator);
    }
    for entry in recent {
        let label = if entry.title.is_empty() {
            entry.url.to_string()
        } else {
            entry.title.clone()
        };
        history_menu.push(match ids {
            Some((_, tab)) => MenuEntry::item(
                label,
                true,
                Command::Navigate {
                    tab,
                    url: entry.url.clone(),
                },
            ),
            None => MenuEntry::Disabled { label, chord: None },
        });
    }

    let bookmarks_menu = bookmarks_entries(window, keymap, bookmarks, true);

    let profiles_menu = profiles_menu(ids.map(|(window, _)| window), profile, profiles);

    vec![
        menu(
            brand,
            vec![
                system(format!("About {brand}"), SystemItem::About),
                updates.entry(),
                MenuEntry::Separator,
                global(
                    CommandKind::OpenSettings,
                    "Settings…",
                    Command::OpenSettings,
                ),
                MenuEntry::Separator,
                system("Services".to_string(), SystemItem::Services),
                MenuEntry::Separator,
                system(format!("Hide {brand}"), SystemItem::Hide),
                system("Hide Others".to_string(), SystemItem::HideOthers),
                system("Show All".to_string(), SystemItem::ShowAll),
                MenuEntry::Separator,
                global(CommandKind::Quit, &format!("Quit {brand}"), Command::Quit),
            ],
        ),
        menu(
            "File",
            vec![
                bound(CommandKind::NewTab, "New Tab", true),
                global(CommandKind::NewWindow, "New Window", Command::NewWindow),
                bound(
                    CommandKind::ReopenClosedTab,
                    "Reopen Closed Tab",
                    can_reopen,
                ),
                MenuEntry::Separator,
                bound(CommandKind::CloseTab, "Close Tab", true),
                bound(CommandKind::CloseWindow, "Close Window", true),
            ],
        ),
        menu(
            "Edit",
            vec![
                edit("Undo", cmd, Code::KeyZ, PageCommand::Undo),
                edit("Redo", cmd_shift, Code::KeyZ, PageCommand::Redo),
                MenuEntry::Separator,
                edit("Cut", cmd, Code::KeyX, PageCommand::Cut),
                edit("Copy", cmd, Code::KeyC, PageCommand::Copy),
                edit("Paste", cmd, Code::KeyV, PageCommand::Paste),
                edit(
                    "Paste and Match Style",
                    cmd_alt_shift,
                    Code::KeyV,
                    PageCommand::PasteAsPlainText,
                ),
                on_tab("Delete", true, |tab| Command::Page {
                    tab,
                    command: PageCommand::Delete,
                }),
                edit("Select All", cmd, Code::KeyA, PageCommand::SelectAll),
            ],
        ),
        menu(
            "View",
            vec![
                bound(CommandKind::Reload, "Reload Page", true),
                bound(CommandKind::Stop, "Stop Loading", loading),
                MenuEntry::Separator,
                bound(CommandKind::FocusAddressBar, "Open Location…", true),
                MenuEntry::Separator,
                bound(CommandKind::ToggleFullscreen, "Toggle Full Screen", true),
            ],
        ),
        menu("History", history_menu),
        menu("Bookmarks", bookmarks_menu),
        menu("Profiles", profiles_menu),
        menu(
            "Tab",
            vec![
                bound(CommandKind::NextTab, "Show Next Tab", several_tabs),
                bound(CommandKind::PreviousTab, "Show Previous Tab", several_tabs),
                MenuEntry::Separator,
                on_tab("Move Tab to New Window", several_tabs, |tab| {
                    Command::MoveTabsToNewWindow(vec![tab])
                }),
                on_tab("Move Tab to Folder…", true, |tab| {
                    Command::PromptMoveToFolder(vec![tab])
                }),
            ],
        ),
        menu(
            WINDOW_MENU_LABEL,
            vec![
                system("Minimize".to_string(), SystemItem::Minimize),
                system("Zoom".to_string(), SystemItem::Zoom),
                MenuEntry::Separator,
                system(
                    "Bring All to Front".to_string(),
                    SystemItem::BringAllToFront,
                ),
            ],
        ),
    ]
}

/// The Profiles menu, behind the toolbar's avatar button and in the
/// menu bar: the current profile (checked) and "Edit…" for it, the
/// other profiles (each opens a browser for it), "Add Profile…", and a
/// "Delete Profile" submenu of the others and, when it is not the
/// default, the current one. The dialogs need a browser window
/// (`window`); without one their entries are disabled.
pub fn profiles_menu(
    window: Option<WindowId>,
    current: CurrentProfile<'_>,
    others: &[ProfileEntry],
) -> Vec<MenuEntry> {
    let in_window = |label: &str, command: fn(WindowId) -> Command| match window {
        Some(window) => MenuEntry::item(label, true, command(window)),
        None => MenuEntry::Disabled {
            label: label.to_string(),
            chord: None,
        },
    };
    let mut entries = vec![
        MenuEntry::Check {
            label: current.name.to_string(),
            enabled: false,
            checked: true,
            command: Command::OpenProfile(current.dir.to_string()),
        },
        in_window("Edit…", Command::PromptEditProfile),
        MenuEntry::Separator,
    ];
    if others.is_empty() {
        entries.push(MenuEntry::Disabled {
            label: "No Other Profiles".to_string(),
            chord: None,
        });
    }
    for other in others {
        entries.push(MenuEntry::item(
            other.name.clone(),
            true,
            Command::OpenProfile(other.dir.clone()),
        ));
    }
    entries.push(MenuEntry::Separator);
    entries.push(in_window("Add Profile…", Command::PromptAddProfile));
    let deletable: Vec<(&str, &str)> = others
        .iter()
        .map(|p| (p.dir.as_str(), p.name.as_str()))
        .chain((current.dir != DEFAULT_PROFILE_NAME).then_some((current.dir, current.name)))
        .filter(|(dir, _)| *dir != DEFAULT_PROFILE_NAME)
        .collect();
    entries.push(MenuEntry::Submenu {
        label: "Delete Profile".to_string(),
        enabled: window.is_some() && !deletable.is_empty(),
        entries: match window {
            Some(window) => deletable
                .iter()
                .map(|(dir, name)| {
                    MenuEntry::item(
                        *name,
                        true,
                        Command::PromptDeleteProfile {
                            window,
                            profile: dir.to_string(),
                        },
                    )
                })
                .collect(),
            None => Vec::new(),
        },
    });
    entries
}

/// The context menu of a folder in the strip.
pub fn folder_menu(window: &Window, folder: FolderId) -> Vec<MenuEntry> {
    let Some(f) = window.folder(folder) else {
        return Vec::new();
    };
    let count = window.folder_tabs(folder).len();
    vec![
        if f.expanded {
            MenuEntry::item(
                "Collapse",
                true,
                Command::SetFolderExpanded {
                    folder,
                    expanded: false,
                },
            )
        } else {
            MenuEntry::item(
                "Expand",
                true,
                Command::SetFolderExpanded {
                    folder,
                    expanded: true,
                },
            )
        },
        MenuEntry::item("Rename…", true, Command::PromptRenameFolder(folder)),
        MenuEntry::Separator,
        MenuEntry::item("Ungroup", true, Command::Ungroup(folder)),
        MenuEntry::item(
            if count == 1 {
                "Close folder (1 tab)".to_string()
            } else {
                format!("Close folder ({count} tabs)")
            },
            true,
            Command::CloseFolder(folder),
        ),
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

/// The context menu of `tab`'s page, over what the engine found under the
/// pointer (`context`). Shaped like Chromium's: the link's entries, then
/// the media's, then the selection's, then the editing entries, and, when
/// none of those apply, the page's own (back, forward, reload, save,
/// print, view source); "Inspect" is last. `tools` leaves out what the
/// engine cannot do; `search_name` labels the search of selected text.
/// Empty if `tab` is not in `window`.
pub fn page_menu(
    window: &Window,
    tab: TabId,
    context: &PageContext,
    tools: &PageTools,
    keymap: &Keymap,
    platform: Platform,
    search_name: &str,
) -> Vec<MenuEntry> {
    let Some(page_tab) = window.tab(tab) else {
        return Vec::new();
    };
    let mut entries: Vec<MenuEntry> = Vec::new();
    let page = |command: PageCommand| Command::Page { tab, command };
    let primary = platform.primary();
    let chord = |modifiers: Modifiers, code: Code| Some(KeyChord { modifiers, code });
    let search = |text: &str| {
        MenuEntry::item(
            format!(
                "Search {search_name} for \u{201c}{}\u{201d}",
                elide(text, 32)
            ),
            true,
            Command::SearchInNewTab {
                window: window.id,
                query: text.to_string(),
            },
        )
    };

    if let Some(link) = &context.link {
        entries.push(MenuEntry::item(
            "Open link in new tab",
            true,
            Command::NewBackgroundTab {
                opener: tab,
                url: link.clone(),
            },
        ));
        entries.push(MenuEntry::item(
            "Open link in new window",
            true,
            Command::OpenInNewWindow(link.clone()),
        ));
        entries.push(MenuEntry::Separator);
        if tools.save {
            entries.push(MenuEntry::item(
                "Save link as\u{2026}",
                true,
                page(PageCommand::Save(link.clone())),
            ));
        }
        entries.push(MenuEntry::item(
            "Copy link address",
            true,
            Command::CopyText(link.to_string()),
        ));
    }

    if let Some(media) = &context.media {
        separate(&mut entries);
        let noun = media.kind.noun();
        let open = MenuEntry::item(
            format!("Open {noun} in new tab"),
            true,
            Command::NewBackgroundTab {
                opener: tab,
                url: media.src.clone(),
            },
        );
        let save = tools.save.then(|| {
            MenuEntry::item(
                format!("Save {noun} as\u{2026}"),
                true,
                page(PageCommand::Save(media.src.clone())),
            )
        });
        let address = MenuEntry::item(
            format!("Copy {noun} address"),
            true,
            Command::CopyText(media.src.to_string()),
        );
        match media.kind {
            MediaKind::Image => {
                entries.push(open);
                entries.extend(save);
                if tools.copy_image {
                    entries.push(MenuEntry::item(
                        "Copy image",
                        true,
                        page(PageCommand::CopyImage(media.src.clone())),
                    ));
                }
                entries.push(address);
            }
            MediaKind::Video | MediaKind::Audio => {
                if tools.media_controls {
                    if let Some(looping) = media.looping {
                        entries.push(MenuEntry::Check {
                            label: "Loop".to_string(),
                            enabled: true,
                            checked: looping,
                            command: page(PageCommand::SetMediaLoop(!looping)),
                        });
                    }
                    if let Some(controls) = media.controls {
                        entries.push(MenuEntry::Check {
                            label: "Show controls".to_string(),
                            enabled: true,
                            checked: controls,
                            command: page(PageCommand::SetMediaControls(!controls)),
                        });
                    }
                    separate(&mut entries);
                }
                entries.extend(save);
                entries.push(address);
                entries.push(open);
            }
        }
    }

    let selection = context
        .selection
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());

    match context.editable {
        None => {
            if context.selection.is_some() {
                separate(&mut entries);
                entries.push(MenuEntry::Item {
                    label: "Copy".to_string(),
                    enabled: true,
                    chord: chord(primary, Code::KeyC),
                    command: page(PageCommand::Copy),
                });
                if let Some(text) = selection {
                    entries.push(search(text));
                }
                if tools.print {
                    entries.push(MenuEntry::item(
                        "Print\u{2026}",
                        true,
                        page(PageCommand::Print),
                    ));
                }
            }
        }
        Some(edit) => {
            separate(&mut entries);
            if let Some(misspelling) = &context.misspelling {
                if misspelling.suggestions.is_empty() {
                    entries.push(MenuEntry::item(
                        "No spelling suggestions",
                        false,
                        page(PageCommand::AddToDictionary(misspelling.word.clone())),
                    ));
                }
                for suggestion in misspelling.suggestions.iter().take(5) {
                    entries.push(MenuEntry::item(
                        suggestion.clone(),
                        true,
                        page(PageCommand::ReplaceMisspelling(suggestion.clone())),
                    ));
                }
                entries.push(MenuEntry::item(
                    "Add to dictionary",
                    true,
                    page(PageCommand::AddToDictionary(misspelling.word.clone())),
                ));
                entries.push(MenuEntry::Separator);
            }
            let redo = if platform == Platform::MacOs {
                chord(primary | Modifiers::SHIFT, Code::KeyZ)
            } else {
                chord(primary, Code::KeyY)
            };
            let mut editing = vec![
                (
                    "Undo",
                    edit.can_undo,
                    chord(primary, Code::KeyZ),
                    PageCommand::Undo,
                ),
                ("Redo", edit.can_redo, redo, PageCommand::Redo),
            ];
            let mut clipboard = vec![
                (
                    "Cut",
                    edit.can_cut,
                    chord(primary, Code::KeyX),
                    PageCommand::Cut,
                ),
                (
                    "Copy",
                    edit.can_copy,
                    chord(primary, Code::KeyC),
                    PageCommand::Copy,
                ),
                (
                    "Paste",
                    edit.can_paste,
                    chord(primary, Code::KeyV),
                    PageCommand::Paste,
                ),
            ];
            if tools.paste_plain {
                clipboard.push((
                    "Paste as plain text",
                    edit.can_paste,
                    chord(primary | Modifiers::SHIFT, Code::KeyV),
                    PageCommand::PasteAsPlainText,
                ));
            }
            clipboard.push(("Delete", edit.can_delete, None, PageCommand::Delete));
            let select = (
                "Select all",
                edit.can_select_all,
                chord(primary, Code::KeyA),
                PageCommand::SelectAll,
            );
            for group in [&mut editing, &mut clipboard, &mut vec![select]] {
                for (label, enabled, chord, command) in group.drain(..) {
                    entries.push(MenuEntry::Item {
                        label: label.to_string(),
                        enabled,
                        chord,
                        command: page(command),
                    });
                }
                entries.push(MenuEntry::Separator);
            }
            match selection {
                Some(text) => entries.push(search(text)),
                None => {
                    entries.pop();
                }
            }
        }
    }

    if entries.is_empty() {
        let mut history = |kind: CommandKind, enabled: bool| {
            let mut entry = MenuEntry::bound(keymap, kind, &kind.label(), window.id, tab);
            if let MenuEntry::Item { enabled: e, .. } = &mut entry {
                *e = enabled;
            }
            entries.push(entry);
        };
        history(CommandKind::Back, page_tab.can_go_back);
        history(CommandKind::Forward, page_tab.can_go_forward);
        history(CommandKind::Reload, true);
        if tools.save || tools.print {
            entries.push(MenuEntry::Separator);
        }
        if tools.save {
            entries.push(MenuEntry::item(
                "Save as\u{2026}",
                true,
                page(PageCommand::Save(page_tab.url.clone())),
            ));
        }
        if tools.print {
            entries.push(MenuEntry::item(
                "Print\u{2026}",
                true,
                page(PageCommand::Print),
            ));
        }
        if tools.view_source {
            entries.push(MenuEntry::Separator);
            entries.push(MenuEntry::item(
                "View page source",
                true,
                page(PageCommand::ViewSource),
            ));
        }
    }

    if tools.inspect {
        separate(&mut entries);
        entries.push(MenuEntry::item("Inspect", true, page(PageCommand::Inspect)));
    }
    entries
}

/// A separator before the next group, unless the menu is empty or one is
/// there already.
fn separate(entries: &mut Vec<MenuEntry>) {
    if !entries.is_empty() && !matches!(entries.last(), Some(MenuEntry::Separator)) {
        entries.push(MenuEntry::Separator);
    }
}

/// `text` on one line, cut to `max` characters with an ellipsis.
fn elide(text: &str, max: usize) -> String {
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let mut cut: String = one_line.chars().take(max).collect();
        cut.push('\u{2026}');
        cut
    }
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
    profiles: &[ProfileEntry],
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
    let selection = window.selection();
    let selected = selection.len();

    vec![
        MenuEntry::item(
            plural("Close tab", "Close {n} tabs"),
            true,
            Command::CloseTabs(targets.to_vec()),
        ),
        MenuEntry::item("Close other tabs", has_rest, Command::CloseTabs(rest)),
        MenuEntry::item(
            format!("Close all selected ({selected})"),
            selected > 1,
            Command::CloseTabs(selection.clone()),
        ),
        MenuEntry::Separator,
        MenuEntry::item(
            plural("Move tab to folder…", "Move {n} tabs to folder…"),
            true,
            Command::PromptMoveToFolder(targets.to_vec()),
        ),
        MenuEntry::item(
            format!("Bookmark all selected ({selected})…"),
            true,
            Command::PromptBookmarkTabs(selection),
        ),
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
                .map(|profile| {
                    MenuEntry::item(
                        profile.name.clone(),
                        true,
                        Command::OpenTabsInProfile {
                            tabs: targets.to_vec(),
                            profile: profile.dir.clone(),
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

    /// Another profile, in directory `work`, called "Work Stuff".
    fn work_profile() -> ProfileEntry {
        ProfileEntry {
            dir: "work".to_string(),
            name: "Work Stuff".to_string(),
            avatar: osmena_core::Avatar::Whale,
            color: osmena_core::ChromeColor::Plum,
        }
    }

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
                | MenuEntry::Check { label, .. }
                | MenuEntry::Disabled { label, .. }
                | MenuEntry::System { label, .. } => label.clone(),
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
                MenuEntry::Disabled { .. } => false,
                MenuEntry::System { .. } | MenuEntry::Separator => true,
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

    /// A browser that can update and has nothing under way.
    fn idle_updates() -> UpdateMenu<'static> {
        UpdateMenu {
            brand: "Test",
            status: &UpdateStatus::Idle,
            can_update: true,
        }
    }

    #[test]
    fn window_menu_carries_the_keymap_chords_and_commands() {
        let w = window_with(1);
        let keymap = Keymap::defaults(Platform::Windows);
        let entries = window_menu(&w, &keymap, true, no_bookmarks(), idle_updates());
        assert_eq!(
            labels(&entries),
            vec![
                "New tab",
                "New window",
                "Reopen closed tab",
                "-",
                "Open homepage",
                "Bookmarks",
                "Full screen",
                "-",
                "Check for updates…",
                "Settings",
                "-",
                "Quit"
            ]
        );
        // The Bookmarks submenu is always there: it holds the actions.
        assert_eq!(
            enabled(&entries),
            vec![
                true, true, true, true, true, true, true, true, true, true, true, true
            ]
        );
        assert!(matches!(
            &entries[8],
            MenuEntry::Item {
                command: Command::CheckForUpdates,
                ..
            }
        ));
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
            &entries[6],
            MenuEntry::Item {
                command: Command::ToggleFullscreen(WindowId(1)),
                ..
            }
        ));
        assert!(matches!(
            &entries[11],
            MenuEntry::Item {
                command: Command::Quit,
                ..
            }
        ));
    }

    #[test]
    fn reopen_is_disabled_with_nothing_to_reopen() {
        let w = window_with(1);
        let entries = window_menu(
            &w,
            &Keymap::defaults(Platform::Windows),
            false,
            no_bookmarks(),
            idle_updates(),
        );
        assert!(!enabled(&entries)[2]);
    }

    fn no_bookmarks() -> BookmarksMenu<'static> {
        static NONE: Bookmarks = Bookmarks { groups: Vec::new() };
        BookmarksMenu {
            bookmarks: &NONE,
            bar_shown: false,
        }
    }

    fn is_enabled(entries: &[MenuEntry], label: &str) -> bool {
        entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Disabled { label: l, .. } if l == label => Some(false),
                MenuEntry::Item {
                    label: l, enabled, ..
                }
                | MenuEntry::Check {
                    label: l, enabled, ..
                }
                | MenuEntry::Submenu {
                    label: l, enabled, ..
                } if l == label => Some(*enabled),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no entry {label:?}"))
    }

    #[test]
    fn the_bookmarks_submenu_holds_the_actions_and_the_bar_switch() {
        let w = window_with(2);
        let keymap = Keymap::defaults(Platform::Windows);
        let entries = window_menu(&w, &keymap, true, no_bookmarks(), idle_updates());
        let MenuEntry::Submenu { entries: menu, .. } = &entries[5] else {
            panic!("bookmarks is a submenu");
        };
        assert_eq!(
            labels(menu),
            vec![
                "Bookmark this tab…",
                "Bookmark all tabs…",
                "Show bookmarks bar",
                "-",
                "Import bookmarks…",
                "Export bookmarks…",
                "-",
                "No bookmarks"
            ]
        );
        assert_eq!(
            enabled(menu),
            vec![true, true, true, true, true, false, true, false]
        );
        assert!(matches!(
            &menu[0],
            MenuEntry::Item {
                command: Command::ToggleBookmark(TabId(1)),
                chord: Some(_),
                ..
            }
        ));
        assert!(matches!(
            &menu[1],
            MenuEntry::Item { command: Command::PromptBookmarkTabs(tabs), .. }
                if *tabs == vec![TabId(1), TabId(2)]
        ));
        assert!(matches!(
            &menu[2],
            MenuEntry::Check {
                checked: false,
                command: Command::ToggleBookmarksBar,
                ..
            }
        ));
        assert!(matches!(
            &menu[4],
            MenuEntry::Item {
                command: Command::PickBookmarksFile(BookmarksFile::Import),
                ..
            }
        ));
        // A blank page cannot be bookmarked.
        let mut blank = window_with(1);
        blank.tabs[0].url = Url::parse("about:blank").unwrap();
        let entries = window_menu(&blank, &keymap, true, no_bookmarks(), idle_updates());
        let MenuEntry::Submenu { entries: menu, .. } = &entries[5] else {
            panic!("bookmarks is a submenu");
        };
        assert_eq!(&enabled(menu)[..2], &[false, false]);
    }

    #[test]
    fn a_chip_menu_and_a_group_menu() {
        let url = Url::parse("https://a.example/").unwrap();
        let entries = bookmark_menu(WindowId(1), &url);
        assert_eq!(
            labels(&entries),
            vec![
                "Open in new tab",
                "Open in new window",
                "Copy link address",
                "-",
                "Edit…",
                "Delete"
            ]
        );
        assert!(matches!(
            &entries[4],
            MenuEntry::Item { command: Command::PromptEditBookmark { window: WindowId(1), url: u }, .. }
                if *u == url
        ));
        assert!(matches!(
            &entries[5],
            MenuEntry::Item { command: Command::RemoveBookmark(u), .. } if *u == url
        ));
        let group = BookmarkGroup {
            name: "Work".to_string(),
            bookmarks: vec![Bookmark {
                title: String::new(),
                url: url.clone(),
            }],
        };
        let entries = bookmark_group_menu(WindowId(1), &group);
        assert_eq!(
            labels(&entries),
            vec![
                "Open all (1)",
                "-",
                "https://a.example/",
                "-",
                "Rename group…",
                "Delete group"
            ]
        );
        assert!(matches!(
            &entries[4],
            MenuEntry::Item { command: Command::PromptRenameBookmarkGroup { group, .. }, .. }
                if group == "Work"
        ));
        let rest = bookmark_overflow_menu(TabId(3), &group.bookmarks);
        assert!(matches!(
            &rest[0],
            MenuEntry::Item {
                command: Command::Navigate { tab: TabId(3), .. },
                ..
            }
        ));
    }

    fn sample_update() -> osmena_core::AvailableUpdate {
        osmena_core::AvailableUpdate {
            id: osmena_core::BuildId::new("0.1.0", 150),
            notes_url: None,
            asset: osmena_core::Asset {
                name: "setup.exe".to_string(),
                url: "https://updates.example/setup.exe".to_string(),
                sha256: "0".repeat(64),
                size: 200,
            },
        }
    }

    #[test]
    fn the_update_entry_follows_the_status() {
        let entry = |status: &UpdateStatus, can_update: bool| {
            UpdateMenu {
                brand: "Test",
                status,
                can_update,
            }
            .entry()
        };
        // A local build: the entry is there, disabled.
        assert_eq!(
            entry(&UpdateStatus::Idle, false),
            MenuEntry::Disabled {
                label: "Check for updates…".to_string(),
                chord: None,
            }
        );
        for status in [
            UpdateStatus::Idle,
            UpdateStatus::UpToDate,
            UpdateStatus::Failed("x".to_string()),
        ] {
            assert_eq!(
                entry(&status, true),
                MenuEntry::item("Check for updates…", true, Command::CheckForUpdates)
            );
        }
        assert_eq!(
            entry(&UpdateStatus::Checking, true),
            MenuEntry::Disabled {
                label: "Checking for updates…".to_string(),
                chord: None,
            }
        );
        assert_eq!(
            entry(
                &UpdateStatus::Downloading {
                    update: sample_update(),
                    received: 100,
                },
                true
            ),
            MenuEntry::Disabled {
                label: "Downloading Test 0.1.0 (build 150)… 50%".to_string(),
                chord: None,
            }
        );
        assert_eq!(
            entry(
                &UpdateStatus::Ready {
                    update: sample_update(),
                    file: std::path::PathBuf::from("setup.exe"),
                },
                true
            ),
            MenuEntry::item("Restart to update Test", true, Command::InstallUpdate)
        );
        assert!(matches!(
            entry(&UpdateStatus::Installing(sample_update()), true),
            MenuEntry::Disabled { .. }
        ));
    }

    #[test]
    fn bookmark_groups_become_submenus_with_open_all_and_a_tab_per_page() {
        let w = window_with(1);
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(
            "Work",
            vec![
                osmena_core::Bookmark {
                    title: "A".to_string(),
                    url: Url::parse("https://a.example/").unwrap(),
                },
                osmena_core::Bookmark {
                    title: String::new(),
                    url: Url::parse("https://b.example/").unwrap(),
                },
            ],
        );
        let entries = window_menu(
            &w,
            &Keymap::defaults(Platform::Windows),
            true,
            BookmarksMenu {
                bookmarks: &bookmarks,
                bar_shown: true,
            },
            idle_updates(),
        );
        let MenuEntry::Submenu {
            entries: menu,
            enabled: true,
            ..
        } = &entries[5]
        else {
            panic!("bookmarks is an enabled submenu");
        };
        assert!(matches!(&menu[2], MenuEntry::Check { checked: true, .. }));
        assert!(is_enabled(menu, "Export bookmarks…"));
        let groups = &menu[7..];
        assert_eq!(labels(groups), vec!["Work"]);
        let MenuEntry::Submenu { entries: pages, .. } = &groups[0] else {
            panic!("a group is a submenu");
        };
        assert_eq!(
            labels(pages),
            vec![
                "Open all (2)",
                "-",
                "A",
                "https://b.example/",
                "-",
                "Rename group…",
                "Delete group"
            ]
        );
        assert!(matches!(
            &pages[0],
            MenuEntry::Item { command: Command::OpenBookmarkGroup { window: WindowId(1), group }, .. }
                if group == "Work"
        ));
        assert!(matches!(
            &pages[3],
            MenuEntry::Item { command: Command::NewTab { window: WindowId(1), url: Some(u) }, .. }
                if u.as_str() == "https://b.example/"
        ));
    }

    #[test]
    fn folder_menu_toggles_expansion_and_closes_the_folder() {
        let mut w = window_with(3);
        w.folders.push(osmena_core::Folder {
            id: FolderId(4),
            name: "F".to_string(),
            expanded: false,
        });
        w.tabs[0].folder = Some(FolderId(4));
        w.tabs[1].folder = Some(FolderId(4));
        let entries = folder_menu(&w, FolderId(4));
        assert_eq!(
            labels(&entries),
            vec!["Expand", "Rename…", "-", "Ungroup", "Close folder (2 tabs)"]
        );
        assert!(matches!(
            &entries[0],
            MenuEntry::Item {
                command: Command::SetFolderExpanded {
                    folder: FolderId(4),
                    expanded: true
                },
                ..
            }
        ));
        assert!(matches!(
            &entries[4],
            MenuEntry::Item {
                command: Command::CloseFolder(FolderId(4)),
                ..
            }
        ));
        w.folders[0].expanded = true;
        assert_eq!(labels(&folder_menu(&w, FolderId(4)))[0], "Collapse");
        assert!(folder_menu(&w, FolderId(9)).is_empty());
    }

    #[test]
    fn tab_menu_for_one_tab_of_three() {
        let w = window_with(3);
        let entries = tab_menu(
            &w,
            &[TabId(2)],
            &[(WindowId(7), "Other".to_string())],
            &[work_profile()],
        );
        assert_eq!(
            labels(&entries),
            vec![
                "Close tab",
                "Close other tabs",
                "Close all selected (1)",
                "-",
                "Move tab to folder…",
                "Bookmark all selected (1)…",
                "-",
                "Move tab to new window",
                "Move tab to window",
                "Open tab in profile",
            ]
        );
        // One tab selected: "close all selected" has nothing more to do.
        assert_eq!(
            enabled(&entries),
            vec![true, true, false, true, true, true, true, true, true, true]
        );
        assert!(matches!(
            &entries[1],
            MenuEntry::Item { command: Command::CloseTabs(rest), .. } if *rest == vec![TabId(1), TabId(3)]
        ));
        assert!(matches!(
            &entries[4],
            MenuEntry::Item { command: Command::PromptMoveToFolder(tabs), .. } if *tabs == vec![TabId(2)]
        ));
        assert!(matches!(
            &entries[5],
            MenuEntry::Item { command: Command::PromptBookmarkTabs(tabs), .. } if *tabs == vec![TabId(1)]
        ));
        let MenuEntry::Submenu {
            entries: windows, ..
        } = &entries[8]
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
        } = &entries[9]
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
        let mut w = window_with(3);
        w.selected = vec![TabId(1), TabId(2), TabId(3)];
        let all = [TabId(1), TabId(2), TabId(3)];
        let entries = tab_menu(&w, &all, &[], &[]);
        assert_eq!(labels(&entries)[0], "Close 3 tabs");
        assert_eq!(labels(&entries)[2], "Close all selected (3)");
        assert_eq!(labels(&entries)[4], "Move 3 tabs to folder…");
        assert_eq!(labels(&entries)[5], "Bookmark all selected (3)…");
        assert_eq!(labels(&entries)[7], "Move 3 tabs to new window");
        // Close others, move to new window: nothing left; no other windows
        // or profiles: the submenus are disabled.
        assert_eq!(
            enabled(&entries),
            vec![
                true, false, true, true, true, true, true, false, false, false
            ]
        );
        assert!(matches!(
            &entries[2],
            MenuEntry::Item { command: Command::CloseTabs(tabs), .. } if *tabs == all.to_vec()
        ));
    }
}

#[cfg(test)]
mod page_menu_tests {
    use super::*;
    use osmena_core::{EditState, Media, Misspelling, Tab};
    use url::Url;

    /// A browser that can update and has nothing under way.
    fn idle_updates() -> UpdateMenu<'static> {
        UpdateMenu {
            brand: "Test",
            status: &UpdateStatus::Idle,
            can_update: true,
        }
    }

    fn window() -> Window {
        Window::with_tab(
            WindowId(1),
            Tab::new(TabId(1), Url::parse("https://page.example/").unwrap()),
        )
    }

    fn labels(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                MenuEntry::Item { label, .. }
                | MenuEntry::Submenu { label, .. }
                | MenuEntry::Check { label, .. }
                | MenuEntry::Disabled { label, .. }
                | MenuEntry::System { label, .. } => label.clone(),
                MenuEntry::Separator => "-".to_string(),
            })
            .collect()
    }

    fn command(entries: &[MenuEntry], label: &str) -> Command {
        entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Item {
                    label: l, command, ..
                }
                | MenuEntry::Check {
                    label: l, command, ..
                } if l == label => Some(command.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no entry {label:?} in {:?}", labels(entries)))
    }

    fn is_enabled(entries: &[MenuEntry], label: &str) -> bool {
        entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Disabled { label: l, .. } if l == label => Some(false),
                MenuEntry::Item {
                    label: l, enabled, ..
                }
                | MenuEntry::Check {
                    label: l, enabled, ..
                }
                | MenuEntry::Submenu {
                    label: l, enabled, ..
                } if l == label => Some(*enabled),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no entry {label:?}"))
    }

    fn chord_of(entries: &[MenuEntry], label: &str) -> Option<KeyChord> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: l, chord, ..
            } if l == label => *chord,
            _ => None,
        })
    }

    fn menu(context: &PageContext, tools: &PageTools, platform: Platform) -> Vec<MenuEntry> {
        page_menu(
            &window(),
            TabId(1),
            context,
            tools,
            &Keymap::defaults(platform),
            platform,
            "Test",
        )
    }

    #[test]
    fn a_plain_page_gets_history_save_print_source_and_inspect() {
        let entries = menu(&PageContext::default(), &PageTools::ALL, Platform::Windows);
        assert_eq!(
            labels(&entries),
            [
                "Back",
                "Forward",
                "Reload",
                "-",
                "Save as\u{2026}",
                "Print\u{2026}",
                "-",
                "View page source",
                "-",
                "Inspect"
            ]
        );
        assert!(!is_enabled(&entries, "Back"), "nothing to go back to");
        assert_eq!(command(&entries, "Reload"), Command::Reload(TabId(1)));
        assert_eq!(
            chord_of(&entries, "Reload"),
            Keymap::defaults(Platform::Windows).chord_for(CommandKind::Reload)
        );
        assert_eq!(
            command(&entries, "Save as\u{2026}"),
            Command::Page {
                tab: TabId(1),
                command: PageCommand::Save(Url::parse("https://page.example/").unwrap()),
            }
        );
        assert_eq!(
            command(&entries, "Inspect"),
            Command::Page {
                tab: TabId(1),
                command: PageCommand::Inspect,
            }
        );
    }

    #[test]
    fn an_engine_without_tools_keeps_only_history_on_a_plain_page() {
        let entries = menu(&PageContext::default(), &PageTools::NONE, Platform::Windows);
        assert_eq!(labels(&entries), ["Back", "Forward", "Reload"]);
        assert!(
            page_menu(
                &window(),
                TabId(9),
                &PageContext::default(),
                &PageTools::ALL,
                &Keymap::defaults(Platform::Windows),
                Platform::Windows,
                "Test",
            )
            .is_empty(),
            "a tab not in the window has no menu"
        );
    }

    #[test]
    fn a_link_opens_in_a_background_tab_or_a_window_and_copies_its_address() {
        let link = Url::parse("https://link.example/a?b=c").unwrap();
        let context = PageContext {
            link: Some(link.clone()),
            ..PageContext::default()
        };
        let entries = menu(&context, &PageTools::ALL, Platform::Windows);
        assert_eq!(
            labels(&entries),
            [
                "Open link in new tab",
                "Open link in new window",
                "-",
                "Save link as\u{2026}",
                "Copy link address",
                "-",
                "Inspect"
            ]
        );
        assert_eq!(
            command(&entries, "Open link in new tab"),
            Command::NewBackgroundTab {
                opener: TabId(1),
                url: link.clone(),
            }
        );
        assert_eq!(
            command(&entries, "Open link in new window"),
            Command::OpenInNewWindow(link.clone())
        );
        assert_eq!(
            command(&entries, "Copy link address"),
            Command::CopyText(link.to_string())
        );
        let bare = menu(&context, &PageTools::NONE, Platform::Windows);
        assert_eq!(
            labels(&bare),
            [
                "Open link in new tab",
                "Open link in new window",
                "-",
                "Copy link address"
            ]
        );
    }

    #[test]
    fn an_image_in_a_link_lists_both_and_a_video_has_its_toggles() {
        let image = Url::parse("https://img.example/p.png").unwrap();
        let context = PageContext {
            link: Some(Url::parse("https://link.example/").unwrap()),
            media: Some(Media {
                kind: MediaKind::Image,
                src: image.clone(),
                looping: None,
                controls: None,
            }),
            ..PageContext::default()
        };
        let entries = menu(&context, &PageTools::ALL, Platform::Windows);
        assert_eq!(
            labels(&entries)[5..],
            [
                "-",
                "Open image in new tab",
                "Save image as\u{2026}",
                "Copy image",
                "Copy image address",
                "-",
                "Inspect"
            ]
        );
        assert_eq!(
            command(&entries, "Copy image"),
            Command::Page {
                tab: TabId(1),
                command: PageCommand::CopyImage(image),
            }
        );

        let video = Url::parse("https://v.example/clip.mp4").unwrap();
        let context = PageContext {
            media: Some(Media {
                kind: MediaKind::Video,
                src: video.clone(),
                looping: Some(false),
                controls: Some(true),
            }),
            ..PageContext::default()
        };
        let entries = menu(&context, &PageTools::ALL, Platform::Windows);
        assert_eq!(
            labels(&entries),
            [
                "Loop",
                "Show controls",
                "-",
                "Save video as\u{2026}",
                "Copy video address",
                "Open video in new tab",
                "-",
                "Inspect"
            ]
        );
        assert!(matches!(
            &entries[0],
            MenuEntry::Check {
                checked: false,
                command: Command::Page {
                    command: PageCommand::SetMediaLoop(true),
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            &entries[1],
            MenuEntry::Check {
                checked: true,
                command: Command::Page {
                    command: PageCommand::SetMediaControls(false),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn selected_text_can_be_copied_searched_and_printed() {
        let context = PageContext {
            selection: Some(
                "  hello   there, this is a rather long piece of selected text ".to_string(),
            ),
            ..PageContext::default()
        };
        let entries = menu(&context, &PageTools::ALL, Platform::MacOs);
        assert_eq!(
            labels(&entries),
            [
                "Copy",
                "Search Test for \u{201c}hello there, this is a rather lo\u{2026}\u{201d}",
                "Print\u{2026}",
                "-",
                "Inspect"
            ]
        );
        assert_eq!(
            chord_of(&entries, "Copy"),
            Some(KeyChord {
                modifiers: Modifiers::META,
                code: Code::KeyC
            })
        );
        assert_eq!(
            command(&entries, &labels(&entries)[1]),
            Command::SearchInNewTab {
                window: WindowId(1),
                query: "hello   there, this is a rather long piece of selected text".to_string(),
            }
        );
        // A selection the engine cannot read still copies, but is not searched.
        let unknown = PageContext {
            selection: Some(String::new()),
            ..PageContext::default()
        };
        let entries = menu(&unknown, &PageTools::NONE, Platform::Windows);
        assert_eq!(labels(&entries), ["Copy"]);
    }

    #[test]
    fn an_editable_field_gets_the_editing_entries_with_their_chords() {
        let context = PageContext {
            editable: Some(EditState {
                can_undo: true,
                can_redo: false,
                can_cut: false,
                can_copy: false,
                can_paste: true,
                can_delete: false,
                can_select_all: true,
            }),
            ..PageContext::default()
        };
        let entries = menu(&context, &PageTools::ALL, Platform::Windows);
        assert_eq!(
            labels(&entries),
            [
                "Undo",
                "Redo",
                "-",
                "Cut",
                "Copy",
                "Paste",
                "Paste as plain text",
                "Delete",
                "-",
                "Select all",
                "-",
                "Inspect"
            ]
        );
        assert!(is_enabled(&entries, "Undo"));
        assert!(!is_enabled(&entries, "Redo"));
        assert!(!is_enabled(&entries, "Cut"));
        assert!(is_enabled(&entries, "Paste as plain text"));
        assert_eq!(
            chord_of(&entries, "Redo"),
            Some(KeyChord {
                modifiers: Modifiers::CONTROL,
                code: Code::KeyY
            })
        );
        assert_eq!(
            chord_of(&entries, "Paste as plain text"),
            Some(KeyChord {
                modifiers: Modifiers::CONTROL | Modifiers::SHIFT,
                code: Code::KeyV
            })
        );
        assert_eq!(chord_of(&entries, "Delete"), None);
        assert_eq!(
            command(&entries, "Select all"),
            Command::Page {
                tab: TabId(1),
                command: PageCommand::SelectAll,
            }
        );
        let mac = menu(&context, &PageTools::NONE, Platform::MacOs);
        assert_eq!(
            chord_of(&mac, "Redo"),
            Some(KeyChord {
                modifiers: Modifiers::META | Modifiers::SHIFT,
                code: Code::KeyZ
            })
        );
        assert_eq!(
            labels(&mac),
            [
                "Undo",
                "Redo",
                "-",
                "Cut",
                "Copy",
                "Paste",
                "Delete",
                "-",
                "Select all"
            ]
        );
    }

    #[test]
    fn a_misspelling_and_a_selection_in_a_field_add_their_entries() {
        let context = PageContext {
            editable: Some(EditState::ALL),
            selection: Some("teh".to_string()),
            misspelling: Some(Misspelling {
                word: "teh".to_string(),
                suggestions: vec!["the".to_string(), "tech".to_string()],
            }),
            ..PageContext::default()
        };
        let entries = menu(&context, &PageTools::NONE, Platform::Windows);
        assert_eq!(
            labels(&entries),
            [
                "the",
                "tech",
                "Add to dictionary",
                "-",
                "Undo",
                "Redo",
                "-",
                "Cut",
                "Copy",
                "Paste",
                "Delete",
                "-",
                "Select all",
                "-",
                "Search Test for \u{201c}teh\u{201d}"
            ]
        );
        assert_eq!(
            command(&entries, "the"),
            Command::Page {
                tab: TabId(1),
                command: PageCommand::ReplaceMisspelling("the".to_string()),
            }
        );
        assert_eq!(
            command(&entries, "Add to dictionary"),
            Command::Page {
                tab: TabId(1),
                command: PageCommand::AddToDictionary("teh".to_string()),
            }
        );
        let none = PageContext {
            misspelling: Some(Misspelling {
                word: "zzz".to_string(),
                suggestions: Vec::new(),
            }),
            ..context
        };
        let entries = menu(&none, &PageTools::NONE, Platform::Windows);
        assert_eq!(labels(&entries)[0], "No spelling suggestions");
        assert!(!is_enabled(&entries, "No spelling suggestions"));
    }

    fn submenu<'a>(entries: &'a [MenuEntry], name: &str) -> &'a [MenuEntry] {
        entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Submenu { label, entries, .. } if label == name => Some(&entries[..]),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no menu {name:?} in {:?}", labels(entries)))
    }

    fn chord(entries: &[MenuEntry], label: &str) -> Option<KeyChord> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: l, chord, ..
            }
            | MenuEntry::Disabled { label: l, chord }
                if l == label =>
            {
                *chord
            }
            _ => None,
        })
    }

    /// Another profile, in directory `work`, called "Work Stuff".
    fn work_profile() -> ProfileEntry {
        ProfileEntry {
            dir: "work".to_string(),
            name: "Work Stuff".to_string(),
            avatar: osmena_core::Avatar::Whale,
            color: osmena_core::ChromeColor::Plum,
        }
    }

    fn menu_bar_input<'a>(
        window: Option<&'a Window>,
        keymap: &'a Keymap,
        history: &'a History,
        bookmarks: &'a Bookmarks,
        profiles: &'a [ProfileEntry],
    ) -> MenuBarInput<'a> {
        MenuBarInput {
            brand: "Test",
            window,
            keymap,
            can_reopen: false,
            bookmarks: BookmarksMenu {
                bookmarks,
                bar_shown: false,
            },
            history,
            profile: CurrentProfile {
                dir: "default",
                name: "Default",
            },
            profiles,
            updates: idle_updates(),
        }
    }

    #[test]
    fn the_menu_bar_is_the_application_menu_then_the_eight_menus() {
        let mut w = Window::with_tab(
            WindowId(1),
            Tab::new(TabId(1), Url::parse("https://a.example/").unwrap()),
        );
        w.tabs.push(Tab::new(
            TabId(2),
            Url::parse("https://b.example/").unwrap(),
        ));
        let keymap = Keymap::defaults(Platform::MacOs);
        let mut history = History::default();
        let old = Url::parse("https://old.example/").unwrap();
        let new = Url::parse("https://new.example/").unwrap();
        history.record(&old, "Old", 10);
        history.record(&new, "New", 20);
        let bookmarks = Bookmarks::default();
        let profiles = vec![work_profile()];
        let entries = menu_bar(&menu_bar_input(
            Some(&w),
            &keymap,
            &history,
            &bookmarks,
            &profiles,
        ));
        assert_eq!(
            labels(&entries),
            [
                "Test",
                "File",
                "Edit",
                "View",
                "History",
                "Bookmarks",
                "Profiles",
                "Tab",
                WINDOW_MENU_LABEL
            ]
        );

        // The application menu: the system's items around ours.
        let app = submenu(&entries, "Test");
        assert!(matches!(
            app[0],
            MenuEntry::System {
                item: SystemItem::About,
                ..
            }
        ));
        assert_eq!(labels(app)[0], "About Test");
        assert_eq!(labels(app)[1], "Check for updates…");
        assert_eq!(command(app, "Check for updates…"), Command::CheckForUpdates);
        assert_eq!(command(app, "Settings…"), Command::OpenSettings);
        assert_eq!(command(app, "Quit Test"), Command::Quit);
        assert_eq!(chord(app, "Quit Test"), keymap.chord_for(CommandKind::Quit));

        // File acts on the window with the keymap's chords.
        let file = submenu(&entries, "File");
        assert_eq!(
            command(file, "New Tab"),
            Command::NewTab {
                window: w.id,
                url: None,
            }
        );
        assert_eq!(
            chord(file, "New Tab"),
            keymap.chord_for(CommandKind::NewTab)
        );
        assert_eq!(command(file, "Close Tab"), Command::CloseTab(TabId(1)));
        assert!(!is_enabled(file, "Reopen Closed Tab"));

        // Edit is the page's, with the standard chords.
        let edit = submenu(&entries, "Edit");
        assert_eq!(
            command(edit, "Copy"),
            Command::Page {
                tab: TabId(1),
                command: PageCommand::Copy,
            }
        );
        assert_eq!(
            chord(edit, "Copy"),
            Some(KeyChord::new(Modifiers::META, Code::KeyC))
        );
        assert_eq!(
            chord(edit, "Redo"),
            Some(KeyChord::new(
                Modifiers::META | Modifiers::SHIFT,
                Code::KeyZ
            ))
        );

        // View knows the tab is not loading.
        let view = submenu(&entries, "View");
        assert!(!is_enabled(view, "Stop Loading"));
        assert!(is_enabled(view, "Reload Page"));

        // History: nothing to go back to, the recent pages newest first.
        let history_menu = submenu(&entries, "History");
        assert!(!is_enabled(history_menu, "Back"));
        let names = labels(history_menu);
        assert_eq!(&names[names.len() - 2..], ["New", "Old"]);
        assert_eq!(
            command(history_menu, "New"),
            Command::Navigate {
                tab: TabId(1),
                url: new,
            }
        );

        // Bookmarks with none, Profiles with the current one checked.
        let bookmarks_menu = submenu(&entries, "Bookmarks");
        assert_eq!(
            command(bookmarks_menu, "Bookmark This Tab…"),
            Command::ToggleBookmark(TabId(1))
        );
        assert!(!is_enabled(bookmarks_menu, "No Bookmarks"));
        assert!(is_enabled(bookmarks_menu, "Import Bookmarks…"));
        let profiles_menu = submenu(&entries, "Profiles");
        assert!(!is_enabled(profiles_menu, "Default"));
        assert_eq!(
            command(profiles_menu, "Work Stuff"),
            Command::OpenProfile("work".to_string())
        );
        assert_eq!(
            command(profiles_menu, "Edit…"),
            Command::PromptEditProfile(w.id)
        );

        // Tab: two tabs, so moving one out is possible.
        let tab = submenu(&entries, "Tab");
        assert_eq!(
            command(tab, "Move Tab to New Window"),
            Command::MoveTabsToNewWindow(vec![TabId(1)])
        );
        assert!(is_enabled(tab, "Show Next Tab"));

        // Window is the system's.
        let window_menu = submenu(&entries, WINDOW_MENU_LABEL);
        assert!(matches!(
            window_menu[0],
            MenuEntry::System {
                item: SystemItem::Minimize,
                ..
            }
        ));
    }

    #[test]
    fn without_a_window_the_menu_bar_keeps_only_what_needs_none() {
        let keymap = Keymap::defaults(Platform::MacOs);
        let history = History::default();
        let bookmarks = Bookmarks::default();
        let entries = menu_bar(&menu_bar_input(None, &keymap, &history, &bookmarks, &[]));
        let file = submenu(&entries, "File");
        assert!(!is_enabled(file, "New Tab"));
        assert!(is_enabled(file, "New Window"));
        // A disabled entry still shows its shortcut.
        assert_eq!(
            chord(file, "New Tab"),
            keymap.chord_for(CommandKind::NewTab)
        );
        assert!(!is_enabled(submenu(&entries, "Edit"), "Copy"));
        assert!(is_enabled(submenu(&entries, "Test"), "Quit Test"));
        let profiles = submenu(&entries, "Profiles");
        assert!(!is_enabled(profiles, "No Other Profiles"));
        assert!(!is_enabled(profiles, "Edit…"));
        assert!(!is_enabled(profiles, "Add Profile…"));
        assert!(!is_enabled(profiles, "Delete Profile"));
    }

    #[test]
    fn the_profiles_menu_lists_the_current_the_others_and_the_actions() {
        let window = WindowId(3);
        let others = vec![
            ProfileEntry {
                dir: "default".to_string(),
                name: "Default".to_string(),
                avatar: osmena_core::Avatar::Initial,
                color: osmena_core::ChromeColor::Shell,
            },
            work_profile(),
        ];
        let current = CurrentProfile {
            dir: "play",
            name: "Play",
        };
        let entries = profiles_menu(Some(window), current, &others);
        assert_eq!(
            labels(&entries),
            [
                "Play",
                "Edit…",
                "-",
                "Default",
                "Work Stuff",
                "-",
                "Add Profile…",
                "Delete Profile"
            ]
        );
        assert!(matches!(
            &entries[0],
            MenuEntry::Check {
                enabled: false,
                checked: true,
                ..
            }
        ));
        assert_eq!(
            command(&entries, "Edit…"),
            Command::PromptEditProfile(window)
        );
        assert_eq!(
            command(&entries, "Work Stuff"),
            Command::OpenProfile("work".to_string())
        );
        assert_eq!(
            command(&entries, "Add Profile…"),
            Command::PromptAddProfile(window)
        );
        // Deletable: the others but the default profile, then the
        // current one.
        let delete = submenu(&entries, "Delete Profile");
        assert_eq!(labels(delete), ["Work Stuff", "Play"]);
        assert_eq!(
            command(delete, "Play"),
            Command::PromptDeleteProfile {
                window,
                profile: "play".to_string(),
            }
        );

        // The default profile alone: nothing to delete, nothing to open.
        let alone = profiles_menu(
            Some(window),
            CurrentProfile {
                dir: "default",
                name: "Default",
            },
            &[],
        );
        assert!(!is_enabled(&alone, "No Other Profiles"));
        assert!(!is_enabled(&alone, "Delete Profile"));
        assert!(is_enabled(&alone, "Add Profile…"));
    }

    #[test]
    fn elide_joins_lines_and_cuts_long_text() {
        assert_eq!(elide(" a \n b ", 10), "a b");
        assert_eq!(elide("abcdefghij", 10), "abcdefghij");
        assert_eq!(elide("abcdefghijk", 10), "abcdefghij\u{2026}");
    }
}
