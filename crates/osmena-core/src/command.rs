//! Named user actions and the pure `dispatch` that applies them.

use url::Url;

use crate::bookmarks::Bookmark;
use crate::event::PendingTab;
use crate::extension::ExtensionSource;
use crate::model::{Browser, Folder, FolderId, LoadState, Tab, TabId, Window, WindowId};

/// Every user action in the browser is one of these. The chrome, the keymap,
/// and (in Phase 2) remappable hotkeys all produce `Command`s; nothing else
/// mutates the model from the UI side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    NewTab {
        window: WindowId,
        url: Option<Url>,
    },
    CloseTab(TabId),
    /// Make `tab` active and the only selected tab (a plain click).
    ActivateTab(TabId),
    /// Add `tab` to its window's selection or remove it (Ctrl-click). The
    /// active tab is always selected, so toggling it does nothing.
    ToggleTabSelection(TabId),
    /// Select every tab between the active tab and `tab`, inclusive, and
    /// nothing else (Shift-click). The active tab stays active.
    SelectTabRange(TabId),
    /// Close each of these tabs (the context menu's "Close tabs").
    CloseTabs(Vec<TabId>),
    /// Move these tabs to a new window, in strip order. An engine's page
    /// cannot leave the native window it was created for, so each
    /// tab is recreated at its URL in the new window and closed here: the
    /// page reloads and its history and form state are lost.
    MoveTabsToNewWindow(Vec<TabId>),
    /// Move these tabs to the end of `window`'s strip, the same way.
    MoveTabsToWindow {
        tabs: Vec<TabId>,
        window: WindowId,
    },
    /// Reorder: put these tabs, together and in strip order, at `index`
    /// among their window's other tabs (0 is first; the count of the
    /// other tabs is last). Tabs dropped between two tabs of one folder
    /// join it; dropped anywhere else they leave the folder they were
    /// in. Nothing changes hands with the engine.
    MoveTabs {
        tabs: Vec<TabId>,
        index: usize,
    },
    /// Open these tabs' URLs in a browser process for `profile` (another
    /// profile is another process: an engine's data directory belongs
    /// to one browser process at a time) and close
    /// them here.
    OpenTabsInProfile {
        tabs: Vec<TabId>,
        profile: String,
    },
    /// Put these tabs in a folder of their window (a new one, or one
    /// of its existing ones), together where the first of them is.
    MoveTabsToFolder {
        tabs: Vec<TabId>,
        folder: FolderTarget,
    },
    SetFolderExpanded {
        folder: FolderId,
        expanded: bool,
    },
    RenameFolder {
        folder: FolderId,
        name: String,
    },
    /// Dissolve the folder; its tabs stay where they are.
    Ungroup(FolderId),
    /// Close every tab of the folder.
    CloseFolder(FolderId),
    /// Add these tabs' pages to the bookmark group called `group`
    /// (made if new).
    BookmarkTabs {
        tabs: Vec<TabId>,
        group: String,
    },
    /// Open every page of the bookmark group in new tabs of `window`.
    OpenBookmarkGroup {
        window: WindowId,
        group: String,
    },
    /// Ask the user for the folder to move these tabs to.
    PromptMoveToFolder(Vec<TabId>),
    /// Ask the user for the group to bookmark these tabs in.
    PromptBookmarkTabs(Vec<TabId>),
    PromptRenameFolder(FolderId),
    NextTab(WindowId),
    PreviousTab(WindowId),
    /// Activate the tab at `index` in `window`'s strip; nothing if there
    /// is no such tab.
    ActivateTabAt {
        window: WindowId,
        index: usize,
    },
    ActivateLastTab(WindowId),
    /// Open the most recently closed tab (its URL) as a new tab of
    /// `window`; nothing if no tab was closed.
    ReopenClosedTab(WindowId),
    Navigate {
        tab: TabId,
        url: Url,
    },
    Back(TabId),
    Forward(TabId),
    Reload(TabId),
    Stop(TabId),
    /// Load the homepage in `tab`.
    Home(TabId),
    FocusAddressBar(WindowId),
    NewWindow,
    CloseWindow(WindowId),
    /// Toggle `window` between full screen (no chrome) and normal.
    ToggleFullscreen(WindowId),
    OpenSettings,
    /// Clear the engine error shown in the status area.
    DismissEngineError,
    /// Open the popup of `extension`'s toolbar action over `window`
    /// (the toolbar button or the menu entry was clicked). Nothing if
    /// the extension is unknown, disabled, or has no popup.
    OpenExtensionAction {
        window: WindowId,
        extension: String,
    },
    /// Put `extension`'s action in the toolbar, or take it out.
    PinExtension {
        extension: String,
        pinned: bool,
    },
    SetExtensionEnabled {
        extension: String,
        enabled: bool,
    },
    /// Remove `extension` for good; it is unpinned too.
    UninstallExtension(String),
    InstallExtension(ExtensionSource),
    /// Load the search for `query` (`Settings::search`) in `tab`:
    /// address-bar text that is not a URL.
    Search {
        tab: TabId,
        query: String,
    },
    /// Open the search for `query` in a new tab of `window`.
    SearchInNewTab {
        window: WindowId,
        query: String,
    },
    /// Open `url` in a new tab beside `opener`, in its window, without
    /// leaving the active tab ("Open link in new tab").
    NewBackgroundTab {
        opener: TabId,
        url: Url,
    },
    /// Open `url` in a new window ("Open link in new window").
    OpenInNewWindow(Url),
    /// Put `text` on the clipboard.
    CopyText(String),
    /// Act on the page of `tab` where its context menu was opened.
    Page {
        tab: TabId,
        command: PageCommand,
    },
    /// The context menu of `tab`'s page closed, with or without a choice
    /// (which was dispatched first): the engine may let go of what it
    /// held for it.
    ContextMenuClosed(TabId),
    Quit,
}

/// Something the page's context menu asks of the engine. It acts on the
/// page of the tab where the menu was opened, at the point it was opened
/// (the engine remembers the point).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteAsPlainText,
    Delete,
    SelectAll,
    /// Replace the misspelled word under the pointer with this one.
    ReplaceMisspelling(String),
    /// Add the misspelled word to the dictionary.
    AddToDictionary(String),
    /// Save what `url` names (a link's target, an image, a video, the
    /// page itself) to disk, asking where.
    Save(Url),
    /// Put the image at `url` on the clipboard: the engine fetches it and
    /// reports `EngineEvent::ClipboardImage`.
    CopyImage(Url),
    /// Make the video or audio under the pointer loop, or stop it.
    SetMediaLoop(bool),
    /// Show or hide the controls of the video or audio under the pointer.
    SetMediaControls(bool),
    Print,
    /// Show the page's source.
    ViewSource,
    /// Open the developer tools on the element under the pointer.
    Inspect,
}

/// The folder `Command::MoveTabsToFolder` puts tabs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderTarget {
    /// A new folder called `name`; empty, the window's next suggested
    /// name ("Folder A", ...).
    New {
        name: String,
    },
    Existing(FolderId),
}

/// A question the chrome asks the user, with what it needs to ask it.
/// The answer comes back as a `Command`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// A name for a new folder (prefilled with `suggested`), or one of
    /// the window's `folders` to use instead.
    MoveToFolder {
        tabs: Vec<TabId>,
        suggested: String,
        folders: Vec<(FolderId, String)>,
    },
    /// A name for a new bookmark group, or one of the existing `groups`.
    BookmarkTabs {
        tabs: Vec<TabId>,
        groups: Vec<String>,
    },
    RenameFolder {
        folder: FolderId,
        name: String,
    },
}

/// What the application must do after [`dispatch`] (or [`crate::apply`])
/// has updated the model. Most variants are forwarded to the engine's tab
/// host; `OpenWindow`, `CloseWindow`, `FocusAddressBar`, `SetFullscreen`,
/// `OpenSettings`, and `Quit` are handled by the app itself. Returning
/// these instead of calling the engine directly keeps `dispatch` pure and
/// testable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineAction {
    /// Create and show a new engine tab for `tab` in `window`, loading `url`.
    CreateTab {
        window: WindowId,
        tab: TabId,
        url: Url,
    },
    /// Create an engine tab for `tab` in `window` loading `url` that stays
    /// hidden behind the active tab, which keeps the focus.
    CreateHiddenTab {
        window: WindowId,
        tab: TabId,
        url: Url,
    },
    /// Adopt a tab the engine already created (a `window.open` request) as
    /// `tab` in `window`, and show it, or, `background`, keep it hidden
    /// behind the active tab.
    AdoptTab {
        window: WindowId,
        tab: TabId,
        pending: PendingTab,
        background: bool,
    },
    Navigate {
        tab: TabId,
        url: Url,
    },
    Back(TabId),
    Forward(TabId),
    Reload(TabId),
    Stop(TabId),
    Show(TabId),
    Hide(TabId),
    CloseTab(TabId),
    /// Open a native window for `window` whose first tab is `tab` at `url`.
    OpenWindow {
        window: WindowId,
        tab: TabId,
        url: Url,
    },
    CloseWindow(WindowId),
    FocusAddressBar(WindowId),
    /// Put the native window into or out of full screen; the model's
    /// `Window::fullscreen` already says which.
    SetFullscreen {
        window: WindowId,
        fullscreen: bool,
    },
    OpenSettings,
    /// Start a `<binary> --profile <profile> <urls>` process.
    OpenInProfile {
        profile: String,
        urls: Vec<Url>,
    },
    /// The settings changed (pinned extensions): persist them and redraw.
    SaveSettings,
    /// The bookmarks changed: persist them.
    SaveBookmarks,
    /// The history changed: persist it (the shell may wait a moment and
    /// save once for a run of changes).
    SaveHistory,
    /// Show `prompt` in `window`'s chrome.
    ShowPrompt {
        window: WindowId,
        prompt: Prompt,
    },
    /// Open the popup of `extension`'s toolbar action for `window`'s
    /// active tab; the window knows where its button is.
    OpenExtensionAction {
        window: WindowId,
        extension: String,
    },
    SetExtensionEnabled {
        extension: String,
        enabled: bool,
    },
    UninstallExtension(String),
    InstallExtension(ExtensionSource),
    /// Put `text` on the clipboard.
    CopyText(String),
    /// Act on `tab`'s page (see `PageCommand`).
    Page {
        tab: TabId,
        command: PageCommand,
    },
    /// The context menu of `tab`'s page closed.
    ContextMenuClosed(TabId),
    Quit,
}

/// Apply a command to the model and return what the app must do next.
pub fn dispatch(browser: &mut Browser, cmd: &Command) -> Vec<EngineAction> {
    match cmd {
        Command::NewTab { window, url } => new_tab(browser, *window, url.clone()),
        Command::CloseTab(tab) => {
            remember_closed(browser, &[*tab]);
            close_tab(browser, *tab)
        }
        Command::ActivateTab(tab) => activate_tab(browser, *tab),
        Command::ToggleTabSelection(tab) => {
            toggle_tab_selection(browser, *tab);
            Vec::new()
        }
        Command::SelectTabRange(tab) => {
            select_tab_range(browser, *tab);
            Vec::new()
        }
        Command::CloseTabs(tabs) => {
            remember_closed(browser, tabs);
            close_tabs(browser, tabs)
        }
        Command::MoveTabsToNewWindow(tabs) => move_tabs_to_new_window(browser, tabs),
        Command::MoveTabsToWindow { tabs, window } => move_tabs_to_window(browser, tabs, *window),
        Command::MoveTabs { tabs, index } => {
            move_tabs(browser, tabs, *index);
            Vec::new()
        }
        Command::OpenTabsInProfile { tabs, profile } => {
            open_tabs_in_profile(browser, tabs, profile)
        }
        Command::MoveTabsToFolder { tabs, folder } => {
            move_tabs_to_folder(browser, tabs, folder);
            Vec::new()
        }
        Command::SetFolderExpanded { folder, expanded } => {
            if let Some(f) = folder_mut(browser, *folder) {
                f.expanded = *expanded;
            }
            Vec::new()
        }
        Command::RenameFolder { folder, name } => {
            let name = name.trim();
            if !name.is_empty()
                && let Some(f) = folder_mut(browser, *folder)
            {
                f.name = name.to_string();
            }
            Vec::new()
        }
        Command::Ungroup(folder) => {
            if let Some(window) = browser.window_of_folder(*folder)
                && let Some(w) = browser.window_mut(window)
            {
                for tab in &mut w.tabs {
                    if tab.folder == Some(*folder) {
                        tab.folder = None;
                    }
                }
                w.prune_folders();
            }
            Vec::new()
        }
        Command::CloseFolder(folder) => {
            let tabs = browser
                .window_of_folder(*folder)
                .and_then(|window| browser.window(window))
                .map(|w| w.folder_tabs(*folder))
                .unwrap_or_default();
            remember_closed(browser, &tabs);
            close_tabs(browser, &tabs)
        }
        Command::BookmarkTabs { tabs, group } => bookmark_tabs(browser, tabs, group),
        Command::OpenBookmarkGroup { window, group } => {
            let urls: Vec<Url> = browser
                .bookmarks
                .group(group)
                .map(|g| g.bookmarks.iter().map(|b| b.url.clone()).collect())
                .unwrap_or_default();
            let mut actions = Vec::new();
            for url in urls {
                actions.extend(new_tab(browser, *window, Some(url)));
            }
            actions
        }
        Command::PromptMoveToFolder(tabs) => {
            let Some(window) = tabs.iter().find_map(|t| browser.window_of_tab(*t)) else {
                return Vec::new();
            };
            let w = browser.window(window).expect("window_of_tab");
            let tabs: Vec<TabId> = tabs
                .iter()
                .copied()
                .filter(|t| w.tab(*t).is_some())
                .collect();
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::MoveToFolder {
                    tabs,
                    suggested: w.suggested_folder_name(),
                    folders: w.folders.iter().map(|f| (f.id, f.name.clone())).collect(),
                },
            }]
        }
        Command::PromptBookmarkTabs(tabs) => {
            let Some(window) = tabs.iter().find_map(|t| browser.window_of_tab(*t)) else {
                return Vec::new();
            };
            let tabs: Vec<TabId> = tabs
                .iter()
                .copied()
                .filter(|t| browser.tab(*t).is_some())
                .collect();
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::BookmarkTabs {
                    tabs,
                    groups: browser.bookmarks.group_names(),
                },
            }]
        }
        Command::PromptRenameFolder(folder) => {
            let Some(window) = browser.window_of_folder(*folder) else {
                return Vec::new();
            };
            let name = browser
                .window(window)
                .and_then(|w| w.folder(*folder))
                .map(|f| f.name.clone())
                .unwrap_or_default();
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::RenameFolder {
                    folder: *folder,
                    name,
                },
            }]
        }
        Command::NextTab(window) => step_tab(browser, *window, 1),
        Command::PreviousTab(window) => step_tab(browser, *window, -1),
        Command::ActivateTabAt { window, index } => {
            match browser.window(*window).and_then(|w| w.tabs.get(*index)) {
                Some(t) => activate_tab(browser, t.id),
                None => Vec::new(),
            }
        }
        Command::ActivateLastTab(window) => {
            match browser.window(*window).and_then(|w| w.tabs.last()) {
                Some(t) => activate_tab(browser, t.id),
                None => Vec::new(),
            }
        }
        Command::ReopenClosedTab(window) => {
            if browser.window(*window).is_none() {
                return Vec::new();
            }
            match browser.closed_tabs.pop() {
                Some(url) => new_tab(browser, *window, Some(url)),
                None => Vec::new(),
            }
        }
        Command::Home(tab) => {
            let url = browser.homepage();
            dispatch(browser, &Command::Navigate { tab: *tab, url })
        }
        Command::ToggleFullscreen(window) => match browser.window_mut(*window) {
            Some(w) => {
                w.fullscreen = !w.fullscreen;
                vec![EngineAction::SetFullscreen {
                    window: *window,
                    fullscreen: w.fullscreen,
                }]
            }
            None => Vec::new(),
        },
        Command::Navigate { tab, url } => match browser.tab_mut(*tab) {
            Some(t) => {
                t.url = url.clone();
                t.load = LoadState::Loading;
                t.crashed = None;
                vec![EngineAction::Navigate {
                    tab: *tab,
                    url: url.clone(),
                }]
            }
            None => Vec::new(),
        },
        Command::Back(tab) => history_step(browser, *tab, EngineAction::Back(*tab)),
        Command::Forward(tab) => history_step(browser, *tab, EngineAction::Forward(*tab)),
        Command::Reload(tab) => match browser.tab_mut(*tab) {
            Some(t) => {
                t.load = LoadState::Loading;
                t.crashed = None;
                vec![EngineAction::Reload(*tab)]
            }
            None => Vec::new(),
        },
        Command::Stop(tab) => pass_through(browser, *tab, EngineAction::Stop(*tab)),
        Command::FocusAddressBar(window) => {
            if browser.window(*window).is_some() {
                vec![EngineAction::FocusAddressBar(*window)]
            } else {
                Vec::new()
            }
        }
        Command::NewWindow => {
            let url = browser.new_tab_url();
            open_window(browser, url)
        }
        Command::CloseWindow(window) => {
            let tabs: Vec<TabId> = browser
                .window(*window)
                .map(|w| w.tabs.iter().map(|t| t.id).collect())
                .unwrap_or_default();
            remember_closed(browser, &tabs);
            close_window(browser, *window)
        }
        Command::OpenSettings => vec![EngineAction::OpenSettings],
        Command::DismissEngineError => {
            browser.engine_error = None;
            Vec::new()
        }
        Command::OpenExtensionAction { window, extension } => {
            let usable = browser.window(*window).is_some()
                && browser.extension(extension).is_some_and(|e| e.has_action());
            if usable {
                vec![EngineAction::OpenExtensionAction {
                    window: *window,
                    extension: extension.clone(),
                }]
            } else {
                Vec::new()
            }
        }
        Command::PinExtension { extension, pinned } => {
            if set_pinned(browser, extension, *pinned) {
                vec![EngineAction::SaveSettings]
            } else {
                Vec::new()
            }
        }
        Command::SetExtensionEnabled { extension, enabled } => {
            if browser.extension(extension).is_some() {
                vec![EngineAction::SetExtensionEnabled {
                    extension: extension.clone(),
                    enabled: *enabled,
                }]
            } else {
                Vec::new()
            }
        }
        Command::UninstallExtension(extension) => {
            if browser.extension(extension).is_none() {
                return Vec::new();
            }
            let mut actions = vec![EngineAction::UninstallExtension(extension.clone())];
            if set_pinned(browser, extension, false) {
                actions.push(EngineAction::SaveSettings);
            }
            actions
        }
        Command::InstallExtension(source) => vec![EngineAction::InstallExtension(source.clone())],
        Command::Search { tab, query } => match browser.settings.search.url_for(query) {
            Some(url) => dispatch(browser, &Command::Navigate { tab: *tab, url }),
            None => Vec::new(),
        },
        Command::SearchInNewTab { window, query } => match browser.settings.search.url_for(query) {
            Some(url) => new_tab(browser, *window, Some(url)),
            None => Vec::new(),
        },
        Command::NewBackgroundTab { opener, url } => new_background_tab(browser, *opener, url),
        Command::OpenInNewWindow(url) => open_window(browser, url.clone()),
        Command::CopyText(text) => vec![EngineAction::CopyText(text.clone())],
        Command::Page { tab, command } => pass_through(
            browser,
            *tab,
            EngineAction::Page {
                tab: *tab,
                command: command.clone(),
            },
        ),
        Command::ContextMenuClosed(tab) => {
            pass_through(browser, *tab, EngineAction::ContextMenuClosed(*tab))
        }
        Command::Quit => vec![EngineAction::Quit],
    }
}

/// Pin or unpin `extension` in the settings. Returns true if the settings
/// changed.
fn set_pinned(browser: &mut Browser, extension: &str, pinned: bool) -> bool {
    let list = &mut browser.settings.pinned_extensions;
    let was = list.iter().any(|p| p == extension);
    if pinned && !was {
        list.push(extension.to_string());
        true
    } else if !pinned && was {
        list.retain(|p| p != extension);
        true
    } else {
        false
    }
}

fn folder_mut(browser: &mut Browser, folder: FolderId) -> Option<&mut Folder> {
    let window = browser.window_of_folder(folder)?;
    browser.window_mut(window)?.folder_mut(folder)
}

/// Put the tabs (those in the window of the first) in the folder, which
/// is made if `target` names a new one, and gather the folder's tabs
/// where its first tab is. A tab leaves the folder it was in; a folder
/// left empty goes.
fn move_tabs_to_folder(browser: &mut Browser, tabs: &[TabId], target: &FolderTarget) {
    let Some(window) = tabs.iter().find_map(|t| browser.window_of_tab(*t)) else {
        return;
    };
    let folder = match target {
        FolderTarget::Existing(id) => {
            if browser.window_of_folder(*id) != Some(window) {
                return;
            }
            *id
        }
        FolderTarget::New { name } => {
            let id = browser.allocate_folder_id();
            let w = browser.window_mut(window).expect("window_of_tab");
            let name = match name.trim() {
                "" => w.suggested_folder_name(),
                name => name.to_string(),
            };
            w.folders.push(Folder {
                id,
                name,
                expanded: false,
            });
            id
        }
    };
    let w = browser.window_mut(window).expect("window_of_tab");
    for tab in &mut w.tabs {
        if tabs.contains(&tab.id) {
            tab.folder = Some(folder);
        }
    }
    w.gather_folder(folder);
    w.prune_folders();
}

/// Bookmark the pages of `tabs` (those that exist) in `group`, which is
/// made if new; an empty name means "Bookmarks".
fn bookmark_tabs(browser: &mut Browser, tabs: &[TabId], group: &str) -> Vec<EngineAction> {
    let bookmarks: Vec<Bookmark> = tabs
        .iter()
        .filter_map(|tab| browser.tab(*tab))
        .map(|t| Bookmark {
            title: t.display_title().to_string(),
            url: t.url.clone(),
        })
        .collect();
    if bookmarks.is_empty() {
        return Vec::new();
    }
    let group = match group.trim() {
        "" => "Bookmarks",
        name => name,
    };
    browser.bookmarks.add(group, bookmarks);
    vec![EngineAction::SaveBookmarks]
}

/// Add a window to the model whose first tab shows `url`. Used by
/// `Command::NewWindow` and by `main` for the initial window, which opens
/// the homepage or a URL given on the command line.
pub fn open_window(browser: &mut Browser, url: Url) -> Vec<EngineAction> {
    let window = browser.allocate_window_id();
    let tab = browser.allocate_tab_id();
    browser
        .windows
        .push(Window::with_tab(window, Tab::new(tab, url.clone())));
    vec![EngineAction::OpenWindow { window, tab, url }]
}

/// Remember the URLs of `tabs` (those that exist, in the given order) for
/// "Reopen closed tab". Only tabs the user closes are remembered: a tab
/// moved to another window or profile is closed here too, but not lost.
fn remember_closed(browser: &mut Browser, tabs: &[TabId]) {
    let urls: Vec<Url> = tabs
        .iter()
        .filter_map(|tab| browser.tab(*tab).map(|t| t.url.clone()))
        .collect();
    for url in urls {
        browser.remember_closed(url);
    }
}

fn pass_through(browser: &Browser, tab: TabId, action: EngineAction) -> Vec<EngineAction> {
    if browser.tab(tab).is_some() {
        vec![action]
    } else {
        Vec::new()
    }
}

/// Back and Forward ask for a new load of the tab's content, so like
/// `Reload` they clear a crashed tab's error panel.
fn history_step(browser: &mut Browser, tab: TabId, action: EngineAction) -> Vec<EngineAction> {
    match browser.tab_mut(tab) {
        Some(t) => {
            t.crashed = None;
            vec![action]
        }
        None => Vec::new(),
    }
}

fn new_tab(browser: &mut Browser, window: WindowId, url: Option<Url>) -> Vec<EngineAction> {
    if browser.window(window).is_none() {
        return Vec::new();
    }
    let focus_address = url.is_none();
    let url = url.unwrap_or_else(|| browser.new_tab_url());
    let tab = browser.allocate_tab_id();
    let w = browser
        .window_mut(window)
        .expect("window existence checked above");
    let previous = w.active;
    w.tabs.push(Tab::new(tab, url.clone()));
    w.active = tab;
    w.select_only(tab);
    let mut actions = vec![
        EngineAction::Hide(previous),
        EngineAction::CreateTab { window, tab, url },
    ];
    // A tab the user opened empty is there to type an address into; a tab
    // opened on a URL (a popup, a link into a new tab) keeps the focus on
    // the page.
    if focus_address {
        actions.push(EngineAction::FocusAddressBar(window));
    }
    actions
}

/// Add a tab at `url` beside `opener` that does not become active: the
/// engine creates it hidden, and the active tab keeps the focus.
fn new_background_tab(browser: &mut Browser, opener: TabId, url: &Url) -> Vec<EngineAction> {
    let Some(window) = browser.window_of_tab(opener) else {
        return Vec::new();
    };
    let tab = browser.allocate_tab_id();
    let w = browser
        .window_mut(window)
        .expect("window_of_tab returned an existing window");
    w.insert_tab_after(opener, Tab::new(tab, url.clone()));
    vec![EngineAction::CreateHiddenTab {
        window,
        tab,
        url: url.clone(),
    }]
}

/// Remove `tab`. If it was active, the tab to its right becomes active, or
/// the one to its left if it was last. Closing the last tab closes the
/// window, and closing the last window quits.
pub(crate) fn close_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    let mut actions = vec![EngineAction::CloseTab(tab)];
    let new_active = {
        let w = browser
            .window_mut(window_id)
            .expect("window_of_tab returned an existing window");
        let index = w.index_of(tab).expect("tab is in this window");
        w.tabs.remove(index);
        w.prune_folders();
        w.selected.retain(|t| *t != tab);
        if w.tabs.is_empty() {
            None
        } else if w.active == tab {
            let next = index.min(w.tabs.len() - 1);
            w.active = w.tabs[next].id;
            w.select_only(w.active);
            Some(w.active)
        } else {
            return actions;
        }
    };
    match new_active {
        Some(next) => actions.push(EngineAction::Show(next)),
        None => {
            browser.windows.retain(|w| w.id != window_id);
            actions.push(EngineAction::CloseWindow(window_id));
            if browser.windows.is_empty() {
                actions.push(EngineAction::Quit);
            }
        }
    }
    actions
}

fn activate_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    let w = browser
        .window_mut(window_id)
        .expect("window_of_tab returned an existing window");
    if w.active == tab {
        w.select_only(tab);
        return Vec::new();
    }
    let previous = w.active;
    w.active = tab;
    w.select_only(tab);
    vec![EngineAction::Hide(previous), EngineAction::Show(tab)]
}

fn toggle_tab_selection(browser: &mut Browser, tab: TabId) {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return;
    };
    let w = browser
        .window_mut(window_id)
        .expect("window_of_tab returned an existing window");
    if w.active == tab {
        return;
    }
    if w.is_selected(tab) {
        w.selected.retain(|t| *t != tab);
    } else {
        w.selected.push(tab);
    }
}

fn select_tab_range(browser: &mut Browser, tab: TabId) {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return;
    };
    let w = browser
        .window_mut(window_id)
        .expect("window_of_tab returned an existing window");
    let (from, to) = {
        let a = w.active_index();
        let b = w.index_of(tab).expect("tab is in this window");
        (a.min(b), a.max(b))
    };
    w.selected = w.tabs[from..=to].iter().map(|t| t.id).collect();
}

fn close_tabs(browser: &mut Browser, tabs: &[TabId]) -> Vec<EngineAction> {
    let mut actions = Vec::new();
    for tab in tabs {
        actions.extend(close_tab(browser, *tab));
    }
    actions
}

/// The tabs of `tabs` that exist, in strip order (window by window), with
/// their URLs, and whether each is the active tab of its window.
fn moving_tabs(browser: &Browser, tabs: &[TabId]) -> Vec<(TabId, Url, bool)> {
    browser
        .windows
        .iter()
        .flat_map(|w| {
            w.tabs
                .iter()
                .filter(|t| tabs.contains(&t.id))
                .map(move |t| (t.id, t.url.clone(), w.active == t.id))
        })
        .collect()
}

fn move_tabs_to_new_window(browser: &mut Browser, tabs: &[TabId]) -> Vec<EngineAction> {
    let moving = moving_tabs(browser, tabs);
    let Some((_, first_url, _)) = moving.first() else {
        return Vec::new();
    };
    let mut actions = open_window(browser, first_url.clone());
    let window = browser
        .windows
        .last()
        .expect("open_window pushed a window")
        .id;
    actions.extend(place_moved_tabs(browser, window, &moving, 1));
    actions
}

fn move_tabs_to_window(
    browser: &mut Browser,
    tabs: &[TabId],
    window: WindowId,
) -> Vec<EngineAction> {
    if browser.window(window).is_none() {
        return Vec::new();
    }
    let moving: Vec<_> = moving_tabs(browser, tabs)
        .into_iter()
        .filter(|(tab, _, _)| browser.window_of_tab(*tab) != Some(window))
        .collect();
    if moving.is_empty() {
        return Vec::new();
    }
    place_moved_tabs(browser, window, &moving, 0)
}

/// Reorder within a window: see `Command::MoveTabs`. Only the tabs of
/// the first tab's window move; `index` past the end means the end.
fn move_tabs(browser: &mut Browser, tabs: &[TabId], index: usize) {
    let Some(window) = tabs.first().and_then(|tab| browser.window_of_tab(*tab)) else {
        return;
    };
    let Some(w) = browser.window_mut(window) else {
        return;
    };
    let (mut moving, mut rest): (Vec<Tab>, Vec<Tab>) =
        w.tabs.drain(..).partition(|tab| tabs.contains(&tab.id));
    if moving.is_empty() {
        w.tabs = rest;
        return;
    }
    let at = index.min(rest.len());
    // The folder of the landing place: the one both neighbours share.
    let folder = match (at.checked_sub(1).and_then(|i| rest.get(i)), rest.get(at)) {
        (Some(before), Some(after)) if before.folder.is_some() && before.folder == after.folder => {
            before.folder
        }
        _ => None,
    };
    for tab in &mut moving {
        tab.folder = folder;
    }
    rest.splice(at..at, moving);
    w.tabs = rest;
    w.prune_folders();
    let folders: Vec<FolderId> = w.folders.iter().map(|f| f.id).collect();
    for folder in folders {
        w.gather_folder(folder);
    }
}

/// Recreate `moving` (from index `already` on; the earlier ones are
/// already in `window`, at its end) as new tabs at the end of `window`,
/// activate the one standing in for the tab that was active in its old
/// window (or the first), then close the originals.
fn place_moved_tabs(
    browser: &mut Browser,
    window: WindowId,
    moving: &[(TabId, Url, bool)],
    already: usize,
) -> Vec<EngineAction> {
    let mut actions = Vec::new();
    let first_new = browser
        .window(window)
        .map(|w| w.tabs.len() - already)
        .unwrap_or(0);
    for (_, url, _) in &moving[already..] {
        actions.extend(new_tab(browser, window, Some(url.clone())));
    }
    let to_activate = {
        let w = browser.window(window).expect("window existence checked");
        let offset = moving
            .iter()
            .position(|(_, _, active)| *active)
            .unwrap_or(0);
        w.tabs[first_new + offset].id
    };
    actions.extend(activate_tab(browser, to_activate));
    let old: Vec<TabId> = moving.iter().map(|(tab, _, _)| *tab).collect();
    actions.extend(close_tabs(browser, &old));
    actions
}

fn open_tabs_in_profile(browser: &mut Browser, tabs: &[TabId], profile: &str) -> Vec<EngineAction> {
    if profile == browser.profile.name {
        return Vec::new();
    }
    let moving = moving_tabs(browser, tabs);
    if moving.is_empty() {
        return Vec::new();
    }
    let urls = moving.iter().map(|(_, url, _)| url.clone()).collect();
    let old: Vec<TabId> = moving.iter().map(|(tab, _, _)| *tab).collect();
    let mut actions = vec![EngineAction::OpenInProfile {
        profile: profile.to_string(),
        urls,
    }];
    actions.extend(close_tabs(browser, &old));
    actions
}

fn step_tab(browser: &mut Browser, window: WindowId, delta: isize) -> Vec<EngineAction> {
    let Some(w) = browser.window(window) else {
        return Vec::new();
    };
    let len = w.tabs.len() as isize;
    let next = (w.active_index() as isize + delta).rem_euclid(len) as usize;
    let target = w.tabs[next].id;
    activate_tab(browser, target)
}

fn close_window(browser: &mut Browser, window: WindowId) -> Vec<EngineAction> {
    let Some(position) = browser.windows.iter().position(|w| w.id == window) else {
        return Vec::new();
    };
    let removed = browser.windows.remove(position);
    let mut actions: Vec<EngineAction> = removed
        .tabs
        .iter()
        .map(|t| EngineAction::CloseTab(t.id))
        .collect();
    actions.push(EngineAction::CloseWindow(window));
    if browser.windows.is_empty() {
        actions.push(EngineAction::Quit);
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_support::{browser, url};

    /// A browser with one window holding one tab. Returns the ids.
    fn one_window() -> (Browser, WindowId, TabId) {
        let mut b = browser();
        let actions = open_window(&mut b, url("https://a.example/"));
        let EngineAction::OpenWindow { window, tab, .. } = actions[0].clone() else {
            panic!("open_window returns OpenWindow");
        };
        (b, window, tab)
    }

    #[test]
    fn a_search_loads_the_search_url_in_the_tab_or_a_new_tab() {
        let (mut b, window, tab) = one_window();
        let actions = dispatch(
            &mut b,
            &Command::Search {
                tab,
                query: "two words".to_string(),
            },
        );
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("https://www.google.com/search?q=two+words"),
            }]
        );
        assert_eq!(
            b.tab(tab).unwrap().url.as_str(),
            "https://www.google.com/search?q=two+words"
        );
        let actions = dispatch(
            &mut b,
            &Command::SearchInNewTab {
                window,
                query: "more".to_string(),
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 2);
        assert_eq!(w.active, w.tabs[1].id);
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::Hide(_), EngineAction::CreateTab { url, .. }]
                if url.as_str() == "https://www.google.com/search?q=more"
        ));
        // A blank query does nothing.
        assert!(
            dispatch(
                &mut b,
                &Command::Search {
                    tab,
                    query: "  ".to_string(),
                },
            )
            .is_empty()
        );
    }

    #[test]
    fn a_background_tab_opens_beside_its_opener_hidden_and_in_its_folder() {
        let (mut b, window, opener) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://z.example/")),
            },
        );
        dispatch(&mut b, &Command::ActivateTab(opener));
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![opener],
                folder: FolderTarget::New {
                    name: String::new(),
                },
            },
        );
        let folder = b.tab(opener).unwrap().folder.expect("in a folder");
        let actions = dispatch(
            &mut b,
            &Command::NewBackgroundTab {
                opener,
                url: url("https://link.example/"),
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.active, opener);
        assert_eq!(w.tabs.len(), 3);
        let new = &w.tabs[1];
        assert_eq!(new.url.as_str(), "https://link.example/");
        assert_eq!(new.folder, Some(folder));
        assert_eq!(
            actions,
            vec![EngineAction::CreateHiddenTab {
                window,
                tab: new.id,
                url: url("https://link.example/"),
            }]
        );
        assert!(
            dispatch(
                &mut b,
                &Command::NewBackgroundTab {
                    opener: TabId(999),
                    url: url("https://link.example/"),
                },
            )
            .is_empty()
        );
    }

    #[test]
    fn open_in_new_window_opens_a_window_at_the_url() {
        let (mut b, _window, _tab) = one_window();
        let actions = dispatch(&mut b, &Command::OpenInNewWindow(url("https://w.example/")));
        assert_eq!(b.windows.len(), 2);
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::OpenWindow { url, .. }] if url.as_str() == "https://w.example/"
        ));
    }

    #[test]
    fn page_commands_and_the_menu_closing_pass_through_for_live_tabs() {
        let (mut b, _window, tab) = one_window();
        assert_eq!(
            dispatch(
                &mut b,
                &Command::Page {
                    tab,
                    command: PageCommand::Copy,
                },
            ),
            vec![EngineAction::Page {
                tab,
                command: PageCommand::Copy,
            }]
        );
        assert_eq!(
            dispatch(&mut b, &Command::ContextMenuClosed(tab)),
            vec![EngineAction::ContextMenuClosed(tab)]
        );
        assert!(
            dispatch(
                &mut b,
                &Command::Page {
                    tab: TabId(999),
                    command: PageCommand::Copy,
                },
            )
            .is_empty()
        );
        assert_eq!(
            dispatch(&mut b, &Command::CopyText("hi".to_string())),
            vec![EngineAction::CopyText("hi".to_string())]
        );
    }

    #[test]
    fn open_window_creates_window_with_one_active_tab() {
        let (b, window, tab) = one_window();
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 1);
        assert_eq!(w.active, tab);
        assert_eq!(w.active_tab().url.as_str(), "https://a.example/");
    }

    #[test]
    fn new_tab_uses_new_tab_url_and_becomes_active() {
        let (mut b, window, first) = one_window();
        b.settings.new_tab_url = "https://new.example/".to_string();
        let actions = dispatch(&mut b, &Command::NewTab { window, url: None });
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 2);
        let second = w.tabs[1].id;
        assert_eq!(w.active, second);
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(first),
                EngineAction::CreateTab {
                    window,
                    tab: second,
                    url: url("https://new.example/"),
                },
                EngineAction::FocusAddressBar(window),
            ]
        );
    }

    #[test]
    fn new_tab_with_explicit_url_uses_it_and_leaves_focus_on_the_page() {
        let (mut b, window, first) = one_window();
        let actions = dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://b.example/")),
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.active_tab().url.as_str(), "https://b.example/");
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(first),
                EngineAction::CreateTab {
                    window,
                    tab: w.active,
                    url: url("https://b.example/"),
                },
            ]
        );
    }

    #[test]
    fn new_tab_in_unknown_window_does_nothing() {
        let (mut b, _, _) = one_window();
        let actions = dispatch(
            &mut b,
            &Command::NewTab {
                window: WindowId(99),
                url: None,
            },
        );
        assert!(actions.is_empty());
        assert_eq!(b.windows.len(), 1);
    }

    #[test]
    fn activate_tab_hides_previous_and_shows_target() {
        let (mut b, window, first) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let second = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::ActivateTab(first));
        assert_eq!(b.window(window).unwrap().active, first);
        assert_eq!(
            actions,
            vec![EngineAction::Hide(second), EngineAction::Show(first)]
        );
        assert!(dispatch(&mut b, &Command::ActivateTab(first)).is_empty());
    }

    #[test]
    fn close_active_tab_activates_tab_to_the_right_then_left() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t3 = b.window(window).unwrap().active;

        dispatch(&mut b, &Command::ActivateTab(t2));
        let actions = dispatch(&mut b, &Command::CloseTab(t2));
        assert_eq!(b.window(window).unwrap().active, t3, "right neighbour wins");
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t2), EngineAction::Show(t3)]
        );

        let actions = dispatch(&mut b, &Command::CloseTab(t3));
        assert_eq!(
            b.window(window).unwrap().active,
            t1,
            "last tab falls back left"
        );
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t3), EngineAction::Show(t1)]
        );
    }

    #[test]
    fn close_inactive_tab_keeps_active_and_emits_no_show() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::CloseTab(t1));
        assert_eq!(b.window(window).unwrap().active, t2);
        assert_eq!(actions, vec![EngineAction::CloseTab(t1)]);
    }

    #[test]
    fn closing_last_tab_closes_window_and_last_window_quits() {
        let (mut b, w1, t1) = one_window();
        let w2_actions = dispatch(&mut b, &Command::NewWindow);
        let EngineAction::OpenWindow {
            window: w2,
            tab: t2,
            ..
        } = w2_actions[0].clone()
        else {
            panic!("NewWindow returns OpenWindow");
        };

        let actions = dispatch(&mut b, &Command::CloseTab(t1));
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t1), EngineAction::CloseWindow(w1)]
        );
        assert!(b.window(w1).is_none());

        let actions = dispatch(&mut b, &Command::CloseTab(t2));
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseTab(t2),
                EngineAction::CloseWindow(w2),
                EngineAction::Quit,
            ]
        );
        assert!(b.windows.is_empty());
    }

    #[test]
    fn next_and_previous_tab_wrap_around() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t3 = b.window(window).unwrap().active;

        dispatch(&mut b, &Command::NextTab(window));
        assert_eq!(
            b.window(window).unwrap().active,
            t1,
            "wraps from last to first"
        );
        dispatch(&mut b, &Command::PreviousTab(window));
        assert_eq!(
            b.window(window).unwrap().active,
            t3,
            "wraps from first to last"
        );
        dispatch(&mut b, &Command::PreviousTab(window));
        assert_eq!(b.window(window).unwrap().active, t2);
    }

    #[test]
    fn next_tab_with_single_tab_is_a_no_op() {
        let (mut b, window, t1) = one_window();
        assert!(dispatch(&mut b, &Command::NextTab(window)).is_empty());
        assert_eq!(b.window(window).unwrap().active, t1);
    }

    #[test]
    fn navigate_updates_url_marks_loading_and_clears_crash() {
        let (mut b, _, tab) = one_window();
        b.tab_mut(tab).unwrap().crashed = Some("boom".to_string());
        let actions = dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("https://b.example/"),
            },
        );
        let t = b.tab(tab).unwrap();
        assert_eq!(t.url.as_str(), "https://b.example/");
        assert_eq!(t.load, LoadState::Loading);
        assert_eq!(t.crashed, None);
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("https://b.example/"),
            }]
        );
    }

    #[test]
    fn back_forward_reload_stop_pass_through_for_known_tabs_only() {
        let (mut b, _, tab) = one_window();
        assert_eq!(
            dispatch(&mut b, &Command::Back(tab)),
            vec![EngineAction::Back(tab)]
        );
        assert_eq!(
            dispatch(&mut b, &Command::Forward(tab)),
            vec![EngineAction::Forward(tab)]
        );
        assert_eq!(
            dispatch(&mut b, &Command::Reload(tab)),
            vec![EngineAction::Reload(tab)]
        );
        assert_eq!(b.tab(tab).unwrap().load, LoadState::Loading);
        assert_eq!(
            dispatch(&mut b, &Command::Stop(tab)),
            vec![EngineAction::Stop(tab)]
        );
        assert!(dispatch(&mut b, &Command::Back(TabId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::Reload(TabId(99))).is_empty());
    }

    #[test]
    fn back_clears_a_crashed_tab() {
        let (mut b, _, tab) = one_window();
        b.tab_mut(tab).unwrap().crashed = Some("boom".to_string());
        assert_eq!(
            dispatch(&mut b, &Command::Back(tab)),
            vec![EngineAction::Back(tab)]
        );
        assert_eq!(b.tab(tab).unwrap().crashed, None);
    }

    #[test]
    fn forward_clears_a_crashed_tab() {
        let (mut b, _, tab) = one_window();
        b.tab_mut(tab).unwrap().crashed = Some("boom".to_string());
        assert_eq!(
            dispatch(&mut b, &Command::Forward(tab)),
            vec![EngineAction::Forward(tab)]
        );
        assert_eq!(b.tab(tab).unwrap().crashed, None);
    }

    #[test]
    fn new_window_opens_with_new_tab_url() {
        let (mut b, _, _) = one_window();
        b.settings.new_tab_url = "https://new.example/".to_string();
        let actions = dispatch(&mut b, &Command::NewWindow);
        assert_eq!(b.windows.len(), 2);
        let w = &b.windows[1];
        assert_eq!(
            actions,
            vec![EngineAction::OpenWindow {
                window: w.id,
                tab: w.active,
                url: url("https://new.example/"),
            }]
        );
    }

    #[test]
    fn close_window_closes_all_its_tabs_and_quits_when_last() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::CloseWindow(window));
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseTab(t1),
                EngineAction::CloseTab(t2),
                EngineAction::CloseWindow(window),
                EngineAction::Quit,
            ]
        );
        assert!(b.windows.is_empty());
        assert!(dispatch(&mut b, &Command::CloseWindow(window)).is_empty());
    }

    #[test]
    fn focus_settings_and_quit_are_forwarded_to_the_app() {
        let (mut b, window, _) = one_window();
        assert_eq!(
            dispatch(&mut b, &Command::FocusAddressBar(window)),
            vec![EngineAction::FocusAddressBar(window)]
        );
        assert!(dispatch(&mut b, &Command::FocusAddressBar(WindowId(99))).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::OpenSettings),
            vec![EngineAction::OpenSettings]
        );
        assert_eq!(dispatch(&mut b, &Command::Quit), vec![EngineAction::Quit]);
    }

    #[test]
    fn tab_and_window_commands_for_unknown_ids_are_no_ops() {
        let (mut b, _, _) = one_window();
        let before = b.windows.clone();
        assert!(dispatch(&mut b, &Command::CloseTab(TabId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::ActivateTab(TabId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::NextTab(WindowId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::PreviousTab(WindowId(99))).is_empty());
        assert_eq!(b.windows, before);
    }

    #[test]
    fn dismiss_engine_error_clears_it() {
        let (mut b, _, _) = one_window();
        b.engine_error = Some("lost backend".to_string());
        assert!(dispatch(&mut b, &Command::DismissEngineError).is_empty());
        assert_eq!(b.engine_error, None);
    }

    /// A window with three tabs; returns the browser, the window, and the
    /// tab ids in strip order. The third tab is active.
    fn three_tabs() -> (Browser, WindowId, [TabId; 3]) {
        let (mut b, window, first) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://b.example/")),
            },
        );
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://c.example/")),
            },
        );
        let w = b.window(window).unwrap();
        let ids = [first, w.tabs[1].id, w.tabs[2].id];
        assert_eq!(w.active, ids[2]);
        (b, window, ids)
    }

    #[test]
    fn tabs_move_together_to_an_index_among_the_others() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        assert!(
            dispatch(
                &mut b,
                &Command::MoveTabs {
                    tabs: vec![c],
                    index: 0
                }
            )
            .is_empty()
        );
        assert_eq!(order(&b, window), [c, a, bb]);
        // Two tabs, given out of strip order, land in strip order; the
        // index counts the tabs that are not moving.
        dispatch(
            &mut b,
            &Command::MoveTabs {
                tabs: vec![a, c],
                index: 1,
            },
        );
        assert_eq!(order(&b, window), [bb, c, a]);
        // Past the end is the end; a tab from nowhere is ignored.
        dispatch(
            &mut b,
            &Command::MoveTabs {
                tabs: vec![bb, TabId(99)],
                index: 7,
            },
        );
        assert_eq!(order(&b, window), [c, a, bb]);
        assert_eq!(
            b.window(window).unwrap().active,
            c,
            "the active tab stays active"
        );
    }

    #[test]
    fn a_tab_dropped_inside_a_folder_joins_it_and_dropped_outside_leaves_it() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![a, bb],
                folder: FolderTarget::New {
                    name: "F".to_string(),
                },
            },
        );
        let folder = b.tab(a).unwrap().folder.expect("in a folder");
        // Between a and bb: joins.
        dispatch(
            &mut b,
            &Command::MoveTabs {
                tabs: vec![c],
                index: 1,
            },
        );
        assert_eq!(order(&b, window), [a, c, bb]);
        assert_eq!(b.tab(c).unwrap().folder, Some(folder));
        // Before the folder: leaves it, and the folder stays contiguous.
        dispatch(
            &mut b,
            &Command::MoveTabs {
                tabs: vec![c],
                index: 0,
            },
        );
        assert_eq!(order(&b, window), [c, a, bb]);
        assert_eq!(b.tab(c).unwrap().folder, None);
        assert_eq!(b.window(window).unwrap().folder_tabs(folder), [a, bb]);
        // Moving the folder's last tab out of it empties nothing: a
        // folder of one remains; moving that one out too drops the folder.
        dispatch(
            &mut b,
            &Command::MoveTabs {
                tabs: vec![bb],
                index: 0,
            },
        );
        dispatch(
            &mut b,
            &Command::MoveTabs {
                tabs: vec![a],
                index: 0,
            },
        );
        assert_eq!(order(&b, window), [a, bb, c]);
        assert!(b.window(window).unwrap().folders.is_empty());
    }

    #[test]
    fn a_new_or_activated_tab_is_the_only_selected_tab() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        assert_eq!(b.window(window).unwrap().selection(), vec![c]);
        dispatch(&mut b, &Command::ToggleTabSelection(a));
        assert_eq!(b.window(window).unwrap().selection(), vec![a, c]);
        dispatch(&mut b, &Command::ActivateTab(bb));
        assert_eq!(b.window(window).unwrap().selection(), vec![bb]);
    }

    #[test]
    fn toggling_adds_and_removes_tabs_but_never_the_active_one() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        assert!(dispatch(&mut b, &Command::ToggleTabSelection(a)).is_empty());
        dispatch(&mut b, &Command::ToggleTabSelection(bb));
        assert_eq!(b.window(window).unwrap().selection(), vec![a, bb, c]);
        dispatch(&mut b, &Command::ToggleTabSelection(a));
        assert_eq!(b.window(window).unwrap().selection(), vec![bb, c]);
        dispatch(&mut b, &Command::ToggleTabSelection(c));
        assert_eq!(b.window(window).unwrap().selection(), vec![bb, c]);
        assert_eq!(b.window(window).unwrap().active, c);
        assert!(dispatch(&mut b, &Command::ToggleTabSelection(TabId(99))).is_empty());
    }

    #[test]
    fn range_selection_runs_from_the_active_tab_in_either_direction() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        dispatch(&mut b, &Command::SelectTabRange(a));
        assert_eq!(b.window(window).unwrap().selection(), vec![a, bb, c]);
        assert_eq!(b.window(window).unwrap().active, c);
        dispatch(&mut b, &Command::ActivateTab(a));
        dispatch(&mut b, &Command::SelectTabRange(bb));
        assert_eq!(b.window(window).unwrap().selection(), vec![a, bb]);
        dispatch(&mut b, &Command::SelectTabRange(a));
        assert_eq!(b.window(window).unwrap().selection(), vec![a]);
    }

    #[test]
    fn closing_a_tab_drops_it_from_the_selection() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        dispatch(&mut b, &Command::SelectTabRange(a));
        dispatch(&mut b, &Command::CloseTab(bb));
        assert_eq!(b.window(window).unwrap().selection(), vec![a, c]);
        // Closing the active tab makes the neighbour active and the only
        // selected tab.
        dispatch(&mut b, &Command::CloseTab(c));
        assert_eq!(b.window(window).unwrap().selection(), vec![a]);
    }

    #[test]
    fn close_tabs_closes_each_and_the_window_when_none_remain() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        let actions = dispatch(&mut b, &Command::CloseTabs(vec![a, c]));
        assert_eq!(b.window(window).unwrap().tabs.len(), 1);
        assert_eq!(b.window(window).unwrap().active, bb);
        assert!(actions.contains(&EngineAction::CloseTab(a)));
        assert!(actions.contains(&EngineAction::CloseTab(c)));
        let actions = dispatch(&mut b, &Command::CloseTabs(vec![bb, TabId(99)]));
        assert!(b.windows.is_empty());
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseTab(bb),
                EngineAction::CloseWindow(window),
                EngineAction::Quit,
            ]
        );
    }

    #[test]
    fn moving_tabs_to_a_new_window_recreates_them_there_and_closes_the_originals() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        let actions = dispatch(&mut b, &Command::MoveTabsToNewWindow(vec![c, a]));
        assert_eq!(b.windows.len(), 2);
        let new = &b.windows[1];
        let urls: Vec<&str> = new.tabs.iter().map(|t| t.url.as_str()).collect();
        // Strip order, not the order given.
        assert_eq!(urls, vec!["https://a.example/", "https://c.example/"]);
        // The tab that was active moves as the active tab.
        assert_eq!(new.active_tab().url.as_str(), "https://c.example/");
        assert_eq!(new.selection(), vec![new.active]);
        let old = b.window(window).unwrap();
        assert_eq!(old.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), vec![bb]);
        assert_eq!(old.active, bb);
        // The new window opens before the old tabs close, so an old window
        // that empties out never looks like the last one.
        let open = actions
            .iter()
            .position(|x| matches!(x, EngineAction::OpenWindow { .. }))
            .unwrap();
        let close = actions
            .iter()
            .position(|x| *x == EngineAction::CloseTab(a))
            .unwrap();
        assert!(open < close);
        assert!(!actions.contains(&EngineAction::Quit));
    }

    #[test]
    fn moving_every_tab_to_a_new_window_closes_the_old_window_without_quitting() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        let actions = dispatch(&mut b, &Command::MoveTabsToNewWindow(vec![a, bb, c]));
        assert_eq!(b.windows.len(), 1);
        assert_ne!(b.windows[0].id, window);
        assert!(actions.contains(&EngineAction::CloseWindow(window)));
        assert!(!actions.contains(&EngineAction::Quit));
    }

    #[test]
    fn moving_tabs_to_another_window_appends_them_and_activates_the_moved_active_tab() {
        let (mut b, window, [a, _bb, c]) = three_tabs();
        let other_actions = dispatch(&mut b, &Command::NewWindow);
        let EngineAction::OpenWindow { window: other, .. } = other_actions[0].clone() else {
            panic!("NewWindow opens a window");
        };
        let actions = dispatch(
            &mut b,
            &Command::MoveTabsToWindow {
                tabs: vec![a, c],
                window: other,
            },
        );
        let target = b.window(other).unwrap();
        let urls: Vec<&str> = target.tabs.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls.len(), 3);
        assert_eq!(&urls[1..], &["https://a.example/", "https://c.example/"]);
        assert_eq!(target.active_tab().url.as_str(), "https://c.example/");
        assert_eq!(b.window(window).unwrap().tabs.len(), 1);
        assert!(actions.contains(&EngineAction::CloseTab(a)));
        assert!(actions.contains(&EngineAction::CloseTab(c)));
        // Tabs already in the target, unknown tabs, and unknown windows are ignored.
        let before = b.windows.clone();
        let moved = target.tabs[1].id;
        assert!(
            dispatch(
                &mut b,
                &Command::MoveTabsToWindow {
                    tabs: vec![moved, TabId(99)],
                    window: other,
                },
            )
            .is_empty()
        );
        assert!(
            dispatch(
                &mut b,
                &Command::MoveTabsToWindow {
                    tabs: vec![a],
                    window: WindowId(99),
                },
            )
            .is_empty()
        );
        assert_eq!(b.windows, before);
    }

    #[test]
    fn opening_tabs_in_another_profile_hands_over_the_urls_and_closes_them() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        let actions = dispatch(
            &mut b,
            &Command::OpenTabsInProfile {
                tabs: vec![c, a],
                profile: "work".to_string(),
            },
        );
        assert_eq!(
            actions[0],
            EngineAction::OpenInProfile {
                profile: "work".to_string(),
                urls: vec![url("https://a.example/"), url("https://c.example/")],
            }
        );
        assert!(actions.contains(&EngineAction::CloseTab(a)));
        assert!(actions.contains(&EngineAction::CloseTab(c)));
        assert_eq!(
            b.window(window)
                .unwrap()
                .tabs
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            vec![bb]
        );
        // The current profile is not another process: nothing happens.
        let before = b.windows.clone();
        let own_profile = b.profile.name.clone();
        assert!(
            dispatch(
                &mut b,
                &Command::OpenTabsInProfile {
                    tabs: vec![bb],
                    profile: own_profile,
                },
            )
            .is_empty()
        );
        assert_eq!(b.windows, before);
    }

    #[test]
    fn activate_tab_at_index_and_last_tab() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        let actions = dispatch(&mut b, &Command::ActivateTabAt { window, index: 0 });
        assert_eq!(actions, vec![EngineAction::Hide(c), EngineAction::Show(a)]);
        assert_eq!(b.window(window).unwrap().active, a);
        // Past the end: nothing happens.
        assert!(dispatch(&mut b, &Command::ActivateTabAt { window, index: 7 }).is_empty());
        assert_eq!(b.window(window).unwrap().active, a);
        let actions = dispatch(&mut b, &Command::ActivateLastTab(window));
        assert_eq!(actions, vec![EngineAction::Hide(a), EngineAction::Show(c)]);
        // The active tab again: nothing to do.
        assert!(dispatch(&mut b, &Command::ActivateLastTab(window)).is_empty());
        let _ = bb;
        assert!(
            dispatch(
                &mut b,
                &Command::ActivateTabAt {
                    window: WindowId(99),
                    index: 0
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn closed_tabs_are_remembered_and_reopened_most_recent_first() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        assert!(b.closed_tabs.is_empty());
        assert!(dispatch(&mut b, &Command::ReopenClosedTab(window)).is_empty());
        dispatch(&mut b, &Command::CloseTab(a));
        dispatch(&mut b, &Command::CloseTabs(vec![bb]));
        assert_eq!(
            b.closed_tabs,
            vec![url("https://a.example/"), url("https://b.example/")]
        );
        let actions = dispatch(&mut b, &Command::ReopenClosedTab(window));
        let w = b.window(window).unwrap();
        let reopened = w.tabs[1].id;
        assert_eq!(w.active, reopened);
        assert_eq!(w.tabs[1].url.as_str(), "https://b.example/");
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(c),
                EngineAction::CreateTab {
                    window,
                    tab: reopened,
                    url: url("https://b.example/"),
                },
            ],
            "a reopened tab keeps the focus on the page"
        );
        assert_eq!(b.closed_tabs, vec![url("https://a.example/")]);
        assert!(dispatch(&mut b, &Command::ReopenClosedTab(WindowId(99))).is_empty());
        assert_eq!(b.closed_tabs.len(), 1, "an unknown window pops nothing");
    }

    #[test]
    fn closing_a_window_remembers_its_tabs_but_moving_tabs_does_not() {
        let (mut b, window, [a, bb, c]) = three_tabs();
        dispatch(&mut b, &Command::MoveTabsToNewWindow(vec![a]));
        assert!(b.closed_tabs.is_empty(), "a moved tab is not a closed tab");
        let new_window = b.windows[1].id;
        dispatch(&mut b, &Command::CloseWindow(window));
        assert_eq!(
            b.closed_tabs,
            vec![url("https://b.example/"), url("https://c.example/")]
        );
        let _ = (bb, c);
        let actions = dispatch(&mut b, &Command::ReopenClosedTab(new_window));
        assert!(matches!(
            actions.as_slice(),
            [_, EngineAction::CreateTab { url, .. }] if url.as_str() == "https://c.example/"
        ));
    }

    #[test]
    fn home_navigates_the_tab_to_the_homepage() {
        let (mut b, _, tab) = one_window();
        b.settings.homepage = "https://home.example/".to_string();
        let actions = dispatch(&mut b, &Command::Home(tab));
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("https://home.example/")
            }]
        );
        assert_eq!(b.tab(tab).unwrap().url.as_str(), "https://home.example/");
        assert_eq!(b.tab(tab).unwrap().load, LoadState::Loading);
        assert!(dispatch(&mut b, &Command::Home(TabId(99))).is_empty());
    }

    #[test]
    fn toggle_fullscreen_flips_the_window_and_tells_the_app() {
        let (mut b, window, _) = one_window();
        assert!(!b.window(window).unwrap().fullscreen);
        let actions = dispatch(&mut b, &Command::ToggleFullscreen(window));
        assert_eq!(
            actions,
            vec![EngineAction::SetFullscreen {
                window,
                fullscreen: true
            }]
        );
        assert!(b.window(window).unwrap().fullscreen);
        let actions = dispatch(&mut b, &Command::ToggleFullscreen(window));
        assert_eq!(
            actions,
            vec![EngineAction::SetFullscreen {
                window,
                fullscreen: false
            }]
        );
        assert!(!b.window(window).unwrap().fullscreen);
        assert!(dispatch(&mut b, &Command::ToggleFullscreen(WindowId(99))).is_empty());
    }

    fn installed(b: &mut Browser, id: &str, enabled: bool, popup: bool) {
        b.extensions.push(crate::extension::Extension {
            id: id.to_string(),
            name: id.to_uppercase(),
            version: "1".to_string(),
            description: String::new(),
            enabled,
            icon: None,
            action_title: popup.then(|| "Act".to_string()),
        });
    }

    #[test]
    fn open_extension_action_needs_a_window_and_an_enabled_extension_with_a_popup() {
        let (mut b, window, _) = one_window();
        installed(&mut b, "a", true, true);
        installed(&mut b, "off", false, true);
        installed(&mut b, "nopopup", true, false);
        let open = |b: &mut Browser, w, id: &str| {
            dispatch(
                b,
                &Command::OpenExtensionAction {
                    window: w,
                    extension: id.to_string(),
                },
            )
        };
        assert_eq!(
            open(&mut b, window, "a"),
            vec![EngineAction::OpenExtensionAction {
                window,
                extension: "a".to_string()
            }]
        );
        assert!(open(&mut b, window, "off").is_empty());
        assert!(open(&mut b, window, "nopopup").is_empty());
        assert!(open(&mut b, window, "missing").is_empty());
        assert!(open(&mut b, WindowId(99), "a").is_empty());
    }

    #[test]
    fn pinning_edits_the_settings_in_order_and_reports_only_real_changes() {
        let (mut b, _, _) = one_window();
        let pin = |b: &mut Browser, id: &str, pinned: bool| {
            dispatch(
                b,
                &Command::PinExtension {
                    extension: id.to_string(),
                    pinned,
                },
            )
        };
        assert_eq!(pin(&mut b, "a", true), vec![EngineAction::SaveSettings]);
        assert_eq!(pin(&mut b, "b", true), vec![EngineAction::SaveSettings]);
        assert!(pin(&mut b, "a", true).is_empty(), "already pinned");
        assert_eq!(b.settings.pinned_extensions, vec!["a", "b"]);
        assert_eq!(pin(&mut b, "a", false), vec![EngineAction::SaveSettings]);
        assert!(pin(&mut b, "a", false).is_empty(), "already unpinned");
        assert_eq!(b.settings.pinned_extensions, vec!["b"]);
    }

    #[test]
    fn enabling_and_installing_go_to_the_engine() {
        let (mut b, _, _) = one_window();
        installed(&mut b, "a", true, true);
        assert_eq!(
            dispatch(
                &mut b,
                &Command::SetExtensionEnabled {
                    extension: "a".to_string(),
                    enabled: false
                }
            ),
            vec![EngineAction::SetExtensionEnabled {
                extension: "a".to_string(),
                enabled: false
            }]
        );
        assert!(
            dispatch(
                &mut b,
                &Command::SetExtensionEnabled {
                    extension: "zz".to_string(),
                    enabled: false
                }
            )
            .is_empty()
        );
        let source = ExtensionSource::Store {
            id: "abc".to_string(),
        };
        assert_eq!(
            dispatch(&mut b, &Command::InstallExtension(source.clone())),
            vec![EngineAction::InstallExtension(source)]
        );
    }

    #[test]
    fn uninstalling_unpins_too() {
        let (mut b, _, _) = one_window();
        installed(&mut b, "a", true, true);
        installed(&mut b, "b", true, true);
        b.settings.pinned_extensions = vec!["a".to_string()];
        assert_eq!(
            dispatch(&mut b, &Command::UninstallExtension("a".to_string())),
            vec![
                EngineAction::UninstallExtension("a".to_string()),
                EngineAction::SaveSettings
            ]
        );
        assert!(b.settings.pinned_extensions.is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::UninstallExtension("b".to_string())),
            vec![EngineAction::UninstallExtension("b".to_string())]
        );
        assert!(dispatch(&mut b, &Command::UninstallExtension("zz".to_string())).is_empty());
    }

    /// A window with `n` tabs; returns the browser, the window, and the tab ids.
    fn window_with_tabs(n: usize) -> (Browser, WindowId, Vec<TabId>) {
        let (mut b, window, first) = one_window();
        let mut tabs = vec![first];
        for _ in 1..n {
            dispatch(&mut b, &Command::NewTab { window, url: None });
            tabs.push(b.window(window).unwrap().active);
        }
        (b, window, tabs)
    }

    fn order(b: &Browser, window: WindowId) -> Vec<TabId> {
        b.window(window)
            .unwrap()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect()
    }

    #[test]
    fn moving_tabs_to_a_new_folder_names_it_gathers_them_and_keeps_them_in_order() {
        let (mut b, window, t) = window_with_tabs(5);
        let actions = dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[3], t[1]],
                folder: FolderTarget::New {
                    name: "  ".to_string(),
                },
            },
        );
        assert!(actions.is_empty());
        let w = b.window(window).unwrap();
        assert_eq!(w.folders.len(), 1);
        let folder = w.folders[0].id;
        assert_eq!(w.folders[0].name, "Folder A");
        assert!(!w.folders[0].expanded);
        assert_eq!(w.folder_tabs(folder), vec![t[1], t[3]]);
        assert_eq!(order(&b, window), vec![t[0], t[1], t[3], t[2], t[4]]);
        assert_eq!(b.window_of_folder(folder), Some(window));

        // Another tab joins the existing folder and is gathered too.
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[4]],
                folder: FolderTarget::Existing(folder),
            },
        );
        assert_eq!(order(&b, window), vec![t[0], t[1], t[3], t[4], t[2]]);
        // A second new folder gets the next name and takes a tab from the first.
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[4], t[2]],
                folder: FolderTarget::New {
                    name: String::new(),
                },
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.folders[1].name, "Folder B");
        assert_eq!(w.folder_tabs(folder), vec![t[1], t[3]]);
        assert_eq!(w.folder_tabs(w.folders[1].id), vec![t[4], t[2]]);
        assert_eq!(order(&b, window), vec![t[0], t[1], t[3], t[4], t[2]]);
        // An unknown folder or unknown tabs change nothing.
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[0]],
                folder: FolderTarget::Existing(FolderId(99)),
            },
        );
        assert_eq!(b.window(window).unwrap().tab(t[0]).unwrap().folder, None);
        assert!(
            dispatch(
                &mut b,
                &Command::MoveTabsToFolder {
                    tabs: vec![TabId(99)],
                    folder: FolderTarget::New {
                        name: "x".to_string()
                    },
                }
            )
            .is_empty()
        );
        assert_eq!(b.window(window).unwrap().folders.len(), 2);
    }

    #[test]
    fn folders_expand_rename_ungroup_and_close() {
        let (mut b, window, t) = window_with_tabs(4);
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[1], t[2]],
                folder: FolderTarget::New {
                    name: "Work".to_string(),
                },
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        dispatch(
            &mut b,
            &Command::SetFolderExpanded {
                folder,
                expanded: true,
            },
        );
        assert!(b.window(window).unwrap().folder(folder).unwrap().expanded);
        dispatch(
            &mut b,
            &Command::RenameFolder {
                folder,
                name: " Play ".to_string(),
            },
        );
        assert_eq!(
            b.window(window).unwrap().folder(folder).unwrap().name,
            "Play"
        );
        dispatch(
            &mut b,
            &Command::RenameFolder {
                folder,
                name: "  ".to_string(),
            },
        );
        assert_eq!(
            b.window(window).unwrap().folder(folder).unwrap().name,
            "Play"
        );

        // Closing one tab keeps the folder; closing the last removes it.
        dispatch(&mut b, &Command::CloseTab(t[1]));
        assert_eq!(b.window(window).unwrap().folder_tabs(folder), vec![t[2]]);
        dispatch(&mut b, &Command::CloseTab(t[2]));
        assert!(b.window(window).unwrap().folders.is_empty());

        // Ungroup leaves the tabs where they are.
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[0], t[3]],
                folder: FolderTarget::New {
                    name: "Again".to_string(),
                },
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        dispatch(&mut b, &Command::Ungroup(folder));
        let w = b.window(window).unwrap();
        assert!(w.folders.is_empty());
        assert!(w.tabs.iter().all(|t| t.folder.is_none()));
        assert_eq!(order(&b, window), vec![t[0], t[3]]);

        // Close folder closes its tabs and remembers them.
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[3]],
                folder: FolderTarget::New {
                    name: "Gone".to_string(),
                },
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        let closed = b.closed_tabs.len();
        let actions = dispatch(&mut b, &Command::CloseFolder(folder));
        assert_eq!(actions[0], EngineAction::CloseTab(t[3]));
        assert_eq!(b.closed_tabs.len(), closed + 1);
        assert_eq!(order(&b, window), vec![t[0]]);
        assert!(dispatch(&mut b, &Command::CloseFolder(FolderId(99))).is_empty());
    }

    #[test]
    fn bookmarking_tabs_fills_a_group_and_saves() {
        let (mut b, window, t) = window_with_tabs(2);
        b.tab_mut(t[0]).unwrap().title = "First".to_string();
        let actions = dispatch(
            &mut b,
            &Command::BookmarkTabs {
                tabs: vec![t[0], t[1], TabId(99)],
                group: " Work ".to_string(),
            },
        );
        assert_eq!(actions, vec![EngineAction::SaveBookmarks]);
        let group = b.bookmarks.group("Work").unwrap();
        assert_eq!(group.bookmarks.len(), 2);
        assert_eq!(group.bookmarks[0].title, "First");
        assert_eq!(group.bookmarks[0].url.as_str(), "https://a.example/");
        assert!(
            dispatch(
                &mut b,
                &Command::BookmarkTabs {
                    tabs: vec![TabId(99)],
                    group: "Work".to_string()
                }
            )
            .is_empty()
        );
        dispatch(
            &mut b,
            &Command::BookmarkTabs {
                tabs: vec![t[0]],
                group: String::new(),
            },
        );
        assert!(b.bookmarks.group("Bookmarks").is_some());

        // Opening the group makes a tab per page, the last one active.
        let before = order(&b, window).len();
        let actions = dispatch(
            &mut b,
            &Command::OpenBookmarkGroup {
                window,
                group: "Work".to_string(),
            },
        );
        assert_eq!(order(&b, window).len(), before + 2);
        assert_eq!(
            actions
                .iter()
                .filter(|a| matches!(a, EngineAction::CreateTab { .. }))
                .count(),
            2
        );
        assert!(
            dispatch(
                &mut b,
                &Command::OpenBookmarkGroup {
                    window,
                    group: "None".to_string()
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn prompts_carry_what_the_chrome_needs_to_ask() {
        let (mut b, window, t) = window_with_tabs(3);
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[0]],
                folder: FolderTarget::New {
                    name: "Folder A".to_string(),
                },
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        assert_eq!(
            dispatch(&mut b, &Command::PromptMoveToFolder(vec![t[1], TabId(99)])),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::MoveToFolder {
                    tabs: vec![t[1]],
                    suggested: "Folder B".to_string(),
                    folders: vec![(folder, "Folder A".to_string())],
                },
            }]
        );
        b.bookmarks.add("Work", Vec::new());
        assert_eq!(
            dispatch(&mut b, &Command::PromptBookmarkTabs(vec![t[1], t[2]])),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::BookmarkTabs {
                    tabs: vec![t[1], t[2]],
                    groups: vec!["Work".to_string()],
                },
            }]
        );
        assert_eq!(
            dispatch(&mut b, &Command::PromptRenameFolder(folder)),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::RenameFolder {
                    folder,
                    name: "Folder A".to_string(),
                },
            }]
        );
        assert!(dispatch(&mut b, &Command::PromptMoveToFolder(vec![TabId(99)])).is_empty());
        assert!(dispatch(&mut b, &Command::PromptRenameFolder(FolderId(99))).is_empty());
    }
}
