//! Windows, tabs, and the identifiers that cross crate boundaries.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::bookmarks::Bookmarks;
use crate::extension::Extension;
use crate::profile::Profile;
use crate::settings::Settings;

/// Identifies a browser window. Allocated by [`Browser`]; engine ids never
/// leave the engine crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WindowId(pub u64);

/// Identifies a tab. Allocated by [`Browser`]; engine ids never leave the
/// engine crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TabId(pub u64);

/// Identifies a folder of tabs in a window's strip. Allocated by
/// [`Browser`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FolderId(pub u64);

/// A named folder of tabs in the strip. Its tabs sit together in the
/// window's tab order; collapsed, the strip shows the folder alone and a
/// click lists its tabs; expanded, its tabs follow it in the strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    pub expanded: bool,
}

/// One thing in the strip, in order: a tab outside any folder, or a
/// folder with the tabs in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StripItem {
    Tab(TabId),
    Folder { folder: FolderId, tabs: Vec<TabId> },
}

/// Where a tab is in its page load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadState {
    #[default]
    Idle,
    Loading,
    Complete,
}

/// A decoded image, straight RGBA8, row-major, top-left origin: a
/// favicon, or an image copied to the clipboard. The pixels sit behind an
/// `Arc` so a renderer can tell a new image from the one it already
/// uploaded by pointer, without hashing the pixels every frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// A tab's icon.
pub type Favicon = RgbaImage;

/// One page shown as a tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub url: Url,
    pub title: String,
    pub favicon: Option<Favicon>,
    pub load: LoadState,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub status: String,
    /// `Some(reason)` after the engine reported this tab's content crashed.
    /// The tab stays in the model so its URL is not lost; the UI shows an
    /// error panel with a reload button instead of page content.
    pub crashed: Option<String>,
    /// The folder this tab is in, if any.
    pub folder: Option<FolderId>,
}

impl Tab {
    pub fn new(id: TabId, url: Url) -> Tab {
        Tab {
            id,
            url,
            title: String::new(),
            favicon: None,
            load: LoadState::Idle,
            can_go_back: false,
            can_go_forward: false,
            status: String::new(),
            crashed: None,
            folder: None,
        }
    }

    /// The text to show in the tab strip: the page title, or the URL until
    /// a title arrives.
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            self.url.as_str()
        } else {
            &self.title
        }
    }
}

/// A browser window. Invariants: a window always has at least one tab,
/// `active` names one of them, and `selected` holds `active` plus any other
/// tabs of this window the user added to the selection (Ctrl-click,
/// Shift-click). Closing the last tab closes the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: WindowId,
    pub tabs: Vec<Tab>,
    pub active: TabId,
    pub selected: Vec<TabId>,
    /// The window fills the screen and shows no chrome, only the page.
    pub fullscreen: bool,
    /// The folders of this window's strip, in the order they were made.
    /// Every folder has at least one tab; a folder whose last tab goes
    /// goes with it.
    pub folders: Vec<Folder>,
}

impl Window {
    /// A window with one tab, which is active and selected.
    pub fn with_tab(id: WindowId, tab: Tab) -> Window {
        let active = tab.id;
        Window {
            id,
            tabs: vec![tab],
            active,
            selected: vec![active],
            fullscreen: false,
            folders: Vec::new(),
        }
    }

    pub fn folder(&self, id: FolderId) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    pub fn folder_mut(&mut self, id: FolderId) -> Option<&mut Folder> {
        self.folders.iter_mut().find(|f| f.id == id)
    }

    /// The tabs in folder `id`, in strip order.
    pub fn folder_tabs(&self, id: FolderId) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|t| t.folder == Some(id))
            .map(|t| t.id)
            .collect()
    }

    /// The strip as drawn: tabs outside folders on their own, and each
    /// folder, where its first tab is, with all its tabs.
    pub fn strip_items(&self) -> Vec<StripItem> {
        let mut items = Vec::new();
        let mut seen: Vec<FolderId> = Vec::new();
        for tab in &self.tabs {
            match tab.folder {
                Some(folder) if self.folder(folder).is_some() => {
                    if !seen.contains(&folder) {
                        seen.push(folder);
                        items.push(StripItem::Folder {
                            folder,
                            tabs: self.folder_tabs(folder),
                        });
                    }
                }
                _ => items.push(StripItem::Tab(tab.id)),
            }
        }
        items
    }

    /// The next free name in the sequence "Folder A", "Folder B", ...,
    /// "Folder Z", "Folder AA", ...
    pub fn suggested_folder_name(&self) -> String {
        (1u32..)
            .map(|n| format!("Folder {}", letters(n)))
            .find(|name| !self.folders.iter().any(|f| f.name == *name))
            .expect("the sequence is unbounded")
    }

    /// Put the tabs of folder `id` together, where the first of them is.
    pub(crate) fn gather_folder(&mut self, id: FolderId) {
        let Some(first) = self.tabs.iter().position(|t| t.folder == Some(id)) else {
            return;
        };
        let mut members = Vec::new();
        let mut rest = Vec::new();
        for tab in self.tabs.drain(..) {
            if tab.folder == Some(id) {
                members.push(tab);
            } else {
                rest.push(tab);
            }
        }
        let at = first.min(rest.len());
        rest.splice(at..at, members);
        self.tabs = rest;
    }

    /// Drop folders with no tabs left, and the folder of a tab whose
    /// folder is gone.
    pub(crate) fn prune_folders(&mut self) {
        let tabs = &self.tabs;
        self.folders
            .retain(|f| tabs.iter().any(|t| t.folder == Some(f.id)));
        let folders = &self.folders;
        for tab in &mut self.tabs {
            if tab
                .folder
                .is_some_and(|id| !folders.iter().any(|f| f.id == id))
            {
                tab.folder = None;
            }
        }
    }

    pub fn is_selected(&self, tab: TabId) -> bool {
        self.selected.contains(&tab)
    }

    /// The selected tabs in strip order. Always includes the active tab.
    pub fn selection(&self) -> Vec<TabId> {
        self.tabs
            .iter()
            .map(|t| t.id)
            .filter(|id| self.selected.contains(id))
            .collect()
    }

    /// Make `tab` (which must be one of this window's tabs) the only
    /// selected tab. Does not change the active tab.
    pub(crate) fn select_only(&mut self, tab: TabId) {
        self.selected.clear();
        self.selected.push(tab);
    }
    pub fn index_of(&self, tab: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == tab)
    }

    pub fn tab(&self, tab: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == tab)
    }

    pub fn tab_mut(&mut self, tab: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == tab)
    }

    /// Index of the active tab. Panics only if the window invariant is broken.
    pub fn active_index(&self) -> usize {
        self.index_of(self.active)
            .expect("window invariant: active tab is one of the window's tabs")
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_index()]
    }

    /// Put `tab` right after `anchor` in the strip, in `anchor`'s folder
    /// (a page's new tab sits beside the page that opened it); at the end,
    /// outside any folder, if `anchor` is not here.
    pub(crate) fn insert_tab_after(&mut self, anchor: TabId, mut tab: Tab) {
        match self.tabs.iter().position(|t| t.id == anchor) {
            Some(index) => {
                tab.folder = self.tabs[index].folder;
                self.tabs.insert(index + 1, tab);
            }
            None => {
                tab.folder = None;
                self.tabs.push(tab);
            }
        }
    }
}

/// `1` → "A", `26` → "Z", `27` → "AA", like spreadsheet columns.
fn letters(mut n: u32) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII letters")
}

/// The whole browser state for one process (one profile).
#[derive(Debug)]
pub struct Browser {
    pub profile: Profile,
    pub settings: Settings,
    pub windows: Vec<Window>,
    /// Last engine-level error reported by the engine, shown in the status area.
    pub engine_error: Option<String>,
    /// The URLs of tabs the user closed, oldest first, at most
    /// [`Browser::MAX_CLOSED_TABS`]; "Reopen closed tab" pops the last.
    pub closed_tabs: Vec<Url>,
    /// The installed extensions, as the engine last reported them. Empty
    /// for an engine without extensions.
    pub extensions: Vec<Extension>,
    pub bookmarks: Bookmarks,
    next_window_id: u64,
    next_tab_id: u64,
    next_folder_id: u64,
}

impl Browser {
    /// How many closed tabs are remembered for reopening.
    pub const MAX_CLOSED_TABS: usize = 32;

    pub fn new(profile: Profile, settings: Settings) -> Browser {
        Browser {
            profile,
            settings,
            windows: Vec::new(),
            engine_error: None,
            closed_tabs: Vec::new(),
            extensions: Vec::new(),
            bookmarks: Bookmarks::default(),
            next_window_id: 1,
            next_tab_id: 1,
            next_folder_id: 1,
        }
    }

    pub fn window_of_folder(&self, folder: FolderId) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|w| w.folder(folder).is_some())
            .map(|w| w.id)
    }

    /// Remember `url` as the most recently closed tab.
    pub(crate) fn remember_closed(&mut self, url: Url) {
        if self.closed_tabs.len() == Self::MAX_CLOSED_TABS {
            self.closed_tabs.remove(0);
        }
        self.closed_tabs.push(url);
    }

    pub fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn extension(&self, id: &str) -> Option<&Extension> {
        self.extensions.iter().find(|e| e.id == id)
    }

    /// The extensions shown in the toolbar: those pinned in the settings,
    /// in that order, that are installed and enabled.
    pub fn pinned_extensions(&self) -> Vec<&Extension> {
        self.settings
            .pinned_extensions
            .iter()
            .filter_map(|id| self.extension(id))
            .filter(|e| e.enabled)
            .collect()
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.settings.pinned_extensions.iter().any(|p| p == id)
    }

    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn window_of_tab(&self, tab: TabId) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|w| w.index_of(tab).is_some())
            .map(|w| w.id)
    }

    pub fn tab(&self, tab: TabId) -> Option<&Tab> {
        self.windows.iter().find_map(|w| w.tab(tab))
    }

    pub fn tab_mut(&mut self, tab: TabId) -> Option<&mut Tab> {
        self.windows.iter_mut().find_map(|w| w.tab_mut(tab))
    }

    /// The URL new tabs open with, from settings, falling back to
    /// `about:blank` if the setting does not parse.
    pub fn new_tab_url(&self) -> Url {
        Url::parse(&self.settings.new_tab_url)
            .unwrap_or_else(|_| Url::parse("about:blank").expect("about:blank parses"))
    }

    /// The homepage from settings, falling back to the new-tab URL.
    pub fn homepage(&self) -> Url {
        Url::parse(&self.settings.homepage).unwrap_or_else(|_| self.new_tab_url())
    }

    pub(crate) fn allocate_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    pub(crate) fn allocate_tab_id(&mut self) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        id
    }

    pub(crate) fn allocate_folder_id(&mut self) -> FolderId {
        let id = FolderId(self.next_folder_id);
        self.next_folder_id += 1;
        id
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    use super::*;

    /// A browser with no windows, a throwaway profile, and default settings.
    pub(crate) fn browser() -> Browser {
        let dir = PathBuf::from("/nonexistent/osmena-test-profile");
        let profile = Profile {
            name: "test".to_string(),
            engine_dir: dir.join("engine"),
            dir,
        };
        Browser::new(profile, Settings::default())
    }

    pub(crate) fn url(s: &str) -> Url {
        Url::parse(s).expect("test URL parses")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{browser, url};
    use super::*;

    #[test]
    fn display_title_falls_back_to_url() {
        let mut tab = Tab::new(TabId(1), url("https://example.com/"));
        assert_eq!(tab.display_title(), "https://example.com/");
        tab.title = "Example".to_string();
        assert_eq!(tab.display_title(), "Example");
    }

    #[test]
    fn new_tab_url_falls_back_to_about_blank_when_setting_is_invalid() {
        let mut b = browser();
        b.settings.new_tab_url = "not a url".to_string();
        assert_eq!(b.new_tab_url().as_str(), "about:blank");
    }

    #[test]
    fn homepage_uses_setting_when_valid() {
        let mut b = browser();
        b.settings.homepage = "https://home.example/".to_string();
        assert_eq!(b.homepage().as_str(), "https://home.example/");
    }

    #[test]
    fn homepage_falls_back_to_new_tab_url_when_invalid() {
        let mut b = browser();
        b.settings.homepage = "not a url".to_string();
        b.settings.new_tab_url = "https://n.example/".to_string();
        assert_eq!(b.homepage().as_str(), "https://n.example/");
    }

    #[test]
    fn closed_tabs_are_capped_dropping_the_oldest() {
        let mut b = browser();
        for i in 0..=Browser::MAX_CLOSED_TABS {
            b.remember_closed(url(&format!("https://{i}.example/")));
        }
        assert_eq!(b.closed_tabs.len(), Browser::MAX_CLOSED_TABS);
        assert_eq!(b.closed_tabs[0].as_str(), "https://1.example/");
        assert_eq!(
            b.closed_tabs.last().unwrap().as_str(),
            format!("https://{}.example/", Browser::MAX_CLOSED_TABS)
        );
    }

    #[test]
    fn pinned_extensions_follow_the_settings_order_and_skip_disabled_and_missing() {
        let mut b = browser();
        let ext = |id: &str, enabled: bool| Extension {
            id: id.to_string(),
            name: id.to_uppercase(),
            version: "1".to_string(),
            description: String::new(),
            enabled,
            icon: None,
            action_title: Some("Act".to_string()),
        };
        b.extensions = vec![ext("a", true), ext("b", false), ext("c", true)];
        b.settings.pinned_extensions = vec!["c".into(), "zz".into(), "b".into(), "a".into()];
        let pinned: Vec<&str> = b
            .pinned_extensions()
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(pinned, vec!["c", "a"]);
        assert!(b.is_pinned("zz"));
        assert!(!b.is_pinned("d"));
        assert!(b.extension("a").unwrap().has_action());
        assert!(!b.extension("b").unwrap().has_action());
    }

    fn window_with_tabs(n: u64) -> Window {
        let mut w = Window::with_tab(WindowId(1), Tab::new(TabId(1), url("https://1.example/")));
        for i in 2..=n {
            w.tabs
                .push(Tab::new(TabId(i), url(&format!("https://{i}.example/"))));
        }
        w
    }

    #[test]
    fn folder_names_run_through_the_alphabet_and_skip_names_in_use() {
        assert_eq!(letters(1), "A");
        assert_eq!(letters(26), "Z");
        assert_eq!(letters(27), "AA");
        assert_eq!(letters(52), "AZ");
        assert_eq!(letters(53), "BA");
        let mut w = window_with_tabs(1);
        assert_eq!(w.suggested_folder_name(), "Folder A");
        w.folders.push(Folder {
            id: FolderId(1),
            name: "Folder A".to_string(),
            expanded: false,
        });
        w.folders.push(Folder {
            id: FolderId(2),
            name: "Folder C".to_string(),
            expanded: false,
        });
        assert_eq!(w.suggested_folder_name(), "Folder B");
    }

    #[test]
    fn strip_items_put_a_folder_where_its_first_tab_is_with_all_its_tabs() {
        let mut w = window_with_tabs(5);
        w.folders.push(Folder {
            id: FolderId(7),
            name: "F".to_string(),
            expanded: false,
        });
        w.tabs[1].folder = Some(FolderId(7));
        w.tabs[3].folder = Some(FolderId(7));
        // A folder that no longer exists does not group.
        w.tabs[4].folder = Some(FolderId(99));
        assert_eq!(
            w.strip_items(),
            vec![
                StripItem::Tab(TabId(1)),
                StripItem::Folder {
                    folder: FolderId(7),
                    tabs: vec![TabId(2), TabId(4)]
                },
                StripItem::Tab(TabId(3)),
                StripItem::Tab(TabId(5)),
            ]
        );
        w.gather_folder(FolderId(7));
        let order: Vec<TabId> = w.tabs.iter().map(|t| t.id).collect();
        assert_eq!(
            order,
            vec![TabId(1), TabId(2), TabId(4), TabId(3), TabId(5)]
        );
        w.prune_folders();
        assert_eq!(w.tabs[4].folder, None, "the stale folder id is cleared");
        assert_eq!(w.folders.len(), 1);
        for tab in &mut w.tabs {
            tab.folder = None;
        }
        w.prune_folders();
        assert!(w.folders.is_empty(), "an empty folder goes");
    }

    #[test]
    fn ids_are_allocated_sequentially() {
        let mut b = browser();
        assert_eq!(b.allocate_tab_id(), TabId(1));
        assert_eq!(b.allocate_tab_id(), TabId(2));
        assert_eq!(b.allocate_window_id(), WindowId(1));
        assert_eq!(b.allocate_window_id(), WindowId(2));
    }
}
