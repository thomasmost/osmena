//! Named user actions and the pure `dispatch` that applies them.

use std::path::PathBuf;

use url::Url;

use crate::blocked::{BlockedPage, BlockedSites, line_seed};
use crate::bookmarks::{Bookmark, Bookmarks};
use crate::bookmarks_html;
use crate::default_browser::DefaultBrowser;
use crate::download::{Download, DownloadId, DownloadState};
use crate::event::PendingTab;
use crate::extension::{ExtensionReply, ExtensionRequest, ExtensionSource, SidePanel};
use crate::history::unix_now;
use crate::launch::{
    DRIVE_LIFETIME, DriveState, Fetched, LOCATION_LIFETIME, Place, Units, WEATHER_LIFETIME,
    WidgetKind,
};
use crate::media::MediaRequest;
use crate::model::{
    Browser, FindState, Folder, FolderId, LoadState, Tab, TabId, Window, WindowId, about_blank,
    site_key,
};
use crate::profile::{Avatar, DEFAULT_PROFILE_NAME, ProfileEntry};
use crate::session::{Session, WindowGeometry};
use crate::settings::{ChromeColor, UrlTemplate};
use crate::update::{AvailableUpdate, UpdateStatus};
use crate::zoom::{ZOOM_MAX, ZOOM_MIN, next_step};

/// Every user action in the browser is one of these. The chrome, the keymap,
/// and remappable hotkeys all produce `Command`s; nothing else
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
    /// Start a browser for `profile` (another process) on its new-window
    /// page; nothing for the current profile.
    OpenProfile(String),
    /// Start a browser for a fresh guest profile: made for it, listed
    /// nowhere, and deleted when that browser exits.
    OpenGuestProfile,
    /// Ask the user to configure the current profile (its name, colour
    /// and avatar, and what it shares with the default profile) in
    /// `window`. Nothing for a guest.
    PromptEditProfile(WindowId),
    /// Show every profile in `window`, each to open, edit or delete,
    /// with the ways to add one.
    PromptManageProfiles(WindowId),
    /// Ask the user for a new profile's name, colour and avatar in
    /// `window`.
    PromptAddProfile(WindowId),
    /// Ask the user to confirm deleting `profile` (a directory name:
    /// the current profile's or another's) in `window`. The default
    /// profile cannot be deleted.
    PromptDeleteProfile {
        window: WindowId,
        profile: String,
    },
    /// The current profile, as configured in the profile dialog.
    SaveProfile(ProfileForm),
    /// Make a profile from the form and start a browser for it.
    CreateProfile(ProfileForm),
    /// Delete the profile in directory `profile` with everything it
    /// holds. For the current profile the browser starts the default
    /// profile and quits, deleting its directory on the way out.
    DeleteProfile(String),
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
    /// The toolbar's star, or `Ctrl+D`: bookmark `tab`'s page in the
    /// last-used group when it is not bookmarked yet, then open the
    /// bookmark popover for it (to name it, move it to another group,
    /// or remove it).
    ToggleBookmark(TabId),
    /// Open the bookmark popover in `window` for `url`, which is
    /// bookmarked already (a chip's "Edit…").
    PromptEditBookmark {
        window: WindowId,
        url: Url,
    },
    /// The popover's answer: the bookmark of `url` is titled `title` and
    /// sits in `group` (made if new; empty means the bar).
    EditBookmark {
        url: Url,
        title: String,
        group: String,
    },
    RemoveBookmark(Url),
    /// Ask for a new name for the bookmark group.
    PromptRenameBookmarkGroup {
        window: WindowId,
        group: String,
    },
    RenameBookmarkGroup {
        group: String,
        name: String,
    },
    /// Ask before deleting the group and its bookmarks.
    PromptDeleteBookmarkGroup {
        window: WindowId,
        group: String,
    },
    DeleteBookmarkGroup(String),
    /// Reorder a group (the bar's chips are dragged): the bookmark at
    /// `from` moves to sit at `to`, an index into the group as it is.
    MoveBookmark {
        group: String,
        from: usize,
        to: usize,
    },
    /// Show or hide the bookmarks bar.
    ToggleBookmarksBar,
    /// Ask the user for a bookmarks file to import, or where to export.
    PickBookmarksFile(BookmarksFile),
    /// Merge the bookmarks of an HTML bookmarks file (`text`); links
    /// outside any folder go to a group named after `source` (the file).
    ImportBookmarks {
        text: String,
        source: String,
    },
    /// Write every bookmark to an HTML bookmarks file at `path`.
    ExportBookmarks(PathBuf),
    /// Open the find bar in `window` (`Ctrl+F`), or give it the focus
    /// again; a search it still holds runs again.
    OpenFind(WindowId),
    /// Close the find bar; the active match stays selected in the page.
    CloseFind(WindowId),
    /// The bar's text or its case switch changed: search the active tab
    /// afresh from the top (nothing is highlighted for empty text).
    FindText {
        window: WindowId,
        text: String,
        match_case: bool,
    },
    FindNext(WindowId),
    FindPrevious(WindowId),
    /// Zoom the page a step in, out, or back to the default; every tab
    /// on the same host follows, and the site remembers the level.
    ZoomIn(TabId),
    ZoomOut(TabId),
    ZoomReset(TabId),
    /// Zoom the page to `percent` (clamped to the step table's range).
    SetZoom {
        tab: TabId,
        percent: u16,
    },
    /// Show or hide `window`'s downloads popover (`Ctrl+J`).
    ToggleDownloads(WindowId),
    CancelDownload(DownloadId),
    PauseDownload(DownloadId),
    ResumeDownload(DownloadId),
    /// Download the file again from its address, in `window`'s active
    /// tab, in place of the old entry.
    RetryDownload {
        window: WindowId,
        id: DownloadId,
    },
    /// Open the finished file with its default application.
    OpenDownload(DownloadId),
    /// Show the finished file in the system's file manager.
    RevealDownload(DownloadId),
    /// Forget a download that is not going.
    RemoveDownload(DownloadId),
    /// Forget every download that is not going.
    ClearDownloads,
    OpenDownloadsFolder,
    /// Ask for the downloads directory with the system's folder dialog.
    PickDownloadsDirectory,
    /// Downloads go to `directory` from now on (`None`: the system's
    /// Downloads folder).
    SetDownloadsDirectory(Option<PathBuf>),
    /// Quit although downloads are going (the confirmation's answer).
    ForceQuit,
    /// Bring back the session the last run left behind
    /// (`Browser::crashed_session`); `window`, the one the offer was
    /// shown in, closes when it shows nothing but its first page.
    RestoreSession(WindowId),
    /// Decline that offer.
    DismissRestore,
    /// Load a restored, deferred tab in the engine out of sight, for
    /// its title and favicon (`Tab::warming`); the strip goes on drawing
    /// it faint. The shell sends one for each tab of
    /// `EngineAction::WarmTabs`, a moment apart.
    WarmTab(TabId),
    /// The warm of a tab is over, its title and favicon in or not: the
    /// engine's tab goes and the model's stays, deferred.
    WarmDone(TabId),
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
    /// Look for a newer build now (the menu entry, the settings window's
    /// button). Nothing while a check or a download is already under
    /// way.
    CheckForUpdates,
    /// Quit and install the update that is ready (the "Restart to
    /// update" button). Nothing unless one is.
    InstallUpdate,
    /// Clear the update failure shown in the status area.
    DismissUpdateFailure,
    /// Ask the system to make this browser the default for web links
    /// (the offer bar's and the settings window's button). The system
    /// asks the user to confirm; the shell reports the outcome through
    /// `Browser::default_browser`. Nothing unless it is not the default.
    MakeDefaultBrowser,
    /// Hide the offer to become the default browser until the next start
    /// ("Not now").
    DismissDefaultBrowser,
    /// Stop offering to become the default browser ("Don't ask again"):
    /// `Settings::check_default_browser` off.
    NeverAskDefaultBrowser,
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
    /// A question typed into the Launch Panel's field of `kind`
    /// (`AskClaude` or `AskChatGpt`): open that template's URL for
    /// `query` in a new tab of `window`.
    AskInNewTab {
        window: WindowId,
        kind: WidgetKind,
        query: String,
    },
    /// Put a tile of the Launch Panel at `index` among the others
    /// (0 first; the count of the others last).
    MoveWidget {
        kind: WidgetKind,
        index: usize,
    },
    /// Take a tile off the Launch Panel.
    RemoveWidget(WidgetKind),
    /// The default tiles in the default order again.
    RestoreWidgets,
    /// Keep a host off the "Most visited" tile.
    HideTopSite(String),
    /// The address bar's search engine (the Launch Settings' choice).
    SetSearch(UrlTemplate),
    /// The URL template of a question tile.
    SetAskTemplate {
        kind: WidgetKind,
        template: UrlTemplate,
    },
    /// Find places named `query` for the weather tile.
    LookUpPlace(String),
    /// The weather tile's place; `None` asks for one again.
    SetWeatherPlace(Option<Place>),
    SetWeatherUnits(Units),
    /// Sign in to Google for the Drive tile, through a tab of `window`.
    ConnectGoogle(WindowId),
    /// Forget the Google account.
    DisconnectGoogle,
    /// Fetch the tiles' data again now (a tile's Retry).
    RefreshLaunch,
    /// Ask the user to confirm blocking the sites of these tabs on this
    /// profile (the tab menu's "Block on this profile"); sites already
    /// blocked are left out of the question.
    PromptBlockSites {
        window: WindowId,
        tabs: Vec<TabId>,
    },
    /// Block these sites (registrable domains) on this profile: every
    /// open tab on one shows the block page from now on.
    BlockSites(Vec<String>),
    /// Unblock `site`; the tabs blocked on it return to their pages.
    /// Only the settings window issues it.
    UnblockSite(String),
    /// The user's answer to `EngineAction::AskMediaAccess`: `tab`'s page
    /// may, or may not, have what `request` asks for. An allowed origin is not
    /// asked again in this run.
    AnswerMediaAccess {
        tab: TabId,
        request: MediaRequest,
        allow: bool,
    },
    /// Close the side panel `window` shows: the active tab's, else the
    /// window's (the panel's close control).
    CloseSidePanel {
        window: WindowId,
    },
    /// Run `extension`'s keyboard command `name` for `window`'s active
    /// tab (a chord the extension declared and the keymap does not
    /// hold).
    ExtensionCommand {
        window: WindowId,
        extension: String,
        name: String,
    },
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

impl Command {
    /// Whether this command writes the bookmarks: the shell reads shared
    /// bookmarks again first, so another browser's additions survive.
    pub fn changes_bookmarks(&self) -> bool {
        matches!(
            self,
            Command::BookmarkTabs { .. }
                | Command::ToggleBookmark(_)
                | Command::EditBookmark { .. }
                | Command::RemoveBookmark(_)
                | Command::RenameBookmarkGroup { .. }
                | Command::DeleteBookmarkGroup(_)
                | Command::MoveBookmark { .. }
                | Command::ImportBookmarks { .. }
        )
    }
}

/// Which way a bookmarks file goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookmarksFile {
    Import,
    Export,
}

/// What the profile dialog edits: the profile's name, colour and
/// avatar, and what it takes from the default profile. `share_bookmarks`
/// is a lasting setting (the profile reads and writes the default
/// profile's bookmarks); `copy_extensions` is a one-time action (the
/// default profile's extensions are installed here, and stay
/// independent after). Neither means anything for the default profile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileForm {
    pub name: String,
    pub color: ChromeColor,
    pub avatar: Avatar,
    pub share_bookmarks: bool,
    pub copy_extensions: bool,
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
    /// Configure a profile: `form` is prefilled (the current profile's
    /// values, or a new profile's defaults when `new`); `deletable`
    /// says the dialog may offer to delete the current profile.
    Profile {
        form: ProfileForm,
        new: bool,
        deletable: bool,
    },
    /// Confirm deleting the profile in directory `profile`, shown as
    /// `name`; `current` when it is the one this browser runs.
    DeleteProfile {
        profile: String,
        name: String,
        current: bool,
    },
    /// Every profile, to open, edit or delete: this browser's
    /// (`current`, a guest when `guest`: neither edited nor deleted)
    /// and the `others` as the Profiles menu lists them.
    ManageProfiles {
        current: ProfileEntry,
        guest: bool,
        others: Vec<ProfileEntry>,
    },
    /// The bookmark popover under the star: the bookmark of `url`, its
    /// `title` and `group` as they are, the existing `groups` to pick
    /// from, and whether the star just `added` it ("Bookmark added"
    /// rather than "Edit bookmark").
    Bookmark {
        url: Url,
        title: String,
        group: String,
        groups: Vec<String>,
        added: bool,
    },
    RenameBookmarkGroup {
        group: String,
    },
    /// Confirm deleting the group and its `count` bookmarks.
    DeleteBookmarkGroup {
        group: String,
        count: usize,
    },
    /// `count` downloads are still going: quit anyway?
    QuitWithDownloads {
        count: usize,
    },
    /// Confirm blocking these sites on this profile.
    BlockSites {
        sites: Vec<String>,
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
    /// Warm `tabs`, restored and deferred, in this order and a moment
    /// apart: a `Command::WarmTab` for each, so the strip fills with
    /// their titles and favicons without every page loading at once,
    /// and a `Command::WarmDone` once each has them, or has had long
    /// enough. The shell keeps the time.
    WarmTabs(Vec<TabId>),
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
    /// Fetch the brand's update manifest, compare it with this build,
    /// and download the asset for this platform; the shell reports what
    /// happens through `Browser::update`.
    CheckForUpdates,
    /// Install the downloaded update, verified at `file`, and start the
    /// new build: the shell arranges the installation to run once this
    /// process has exited, then quits as `Quit` does.
    InstallUpdate {
        update: AvailableUpdate,
        file: std::path::PathBuf,
    },
    /// Ask the system to make this browser the default for web links;
    /// the shell reports the answer as `Browser::default_browser`.
    MakeDefaultBrowser,
    /// Start a `<binary> --profile <profile> <urls>` process.
    OpenInProfile {
        profile: String,
        urls: Vec<Url>,
    },
    /// Make a guest profile directory (marked as one, named "Guest")
    /// and start a browser for it, which deletes it on its way out.
    OpenGuestProfile,
    /// Make a profile directory for `form` (the brand's defaults with
    /// the form's name, colour, avatar and flags) and start a browser
    /// for it.
    CreateProfile(ProfileForm),
    /// Delete the profile in directory `profile`: another profile's
    /// directory now, or, for the current profile, start the default
    /// profile, quit, and delete the directory on the way out.
    DeleteProfile(String),
    /// The profile's bookmarks now come from the default profile
    /// (`shared`) or from its own file again: reload them from there,
    /// or keep a copy of the shared ones as its own.
    SwitchBookmarks {
        shared: bool,
    },
    /// Install the default profile's extensions that this profile lacks.
    CopyExtensionsFromDefault,
    /// The settings changed (pinned extensions): persist them and redraw.
    SaveSettings,
    /// The bookmarks changed: persist them.
    SaveBookmarks,
    /// Show the system's file dialog for a bookmarks file; the answer is
    /// `Command::ImportBookmarks` or `Command::ExportBookmarks`.
    PickFile(BookmarksFile),
    /// Write the bookmarks as an HTML bookmarks file at `path`.
    ExportBookmarks(PathBuf),
    /// Search `tab`'s page for `text`: afresh from the top, or on to the
    /// next match (`find_next`) `forward` or back, wrapping at the ends.
    /// The engine highlights the matches and answers with
    /// `EngineEvent::FindResult`s.
    Find {
        tab: TabId,
        text: String,
        forward: bool,
        match_case: bool,
        find_next: bool,
    },
    /// End the search in `tab`: drop the highlights, and the selection of
    /// the active match too when `clear_selection`.
    StopFinding {
        tab: TabId,
        clear_selection: bool,
    },
    /// Put the keyboard focus in `window`'s find bar.
    FocusFind(WindowId),
    /// Zoom `tab`'s page to `percent`.
    SetZoom {
        tab: TabId,
        percent: u16,
    },
    /// The sites' zoom levels changed: persist them.
    SaveZoom,
    /// The profile's blocked sites, for the engine's before-navigate hook:
    /// at start and after every change.
    ConfigureBlockedSites(BlockedSites),
    /// Replace `tab`'s current history entry with `about:blank`: the page
    /// it shows is blocked now, and Back should skip it.
    ReplaceWithBlank(TabId),
    /// The blocked sites changed: persist them.
    SaveBlockedSites,
    /// Show or hide `window`'s downloads popover.
    ToggleDownloads(WindowId),
    /// To the engine, by its own id for the download.
    CancelDownload(u32),
    PauseDownload(u32),
    ResumeDownload(u32),
    /// Download `url` in `tab` (a retry).
    StartDownload {
        tab: TabId,
        url: Url,
    },
    /// Open the file with its default application.
    OpenPath(PathBuf),
    /// Show the file in the system's file manager.
    RevealPath(PathBuf),
    /// Open the downloads directory in the file manager.
    OpenDownloadsFolder,
    /// The downloads list changed in a way worth keeping (a state, not
    /// progress): persist it.
    SaveDownloads,
    /// Show the system's folder dialog for the downloads directory; the
    /// answer is `Command::SetDownloadsDirectory`.
    PickFolder,
    /// The window about to be opened (`OpenWindow` follows) goes where
    /// it was last time. Sent before the window exists.
    PlaceWindow {
        window: WindowId,
        geometry: WindowGeometry,
    },
    /// The history changed: persist it (the shell may wait a moment and
    /// save once for a run of changes).
    SaveHistory,
    /// Show the new-tab launcher over `window`: a large field in which
    /// the user types an address or a search, or picks a page from the
    /// history. Nothing is opened until they do; cancelling opens
    /// nothing.
    ShowLauncher {
        window: WindowId,
    },
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
    /// Ask the user, in a dialog of the system's over `window`, whether
    /// `tab`'s page may have what `request` asks for; the answer is
    /// `Command::AnswerMediaAccess`, and a dialog closed any other way
    /// is a refusal. Comes from `apply`, not from a command.
    AskMediaAccess {
        window: WindowId,
        tab: TabId,
        request: MediaRequest,
    },
    /// Answer the media request the engine reported for `tab` as
    /// `request` (`MediaRequest::id`).
    AnswerMediaAccess {
        tab: TabId,
        request: u64,
        allow: bool,
    },
    /// Find places named this for the weather tile; answered with
    /// `LaunchUpdate::Places`.
    LookUpPlace(String),
    /// Ask the system where this device is, which asks the user for
    /// permission the first time. The answer is a `LaunchUpdate::
    /// Location`, and the forecast for it follows.
    FetchLocation {
        /// The units to ask the forecast in, or `None` when nobody has
        /// chosen any: the fetcher then takes them from the country
        /// the fix lands in (`Units::for_country`), which it learns
        /// before the model does.
        units: Option<Units>,
    },
    /// Fetch today's weather at `place`; answered with
    /// `LaunchUpdate::Weather`.
    FetchWeather {
        place: Place,
        units: Units,
    },
    /// Start the Google sign-in: open the consent page in a new tab of
    /// `window` and wait for the redirect; answered with
    /// `LaunchUpdate::GoogleConnected`.
    BeginGoogleSignIn {
        window: WindowId,
    },
    /// Revoke and remove the Google token.
    ForgetGoogle,
    /// Fetch the Drive files the user opened last; answered with
    /// `LaunchUpdate::DriveFiles`.
    FetchDriveRecent,
    /// Tell `extension` its keyboard command `name` ran, with
    /// `window`'s active tab.
    ExtensionCommand {
        window: WindowId,
        extension: String,
        name: String,
    },
    /// The answer to the `EngineEvent::ExtensionRequest` that carried
    /// `token`. Always the last action of that request.
    ExtensionReply {
        token: u64,
        reply: ExtensionReply,
    },
    /// Create the engine view `view` in `window` for a side panel:
    /// `extension`'s `page`. The view is no tab; it is sized, painted
    /// and given input under its id like one.
    CreateView {
        window: WindowId,
        view: TabId,
        extension: String,
        page: String,
    },
    /// Close the engine view `view` of `window`: its side panel is no
    /// longer in the model.
    CloseView {
        window: WindowId,
        view: TabId,
    },
    Quit,
}

/// Apply a command to the model and return what the app must do next.
pub fn dispatch(browser: &mut Browser, cmd: &Command) -> Vec<EngineAction> {
    match cmd {
        Command::NewTab { window, url } => match url {
            Some(url) => new_tab(browser, *window, url.clone()),
            None if browser.window(*window).is_some() => {
                let mut actions = vec![EngineAction::ShowLauncher { window: *window }];
                actions.extend(launch_fetches(browser, false));
                actions
            }
            None => Vec::new(),
        },
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
        Command::OpenProfile(profile) if *profile == browser.profile.name => Vec::new(),
        Command::OpenProfile(profile) => vec![EngineAction::OpenInProfile {
            profile: profile.clone(),
            urls: Vec::new(),
        }],
        Command::OpenGuestProfile => vec![EngineAction::OpenGuestProfile],
        Command::PromptManageProfiles(window) => {
            if browser.window(*window).is_none() {
                return Vec::new();
            }
            let settings = &browser.settings;
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::ManageProfiles {
                    current: ProfileEntry {
                        dir: browser.profile.name.clone(),
                        name: browser.profile_name(),
                        avatar: settings.profile.avatar,
                        color: settings.chrome_color,
                    },
                    guest: browser.profile.guest,
                    others: browser.profiles.clone(),
                },
            }]
        }
        Command::PromptEditProfile(window) => {
            if browser.window(*window).is_none() || browser.profile.guest {
                return Vec::new();
            }
            let settings = &browser.settings;
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::Profile {
                    form: ProfileForm {
                        name: browser.profile_name(),
                        color: settings.chrome_color,
                        avatar: settings.profile.avatar,
                        share_bookmarks: settings.profile.share_bookmarks,
                        copy_extensions: false,
                    },
                    new: false,
                    deletable: !browser.profile.is_default(),
                },
            }]
        }
        Command::PromptAddProfile(window) => {
            if browser.window(*window).is_none() {
                return Vec::new();
            }
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::Profile {
                    form: ProfileForm {
                        // A different colour from the current profile's,
                        // so the two tell apart from the start.
                        color: next_color(browser.settings.chrome_color),
                        ..ProfileForm::default()
                    },
                    new: true,
                    deletable: false,
                },
            }]
        }
        Command::PromptDeleteProfile { window, profile } => {
            let current = *profile == browser.profile.name;
            // A guest's directory goes with its browser; there is
            // nothing to confirm.
            if browser.window(*window).is_none()
                || profile == DEFAULT_PROFILE_NAME
                || (current && browser.profile.guest)
            {
                return Vec::new();
            }
            let name = if current {
                browser.profile_name()
            } else {
                match browser.profiles.iter().find(|p| p.dir == *profile) {
                    Some(entry) => entry.name.clone(),
                    None => return Vec::new(),
                }
            };
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::DeleteProfile {
                    profile: profile.clone(),
                    name,
                    current,
                },
            }]
        }
        Command::SaveProfile(form) => save_profile(browser, form),
        Command::CreateProfile(form) => {
            if form.name.trim().is_empty() {
                return Vec::new();
            }
            let mut form = form.clone();
            form.name = form.name.trim().to_string();
            vec![EngineAction::CreateProfile(form)]
        }
        Command::DeleteProfile(profile) => {
            let current = *profile == browser.profile.name;
            let known = current || browser.profiles.iter().any(|p| p.dir == *profile);
            if profile == DEFAULT_PROFILE_NAME || !known || (current && browser.profile.guest) {
                return Vec::new();
            }
            vec![EngineAction::DeleteProfile(profile.clone())]
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
                actions.extend(new_tab(browser, *window, url));
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
        Command::ToggleBookmark(tab) => toggle_bookmark(browser, *tab),
        Command::PromptEditBookmark { window, url } => {
            let Some((group, bookmark)) = browser.bookmarks.find(url) else {
                return Vec::new();
            };
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::Bookmark {
                    url: url.clone(),
                    title: bookmark.title.clone(),
                    group: group.to_string(),
                    groups: browser.bookmarks.group_names(),
                    added: false,
                },
            }]
        }
        Command::EditBookmark { url, title, group } => {
            browser.bookmarks.update(url, title, group);
            let mut actions = vec![EngineAction::SaveBookmarks];
            let group = match group.trim() {
                "" => Bookmarks::BAR,
                name => name,
            };
            if browser.settings.last_bookmark_group != group {
                browser.settings.last_bookmark_group = group.to_string();
                actions.push(EngineAction::SaveSettings);
            }
            actions
        }
        Command::RemoveBookmark(url) => {
            if browser.bookmarks.remove(url) {
                vec![EngineAction::SaveBookmarks]
            } else {
                Vec::new()
            }
        }
        Command::PromptRenameBookmarkGroup { window, group } => {
            if browser.bookmarks.group(group).is_none() {
                return Vec::new();
            }
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::RenameBookmarkGroup {
                    group: group.clone(),
                },
            }]
        }
        Command::RenameBookmarkGroup { group, name } => {
            if !browser.bookmarks.rename_group(group, name) {
                return Vec::new();
            }
            let mut actions = vec![EngineAction::SaveBookmarks];
            if browser.settings.last_bookmark_group == *group {
                browser.settings.last_bookmark_group = name.trim().to_string();
                actions.push(EngineAction::SaveSettings);
            }
            actions
        }
        Command::PromptDeleteBookmarkGroup { window, group } => {
            let Some(g) = browser.bookmarks.group(group) else {
                return Vec::new();
            };
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::DeleteBookmarkGroup {
                    group: group.clone(),
                    count: g.bookmarks.len(),
                },
            }]
        }
        Command::DeleteBookmarkGroup(group) => {
            if browser.bookmarks.delete_group(group) {
                vec![EngineAction::SaveBookmarks]
            } else {
                Vec::new()
            }
        }
        Command::MoveBookmark { group, from, to } => {
            if browser.bookmarks.move_within(group, *from, *to) {
                vec![EngineAction::SaveBookmarks]
            } else {
                Vec::new()
            }
        }
        Command::ToggleBookmarksBar => {
            browser.settings.show_bookmarks_bar = !browser.settings.show_bookmarks_bar;
            vec![EngineAction::SaveSettings]
        }
        Command::PickBookmarksFile(which) => vec![EngineAction::PickFile(*which)],
        Command::ImportBookmarks { text, source } => {
            let loose = match source.trim() {
                "" => "Imported".to_string(),
                name => format!("Imported from {name}"),
            };
            let imported = bookmarks_html::import(text, &loose);
            if browser.bookmarks.merge(imported) > 0 {
                vec![EngineAction::SaveBookmarks]
            } else {
                Vec::new()
            }
        }
        Command::ExportBookmarks(path) => vec![EngineAction::ExportBookmarks(path.clone())],
        Command::OpenFind(window) => {
            let Some(w) = browser.window_mut(*window) else {
                return Vec::new();
            };
            let state = w.find.get_or_insert_with(FindState::default);
            let mut actions = vec![EngineAction::FocusFind(*window)];
            if !state.text.is_empty() {
                actions.push(EngineAction::Find {
                    tab: w.active,
                    text: state.text.clone(),
                    forward: true,
                    match_case: state.match_case,
                    find_next: false,
                });
            }
            actions
        }
        Command::CloseFind(window) => {
            let Some(w) = browser.window_mut(*window) else {
                return Vec::new();
            };
            if w.find.take().is_none() {
                return Vec::new();
            }
            vec![EngineAction::StopFinding {
                tab: w.active,
                clear_selection: false,
            }]
        }
        Command::FindText {
            window,
            text,
            match_case,
        } => {
            let Some(w) = browser.window_mut(*window) else {
                return Vec::new();
            };
            let Some(state) = w.find.as_mut() else {
                return Vec::new();
            };
            state.text = text.clone();
            state.match_case = *match_case;
            state.current = 0;
            state.total = 0;
            if text.is_empty() {
                vec![EngineAction::StopFinding {
                    tab: w.active,
                    clear_selection: true,
                }]
            } else {
                vec![EngineAction::Find {
                    tab: w.active,
                    text: text.clone(),
                    forward: true,
                    match_case: *match_case,
                    find_next: false,
                }]
            }
        }
        Command::FindNext(window) => find_step(browser, *window, true),
        Command::FindPrevious(window) => find_step(browser, *window, false),
        Command::ZoomIn(tab) => zoom_step(browser, *tab, true),
        Command::ZoomOut(tab) => zoom_step(browser, *tab, false),
        Command::ZoomReset(tab) => {
            let default = browser.settings.page_zoom;
            set_zoom(browser, *tab, default)
        }
        Command::SetZoom { tab, percent } => set_zoom(browser, *tab, *percent),
        Command::ToggleDownloads(window) => {
            if browser.window(*window).is_some() {
                vec![EngineAction::ToggleDownloads(*window)]
            } else {
                Vec::new()
            }
        }
        Command::CancelDownload(id) => engine_download(
            browser,
            *id,
            |d| d.state.is_active(),
            EngineAction::CancelDownload,
        ),
        Command::PauseDownload(id) => engine_download(
            browser,
            *id,
            |d| d.state == DownloadState::InProgress,
            EngineAction::PauseDownload,
        ),
        Command::ResumeDownload(id) => engine_download(
            browser,
            *id,
            |d| d.state == DownloadState::Paused,
            EngineAction::ResumeDownload,
        ),
        Command::RetryDownload { window, id } => {
            let Some(tab) = browser.window(*window).map(|w| w.active) else {
                return Vec::new();
            };
            let Some(url) = browser
                .downloads
                .get(*id)
                .filter(|d| !d.state.is_active())
                .map(|d| d.url.clone())
            else {
                return Vec::new();
            };
            browser.downloads.remove(*id);
            vec![
                EngineAction::SaveDownloads,
                EngineAction::StartDownload { tab, url },
            ]
        }
        Command::OpenDownload(id) => finished_path(browser, *id)
            .map(|path| vec![EngineAction::OpenPath(path)])
            .unwrap_or_default(),
        Command::RevealDownload(id) => finished_path(browser, *id)
            .map(|path| vec![EngineAction::RevealPath(path)])
            .unwrap_or_default(),
        Command::RemoveDownload(id) => {
            if browser.downloads.remove(*id) {
                vec![EngineAction::SaveDownloads]
            } else {
                Vec::new()
            }
        }
        Command::ClearDownloads => {
            if browser.downloads.clear_finished() {
                vec![EngineAction::SaveDownloads]
            } else {
                Vec::new()
            }
        }
        Command::OpenDownloadsFolder => vec![EngineAction::OpenDownloadsFolder],
        Command::PickDownloadsDirectory => vec![EngineAction::PickFolder],
        Command::SetDownloadsDirectory(directory) => {
            if browser.settings.downloads.directory == *directory {
                return Vec::new();
            }
            browser.settings.downloads.directory = directory.clone();
            vec![EngineAction::SaveSettings]
        }
        Command::ForceQuit => {
            browser.quit_confirmed = true;
            vec![EngineAction::Quit]
        }
        Command::RestoreSession(window) => {
            let Some(session) = browser.crashed_session.take() else {
                return Vec::new();
            };
            let mut actions = restore_session(browser, &session);
            // The window the offer was in gives way when it shows only
            // the page it started on.
            if browser.windows.len() > 1
                && browser.window(*window).is_some_and(|w| w.tabs.len() == 1)
            {
                actions.extend(close_window(browser, *window));
            }
            actions
        }
        Command::DismissRestore => {
            browser.crashed_session = None;
            Vec::new()
        }
        Command::WarmTab(tab) => warm_tab(browser, *tab),
        Command::WarmDone(tab) => warm_done(browser, *tab),
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
                Some(url) => new_tab(browser, *window, url),
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
        Command::Navigate { tab, url } => {
            let page = block_for(browser, url);
            match browser.tab_mut(*tab) {
                Some(t) => {
                    t.url = url.clone();
                    t.load = LoadState::Loading;
                    t.crashed = None;
                    // A blocked destination: the engine loads the blank
                    // page (a push, so Back returns to the page before),
                    // and the chrome paints the block page over it.
                    let load = match page {
                        Some(page) => {
                            t.block(page);
                            about_blank()
                        }
                        None => {
                            t.blocked = None;
                            url.clone()
                        }
                    };
                    vec![EngineAction::Navigate {
                        tab: *tab,
                        url: load,
                    }]
                }
                None => Vec::new(),
            }
        }
        Command::Back(tab) => history_step(browser, *tab, EngineAction::Back(*tab)),
        Command::Forward(tab) => history_step(browser, *tab, EngineAction::Forward(*tab)),
        Command::Reload(tab) => match browser.tab_mut(*tab) {
            // A block page reloads into another of its lines; the engine
            // is asked nothing.
            Some(t) if t.blocked.is_some() => {
                if let Some(page) = t.blocked.as_mut() {
                    page.reroll(line_seed());
                }
                Vec::new()
            }
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
            if browser.windows.len() == 1
                && browser.window(*window).is_some()
                && let Some(ask) = quit_guard(browser, Some(*window))
            {
                return ask;
            }
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
        Command::CheckForUpdates => {
            if browser.update.can_check() {
                browser.update = UpdateStatus::Checking;
                vec![EngineAction::CheckForUpdates]
            } else {
                Vec::new()
            }
        }
        Command::InstallUpdate => match &browser.update {
            UpdateStatus::Ready { update, file } => {
                let (update, file) = (update.clone(), file.clone());
                browser.update = UpdateStatus::Installing(update.clone());
                vec![EngineAction::InstallUpdate { update, file }]
            }
            _ => Vec::new(),
        },
        Command::DismissUpdateFailure => {
            if matches!(browser.update, UpdateStatus::Failed(_)) {
                browser.update = UpdateStatus::Idle;
            }
            Vec::new()
        }
        Command::MakeDefaultBrowser => {
            if browser.default_browser != DefaultBrowser::No {
                return Vec::new();
            }
            // The system's confirmation takes over from the bar; the
            // answer comes back as a new status either way.
            browser.default_browser_dismissed = true;
            vec![EngineAction::MakeDefaultBrowser]
        }
        Command::DismissDefaultBrowser => {
            browser.default_browser_dismissed = true;
            Vec::new()
        }
        Command::NeverAskDefaultBrowser => {
            browser.default_browser_dismissed = true;
            if browser.settings.check_default_browser {
                browser.settings.check_default_browser = false;
                vec![EngineAction::SaveSettings]
            } else {
                Vec::new()
            }
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
            Some(url) => new_tab(browser, *window, url),
            None => Vec::new(),
        },
        Command::AskInNewTab {
            window,
            kind,
            query,
        } => match browser
            .settings
            .launch
            .ask_template(*kind)
            .and_then(|t| t.url_for(query))
        {
            Some(url) => new_tab(browser, *window, url),
            None => Vec::new(),
        },
        Command::MoveWidget { kind, index } => {
            save_if(browser.settings.launch.move_widget(*kind, *index))
        }
        Command::RemoveWidget(kind) => save_if(browser.settings.launch.remove_widget(*kind)),
        Command::RestoreWidgets => save_if(browser.settings.launch.restore_widgets()),
        Command::HideTopSite(host) => save_if(browser.settings.launch.hide_site(host)),
        Command::SetSearch(template) => {
            if browser.settings.search == *template {
                return Vec::new();
            }
            browser.settings.search = template.clone();
            vec![EngineAction::SaveSettings]
        }
        Command::SetAskTemplate { kind, template } => {
            match browser.settings.launch.ask_template_mut(*kind) {
                Some(current) if current != template => {
                    *current = template.clone();
                    vec![EngineAction::SaveSettings]
                }
                _ => Vec::new(),
            }
        }
        Command::LookUpPlace(query) => {
            let query = query.trim();
            if query.is_empty() {
                return Vec::new();
            }
            browser.launch.places = Fetched::Loading;
            vec![EngineAction::LookUpPlace(query.to_string())]
        }
        Command::SetWeatherPlace(place) => {
            browser.settings.launch.weather.location = place.clone();
            browser.launch.places = Fetched::Empty;
            browser.launch.weather = Fetched::Empty;
            let mut actions = vec![EngineAction::SaveSettings];
            actions.extend(launch_fetches(browser, true));
            actions
        }
        Command::SetWeatherUnits(units) => {
            if browser.settings.launch.weather.units == Some(*units) {
                return Vec::new();
            }
            // Choosing what was being followed anyway is still a
            // choice: it stops the tile changing units when the device
            // moves.
            let followed = browser
                .settings
                .launch
                .weather
                .units(&browser.launch.located);
            browser.settings.launch.weather.units = Some(*units);
            if followed == *units {
                return vec![EngineAction::SaveSettings];
            }
            browser.launch.weather = Fetched::Empty;
            let mut actions = vec![EngineAction::SaveSettings];
            actions.extend(launch_fetches(browser, true));
            actions
        }
        Command::ConnectGoogle(window) => {
            if browser.launch.drive != DriveState::Disconnected || browser.window(*window).is_none()
            {
                return Vec::new();
            }
            browser.launch.drive = DriveState::Connecting;
            vec![EngineAction::BeginGoogleSignIn { window: *window }]
        }
        Command::DisconnectGoogle => match browser.launch.drive {
            DriveState::Unavailable | DriveState::Disconnected => Vec::new(),
            _ => {
                browser.launch.drive = DriveState::Disconnected;
                vec![EngineAction::ForgetGoogle]
            }
        },
        Command::RefreshLaunch => launch_fetches(browser, true),
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
        Command::PromptBlockSites { window, tabs } => {
            if browser.window(*window).is_none() {
                return Vec::new();
            }
            let urls = tabs
                .iter()
                .filter_map(|id| browser.tab(*id))
                .map(|t| &t.url);
            let sites = browser.blocked.sites_to_block(urls);
            if sites.is_empty() {
                return Vec::new();
            }
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::BlockSites { sites },
            }]
        }
        Command::BlockSites(sites) => block_sites(browser, sites),
        Command::UnblockSite(site) => unblock_site(browser, site),
        Command::AnswerMediaAccess {
            tab,
            request,
            allow,
        } => {
            if browser.tab(*tab).is_none() {
                return Vec::new();
            }
            if *allow {
                browser.media_grants.allow(request);
            }
            vec![EngineAction::AnswerMediaAccess {
                tab: *tab,
                request: request.id,
                allow: *allow,
            }]
        }
        Command::CloseSidePanel { window } => close_shown_panel(browser, *window),
        Command::ExtensionCommand {
            window,
            extension,
            name,
        } => {
            let declared = browser.window(*window).is_some()
                && browser
                    .extension(extension)
                    .is_some_and(|e| e.has_command(name));
            if declared {
                vec![EngineAction::ExtensionCommand {
                    window: *window,
                    extension: extension.clone(),
                    name: name.clone(),
                }]
            } else {
                Vec::new()
            }
        }
        Command::Quit => quit_guard(browser, None).unwrap_or_else(|| vec![EngineAction::Quit]),
    }
}

/// Open the windows of `session`: each with its active tab loading and
/// its other tabs deferred until shown, its folders back by name, and a
/// `PlaceWindow` before its `OpenWindow` when its place is known. The
/// closed-tab list comes back too. Last comes a `WarmTabs` naming the
/// deferred tabs in strip order, window by window, for their titles
/// and favicons.
pub fn restore_session(browser: &mut Browser, session: &Session) -> Vec<EngineAction> {
    let mut actions = Vec::new();
    let mut warm = Vec::new();
    for sw in &session.windows {
        if sw.tabs.is_empty() {
            continue;
        }
        let active = sw.active.min(sw.tabs.len() - 1);
        let opened = open_window(browser, sw.tabs[active].url.clone());
        let window = browser
            .windows
            .last()
            .expect("open_window pushed a window")
            .id;
        if let Some(geometry) = sw.geometry {
            actions.push(EngineAction::PlaceWindow { window, geometry });
        }
        actions.extend(opened);
        let folder_ids: Vec<(String, FolderId)> = sw
            .folders
            .iter()
            .map(|f| (f.name.clone(), browser.allocate_folder_id()))
            .collect();
        let ids: Vec<TabId> = sw.tabs.iter().map(|_| browser.allocate_tab_id()).collect();
        // Deferred tabs on a blocked site are marked now so the strip
        // can say so; they are re-checked when shown.
        let marks: Vec<Option<BlockedPage>> = sw
            .tabs
            .iter()
            .map(|st| block_for(browser, &st.url))
            .collect();
        let w = browser.window_mut(window).expect("just opened");
        for (f, (_, id)) in sw.folders.iter().zip(&folder_ids) {
            w.folders.push(Folder {
                id: *id,
                name: f.name.clone(),
                expanded: f.expanded,
            });
        }
        let created = w.tabs.remove(0);
        let mut tabs = Vec::with_capacity(sw.tabs.len());
        for (i, st) in sw.tabs.iter().enumerate() {
            let mut t = if i == active {
                created.clone()
            } else {
                let mut t = Tab::new(ids[i], st.url.clone());
                t.deferred = true;
                if let Some(page) = marks[i].clone() {
                    t.block(page);
                }
                t
            };
            // A blocked tab shows its address, not the title it had.
            if t.blocked.is_none() {
                t.title = st.title.clone();
            }
            t.folder = st.folder.as_ref().and_then(|name| {
                folder_ids
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, id)| *id)
            });
            tabs.push(t);
        }
        w.tabs = tabs;
        w.active = created.id;
        w.select_only(created.id);
        w.prune_folders();
        warm.extend(
            w.tabs
                .iter()
                .filter(|t| t.deferred && t.blocked.is_none())
                .map(|t| t.id),
        );
    }
    for url in &session.closed_tabs {
        browser.remember_closed(url.clone());
    }
    if !warm.is_empty() {
        actions.push(EngineAction::WarmTabs(warm));
    }
    actions
}

/// Load `tab`, a deferred one, in the engine out of sight for its title
/// and favicon: created hidden, it stays deferred (faint, no throbber)
/// until [`warm_done`] lets the engine's tab go again or the user shows
/// it first. Nothing for a tab that is not deferred, is warming
/// already, or is gone.
fn warm_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    // A tab on a blocked site is not warmed: nothing is requested from
    // the site, and the shell moves on to the next in line.
    if browser
        .tab(tab)
        .is_some_and(|t| browser.blocked.blocks(&t.url).is_some())
    {
        return Vec::new();
    }
    match browser.tab_mut(tab) {
        Some(t) if t.deferred && !t.warming => {
            t.warming = true;
            vec![EngineAction::CreateHiddenTab {
                window,
                tab,
                url: t.url.clone(),
            }]
        }
        _ => Vec::new(),
    }
}

/// The warm of `tab` is over: the engine's tab goes, the model's stays
/// deferred with whatever title and favicon arrived. Nothing unless the
/// tab is warming.
fn warm_done(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    if end_warm(browser, tab) {
        vec![EngineAction::CloseTab(tab)]
    } else {
        Vec::new()
    }
}

/// Take `tab` out of its warm, whichever way it ends: deferred as
/// before, idle, with whatever arrived. Whether it was warming at all.
pub(crate) fn end_warm(browser: &mut Browser, tab: TabId) -> bool {
    match browser.tab_mut(tab) {
        Some(t) if t.warming => {
            t.warming = false;
            t.load = LoadState::Idle;
            true
        }
        _ => false,
    }
}

/// Show `tab` in `w`: a deferred tab is created in the engine now (it
/// comes up visible), unless a warm has it there already, in which
/// case it is shown like any other. `mark` is the block page a deferred
/// tab wakes on when its site is blocked (`wake_mark`): it is created at
/// `about:blank` behind it.
fn show_tab(w: &mut Window, tab: TabId, mark: Option<BlockedPage>) -> EngineAction {
    let window = w.id;
    match w.tab_mut(tab) {
        Some(t) if t.deferred => {
            t.deferred = false;
            match mark {
                Some(page) => t.block(page),
                None => t.blocked = None,
            }
            if std::mem::take(&mut t.warming) {
                EngineAction::Show(tab)
            } else {
                let url = if t.blocked.is_some() {
                    about_blank()
                } else {
                    t.url.clone()
                };
                EngineAction::CreateTab { window, tab, url }
            }
        }
        _ => EngineAction::Show(tab),
    }
}

/// Pass a download command on to the engine when the download is in a
/// state it applies to and the engine still knows it.
fn engine_download(
    browser: &Browser,
    id: DownloadId,
    applies: impl Fn(&Download) -> bool,
    action: fn(u32) -> EngineAction,
) -> Vec<EngineAction> {
    match browser.downloads.get(id) {
        Some(d) if applies(d) => d.engine_id.map(action).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The file of a finished download, to open or reveal.
fn finished_path(browser: &Browser, id: DownloadId) -> Option<PathBuf> {
    let d = browser.downloads.get(id)?;
    if d.state != DownloadState::Complete {
        return None;
    }
    d.path.clone()
}

/// Quitting (or closing the last window or tab) with downloads going
/// asks first, in `window` or the first one; the prompt's answer is
/// `Command::ForceQuit`. `None` when nothing stands in the way.
fn quit_guard(browser: &Browser, window: Option<WindowId>) -> Option<Vec<EngineAction>> {
    let count = browser.downloads.active();
    if count == 0 || browser.quit_confirmed {
        return None;
    }
    let window = window.or_else(|| browser.windows.first().map(|w| w.id))?;
    Some(vec![EngineAction::ShowPrompt {
        window,
        prompt: Prompt::QuitWithDownloads { count },
    }])
}

/// Pin or unpin `extension` in the settings. Returns true if the settings
/// changed.
/// Apply the profile dialog to the current profile's settings. The name,
/// colour and avatar are saved; sharing the bookmarks, when it changed,
/// has the shell switch the bookmarks' file; copying the extensions is
/// an action, not a setting. The default profile has nothing to share
/// or copy.
fn save_profile(browser: &mut Browser, form: &ProfileForm) -> Vec<EngineAction> {
    let is_default = browser.profile.is_default();
    let settings = &mut browser.settings;
    settings.profile.name = form.name.trim().to_string();
    settings.profile.avatar = form.avatar;
    settings.chrome_color = form.color;
    let share = form.share_bookmarks && !is_default;
    let share_changed = settings.profile.share_bookmarks != share;
    settings.profile.share_bookmarks = share;
    let mut actions = vec![EngineAction::SaveSettings];
    if share_changed {
        actions.push(EngineAction::SwitchBookmarks { shared: share });
    }
    if form.copy_extensions && !is_default {
        actions.push(EngineAction::CopyExtensionsFromDefault);
    }
    actions
}

/// The same shade of the next hue after `color`'s, wrapping: a new
/// profile looks like a sibling of the current one, not a copy.
fn next_color(color: ChromeColor) -> ChromeColor {
    let families = ChromeColor::FAMILIES;
    let (family, shade) = families
        .iter()
        .enumerate()
        .find_map(|(f, shades)| shades.iter().position(|c| *c == color).map(|s| (f, s)))
        .unwrap_or((0, 0));
    families[(family + 1) % families.len()][shade]
}

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
/// left empty goes. Returns the folder, or `None` when nothing was done
/// (no such tab, or a folder of another window).
fn move_tabs_to_folder(
    browser: &mut Browser,
    tabs: &[TabId],
    target: &FolderTarget,
) -> Option<FolderId> {
    let window = tabs.iter().find_map(|t| browser.window_of_tab(*t))?;
    let folder = match target {
        FolderTarget::Existing(id) => {
            if browser.window_of_folder(*id) != Some(window) {
                return None;
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
    Some(folder)
}

/// The star's command: bookmark `tab`'s page in the last-used group
/// unless it is bookmarked already, then ask the popover to show it.
/// Pages with nothing to bookmark (`about:blank`) are left alone.
fn toggle_bookmark(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    let t = browser.tab(tab).expect("window_of_tab");
    if !Bookmarks::bookmarkable(&t.url) {
        return Vec::new();
    }
    let url = t.url.clone();
    let page_title = t.display_title().to_string();
    let mut actions = Vec::new();
    let (group, added) = match browser.bookmarks.find(&url) {
        Some((group, _)) => (group.to_string(), false),
        None => {
            let group = match browser.settings.last_bookmark_group.trim() {
                "" => Bookmarks::BAR.to_string(),
                name => name.to_string(),
            };
            browser.bookmarks.add(
                &group,
                vec![Bookmark {
                    title: page_title.clone(),
                    url: url.clone(),
                }],
            );
            actions.push(EngineAction::SaveBookmarks);
            (group, true)
        }
    };
    let title = browser
        .bookmarks
        .find(&url)
        .map(|(_, b)| b.title.clone())
        .unwrap_or(page_title);
    actions.push(EngineAction::ShowPrompt {
        window,
        prompt: Prompt::Bookmark {
            url,
            title,
            group,
            groups: browser.bookmarks.group_names(),
            added,
        },
    });
    actions
}

/// Bookmark the pages of `tabs` (those that exist) in `group`, which is
/// made if new; an empty name means the bar.
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
        "" => Bookmarks::BAR,
        name => name,
    };
    browser.bookmarks.add(group, bookmarks);
    vec![EngineAction::SaveBookmarks]
}

/// Add a window to the model whose first tab shows `url`. Used by
/// `Command::NewWindow` and by `main` for the initial window, which opens
/// the homepage or a URL given on the command line.
/// The block page a navigation to `url` lands on, if its site is blocked
/// on this profile.
fn block_for(browser: &Browser, url: &Url) -> Option<BlockedPage> {
    browser
        .blocked
        .blocks(url)
        .map(|site| BlockedPage::new(site, line_seed()))
}

/// A new tab at `url`, and what the engine loads for it: the page, or
/// `about:blank` behind the block page when the site is blocked. The
/// tab keeps `url` either way, so the address bar says where the user
/// meant to go.
fn tab_at(browser: &Browser, id: TabId, url: Url) -> (Tab, Url) {
    let mut tab = Tab::new(id, url);
    let load = match block_for(browser, &tab.url) {
        Some(page) => {
            tab.block(page);
            about_blank()
        }
        None => tab.url.clone(),
    };
    (tab, load)
}

/// The block page a deferred tab shows when it wakes, if its site is
/// blocked now (the session file records no block, and a site may have
/// been blocked or unblocked while the tab slept).
fn wake_mark(browser: &Browser, tab: TabId) -> Option<BlockedPage> {
    browser
        .tab(tab)
        .filter(|t| t.deferred)
        .and_then(|t| block_for(browser, &t.url))
}

pub fn open_window(browser: &mut Browser, url: Url) -> Vec<EngineAction> {
    let window = browser.allocate_window_id();
    let tab = browser.allocate_tab_id();
    let (t, load) = tab_at(browser, tab, url);
    browser.windows.push(Window::with_tab(window, t));
    vec![EngineAction::OpenWindow {
        window,
        tab,
        url: load,
    }]
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

/// Block `sites` on this profile. Every tab on one of them, in every
/// window, shows the block page from now on: a live tab has its history
/// entry replaced with `about:blank` (so Back returns to the page before
/// it), a deferred tab is only marked and wakes on the block page, and a
/// tab warming out of sight has its engine tab closed. Then the file is
/// saved and the engine told. Nothing when no site is new.
fn block_sites(browser: &mut Browser, sites: &[String]) -> Vec<EngineAction> {
    let mut changed = false;
    for site in sites {
        changed |= browser.blocked.add(site);
    }
    if !changed {
        return Vec::new();
    }
    let mut actions = Vec::new();
    for t in browser.windows.iter_mut().flat_map(|w| w.tabs.iter_mut()) {
        if t.blocked.is_some() {
            continue;
        }
        let Some(site) = browser.blocked.blocks(&t.url) else {
            continue;
        };
        t.block(BlockedPage::new(site, line_seed()));
        if t.deferred {
            if std::mem::take(&mut t.warming) {
                t.load = LoadState::Idle;
                actions.push(EngineAction::CloseTab(t.id));
            }
        } else {
            actions.push(EngineAction::ReplaceWithBlank(t.id));
        }
    }
    actions.push(EngineAction::SaveBlockedSites);
    actions.push(EngineAction::ConfigureBlockedSites(browser.blocked.clone()));
    actions
}

/// Unblock `site`: every tab blocked on it goes back to its own page
/// (a deferred one simply loses the mark), the file is saved and the
/// engine told. Nothing for a site that was not blocked.
fn unblock_site(browser: &mut Browser, site: &str) -> Vec<EngineAction> {
    if !browser.blocked.remove(site) {
        return Vec::new();
    }
    // The engine learns the new list before any tab navigates, so its
    // before-navigate hook does not cancel the navigations with the old one.
    let mut actions = vec![
        EngineAction::SaveBlockedSites,
        EngineAction::ConfigureBlockedSites(browser.blocked.clone()),
    ];
    for t in browser.windows.iter_mut().flat_map(|w| w.tabs.iter_mut()) {
        if t.blocked.as_ref().is_some_and(|p| p.site == site) {
            t.blocked = None;
            if !t.deferred {
                t.load = LoadState::Loading;
                actions.push(EngineAction::Navigate {
                    tab: t.id,
                    url: t.url.clone(),
                });
            }
        }
    }
    actions
}

/// Zoom `tab`'s page a step in or out.
fn zoom_step(browser: &mut Browser, tab: TabId, up: bool) -> Vec<EngineAction> {
    let Some(t) = browser.tab(tab) else {
        return Vec::new();
    };
    let next = next_step(t.zoom, up);
    set_zoom(browser, tab, next)
}

/// Zoom `tab`'s page, and every other tab on its host, to `percent`;
/// the site remembers the level (the default as no entry). A page
/// without a host (`about:blank`) zooms alone and is not remembered.
fn set_zoom(browser: &mut Browser, tab: TabId, percent: u16) -> Vec<EngineAction> {
    let Some(t) = browser.tab(tab) else {
        return Vec::new();
    };
    let percent = percent.clamp(ZOOM_MIN, ZOOM_MAX);
    let default = browser.settings.page_zoom;
    let mut actions = Vec::new();
    match site_key(&t.url) {
        Some(host) => {
            if browser.zoom.set(&host, percent, default) {
                actions.push(EngineAction::SaveZoom);
            }
            for t in browser.windows.iter_mut().flat_map(|w| w.tabs.iter_mut()) {
                if site_key(&t.url).as_deref() == Some(host.as_str()) && t.zoom != percent {
                    t.zoom = percent;
                    actions.push(EngineAction::SetZoom { tab: t.id, percent });
                }
            }
        }
        None => {
            if let Some(t) = browser.tab_mut(tab)
                && t.zoom != percent
            {
                t.zoom = percent;
                actions.push(EngineAction::SetZoom { tab, percent });
            }
        }
    }
    actions
}

/// The default zoom changed (the settings window): every tab on a site
/// without a level of its own takes the new default.
pub fn apply_default_zoom(browser: &mut Browser) -> Vec<EngineAction> {
    let default = browser.settings.page_zoom;
    let mut actions = Vec::new();
    for w in &mut browser.windows {
        for t in &mut w.tabs {
            let level = browser.zoom.level_for(&t.url, default);
            if t.zoom != level {
                t.zoom = level;
                actions.push(EngineAction::SetZoom {
                    tab: t.id,
                    percent: level,
                });
            }
        }
    }
    actions
}

/// Step to the next or previous match in `window`'s active tab, when
/// the bar is open with something to search for.
fn find_step(browser: &Browser, window: WindowId, forward: bool) -> Vec<EngineAction> {
    let Some(w) = browser.window(window) else {
        return Vec::new();
    };
    match &w.find {
        Some(state) if !state.text.is_empty() => vec![EngineAction::Find {
            tab: w.active,
            text: state.text.clone(),
            forward,
            match_case: state.match_case,
            find_next: true,
        }],
        _ => Vec::new(),
    }
}

/// The active tab of `w` changed from `previous` (`None` when it was
/// closed): a search the find bar holds ends in the old tab and starts
/// over in the new one.
fn find_follows_switch(w: &mut Window, previous: Option<TabId>) -> Vec<EngineAction> {
    let Some(state) = w.find.as_mut() else {
        return Vec::new();
    };
    state.current = 0;
    state.total = 0;
    let mut actions = Vec::new();
    if let Some(previous) = previous {
        actions.push(EngineAction::StopFinding {
            tab: previous,
            clear_selection: true,
        });
    }
    if !state.text.is_empty() {
        actions.push(EngineAction::Find {
            tab: w.active,
            text: state.text.clone(),
            forward: true,
            match_case: state.match_case,
            find_next: false,
        });
    }
    actions
}

/// A new tab at `url`, active, with the focus left on its page.
fn new_tab(browser: &mut Browser, window: WindowId, url: Url) -> Vec<EngineAction> {
    if browser.window(window).is_none() {
        return Vec::new();
    }
    let tab = browser.allocate_tab_id();
    let (t, load) = tab_at(browser, tab, url);
    let w = browser
        .window_mut(window)
        .expect("window existence checked above");
    let previous = w.active;
    w.tabs.push(t);
    w.active = tab;
    w.select_only(tab);
    let mut actions = vec![
        EngineAction::Hide(previous),
        EngineAction::CreateTab {
            window,
            tab,
            url: load,
        },
    ];
    actions.extend(find_follows_switch(w, Some(previous)));
    actions
}

/// Add a tab at `url` beside `opener` that does not become active: the
/// engine creates it hidden, and the active tab keeps the focus.
fn new_background_tab(browser: &mut Browser, opener: TabId, url: &Url) -> Vec<EngineAction> {
    let Some(window) = browser.window_of_tab(opener) else {
        return Vec::new();
    };
    let tab = browser.allocate_tab_id();
    let (t, load) = tab_at(browser, tab, url.clone());
    let w = browser
        .window_mut(window)
        .expect("window_of_tab returned an existing window");
    w.insert_tab_after(opener, t);
    vec![EngineAction::CreateHiddenTab {
        window,
        tab,
        url: load,
    }]
}

/// Remove `tab`. If it was active, the tab to its right becomes active, or
/// the one to its left if it was last. Closing the last tab closes the
/// window, and closing the last window quits.
pub(crate) fn close_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    // The last tab of the last window: closing it quits.
    if browser.windows.len() == 1
        && browser.windows[0].tabs.len() == 1
        && let Some(ask) = quit_guard(browser, Some(window_id))
    {
        return ask;
    }
    // A deferred tab was never created in the engine, unless a warm
    // has it there.
    let deferred = browser.tab(tab).is_some_and(|t| t.deferred && !t.warming);
    let mut actions = if deferred {
        Vec::new()
    } else {
        vec![EngineAction::CloseTab(tab)]
    };
    let new_active = {
        let w = browser
            .window_mut(window_id)
            .expect("window_of_tab returned an existing window");
        let index = w.index_of(tab).expect("tab is in this window");
        let removed = w.tabs.remove(index);
        // The tab's own side panel goes with it.
        if let Some(panel) = removed.panel {
            actions.push(EngineAction::CloseView {
                window: window_id,
                view: panel.view,
            });
        }
        w.prune_folders();
        w.selected.retain(|t| *t != tab);
        if w.tabs.is_empty() {
            None
        } else if w.active == tab {
            let next = index.min(w.tabs.len() - 1);
            w.active = w.tabs[next].id;
            w.select_only(w.active);
            actions.extend(find_follows_switch(w, None));
            Some(w.active)
        } else {
            return actions;
        }
    };
    match new_active {
        Some(next) => {
            let mark = wake_mark(browser, next);
            let w = browser
                .window_mut(window_id)
                .expect("window_of_tab returned an existing window");
            actions.push(show_tab(w, next, mark));
        }
        None => {
            // The window's side panel goes with the window.
            let panel = browser.window_mut(window_id).and_then(|w| w.panel.take());
            if let Some(panel) = panel {
                actions.push(EngineAction::CloseView {
                    window: window_id,
                    view: panel.view,
                });
            }
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
    let mark = wake_mark(browser, tab);
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
    let mut actions = vec![EngineAction::Hide(previous), show_tab(w, tab, mark)];
    actions.extend(find_follows_switch(w, Some(previous)));
    actions
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
        actions.extend(new_tab(browser, window, url.clone()));
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
        .filter(|t| !t.deferred || t.warming)
        .map(|t| EngineAction::CloseTab(t.id))
        .collect();
    // Its side panels go with it: the tabs' in strip order, then its own.
    actions.extend(removed.panels().map(|panel| EngineAction::CloseView {
        window,
        view: panel.view,
    }));
    actions.push(EngineAction::CloseWindow(window));
    if browser.windows.is_empty() {
        actions.push(EngineAction::Quit);
    }
    actions
}

/// The place a side panel request names: `tab`'s own panel, or
/// `window`'s when `tab` is `None`. Refused when the window is not in
/// the model or the tab is not one of its tabs.
fn panel_slot(
    browser: &mut Browser,
    window: WindowId,
    tab: Option<TabId>,
) -> Result<&mut Option<SidePanel>, String> {
    let w = browser
        .window_mut(window)
        .ok_or_else(|| no_window(window))?;
    match tab {
        Some(tab) => w
            .tab_mut(tab)
            .map(|t| &mut t.panel)
            .ok_or_else(|| no_tab(tab)),
        None => Ok(&mut w.panel),
    }
}

/// Show `extension`'s `page` in the side panel of `tab`, or of `window`.
/// The model picks the view's id (no engine allocates a `TabId`) and
/// asks the engine for the view. The same page already there changes
/// nothing; another panel there is replaced, its view closed first.
fn open_side_panel(
    browser: &mut Browser,
    extension: &str,
    window: WindowId,
    tab: Option<TabId>,
    page: &str,
) -> Result<Vec<EngineAction>, String> {
    let slot = panel_slot(browser, window, tab)?;
    if slot
        .as_ref()
        .is_some_and(|p| p.extension == extension && p.page == page)
    {
        return Ok(Vec::new());
    }
    let mut actions = Vec::new();
    if let Some(old) = slot.take() {
        actions.push(EngineAction::CloseView {
            window,
            view: old.view,
        });
    }
    let view = browser.allocate_tab_id();
    *panel_slot(browser, window, tab)? = Some(SidePanel {
        extension: extension.to_string(),
        page: page.to_string(),
        view,
    });
    actions.push(EngineAction::CreateView {
        window,
        view,
        extension: extension.to_string(),
        page: page.to_string(),
    });
    Ok(actions)
}

/// Close `extension`'s side panel of `tab`, or of `window`. Nothing
/// when that place has no panel, or another extension's.
fn close_side_panel(
    browser: &mut Browser,
    extension: &str,
    window: WindowId,
    tab: Option<TabId>,
) -> Result<Vec<EngineAction>, String> {
    let slot = panel_slot(browser, window, tab)?;
    if slot.as_ref().is_none_or(|p| p.extension != extension) {
        return Ok(Vec::new());
    }
    let panel = slot.take().expect("checked above");
    Ok(vec![EngineAction::CloseView {
        window,
        view: panel.view,
    }])
}

/// The words the extension API itself has for a tab, a group and a
/// window that do not exist, which an extension's own error handling
/// already knows.
fn no_tab(tab: TabId) -> String {
    format!("No tab with id: {}.", tab.0)
}

fn no_window(window: WindowId) -> String {
    format!("No window with id: {}.", window.0)
}

fn no_group(folder: FolderId) -> String {
    format!("No group with id: {}.", folder.0)
}

/// The first of `tabs` the model does not have, as a refusal. A request
/// naming several tabs is refused whole when one is unknown, as the
/// extension API refuses it.
fn known_tabs(browser: &Browser, tabs: &[TabId]) -> Result<(), String> {
    match tabs.iter().find(|tab| browser.tab(**tab).is_none()) {
        Some(tab) => Err(no_tab(*tab)),
        None => Ok(()),
    }
}

/// The name a folder made for `extension` has until the extension names
/// it: "<Extension name> Session", with the id standing in for an
/// extension the model does not list.
fn session_folder_name(browser: &Browser, extension: &str) -> String {
    let name = browser.extension(extension).map_or(extension, |e| &e.name);
    format!("{name} Session")
}

/// A new tab at `url` at the end of `window`'s strip, outside any
/// folder, that does not become active: the engine creates it hidden.
/// The window must exist.
fn new_hidden_tab(browser: &mut Browser, window: WindowId, url: Url) -> (TabId, EngineAction) {
    let tab = browser.allocate_tab_id();
    let (t, load) = tab_at(browser, tab, url);
    browser
        .window_mut(window)
        .expect("the caller checked the window")
        .tabs
        .push(t);
    let action = EngineAction::CreateHiddenTab {
        window,
        tab,
        url: load,
    };
    (tab, action)
}

/// Take `tabs` out of their folders. A folder left empty goes, and one
/// left in two parts is gathered where its first tab is.
fn ungroup_tabs(browser: &mut Browser, tabs: &[TabId]) {
    for w in &mut browser.windows {
        let mut changed = false;
        for tab in &mut w.tabs {
            if tabs.contains(&tab.id) && tab.folder.take().is_some() {
                changed = true;
            }
        }
        if changed {
            w.prune_folders();
            let folders: Vec<FolderId> = w.folders.iter().map(|f| f.id).collect();
            for folder in folders {
                w.gather_folder(folder);
            }
        }
    }
}

/// Answer an extension's request (`EngineEvent::ExtensionRequest`):
/// what the request takes, then exactly one `ExtensionReply` carrying
/// `token`. A request that names a tab, folder or window the model lacks
/// changes nothing and is `Refused`.
pub(crate) fn extension_request(
    browser: &mut Browser,
    token: u64,
    extension: &str,
    request: &ExtensionRequest,
) -> Vec<EngineAction> {
    let (mut actions, reply) = match answer_request(browser, extension, request) {
        Ok(answer) => answer,
        Err(why) => (Vec::new(), ExtensionReply::Refused(why)),
    };
    actions.push(EngineAction::ExtensionReply { token, reply });
    actions
}

/// The work of [`extension_request`]: the actions and the reply, or why
/// the request is refused. Everything is checked before anything
/// changes.
fn answer_request(
    browser: &mut Browser,
    extension: &str,
    request: &ExtensionRequest,
) -> Result<(Vec<EngineAction>, ExtensionReply), String> {
    match request {
        ExtensionRequest::OpenTab {
            window,
            url,
            active,
        } => {
            let window = match window {
                Some(window) if browser.window(*window).is_some() => *window,
                Some(window) => return Err(no_window(*window)),
                None => browser
                    .focused_window()
                    .ok_or_else(|| "No current window".to_string())?,
            };
            if *active {
                let actions = new_tab(browser, window, url.clone());
                let tab = browser.window(window).expect("checked above").active;
                Ok((actions, ExtensionReply::Tab(tab)))
            } else {
                let (tab, action) = new_hidden_tab(browser, window, url.clone());
                Ok((vec![action], ExtensionReply::Tab(tab)))
            }
        }
        ExtensionRequest::CloseTabs(tabs) => {
            known_tabs(browser, tabs)?;
            remember_closed(browser, tabs);
            Ok((close_tabs(browser, tabs), ExtensionReply::Done))
        }
        ExtensionRequest::ActivateTab(tab) => {
            known_tabs(browser, &[*tab])?;
            Ok((activate_tab(browser, *tab), ExtensionReply::Done))
        }
        ExtensionRequest::MoveTab { tab, index } => {
            known_tabs(browser, &[*tab])?;
            move_tabs(browser, &[*tab], *index);
            Ok((Vec::new(), ExtensionReply::Done))
        }
        ExtensionRequest::GroupTabs { tabs, folder } => {
            known_tabs(browser, tabs)?;
            let window = match folder {
                Some(folder) => browser
                    .window_of_folder(*folder)
                    .ok_or_else(|| no_group(*folder))?,
                None => tabs
                    .first()
                    .and_then(|tab| browser.window_of_tab(*tab))
                    .ok_or_else(|| "No tabs to group.".to_string())?,
            };
            if tabs
                .iter()
                .any(|tab| browser.window_of_tab(*tab) != Some(window))
            {
                return Err("Tabs of another window cannot join this group.".to_string());
            }
            let target = match folder {
                Some(folder) => FolderTarget::Existing(*folder),
                None => FolderTarget::New {
                    name: session_folder_name(browser, extension),
                },
            };
            let folder = move_tabs_to_folder(browser, tabs, &target)
                .ok_or_else(|| "No tabs to group.".to_string())?;
            // A folder an extension makes starts open: the extension
            // works in its tabs and the user watches.
            if matches!(target, FolderTarget::New { .. })
                && let Some(f) = folder_mut(browser, folder)
            {
                f.expanded = true;
            }
            Ok((Vec::new(), ExtensionReply::Folder(folder)))
        }
        ExtensionRequest::UngroupTabs(tabs) => {
            known_tabs(browser, tabs)?;
            ungroup_tabs(browser, tabs);
            Ok((Vec::new(), ExtensionReply::Done))
        }
        ExtensionRequest::UpdateFolder {
            folder,
            name,
            expanded,
        } => {
            if browser.window_of_folder(*folder).is_none() {
                return Err(no_group(*folder));
            }
            let name = name.as_deref().map(|name| match name.trim() {
                "" => session_folder_name(browser, extension),
                name => name.to_string(),
            });
            let f = folder_mut(browser, *folder).expect("checked above");
            if let Some(name) = name {
                f.name = name;
            }
            if let Some(expanded) = expanded {
                f.expanded = *expanded;
            }
            Ok((Vec::new(), ExtensionReply::Done))
        }
        ExtensionRequest::OpenWindow { url } => {
            let url = url.clone().unwrap_or_else(|| browser.new_tab_url());
            let actions = open_window(browser, url);
            let window = browser
                .windows
                .last()
                .expect("open_window pushed a window")
                .id;
            Ok((actions, ExtensionReply::Window(window)))
        }
        ExtensionRequest::OpenSidePanel { window, tab, page } => {
            let actions = open_side_panel(browser, extension, *window, *tab, page)?;
            Ok((actions, ExtensionReply::Done))
        }
        ExtensionRequest::CloseSidePanel { window, tab } => {
            let actions = close_side_panel(browser, extension, *window, *tab)?;
            Ok((actions, ExtensionReply::Done))
        }
    }
}

/// Close the side panel `window` shows: the active tab's, else the
/// window's own. With both open, the window's shows next.
fn close_shown_panel(browser: &mut Browser, window: WindowId) -> Vec<EngineAction> {
    let Some(w) = browser.window_mut(window) else {
        return Vec::new();
    };
    let active = w.active;
    let panel = match w.tab_mut(active).and_then(|t| t.panel.take()) {
        Some(panel) => Some(panel),
        None => w.panel.take(),
    };
    match panel {
        Some(panel) => vec![EngineAction::CloseView {
            window,
            view: panel.view,
        }],
        None => Vec::new(),
    }
}

/// The engine says the view `view` is gone (its page closed itself, or
/// crashed): the side panel it rendered leaves the model. Whether `view`
/// was a panel's view at all.
pub(crate) fn drop_panel_view(browser: &mut Browser, view: TabId) -> bool {
    browser
        .windows
        .iter_mut()
        .any(|w| w.take_panel_view(view).is_some())
}

/// `SaveSettings` when a setting changed, nothing otherwise.
fn save_if(changed: bool) -> Vec<EngineAction> {
    if changed {
        vec![EngineAction::SaveSettings]
    } else {
        Vec::new()
    }
}

/// What the Launch Panel's tiles need fetched: the weather when a place
/// is set and the report is older than its lifetime (or `force`), the
/// Drive list when an account is connected and the list is older than
/// its lifetime. A tile the user removed asks for nothing; a fetch
/// already going is never asked for twice. The data moves to `Loading`
/// here so the tile shows it at once.
fn launch_fetches(browser: &mut Browser, force: bool) -> Vec<EngineAction> {
    let now = unix_now();
    let mut actions = Vec::new();
    let settings = &browser.settings.launch;
    let shown = |kind: WidgetKind| settings.widgets.contains(&kind);
    if shown(WidgetKind::Weather) {
        let units = settings.weather.units(&browser.launch.located);
        let chosen = settings.weather.location.clone();
        let report_wanted = browser.launch.weather != Fetched::Loading
            && (force || browser.launch.weather.is_stale(now, WEATHER_LIFETIME));
        match chosen {
            // A place the user named is used as it stands; the system
            // is never asked where the device is.
            Some(place) => {
                if report_wanted {
                    actions.push(EngineAction::FetchWeather { place, units });
                    browser.launch.weather = Fetched::Loading;
                }
            }
            // Otherwise the tile follows the device. The system is
            // asked when nothing is known or what is known has aged
            // out; a refusal is not put to the user again at every
            // opening, only when they ask for it (`force`: Retry, or
            // handing a named place back). A fresh fix is reused even
            // when forced, so changing the units costs no new fix.
            None => {
                let relocate = match &browser.launch.located {
                    Fetched::Loading => false,
                    Fetched::Empty => true,
                    Fetched::Failed(_) => force,
                    Fetched::Ready { at, .. } => now.saturating_sub(*at) >= LOCATION_LIFETIME,
                };
                if relocate {
                    // The stored choice, not the resolved units: with
                    // none stored and nowhere known yet, the country
                    // the fix lands in is the first word on the
                    // matter, and only the fetcher will know it.
                    actions.push(EngineAction::FetchLocation {
                        units: settings.weather.units,
                    });
                    browser.launch.located = Fetched::Loading;
                } else if report_wanted && let Some(place) = browser.launch.located.value().cloned()
                {
                    actions.push(EngineAction::FetchWeather { place, units });
                    browser.launch.weather = Fetched::Loading;
                }
            }
        }
    }
    if shown(WidgetKind::DriveRecent)
        && let DriveState::Connected { files, .. } = &mut browser.launch.drive
        && *files != Fetched::Loading
        && (force || files.is_stale(now, DRIVE_LIFETIME))
    {
        actions.push(EngineAction::FetchDriveRecent);
        *files = Fetched::Loading;
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
    /// Ctrl+T and the "+" button ask for an address before any tab
    /// exists: nothing changes in the model until the launcher answers
    /// with a URL or a search.
    fn new_tab_without_a_url_opens_the_launcher_and_no_tab() {
        let (mut b, window, _) = one_window();
        let actions = dispatch(&mut b, &Command::NewTab { window, url: None });
        // The weather tile follows the device, so opening the panel
        // also asks the system where it is.
        assert_eq!(
            actions,
            vec![
                EngineAction::ShowLauncher { window },
                EngineAction::FetchLocation { units: None }
            ]
        );
        assert_eq!(b.window(window).unwrap().tabs.len(), 1);
        assert!(
            dispatch(
                &mut b,
                &Command::NewTab {
                    window: WindowId(99),
                    url: None
                }
            )
            .is_empty()
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
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
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
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
        let t2 = b.window(window).unwrap().active;
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
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
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
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
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
        let t2 = b.window(window).unwrap().active;
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
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
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
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

    fn an_update() -> AvailableUpdate {
        AvailableUpdate {
            id: crate::update::BuildId::new("0.1.0", 9),
            notes_url: None,
            asset: crate::update::Asset {
                name: "setup.exe".to_string(),
                url: "https://updates.example/setup.exe".to_string(),
                sha256: "0".repeat(64),
                size: 1,
            },
        }
    }

    #[test]
    fn the_default_browser_offer_is_made_until_answered() {
        let (mut b, _, _) = one_window();
        // Unknown here (not a bundle that claims web links): no offer,
        // and the button does nothing.
        assert!(!b.offers_default_browser());
        assert!(dispatch(&mut b, &Command::MakeDefaultBrowser).is_empty());
        // Already the default: no offer either.
        b.default_browser = DefaultBrowser::Yes;
        assert!(!b.offers_default_browser());
        assert!(dispatch(&mut b, &Command::MakeDefaultBrowser).is_empty());

        b.default_browser = DefaultBrowser::No;
        assert!(b.offers_default_browser());
        // "Make default" asks the system and hides the bar meanwhile.
        assert_eq!(
            dispatch(&mut b, &Command::MakeDefaultBrowser),
            vec![EngineAction::MakeDefaultBrowser]
        );
        assert!(!b.offers_default_browser());

        // "Not now" lasts until the next start; the setting is untouched.
        let (mut b, _, _) = one_window();
        b.default_browser = DefaultBrowser::No;
        assert!(dispatch(&mut b, &Command::DismissDefaultBrowser).is_empty());
        assert!(!b.offers_default_browser());
        assert!(b.settings.check_default_browser);

        // "Don't ask again" turns the setting off and saves it, once.
        let (mut b, _, _) = one_window();
        b.default_browser = DefaultBrowser::No;
        assert_eq!(
            dispatch(&mut b, &Command::NeverAskDefaultBrowser),
            vec![EngineAction::SaveSettings]
        );
        assert!(!b.settings.check_default_browser);
        assert!(!b.offers_default_browser());
        assert!(dispatch(&mut b, &Command::NeverAskDefaultBrowser).is_empty());
        // The settings window's button still works without the offer.
        assert_eq!(
            dispatch(&mut b, &Command::MakeDefaultBrowser),
            vec![EngineAction::MakeDefaultBrowser]
        );
    }

    #[test]
    fn a_check_starts_only_when_nothing_is_under_way() {
        let (mut b, _, _) = one_window();
        assert_eq!(b.update, UpdateStatus::Idle);
        assert_eq!(
            dispatch(&mut b, &Command::CheckForUpdates),
            vec![EngineAction::CheckForUpdates]
        );
        assert_eq!(b.update, UpdateStatus::Checking);
        // Already checking: nothing more.
        assert!(dispatch(&mut b, &Command::CheckForUpdates).is_empty());
        // After a failure a check starts again, and the failure can be
        // dismissed.
        b.update = UpdateStatus::Failed("offline".to_string());
        assert!(dispatch(&mut b, &Command::DismissUpdateFailure).is_empty());
        assert_eq!(b.update, UpdateStatus::Idle);
        b.update = UpdateStatus::Failed("offline".to_string());
        assert_eq!(dispatch(&mut b, &Command::CheckForUpdates).len(), 1);
        // Downloading: no check, no dismissal.
        b.update = UpdateStatus::Downloading {
            update: an_update(),
            received: 0,
        };
        assert!(dispatch(&mut b, &Command::CheckForUpdates).is_empty());
        assert!(dispatch(&mut b, &Command::DismissUpdateFailure).is_empty());
        assert!(matches!(b.update, UpdateStatus::Downloading { .. }));
    }

    #[test]
    fn install_needs_a_ready_update() {
        let (mut b, _, _) = one_window();
        assert!(dispatch(&mut b, &Command::InstallUpdate).is_empty());
        b.update = UpdateStatus::Ready {
            update: an_update(),
            file: std::path::PathBuf::from("setup.exe"),
        };
        assert_eq!(
            dispatch(&mut b, &Command::InstallUpdate),
            vec![EngineAction::InstallUpdate {
                update: an_update(),
                file: std::path::PathBuf::from("setup.exe"),
            }]
        );
        assert_eq!(b.update, UpdateStatus::Installing(an_update()));
        // A second click while on the way out does nothing.
        assert!(dispatch(&mut b, &Command::InstallUpdate).is_empty());
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
    fn opening_another_profile_starts_it_with_no_tabs() {
        let (mut b, _, _) = three_tabs();
        assert_eq!(
            dispatch(&mut b, &Command::OpenProfile("work".to_string())),
            vec![EngineAction::OpenInProfile {
                profile: "work".to_string(),
                urls: Vec::new(),
            }]
        );
        let own_profile = b.profile.name.clone();
        assert!(dispatch(&mut b, &Command::OpenProfile(own_profile)).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::OpenGuestProfile),
            vec![EngineAction::OpenGuestProfile]
        );
    }

    #[test]
    fn managing_profiles_lists_this_one_first_and_a_guest_is_neither_edited_nor_deleted() {
        let (mut b, window, _) = three_tabs();
        b.settings.profile.name = "Me".to_string();
        b.settings.profile.avatar = Avatar::Whale;
        b.settings.chrome_color = ChromeColor::Plum;
        b.profiles = vec![ProfileEntry {
            dir: "work".to_string(),
            name: "Work".to_string(),
            avatar: Avatar::Initial,
            color: ChromeColor::Slate,
        }];
        let current = ProfileEntry {
            dir: b.profile.name.clone(),
            name: "Me".to_string(),
            avatar: Avatar::Whale,
            color: ChromeColor::Plum,
        };
        assert_eq!(
            dispatch(&mut b, &Command::PromptManageProfiles(window)),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::ManageProfiles {
                    current: current.clone(),
                    guest: false,
                    others: b.profiles.clone(),
                },
            }]
        );
        assert!(dispatch(&mut b, &Command::PromptManageProfiles(WindowId(99))).is_empty());
        assert!(!dispatch(&mut b, &Command::PromptEditProfile(window)).is_empty());

        b.profile.guest = true;
        assert!(dispatch(&mut b, &Command::PromptEditProfile(window)).is_empty());
        let own = b.profile.name.clone();
        assert!(
            dispatch(
                &mut b,
                &Command::PromptDeleteProfile {
                    window,
                    profile: own,
                }
            )
            .is_empty()
        );
        assert!(matches!(
            dispatch(&mut b, &Command::PromptManageProfiles(window)).as_slice(),
            [EngineAction::ShowPrompt {
                prompt: Prompt::ManageProfiles { guest: true, .. },
                ..
            }]
        ));
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

    /// Install an extension called `id`, with a toolbar action or
    /// without one.
    fn installed(b: &mut Browser, id: &str, enabled: bool, action: bool) {
        b.extensions.push(crate::extension::Extension {
            id: id.to_string(),
            name: id.to_uppercase(),
            version: "1".to_string(),
            description: String::new(),
            enabled,
            icon: None,
            action_title: action.then(|| "Act".to_string()),
            side_panel: false,
            commands: Vec::new(),
        });
    }

    #[test]
    fn open_extension_action_needs_a_window_and_an_enabled_extension_with_an_action() {
        let (mut b, window, _) = one_window();
        installed(&mut b, "a", true, true);
        installed(&mut b, "off", false, true);
        installed(&mut b, "noaction", true, false);
        // An action that opens a side panel instead of a popup is an
        // action like any other: the model keeps no popup flag, and the
        // engine decides what the click does.
        installed(&mut b, "panel", true, true);
        b.extensions.last_mut().unwrap().side_panel = true;
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
        assert_eq!(
            open(&mut b, window, "panel"),
            vec![EngineAction::OpenExtensionAction {
                window,
                extension: "panel".to_string()
            }]
        );
        assert!(open(&mut b, window, "off").is_empty());
        assert!(open(&mut b, window, "noaction").is_empty());
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
    #[test]
    fn the_star_adds_to_the_last_group_once_and_then_edits() {
        let (mut b, window, tab) = one_window();
        b.tab_mut(tab).unwrap().title = "A page".to_string();
        let actions = dispatch(&mut b, &Command::ToggleBookmark(tab));
        let prompt = Prompt::Bookmark {
            url: url("https://a.example/"),
            title: "A page".to_string(),
            group: Bookmarks::BAR.to_string(),
            groups: vec![Bookmarks::BAR.to_string()],
            added: true,
        };
        assert_eq!(
            actions,
            vec![
                EngineAction::SaveBookmarks,
                EngineAction::ShowPrompt {
                    window,
                    prompt: prompt.clone()
                }
            ]
        );
        assert!(b.bookmarks.contains(&url("https://a.example/")));
        // Bookmarked already: only the popover, saying "edit".
        let again = dispatch(&mut b, &Command::ToggleBookmark(tab));
        assert_eq!(again.len(), 1);
        assert!(matches!(
            &again[0],
            EngineAction::ShowPrompt {
                prompt: Prompt::Bookmark { added: false, .. },
                ..
            }
        ));
        // The last group the user picked is where the next star goes.
        b.settings.last_bookmark_group = "Work".to_string();
        dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("https://b.example/"),
            },
        );
        dispatch(&mut b, &Command::ToggleBookmark(tab));
        assert_eq!(
            b.bookmarks.find(&url("https://b.example/")).unwrap().0,
            "Work"
        );
        // Nothing to bookmark on a blank page.
        dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("about:blank"),
            },
        );
        assert!(dispatch(&mut b, &Command::ToggleBookmark(tab)).is_empty());
    }

    #[test]
    fn editing_a_bookmark_moves_it_and_remembers_the_group() {
        let (mut b, _, tab) = one_window();
        dispatch(&mut b, &Command::ToggleBookmark(tab));
        let actions = dispatch(
            &mut b,
            &Command::EditBookmark {
                url: url("https://a.example/"),
                title: "Renamed".to_string(),
                group: "Work".to_string(),
            },
        );
        assert_eq!(
            actions,
            vec![EngineAction::SaveBookmarks, EngineAction::SaveSettings]
        );
        let (group, bookmark) = b.bookmarks.find(&url("https://a.example/")).unwrap();
        assert_eq!((group, bookmark.title.as_str()), ("Work", "Renamed"));
        assert_eq!(b.settings.last_bookmark_group, "Work");
        // The same group again: the settings are left alone.
        let actions = dispatch(
            &mut b,
            &Command::EditBookmark {
                url: url("https://a.example/"),
                title: "Renamed again".to_string(),
                group: "Work".to_string(),
            },
        );
        assert_eq!(actions, vec![EngineAction::SaveBookmarks]);
        assert_eq!(
            dispatch(&mut b, &Command::RemoveBookmark(url("https://a.example/"))),
            vec![EngineAction::SaveBookmarks]
        );
        assert!(dispatch(&mut b, &Command::RemoveBookmark(url("https://a.example/"))).is_empty());
        // A chip's Edit… for a page that is not bookmarked asks nothing.
        let (_, window, _) = one_window();
        assert!(
            dispatch(
                &mut b,
                &Command::PromptEditBookmark {
                    window,
                    url: url("https://a.example/")
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn groups_are_renamed_deleted_and_reordered_through_prompts() {
        let (mut b, window, tab) = one_window();
        b.settings.last_bookmark_group = "Work".to_string();
        dispatch(&mut b, &Command::ToggleBookmark(tab));
        assert_eq!(
            dispatch(
                &mut b,
                &Command::PromptRenameBookmarkGroup {
                    window,
                    group: "Work".to_string()
                }
            ),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::RenameBookmarkGroup {
                    group: "Work".to_string()
                }
            }]
        );
        assert!(
            dispatch(
                &mut b,
                &Command::PromptRenameBookmarkGroup {
                    window,
                    group: "Nope".to_string()
                }
            )
            .is_empty()
        );
        assert_eq!(
            dispatch(
                &mut b,
                &Command::RenameBookmarkGroup {
                    group: "Work".to_string(),
                    name: "Office".to_string()
                }
            ),
            vec![EngineAction::SaveBookmarks, EngineAction::SaveSettings]
        );
        assert_eq!(b.settings.last_bookmark_group, "Office");
        assert_eq!(
            dispatch(
                &mut b,
                &Command::PromptDeleteBookmarkGroup {
                    window,
                    group: "Office".to_string()
                }
            ),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::DeleteBookmarkGroup {
                    group: "Office".to_string(),
                    count: 1
                }
            }]
        );
        b.bookmarks.add(
            "Office",
            vec![Bookmark {
                title: "B".to_string(),
                url: url("https://b.example/"),
            }],
        );
        assert_eq!(
            dispatch(
                &mut b,
                &Command::MoveBookmark {
                    group: "Office".to_string(),
                    from: 1,
                    to: 0
                }
            ),
            vec![EngineAction::SaveBookmarks]
        );
        assert_eq!(b.bookmarks.group("Office").unwrap().bookmarks[0].title, "B");
        assert!(
            dispatch(
                &mut b,
                &Command::MoveBookmark {
                    group: "Office".to_string(),
                    from: 0,
                    to: 1
                }
            )
            .is_empty()
        );
        assert_eq!(
            dispatch(&mut b, &Command::DeleteBookmarkGroup("Office".to_string())),
            vec![EngineAction::SaveBookmarks]
        );
        assert!(b.bookmarks.is_empty());
    }

    #[test]
    fn the_bar_toggles_and_files_import_and_export() {
        let mut b = browser();
        assert!(!b.settings.show_bookmarks_bar);
        assert_eq!(
            dispatch(&mut b, &Command::ToggleBookmarksBar),
            vec![EngineAction::SaveSettings]
        );
        assert!(b.settings.show_bookmarks_bar);
        assert_eq!(
            dispatch(&mut b, &Command::PickBookmarksFile(BookmarksFile::Import)),
            vec![EngineAction::PickFile(BookmarksFile::Import)]
        );
        let html = "<DL><p><DT><A HREF=\"https://x.example/\">X</A></DL>";
        assert_eq!(
            dispatch(
                &mut b,
                &Command::ImportBookmarks {
                    text: html.to_string(),
                    source: "chrome.html".to_string()
                }
            ),
            vec![EngineAction::SaveBookmarks]
        );
        assert_eq!(
            b.bookmarks.find(&url("https://x.example/")).unwrap().0,
            "Imported from chrome.html"
        );
        // Importing the same file again adds nothing and saves nothing.
        assert!(
            dispatch(
                &mut b,
                &Command::ImportBookmarks {
                    text: html.to_string(),
                    source: "chrome.html".to_string()
                }
            )
            .is_empty()
        );
        let path = PathBuf::from("out.html");
        assert_eq!(
            dispatch(&mut b, &Command::ExportBookmarks(path.clone())),
            vec![EngineAction::ExportBookmarks(path)]
        );
        assert!(Command::ToggleBookmark(TabId(1)).changes_bookmarks());
        assert!(
            Command::ImportBookmarks {
                text: String::new(),
                source: String::new()
            }
            .changes_bookmarks()
        );
        assert!(!Command::ToggleBookmarksBar.changes_bookmarks());
        assert!(!Command::PickBookmarksFile(BookmarksFile::Export).changes_bookmarks());
    }

    fn saved_session() -> Session {
        use crate::session::{SessionFolder, SessionTab, SessionWindow};
        Session {
            closed_tabs: vec![url("https://gone.example/")],
            windows: vec![SessionWindow {
                active: 1,
                geometry: Some(WindowGeometry {
                    x: 10,
                    y: 20,
                    width: 800,
                    height: 600,
                    maximized: false,
                }),
                folders: vec![SessionFolder {
                    name: "Reading".to_string(),
                    expanded: true,
                }],
                tabs: vec![
                    SessionTab {
                        url: url("https://a.example/"),
                        title: "A".to_string(),
                        folder: None,
                    },
                    SessionTab {
                        url: url("https://b.example/"),
                        title: "B".to_string(),
                        folder: Some("Reading".to_string()),
                    },
                    SessionTab {
                        url: url("https://c.example/"),
                        title: "C".to_string(),
                        folder: Some("Reading".to_string()),
                    },
                ],
            }],
        }
    }

    #[test]
    fn a_session_comes_back_with_deferred_tabs_that_load_when_shown() {
        let mut b = browser();
        let actions = restore_session(&mut b, &saved_session());
        let w = &b.windows[0];
        let window = w.id;
        assert_eq!(w.tabs.len(), 3);
        assert_eq!(w.active_index(), 1);
        let (a, bb, c) = (w.tabs[0].id, w.tabs[1].id, w.tabs[2].id);
        assert!(w.tabs[0].deferred && !w.tabs[1].deferred && w.tabs[2].deferred);
        assert_eq!(w.tabs[1].title, "B");
        assert_eq!(w.folders.len(), 1);
        assert_eq!(w.folders[0].name, "Reading");
        assert!(w.folders[0].expanded);
        assert_eq!(w.tabs[1].folder, Some(w.folders[0].id));
        assert_eq!(w.tabs[2].folder, Some(w.folders[0].id));
        assert_eq!(w.tabs[0].folder, None);
        // The place comes first, then the window with its active tab
        // (the only one the engine is asked for).
        assert!(matches!(
            actions[0],
            EngineAction::PlaceWindow {
                geometry: WindowGeometry { x: 10, .. },
                ..
            }
        ));
        assert!(matches!(
            &actions[1],
            EngineAction::OpenWindow { tab, url, .. } if *tab == bb && url.as_str() == "https://b.example/"
        ));
        // The deferred tabs, in strip order, for the shell to warm.
        assert_eq!(actions[2], EngineAction::WarmTabs(vec![a, c]));
        assert_eq!(actions.len(), 3);
        assert_eq!(b.closed_tabs, vec![url("https://gone.example/")]);
        // Showing a deferred tab creates it; closing one asks nothing
        // of the engine.
        let actions = dispatch(&mut b, &Command::ActivateTab(a));
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(bb),
                EngineAction::CreateTab {
                    window,
                    tab: a,
                    url: url("https://a.example/")
                }
            ]
        );
        assert!(!b.tab(a).unwrap().deferred);
        assert_eq!(
            dispatch(&mut b, &Command::ActivateTab(bb)),
            vec![EngineAction::Hide(a), EngineAction::Show(bb)]
        );
        let actions = dispatch(&mut b, &Command::CloseTab(c));
        assert!(actions.is_empty(), "{actions:?}");
        assert_eq!(b.windows[0].tabs.len(), 2);
        // Closing the active tab when the one that takes its place is
        // deferred creates that one.
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://d.example/")),
            },
        );
        let d = b.windows[0].active;
        dispatch(&mut b, &Command::ActivateTab(bb));
        b.windows[0]
            .tabs
            .iter_mut()
            .find(|t| t.id == d)
            .unwrap()
            .deferred = true;
        let actions = dispatch(&mut b, &Command::CloseTab(bb));
        assert!(actions.contains(&EngineAction::CloseTab(bb)));
        assert!(
            actions
                .iter()
                .any(|x| matches!(x, EngineAction::CreateTab { tab, .. } if *tab == d)),
            "{actions:?}"
        );
    }

    #[test]
    fn a_restored_tab_warms_out_of_sight_and_is_let_go_again() {
        let mut b = browser();
        restore_session(&mut b, &saved_session());
        let w = &b.windows[0];
        let (window, a, bb, c) = (w.id, w.tabs[0].id, w.tabs[1].id, w.tabs[2].id);
        // Warming creates the tab hidden and leaves it deferred.
        assert_eq!(
            dispatch(&mut b, &Command::WarmTab(a)),
            vec![EngineAction::CreateHiddenTab {
                window,
                tab: a,
                url: url("https://a.example/")
            }]
        );
        let t = b.tab(a).unwrap();
        assert!(t.deferred && t.warming);
        // Again, or a tab that is not deferred, or none: nothing.
        assert!(dispatch(&mut b, &Command::WarmTab(a)).is_empty());
        assert!(dispatch(&mut b, &Command::WarmTab(bb)).is_empty());
        assert!(dispatch(&mut b, &Command::WarmTab(TabId(99))).is_empty());
        // Done: the engine's tab goes; the model's stays, deferred,
        // with what came in.
        crate::apply(
            &mut b,
            &crate::EngineEvent::TitleChanged {
                tab: a,
                title: "A, warmed".to_string(),
            },
        );
        crate::apply(
            &mut b,
            &crate::EngineEvent::LoadStateChanged {
                tab: a,
                state: LoadState::Complete,
            },
        );
        assert_eq!(
            dispatch(&mut b, &Command::WarmDone(a)),
            vec![EngineAction::CloseTab(a)]
        );
        let t = b.tab(a).unwrap();
        assert!(t.deferred && !t.warming);
        assert_eq!(t.title, "A, warmed");
        assert_eq!(t.load, LoadState::Idle);
        assert!(dispatch(&mut b, &Command::WarmDone(a)).is_empty());
        // Shown while warming: the engine has it, so it is shown, not
        // created again.
        dispatch(&mut b, &Command::WarmTab(c));
        assert_eq!(
            dispatch(&mut b, &Command::ActivateTab(c)),
            vec![EngineAction::Hide(bb), EngineAction::Show(c)]
        );
        let t = b.tab(c).unwrap();
        assert!(!t.deferred && !t.warming);
        assert!(dispatch(&mut b, &Command::WarmDone(c)).is_empty());
        // Closed while warming: the engine's tab has to go with it.
        dispatch(&mut b, &Command::WarmTab(a));
        assert_eq!(
            dispatch(&mut b, &Command::CloseTab(a)),
            vec![EngineAction::CloseTab(a)]
        );
        assert!(b.tab(a).is_none());
    }

    #[test]
    fn a_closing_window_takes_its_warming_tabs_with_it() {
        let mut b = browser();
        restore_session(&mut b, &saved_session());
        let w = &b.windows[0];
        let (window, a, c) = (w.id, w.tabs[0].id, w.tabs[2].id);
        dispatch(&mut b, &Command::WarmTab(a));
        let actions = dispatch(&mut b, &Command::CloseWindow(window));
        assert!(actions.contains(&EngineAction::CloseTab(a)), "{actions:?}");
        assert!(!actions.contains(&EngineAction::CloseTab(c)), "{actions:?}");
    }

    #[test]
    fn the_crash_offer_restores_or_is_dismissed() {
        let (mut b, window, _) = one_window();
        assert!(dispatch(&mut b, &Command::RestoreSession(window)).is_empty());
        b.crashed_session = Some(saved_session());
        let actions = dispatch(&mut b, &Command::RestoreSession(window));
        assert!(b.crashed_session.is_none());
        // The restored window arrived and the one-tab window the offer
        // was in went.
        assert_eq!(b.windows.len(), 1);
        assert_ne!(b.windows[0].id, window);
        assert!(actions.contains(&EngineAction::CloseWindow(window)));
        assert!(!actions.contains(&EngineAction::Quit));
        b.crashed_session = Some(saved_session());
        assert!(dispatch(&mut b, &Command::DismissRestore).is_empty());
        assert!(b.crashed_session.is_none());
    }

    fn download_report(name: &str, state: DownloadState) -> crate::download::DownloadReport {
        crate::download::DownloadReport {
            name: name.to_string(),
            url: url("https://x.example/f.zip"),
            path: Some(PathBuf::from("C:/dl/f.zip")),
            received: 5,
            total: Some(10),
            speed: 0,
            state,
        }
    }

    #[test]
    fn downloads_are_controlled_through_the_engine_id_and_opened_when_done() {
        let (mut b, window, tab) = one_window();
        let actions = crate::apply(
            &mut b,
            &crate::EngineEvent::DownloadUpdated {
                id: 42,
                report: download_report("f.zip", DownloadState::InProgress),
            },
        );
        assert_eq!(actions, vec![EngineAction::SaveDownloads]);
        let id = b.downloads.items()[0].id;
        assert_eq!(
            dispatch(&mut b, &Command::ToggleDownloads(window)),
            vec![EngineAction::ToggleDownloads(window)]
        );
        assert_eq!(
            dispatch(&mut b, &Command::PauseDownload(id)),
            vec![EngineAction::PauseDownload(42)]
        );
        // Not paused yet (the engine has not said so): nothing to resume.
        assert!(dispatch(&mut b, &Command::ResumeDownload(id)).is_empty());
        assert!(dispatch(&mut b, &Command::OpenDownload(id)).is_empty());
        assert!(dispatch(&mut b, &Command::RemoveDownload(id)).is_empty());
        // Quitting asks while it goes; the answer quits.
        assert_eq!(
            dispatch(&mut b, &Command::Quit),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::QuitWithDownloads { count: 1 }
            }]
        );
        assert!(matches!(
            dispatch(&mut b, &Command::CloseTab(tab))[0],
            EngineAction::ShowPrompt { .. }
        ));
        assert!(matches!(
            dispatch(&mut b, &Command::CloseWindow(window))[0],
            EngineAction::ShowPrompt { .. }
        ));
        assert_eq!(
            dispatch(&mut b, &Command::ForceQuit),
            vec![EngineAction::Quit]
        );
        assert_eq!(dispatch(&mut b, &Command::Quit), vec![EngineAction::Quit]);
        b.quit_confirmed = false;
        crate::apply(
            &mut b,
            &crate::EngineEvent::DownloadUpdated {
                id: 42,
                report: download_report("f.zip", DownloadState::Complete),
            },
        );
        assert_eq!(dispatch(&mut b, &Command::Quit), vec![EngineAction::Quit]);
        assert_eq!(
            dispatch(&mut b, &Command::OpenDownload(id)),
            vec![EngineAction::OpenPath(PathBuf::from("C:/dl/f.zip"))]
        );
        assert_eq!(
            dispatch(&mut b, &Command::RevealDownload(id)),
            vec![EngineAction::RevealPath(PathBuf::from("C:/dl/f.zip"))]
        );
        assert!(dispatch(&mut b, &Command::CancelDownload(id)).is_empty());
        // A retry drops the entry and downloads the address again.
        assert_eq!(
            dispatch(&mut b, &Command::RetryDownload { window, id }),
            vec![
                EngineAction::SaveDownloads,
                EngineAction::StartDownload {
                    tab,
                    url: url("https://x.example/f.zip")
                }
            ]
        );
        assert!(b.downloads.is_empty());
        assert!(dispatch(&mut b, &Command::ClearDownloads).is_empty());
        assert_eq!(
            dispatch(
                &mut b,
                &Command::SetDownloadsDirectory(Some(PathBuf::from("D:/dl")))
            ),
            vec![EngineAction::SaveSettings]
        );
        assert!(
            dispatch(
                &mut b,
                &Command::SetDownloadsDirectory(Some(PathBuf::from("D:/dl")))
            )
            .is_empty()
        );
        assert_eq!(
            dispatch(&mut b, &Command::PickDownloadsDirectory),
            vec![EngineAction::PickFolder]
        );
    }

    #[test]
    fn zoom_steps_spread_over_the_host_and_are_remembered() {
        let (mut b, window, tab) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://a.example/other")),
            },
        );
        let other = b.window(window).unwrap().active;
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://b.example/")),
            },
        );
        let elsewhere = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::ZoomIn(tab));
        assert_eq!(
            actions,
            vec![
                EngineAction::SaveZoom,
                EngineAction::SetZoom { tab, percent: 110 },
                EngineAction::SetZoom {
                    tab: other,
                    percent: 110
                },
            ]
        );
        assert_eq!(b.tab(elsewhere).unwrap().zoom, 100);
        assert_eq!(b.zoom.sites["a.example"], 110);
        // Out twice from 110 lands on 90; back to the default forgets
        // the site.
        dispatch(&mut b, &Command::ZoomOut(tab));
        dispatch(&mut b, &Command::ZoomOut(other));
        assert_eq!(b.tab(tab).unwrap().zoom, 90);
        let actions = dispatch(&mut b, &Command::ZoomReset(tab));
        assert_eq!(actions[0], EngineAction::SaveZoom);
        assert!(b.zoom.sites.is_empty());
        assert_eq!(b.tab(other).unwrap().zoom, 100);
        // Setting a level that is already there changes nothing.
        assert!(dispatch(&mut b, &Command::SetZoom { tab, percent: 100 }).is_empty());
        // Clamped to the table.
        dispatch(&mut b, &Command::SetZoom { tab, percent: 9000 });
        assert_eq!(b.tab(tab).unwrap().zoom, 500);
        // A blank page zooms alone and is not remembered.
        dispatch(
            &mut b,
            &Command::Navigate {
                tab: elsewhere,
                url: url("about:blank"),
            },
        );
        let actions = dispatch(&mut b, &Command::ZoomIn(elsewhere));
        assert_eq!(
            actions,
            vec![EngineAction::SetZoom {
                tab: elsewhere,
                percent: 110
            }]
        );
        assert!(!b.zoom.sites.contains_key(""));
        // A new default reaches the tabs without a level of their own
        // (the blank page); a site with its own level keeps it.
        b.settings.page_zoom = 125;
        let actions = apply_default_zoom(&mut b);
        assert_eq!(
            actions,
            vec![EngineAction::SetZoom {
                tab: elsewhere,
                percent: 125
            }]
        );
        assert_eq!(b.tab(tab).unwrap().zoom, 500);
        assert_eq!(b.tab(other).unwrap().zoom, 500);
    }

    #[test]
    fn the_find_bar_opens_searches_steps_and_closes() {
        let (mut b, window, tab) = one_window();
        assert_eq!(
            dispatch(&mut b, &Command::OpenFind(window)),
            vec![EngineAction::FocusFind(window)]
        );
        assert_eq!(b.window(window).unwrap().find, Some(FindState::default()));
        // Nothing to step to yet.
        assert!(dispatch(&mut b, &Command::FindNext(window)).is_empty());
        let actions = dispatch(
            &mut b,
            &Command::FindText {
                window,
                text: "needle".to_string(),
                match_case: false,
            },
        );
        assert_eq!(
            actions,
            vec![EngineAction::Find {
                tab,
                text: "needle".to_string(),
                forward: true,
                match_case: false,
                find_next: false
            }]
        );
        assert_eq!(
            dispatch(&mut b, &Command::FindPrevious(window)),
            vec![EngineAction::Find {
                tab,
                text: "needle".to_string(),
                forward: false,
                match_case: false,
                find_next: true
            }]
        );
        // Reopening with text runs the search again and refocuses.
        assert_eq!(
            dispatch(&mut b, &Command::OpenFind(window)),
            vec![
                EngineAction::FocusFind(window),
                EngineAction::Find {
                    tab,
                    text: "needle".to_string(),
                    forward: true,
                    match_case: false,
                    find_next: false
                }
            ]
        );
        // Empty text clears the highlights.
        assert_eq!(
            dispatch(
                &mut b,
                &Command::FindText {
                    window,
                    text: String::new(),
                    match_case: true
                }
            ),
            vec![EngineAction::StopFinding {
                tab,
                clear_selection: true
            }]
        );
        assert_eq!(
            dispatch(&mut b, &Command::CloseFind(window)),
            vec![EngineAction::StopFinding {
                tab,
                clear_selection: false
            }]
        );
        assert!(b.window(window).unwrap().find.is_none());
        assert!(dispatch(&mut b, &Command::CloseFind(window)).is_empty());
        assert!(
            dispatch(
                &mut b,
                &Command::FindText {
                    window,
                    text: "x".to_string(),
                    match_case: false
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn a_search_follows_the_active_tab() {
        let (mut b, window, tabs) = window_with_tabs(2);
        dispatch(&mut b, &Command::OpenFind(window));
        dispatch(
            &mut b,
            &Command::FindText {
                window,
                text: "needle".to_string(),
                match_case: false,
            },
        );
        let actions = dispatch(&mut b, &Command::ActivateTab(tabs[0]));
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(tabs[1]),
                EngineAction::Show(tabs[0]),
                EngineAction::StopFinding {
                    tab: tabs[1],
                    clear_selection: true
                },
                EngineAction::Find {
                    tab: tabs[0],
                    text: "needle".to_string(),
                    forward: true,
                    match_case: false,
                    find_next: false
                }
            ]
        );
        // Closing the active tab moves the search without stopping it in
        // the tab that is gone.
        let actions = dispatch(&mut b, &Command::CloseTab(tabs[0]));
        assert!(actions.contains(&EngineAction::Find {
            tab: tabs[1],
            text: "needle".to_string(),
            forward: true,
            match_case: false,
            find_next: false
        }));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, EngineAction::StopFinding { .. }))
        );
        // A new tab: the search starts over there.
        let actions = dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://c.example/")),
            },
        );
        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::Find {
                find_next: false,
                ..
            }
        )));
    }

    fn window_with_tabs(n: usize) -> (Browser, WindowId, Vec<TabId>) {
        let (mut b, window, first) = one_window();
        let mut tabs = vec![first];
        for _ in 1..n {
            dispatch(
                &mut b,
                &Command::NewTab {
                    window,
                    url: Some(url("https://tab.example/")),
                },
            );
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
        assert!(b.bookmarks.group(Bookmarks::BAR).is_some());

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

    #[test]
    fn the_profile_dialog_is_prefilled_from_the_settings_and_saves_back() {
        use crate::profile::ProfileEntry;

        let (mut b, window, _) = window_with_tabs(1);
        b.settings.chrome_color = ChromeColor::Ocean;
        b.settings.profile.avatar = Avatar::Whale;
        // The test browser's profile is "test", not the default one.
        assert_eq!(
            dispatch(&mut b, &Command::PromptEditProfile(window)),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::Profile {
                    form: ProfileForm {
                        name: "Test".to_string(),
                        color: ChromeColor::Ocean,
                        avatar: Avatar::Whale,
                        share_bookmarks: false,
                        copy_extensions: false,
                    },
                    new: false,
                    deletable: true,
                },
            }]
        );
        // A new profile starts on the next swatch, nameless.
        assert_eq!(
            dispatch(&mut b, &Command::PromptAddProfile(window)),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::Profile {
                    form: ProfileForm {
                        color: ChromeColor::Forest,
                        ..ProfileForm::default()
                    },
                    new: true,
                    deletable: false,
                },
            }]
        );
        assert!(dispatch(&mut b, &Command::PromptEditProfile(WindowId(99))).is_empty());

        // Saving: the name is trimmed, the colour applied, sharing the
        // bookmarks switches their file, copying the extensions is an
        // action.
        let form = ProfileForm {
            name: "  Alice ".to_string(),
            color: ChromeColor::Plum,
            avatar: Avatar::Cat,
            share_bookmarks: true,
            copy_extensions: true,
        };
        assert_eq!(
            dispatch(&mut b, &Command::SaveProfile(form.clone())),
            vec![
                EngineAction::SaveSettings,
                EngineAction::SwitchBookmarks { shared: true },
                EngineAction::CopyExtensionsFromDefault,
            ]
        );
        assert_eq!(b.settings.profile.name, "Alice");
        assert_eq!(b.profile_name(), "Alice");
        assert_eq!(b.settings.chrome_color, ChromeColor::Plum);
        assert_eq!(b.settings.profile.avatar, Avatar::Cat);
        assert!(b.settings.profile.share_bookmarks);
        // Saved again unchanged: nothing switches.
        assert_eq!(
            dispatch(
                &mut b,
                &Command::SaveProfile(ProfileForm {
                    copy_extensions: false,
                    ..form
                })
            ),
            vec![EngineAction::SaveSettings]
        );

        // Creating needs a name.
        assert!(
            dispatch(
                &mut b,
                &Command::CreateProfile(ProfileForm {
                    name: "  ".to_string(),
                    ..ProfileForm::default()
                })
            )
            .is_empty()
        );
        let new = ProfileForm {
            name: " Work ".to_string(),
            ..ProfileForm::default()
        };
        assert_eq!(
            dispatch(&mut b, &Command::CreateProfile(new)),
            vec![EngineAction::CreateProfile(ProfileForm {
                name: "Work".to_string(),
                ..ProfileForm::default()
            })]
        );

        // Deleting: the current profile, or a listed one; never the
        // default, never an unknown directory.
        b.profiles = vec![ProfileEntry {
            dir: "work".to_string(),
            name: "Work".to_string(),
            avatar: Avatar::Initial,
            color: ChromeColor::Slate,
        }];
        assert_eq!(
            dispatch(
                &mut b,
                &Command::PromptDeleteProfile {
                    window,
                    profile: "work".to_string(),
                }
            ),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::DeleteProfile {
                    profile: "work".to_string(),
                    name: "Work".to_string(),
                    current: false,
                },
            }]
        );
        assert_eq!(
            dispatch(
                &mut b,
                &Command::PromptDeleteProfile {
                    window,
                    profile: "test".to_string(),
                }
            ),
            vec![EngineAction::ShowPrompt {
                window,
                prompt: Prompt::DeleteProfile {
                    profile: "test".to_string(),
                    name: "Alice".to_string(),
                    current: true,
                },
            }]
        );
        for profile in ["default", "nope"] {
            assert!(
                dispatch(
                    &mut b,
                    &Command::PromptDeleteProfile {
                        window,
                        profile: profile.to_string(),
                    }
                )
                .is_empty()
            );
            assert!(dispatch(&mut b, &Command::DeleteProfile(profile.to_string())).is_empty());
        }
        assert_eq!(
            dispatch(&mut b, &Command::DeleteProfile("work".to_string())),
            vec![EngineAction::DeleteProfile("work".to_string())]
        );
        assert_eq!(
            dispatch(&mut b, &Command::DeleteProfile("test".to_string())),
            vec![EngineAction::DeleteProfile("test".to_string())]
        );
    }

    #[test]
    fn the_default_profile_neither_shares_nor_copies() {
        let (mut b, _, _) = window_with_tabs(1);
        b.profile.name = DEFAULT_PROFILE_NAME.to_string();
        let form = ProfileForm {
            name: "Me".to_string(),
            share_bookmarks: true,
            copy_extensions: true,
            ..ProfileForm::default()
        };
        assert_eq!(
            dispatch(&mut b, &Command::SaveProfile(form)),
            vec![EngineAction::SaveSettings]
        );
        assert!(!b.settings.profile.share_bookmarks);
        assert_eq!(next_color(ChromeColor::Graphite), ChromeColor::Abyss);
        assert_eq!(next_color(ChromeColor::Peach), ChromeColor::Shell);
    }

    /// A place with no country, as one saved before countries were
    /// asked for: these tests are about what is fetched, not about
    /// which units a country implies.
    fn seattle() -> Place {
        Place {
            name: "Seattle".to_string(),
            latitude: 47.6,
            longitude: -122.3,
            country: None,
        }
    }

    #[test]
    fn a_question_opens_the_template_in_a_new_tab() {
        let (mut b, window, _) = one_window();
        let actions = dispatch(
            &mut b,
            &Command::AskInNewTab {
                window,
                kind: WidgetKind::AskClaude,
                query: "why is the sky blue?".to_string(),
            },
        );
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::Hide(_), EngineAction::CreateTab { url, .. }]
                if url.as_str() == "https://claude.ai/new?q=why+is+the+sky+blue%3F"
        ));
        // Not a question tile, or a blank question: nothing.
        assert!(
            dispatch(
                &mut b,
                &Command::AskInNewTab {
                    window,
                    kind: WidgetKind::Weather,
                    query: "x".to_string(),
                },
            )
            .is_empty()
        );
        assert!(
            dispatch(
                &mut b,
                &Command::AskInNewTab {
                    window,
                    kind: WidgetKind::AskChatGpt,
                    query: " ".to_string(),
                },
            )
            .is_empty()
        );
    }

    #[test]
    fn widget_commands_save_the_settings_when_they_change_them() {
        let (mut b, _, _) = one_window();
        assert_eq!(
            dispatch(
                &mut b,
                &Command::MoveWidget {
                    kind: WidgetKind::TopSites,
                    index: 0
                }
            ),
            vec![EngineAction::SaveSettings]
        );
        assert_eq!(b.settings.launch.widgets[0], WidgetKind::TopSites);
        assert!(
            dispatch(
                &mut b,
                &Command::MoveWidget {
                    kind: WidgetKind::TopSites,
                    index: 0
                }
            )
            .is_empty()
        );
        assert_eq!(
            dispatch(&mut b, &Command::RemoveWidget(WidgetKind::AskChatGpt)),
            vec![EngineAction::SaveSettings]
        );
        assert!(dispatch(&mut b, &Command::RemoveWidget(WidgetKind::AskChatGpt)).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::RestoreWidgets),
            vec![EngineAction::SaveSettings]
        );
        assert_eq!(b.settings.launch.widgets, WidgetKind::ALL);
        assert_eq!(
            dispatch(&mut b, &Command::HideTopSite("News.Example".into())),
            vec![EngineAction::SaveSettings]
        );
        assert_eq!(b.settings.launch.hidden_sites, vec!["news.example"]);
        let ddg = UrlTemplate::preset(1);
        assert_eq!(
            dispatch(&mut b, &Command::SetSearch(ddg.clone())),
            vec![EngineAction::SaveSettings]
        );
        assert!(dispatch(&mut b, &Command::SetSearch(ddg)).is_empty());
        let t = UrlTemplate::new("Claude", "https://claude.ai/new?q={query}&x=1");
        assert_eq!(
            dispatch(
                &mut b,
                &Command::SetAskTemplate {
                    kind: WidgetKind::AskClaude,
                    template: t.clone()
                }
            ),
            vec![EngineAction::SaveSettings]
        );
        assert_eq!(b.settings.launch.ask_claude, t);
        assert!(
            dispatch(
                &mut b,
                &Command::SetAskTemplate {
                    kind: WidgetKind::TopSites,
                    template: t
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn the_launcher_fetches_stale_weather_and_drive_but_not_fresh_or_loading() {
        let (mut b, window, _) = one_window();
        let open = Command::NewTab { window, url: None };
        // No named place, no account: the launcher and the ask for
        // this device's location, which the next opening does not
        // repeat while it is going.
        assert_eq!(
            dispatch(&mut b, &open),
            vec![
                EngineAction::ShowLauncher { window },
                EngineAction::FetchLocation { units: None }
            ]
        );
        assert_eq!(b.launch.located, Fetched::Loading);
        // A place and a connected account with nothing fetched yet.
        b.settings.launch.weather.location = Some(seattle());
        b.launch.drive = DriveState::Connected {
            account: "me@example.com".into(),
            files: Fetched::Empty,
        };
        assert_eq!(
            dispatch(&mut b, &open),
            vec![
                EngineAction::ShowLauncher { window },
                EngineAction::FetchWeather {
                    place: seattle(),
                    units: Units::Celsius
                },
                EngineAction::FetchDriveRecent,
            ]
        );
        assert_eq!(b.launch.weather, Fetched::Loading);
        // Loading: not asked again.
        assert_eq!(
            dispatch(&mut b, &open),
            vec![EngineAction::ShowLauncher { window }]
        );
        // Fresh: not asked again; stale: asked.
        let now = unix_now();
        b.launch.weather = Fetched::Ready {
            value: crate::launch::WeatherReport {
                place: "Seattle".into(),
                temperature: 12.0,
                high: 14.0,
                low: 8.0,
                code: crate::launch::WeatherCode::Rain,
                units: Units::Celsius,
            },
            at: now,
        };
        b.launch.drive = DriveState::Connected {
            account: "me@example.com".into(),
            files: Fetched::Ready {
                value: Vec::new(),
                at: now - DRIVE_LIFETIME - 1,
            },
        };
        assert_eq!(
            dispatch(&mut b, &open),
            vec![
                EngineAction::ShowLauncher { window },
                EngineAction::FetchDriveRecent
            ]
        );
        // A removed tile asks for nothing; Refresh forces the rest.
        b.settings.launch.remove_widget(WidgetKind::DriveRecent);
        assert_eq!(
            dispatch(&mut b, &Command::RefreshLaunch),
            vec![EngineAction::FetchWeather {
                place: seattle(),
                units: Units::Celsius
            }]
        );
    }

    #[test]
    fn the_weather_place_flow() {
        let (mut b, _, _) = one_window();
        assert!(dispatch(&mut b, &Command::LookUpPlace("  ".into())).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::LookUpPlace(" Seattle ".into())),
            vec![EngineAction::LookUpPlace("Seattle".into())]
        );
        assert_eq!(b.launch.places, Fetched::Loading);
        assert_eq!(
            dispatch(&mut b, &Command::SetWeatherPlace(Some(seattle()))),
            vec![
                EngineAction::SaveSettings,
                EngineAction::FetchWeather {
                    place: seattle(),
                    units: Units::Celsius
                }
            ]
        );
        assert_eq!(b.launch.places, Fetched::Empty);
        assert_eq!(b.settings.launch.weather.location, Some(seattle()));
        assert_eq!(
            dispatch(&mut b, &Command::SetWeatherUnits(Units::Fahrenheit)),
            vec![
                EngineAction::SaveSettings,
                EngineAction::FetchWeather {
                    place: seattle(),
                    units: Units::Fahrenheit
                }
            ]
        );
        assert!(dispatch(&mut b, &Command::SetWeatherUnits(Units::Fahrenheit)).is_empty());
        // Handing the place back means "follow this device" again,
        // which asks the system where it is.
        assert_eq!(
            dispatch(&mut b, &Command::SetWeatherPlace(None)),
            vec![
                EngineAction::SaveSettings,
                EngineAction::FetchLocation {
                    units: Some(Units::Fahrenheit)
                }
            ]
        );
        assert_eq!(b.launch.weather, Fetched::Empty);
    }

    /// Nobody having chosen units — no country in the locale when the
    /// profile was made, and no visit to the Launch Settings — the
    /// country the tile is showing decides them. The fetcher is asked
    /// for a location with no units at all, because the country it
    /// lands in is news to the model too.
    #[test]
    fn the_units_follow_the_country_until_someone_chooses() {
        let (mut b, _, _) = one_window();
        let now = unix_now();
        assert_eq!(b.settings.launch.weather.units, None);
        assert_eq!(
            dispatch(&mut b, &Command::RefreshLaunch),
            vec![EngineAction::FetchLocation { units: None }]
        );
        // The fix landed in the United States, so the forecasts that
        // follow are in Fahrenheit without anyone saying so.
        let denver = Place {
            name: "Denver, Colorado".into(),
            latitude: 39.74,
            longitude: -104.98,
            country: Some("US".into()),
        };
        b.launch.located = Fetched::Ready {
            value: denver.clone(),
            at: now,
        };
        b.launch.weather = Fetched::Empty;
        assert_eq!(
            dispatch(&mut b, &Command::RefreshLaunch),
            vec![EngineAction::FetchWeather {
                place: denver.clone(),
                units: Units::Fahrenheit
            }]
        );
        // Choosing what was being followed anyway changes no fetch,
        // but it is still a choice: it is saved, and it stands when
        // the device moves to a country that measures differently.
        assert_eq!(
            dispatch(&mut b, &Command::SetWeatherUnits(Units::Fahrenheit)),
            vec![EngineAction::SaveSettings]
        );
        assert_eq!(
            b.settings.launch.weather.units,
            Some(Units::Fahrenheit),
            "the choice is stored"
        );
        b.launch.located = Fetched::Ready {
            value: Place {
                name: "Berlin".into(),
                latitude: 52.52,
                longitude: 13.4,
                country: Some("DE".into()),
            },
            at: now,
        };
        assert_eq!(
            b.settings.launch.weather.units(&b.launch.located),
            Units::Fahrenheit
        );
    }

    /// With no place named, the tile follows the device: the system is
    /// asked once, its answer is used for the forecast and reused for
    /// an hour, a refusal is not put to the user again unless they ask,
    /// and a fix that ages out is asked for afresh.
    #[test]
    fn the_weather_follows_this_device_when_no_place_is_named() {
        let (mut b, _, _) = one_window();
        let now = unix_now();
        let here = || Place {
            name: "Current location".into(),
            latitude: 47.6,
            longitude: -122.3,
            country: None,
        };
        // Nothing known: ask the system, and not twice over.
        assert_eq!(
            dispatch(&mut b, &Command::RefreshLaunch),
            vec![EngineAction::FetchLocation { units: None }]
        );
        assert!(dispatch(&mut b, &Command::RefreshLaunch).is_empty());
        // The answer arrived: its forecast is what the tile asks for,
        // and the fix itself is not asked for again within the hour,
        // even when the units change (which forces a refresh).
        b.launch.located = Fetched::Ready {
            value: here(),
            at: now,
        };
        b.launch.weather = Fetched::Empty;
        assert_eq!(
            dispatch(&mut b, &Command::RefreshLaunch),
            vec![EngineAction::FetchWeather {
                place: here(),
                units: Units::Celsius
            }]
        );
        assert_eq!(b.launch.weather, Fetched::Loading);
        // A refusal stands until the user asks again; `RefreshLaunch`
        // is that asking, and an unforced opening leaves it alone.
        b.launch.located = Fetched::Failed("No.".into());
        b.launch.weather = Fetched::Empty;
        assert!(launch_fetches(&mut b, false).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::RefreshLaunch),
            vec![EngineAction::FetchLocation { units: None }]
        );
        // A fix older than its lifetime is asked for afresh.
        b.launch.located = Fetched::Ready {
            value: here(),
            at: now - LOCATION_LIFETIME - 1,
        };
        assert_eq!(
            launch_fetches(&mut b, false),
            vec![EngineAction::FetchLocation { units: None }]
        );
    }

    #[test]
    fn the_google_connection_flow() {
        let (mut b, window, _) = one_window();
        // Unavailable: nothing happens.
        assert!(dispatch(&mut b, &Command::ConnectGoogle(window)).is_empty());
        b.launch.drive = DriveState::Disconnected;
        assert_eq!(
            dispatch(&mut b, &Command::ConnectGoogle(window)),
            vec![EngineAction::BeginGoogleSignIn { window }]
        );
        assert_eq!(b.launch.drive, DriveState::Connecting);
        // Twice: not while one is going.
        assert!(dispatch(&mut b, &Command::ConnectGoogle(window)).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::DisconnectGoogle),
            vec![EngineAction::ForgetGoogle]
        );
        assert_eq!(b.launch.drive, DriveState::Disconnected);
        assert!(dispatch(&mut b, &Command::DisconnectGoogle).is_empty());
    }

    /// A session of three tabs on a, b and c, the `active` one first shown.
    fn three_sites_session(active: usize) -> Session {
        let mut s = saved_session();
        s.windows.truncate(1);
        let w = &mut s.windows[0];
        w.active = active;
        w.folders.clear();
        w.tabs.truncate(3);
        for t in &mut w.tabs {
            t.folder = None;
        }
        w.tabs[0].title = "A".to_string();
        w.tabs[1].title = "B".to_string();
        w.tabs[2].title = "C".to_string();
        s
    }

    #[test]
    fn a_typed_address_on_a_blocked_site_shows_the_block_page_over_blank() {
        // AC3.1
        let (mut b, _window, tab) = one_window();
        b.tab_mut(tab).unwrap().title = "Before".to_string();
        b.blocked.add("b.example");
        let actions = dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("https://www.b.example/x"),
            },
        );
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("about:blank"),
            }]
        );
        let t = b.tab(tab).unwrap();
        assert_eq!(t.url.as_str(), "https://www.b.example/x");
        assert_eq!(t.blocked.as_ref().unwrap().site, "b.example");
        assert_eq!(t.load, LoadState::Loading);
        assert!(t.title.is_empty());
        assert!(t.favicon.is_none());

        // Another site is loaded as it is, and the block page clears.
        let actions = dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("https://c.example/"),
            },
        );
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("https://c.example/"),
            }]
        );
        assert!(b.tab(tab).unwrap().blocked.is_none());
    }

    #[test]
    fn a_new_tab_a_search_and_a_question_on_a_blocked_site_are_blocked() {
        // AC3.2
        let (mut b, window, tab) = one_window();
        b.blocked.add("b.example");

        let actions = dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://b.example/")),
            },
        );
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::Hide(_), EngineAction::CreateTab { url, .. }]
                if *url == url_of("about:blank")
        ));
        let w = b.window(window).unwrap();
        let new_tab = w.active;
        let nt = b.tab(new_tab).unwrap();
        assert_eq!(nt.url.as_str(), "https://b.example/");
        assert_eq!(nt.blocked.as_ref().unwrap().site, "b.example");

        b.settings.search =
            crate::settings::UrlTemplate::new("Test", "https://b.example/?q={query}");
        let actions = dispatch(
            &mut b,
            &Command::Search {
                tab,
                query: "q".to_string(),
            },
        );
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("about:blank"),
            }]
        );
        assert_eq!(b.tab(tab).unwrap().url.as_str(), "https://b.example/?q=q");
        assert!(b.tab(tab).unwrap().blocked.is_some());

        let actions = dispatch(
            &mut b,
            &Command::SearchInNewTab {
                window,
                query: "q".to_string(),
            },
        );
        assert!(actions.iter().any(
            |a| matches!(a, EngineAction::CreateTab { url, .. } if *url == url_of("about:blank"))
        ));
        let active = b.window(window).unwrap().active;
        assert!(b.tab(active).unwrap().blocked.is_some());

        b.settings.launch.ask_claude =
            crate::settings::UrlTemplate::new("Ask", "https://b.example/ask?q={query}");
        let actions = dispatch(
            &mut b,
            &Command::AskInNewTab {
                window,
                kind: WidgetKind::AskClaude,
                query: "q".to_string(),
            },
        );
        assert!(actions.iter().any(
            |a| matches!(a, EngineAction::CreateTab { url, .. } if *url == url_of("about:blank"))
        ));
        let active = b.window(window).unwrap().active;
        assert!(b.tab(active).unwrap().blocked.is_some());

        let actions = dispatch(
            &mut b,
            &Command::NewBackgroundTab {
                opener: tab,
                url: url("https://b.example/bg"),
            },
        );
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::CreateHiddenTab { url, .. }] if *url == url_of("about:blank")
        ));
        let bg = b
            .window(window)
            .unwrap()
            .tabs
            .iter()
            .find(|t| t.url.as_str() == "https://b.example/bg")
            .unwrap();
        assert!(bg.blocked.is_some());

        let actions = dispatch(
            &mut b,
            &Command::OpenInNewWindow(url("https://b.example/w")),
        );
        assert!(actions.iter().any(
            |a| matches!(a, EngineAction::OpenWindow { url, .. } if *url == url_of("about:blank"))
        ));
        let new_window = b.windows.last().unwrap();
        assert!(new_window.tabs[0].blocked.is_some());
    }

    #[test]
    fn a_restored_tab_on_a_blocked_site_wakes_on_the_block_page_and_is_not_warmed() {
        // AC3.3, AC2.6
        let mut b = browser();
        b.blocked.add("b.example");
        let actions = restore_session(&mut b, &three_sites_session(0));
        let w = &b.windows[0];
        let (a, bb, c) = (w.tabs[0].id, w.tabs[1].id, w.tabs[2].id);
        // Only the c tab is warmed: a is active, b is blocked.
        assert!(actions.contains(&EngineAction::WarmTabs(vec![c])));
        let t = b.tab(bb).unwrap();
        assert!(t.deferred);
        assert!(t.blocked.is_some());
        assert!(t.title.is_empty());
        assert_eq!(b.tab(a).unwrap().title, "A");
        assert_eq!(b.tab(c).unwrap().title, "C");

        // Warming the blocked tab does nothing.
        assert!(dispatch(&mut b, &Command::WarmTab(bb)).is_empty());

        // Showing it creates the tab on the block page, over blank.
        let actions = dispatch(&mut b, &Command::ActivateTab(bb));
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::Hide(_), EngineAction::CreateTab { url, .. }]
                if *url == url_of("about:blank")
        ));
        let t = b.tab(bb).unwrap();
        assert!(!t.deferred);
        assert!(t.blocked.is_some());
        assert_eq!(t.url.as_str(), "https://b.example/");

        // A blocked-then-unblocked site wakes on its own page.
        let mut b = browser();
        b.blocked.add("b.example");
        restore_session(&mut b, &three_sites_session(0));
        let bb = b.windows[0].tabs[1].id;
        b.blocked.remove("b.example");
        let actions = dispatch(&mut b, &Command::ActivateTab(bb));
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::Hide(_), EngineAction::CreateTab { url, .. }]
                if *url == url_of("https://b.example/")
        ));
        assert!(b.tab(bb).unwrap().blocked.is_none());
    }

    #[test]
    fn a_restored_active_tab_on_a_blocked_site_opens_on_the_block_page() {
        // AC3.3
        let mut b = browser();
        b.blocked.add("b.example");
        restore_session(&mut b, &three_sites_session(1));
        let w = &b.windows[0];
        let active = w.tabs[w.index_of(w.active).unwrap()].clone();
        assert!(active.blocked.is_some());
        assert!(active.title.is_empty());
    }

    #[test]
    fn closing_the_active_tab_shows_a_deferred_blocked_tab_on_the_block_page() {
        // AC3.3, AC2.6: the second wake re-check, in close_tab
        let mut b = browser();
        b.blocked.add("b.example");
        restore_session(&mut b, &three_sites_session(0));
        let w = &b.windows[0];
        let (a, bb) = (w.tabs[0].id, w.tabs[1].id);

        let actions = dispatch(&mut b, &Command::CloseTab(a));
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::CloseTab(closed), EngineAction::CreateTab { tab, url, .. }]
                if *closed == a && *tab == bb && *url == url_of("about:blank")
        ));
        let t = b.tab(bb).unwrap();
        assert!(!t.deferred);
        assert!(t.blocked.is_some());
        assert_eq!(t.url.as_str(), "https://b.example/");
    }

    #[test]
    fn closing_the_active_tab_shows_an_unblocked_deferred_tab_at_its_own_url() {
        // AC3.3: the wake re-check on close_tab finds the site unblocked
        let mut b = browser();
        b.blocked.add("b.example");
        restore_session(&mut b, &three_sites_session(0));
        let w = &b.windows[0];
        let (a, bb) = (w.tabs[0].id, w.tabs[1].id);
        b.blocked.remove("b.example");

        let actions = dispatch(&mut b, &Command::CloseTab(a));
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::CloseTab(closed), EngineAction::CreateTab { tab, url, .. }]
                if *closed == a && *tab == bb && *url == url_of("https://b.example/")
        ));
        let t = b.tab(bb).unwrap();
        assert!(!t.deferred);
        assert!(t.blocked.is_none());
    }

    #[test]
    fn reload_on_a_block_page_rerolls_the_line_and_asks_the_engine_nothing() {
        // AC3.7
        let (mut b, _window, tab) = one_window();
        b.blocked.add("b.example");
        dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("https://b.example/"),
            },
        );
        let first = b.tab(tab).unwrap().blocked.as_ref().unwrap().line;
        assert!(dispatch(&mut b, &Command::Reload(tab)).is_empty());
        let second = b.tab(tab).unwrap().blocked.as_ref().unwrap().line;
        assert_ne!(first, second);
        assert!(dispatch(&mut b, &Command::Reload(tab)).is_empty());
        let third = b.tab(tab).unwrap().blocked.as_ref().unwrap().line;
        assert_ne!(second, third);

        // An unblocked tab still reloads in the engine.
        let (mut plain, _w, ptab) = one_window();
        assert_eq!(
            dispatch(&mut plain, &Command::Reload(ptab)),
            vec![EngineAction::Reload(ptab)]
        );
    }

    fn url_of(s: &str) -> Url {
        url(s)
    }

    /// A browser with one window per list of URLs (the first opened by
    /// `open_window`, the rest as new tabs of it). Returns the tabs in
    /// strip order.
    fn window_with(urls: &[&str]) -> (Browser, WindowId, Vec<TabId>) {
        let mut b = browser();
        let actions = open_window(&mut b, url(urls[0]));
        let EngineAction::OpenWindow { window, tab, .. } = actions[0].clone() else {
            panic!("open_window returns OpenWindow");
        };
        let mut tabs = vec![tab];
        for u in &urls[1..] {
            dispatch(
                &mut b,
                &Command::NewTab {
                    window,
                    url: Some(url(u)),
                },
            );
            tabs.push(b.window(window).unwrap().active);
        }
        (b, window, tabs)
    }

    #[test]
    fn the_block_prompt_names_the_distinct_unblocked_sites_or_nothing() {
        // AC2.2
        let (mut b, window, tabs) = window_with(&[
            "https://www.a.example/",
            "https://m.a.example/",
            "https://b.example/",
        ]);
        let actions = dispatch(
            &mut b,
            &Command::PromptBlockSites {
                window,
                tabs: tabs.clone(),
            },
        );
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::ShowPrompt { window: w, prompt: Prompt::BlockSites { sites } }]
                if *w == window && sites == &vec!["a.example".to_string(), "b.example".to_string()]
        ));

        b.blocked.add("a.example");
        let actions = dispatch(
            &mut b,
            &Command::PromptBlockSites {
                window,
                tabs: tabs.clone(),
            },
        );
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::ShowPrompt { prompt: Prompt::BlockSites { sites }, .. }]
                if sites == &vec!["b.example".to_string()]
        ));

        b.blocked.add("b.example");
        assert!(
            dispatch(
                &mut b,
                &Command::PromptBlockSites {
                    window,
                    tabs: tabs.clone(),
                },
            )
            .is_empty()
        );

        // Only an about:blank tab: nothing to ask.
        let (mut blank, bw, btabs) = window_with(&["about:blank"]);
        assert!(
            dispatch(
                &mut blank,
                &Command::PromptBlockSites {
                    window: bw,
                    tabs: btabs,
                },
            )
            .is_empty()
        );

        // An unknown window: nothing.
        assert!(
            dispatch(
                &mut b,
                &Command::PromptBlockSites {
                    window: WindowId(999),
                    tabs,
                },
            )
            .is_empty()
        );
    }

    #[test]
    fn blocking_replaces_every_live_tab_on_the_site_in_every_window() {
        // AC2.4, AC2.5, AC4.5
        let mut b = browser();
        let first = open_window(&mut b, url("https://a.example/"));
        let EngineAction::OpenWindow {
            window: _w1,
            tab: t1,
            ..
        } = first[0].clone()
        else {
            panic!("open_window returns OpenWindow");
        };
        let second = open_window(&mut b, url("https://m.a.example/x"));
        let EngineAction::OpenWindow {
            window: w2,
            tab: t2,
            ..
        } = second[0].clone()
        else {
            panic!("open_window returns OpenWindow");
        };
        dispatch(
            &mut b,
            &Command::NewTab {
                window: w2,
                url: Some(url("https://b.example/")),
            },
        );
        let t3 = b.window(w2).unwrap().active;

        let actions = dispatch(&mut b, &Command::BlockSites(vec!["a.example".to_string()]));
        assert_eq!(actions.len(), 4, "{actions:?}");
        assert_eq!(actions[0], EngineAction::ReplaceWithBlank(t1));
        assert_eq!(actions[1], EngineAction::ReplaceWithBlank(t2));
        assert_eq!(actions[2], EngineAction::SaveBlockedSites);
        let EngineAction::ConfigureBlockedSites(sites) = &actions[3] else {
            panic!("the engine is told the list last");
        };
        assert_eq!(sites.sites.len(), 1);
        assert!(sites.sites.contains("a.example"));

        for tab in [t1, t2] {
            let t = b.tab(tab).unwrap();
            assert_eq!(t.blocked.as_ref().unwrap().site, "a.example");
        }
        assert_eq!(b.tab(t2).unwrap().url.as_str(), "https://m.a.example/x");
        assert!(b.tab(t3).unwrap().blocked.is_none());
        assert!(b.blocked.sites.contains("a.example"));
    }

    #[test]
    fn blocking_a_deferred_tab_only_marks_it() {
        // AC2.6
        let mut b = browser();
        restore_session(&mut b, &three_sites_session(0));
        let bb = b.windows[0].tabs[1].id;
        let actions = dispatch(&mut b, &Command::BlockSites(vec!["b.example".to_string()]));
        assert_eq!(actions.len(), 2, "{actions:?}");
        assert!(matches!(actions[0], EngineAction::SaveBlockedSites));
        assert!(matches!(actions[1], EngineAction::ConfigureBlockedSites(_)));
        let t = b.tab(bb).unwrap();
        assert!(t.deferred);
        assert!(t.blocked.is_some());

        // A warming tab is closed and is no longer warming.
        let mut b = browser();
        restore_session(&mut b, &three_sites_session(0));
        let bb = b.windows[0].tabs[1].id;
        assert!(matches!(
            dispatch(&mut b, &Command::WarmTab(bb)).as_slice(),
            [EngineAction::CreateHiddenTab { .. }]
        ));
        let actions = dispatch(&mut b, &Command::BlockSites(vec!["b.example".to_string()]));
        assert!(actions.contains(&EngineAction::CloseTab(bb)));
        let t = b.tab(bb).unwrap();
        assert!(!t.warming);
        assert!(t.deferred);
        assert!(t.blocked.is_some());
    }

    #[test]
    fn blocking_a_site_already_blocked_changes_nothing() {
        // AC4.6
        let (mut b, _window, _tab) = one_window();
        b.blocked.add("a.example");
        assert!(dispatch(&mut b, &Command::BlockSites(vec!["a.example".to_string()])).is_empty());
    }

    #[test]
    fn unblocking_sends_every_tab_blocked_on_the_site_back_to_its_page() {
        // AC4.4
        let (mut b, window, t1) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://a.example/x")),
            },
        );
        let t2 = b.window(window).unwrap().active;
        let deferred_id = b.allocate_tab_id();
        let mut deferred = Tab::new(deferred_id, url("https://a.example/d"));
        deferred.deferred = true;
        b.window_mut(window).unwrap().tabs.push(deferred);

        dispatch(&mut b, &Command::BlockSites(vec!["a.example".to_string()]));
        assert!(b.tab(deferred_id).unwrap().blocked.is_some());

        let actions = dispatch(&mut b, &Command::UnblockSite("a.example".to_string()));
        assert_eq!(actions.len(), 4, "{actions:?}");
        assert_eq!(actions[0], EngineAction::SaveBlockedSites);
        let EngineAction::ConfigureBlockedSites(sites) = &actions[1] else {
            panic!("the engine is told the list before any navigation");
        };
        assert!(sites.sites.is_empty());
        assert_eq!(
            actions[2],
            EngineAction::Navigate {
                tab: t1,
                url: url("https://a.example/"),
            }
        );
        assert_eq!(
            actions[3],
            EngineAction::Navigate {
                tab: t2,
                url: url("https://a.example/x"),
            }
        );
        for tab in [t1, t2, deferred_id] {
            assert!(b.tab(tab).unwrap().blocked.is_none());
        }
        assert!(b.tab(deferred_id).unwrap().deferred);

        assert!(dispatch(&mut b, &Command::UnblockSite("a.example".to_string())).is_empty());
    }

    /// Send `request` from the extension "ext" through `apply`, as the
    /// engine does.
    fn ask(b: &mut Browser, token: u64, request: ExtensionRequest) -> Vec<EngineAction> {
        crate::event::apply(
            b,
            &crate::event::EngineEvent::ExtensionRequest {
                token,
                extension: "ext".to_string(),
                request,
            },
        )
    }

    fn reply(token: u64, reply: ExtensionReply) -> EngineAction {
        EngineAction::ExtensionReply { token, reply }
    }

    fn refused(token: u64, why: &str) -> EngineAction {
        reply(token, ExtensionReply::Refused(why.to_string()))
    }

    /// Open "ext"'s `page` in the panel of `tab`, or of `window`, and
    /// return its view.
    fn open_panel(b: &mut Browser, window: WindowId, tab: Option<TabId>, page: &str) -> TabId {
        let actions = ask(
            b,
            0,
            ExtensionRequest::OpenSidePanel {
                window,
                tab,
                page: page.to_string(),
            },
        );
        let Some(EngineAction::CreateView { view, .. }) = actions.iter().rev().nth(1) else {
            panic!("OpenSidePanel returns CreateView before its reply: {actions:?}");
        };
        *view
    }

    #[test]
    fn opening_a_side_panel_creates_a_view_the_model_names() {
        let (mut b, window, tab) = one_window();
        let actions = ask(
            &mut b,
            7,
            ExtensionRequest::OpenSidePanel {
                window,
                tab: None,
                page: "panel.html".to_string(),
            },
        );
        let view = TabId(tab.0 + 1);
        assert_eq!(
            actions,
            vec![
                EngineAction::CreateView {
                    window,
                    view,
                    extension: "ext".to_string(),
                    page: "panel.html".to_string(),
                },
                reply(7, ExtensionReply::Done),
            ]
        );
        let w = b.window(window).unwrap();
        assert_eq!(
            w.panel,
            Some(SidePanel {
                extension: "ext".to_string(),
                page: "panel.html".to_string(),
                view,
            })
        );
        assert_eq!(w.shown_panel(), w.panel.as_ref());
        // The view is no tab, and no tab is given its id afterwards.
        assert_eq!(order(&b, window), vec![tab]);
        assert!(b.tab(view).is_none());
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://tab.example/")),
            },
        );
        assert_eq!(b.window(window).unwrap().active, TabId(view.0 + 1));
    }

    #[test]
    fn a_panel_opened_for_a_tab_shows_only_while_that_tab_is_active() {
        // side-panel.AC1.4
        let (mut b, window, t) = window_with_tabs(3);
        let for_window = open_panel(&mut b, window, None, "window.html");
        let for_tab = open_panel(&mut b, window, Some(t[0]), "tab.html");
        assert_eq!(b.tab(t[0]).unwrap().panel.as_ref().unwrap().view, for_tab);
        let shown = |b: &Browser| b.window(window).unwrap().shown_panel().map(|p| p.view);
        // t[2] is active and has none of its own: the window's shows.
        assert_eq!(shown(&b), Some(for_window));
        dispatch(&mut b, &Command::ActivateTab(t[0]));
        assert_eq!(shown(&b), Some(for_tab));
        dispatch(&mut b, &Command::ActivateTab(t[1]));
        assert_eq!(shown(&b), Some(for_window));
        // Both stay in the model whichever shows.
        let w = b.window(window).unwrap();
        assert!(w.has_panel_view(for_window) && w.has_panel_view(for_tab));
    }

    #[test]
    fn the_same_panel_opened_again_changes_nothing() {
        let (mut b, window, tab) = one_window();
        let view = open_panel(&mut b, window, Some(tab), "panel.html");
        let before = b.windows.clone();
        let actions = ask(
            &mut b,
            3,
            ExtensionRequest::OpenSidePanel {
                window,
                tab: Some(tab),
                page: "panel.html".to_string(),
            },
        );
        assert_eq!(actions, vec![reply(3, ExtensionReply::Done)]);
        assert_eq!(b.windows, before);
        assert_eq!(b.tab(tab).unwrap().panel.as_ref().unwrap().view, view);
    }

    #[test]
    fn another_page_replaces_the_panel_closing_the_old_view_first() {
        let (mut b, window, tab) = one_window();
        let old = open_panel(&mut b, window, None, "one.html");
        let actions = ask(
            &mut b,
            4,
            ExtensionRequest::OpenSidePanel {
                window,
                tab: None,
                page: "two.html".to_string(),
            },
        );
        let new = TabId(old.0 + 1);
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseView { window, view: old },
                EngineAction::CreateView {
                    window,
                    view: new,
                    extension: "ext".to_string(),
                    page: "two.html".to_string(),
                },
                reply(4, ExtensionReply::Done),
            ]
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.panel.as_ref().unwrap().page, "two.html");
        assert!(w.has_panel_view(new) && !w.has_panel_view(old));
        // Another extension asking for the same place replaces it too.
        let actions = crate::event::apply(
            &mut b,
            &crate::event::EngineEvent::ExtensionRequest {
                token: 5,
                extension: "other".to_string(),
                request: ExtensionRequest::OpenSidePanel {
                    window,
                    tab: None,
                    page: "two.html".to_string(),
                },
            },
        );
        assert_eq!(actions[0], EngineAction::CloseView { window, view: new });
        assert_eq!(
            b.window(window).unwrap().panel.as_ref().unwrap().extension,
            "other"
        );
        assert_eq!(order(&b, window), vec![tab]);
    }

    #[test]
    fn a_side_panel_for_an_unknown_window_or_tab_is_refused() {
        // side-panel.AC5.3 at the model level
        let (mut b, window, tab) = one_window();
        let other = dispatch(&mut b, &Command::NewWindow);
        let EngineAction::OpenWindow {
            window: w2,
            tab: t2,
            ..
        } = other[0].clone()
        else {
            panic!("NewWindow returns OpenWindow");
        };
        let before = b.windows.clone();
        let open = |window, tab| ExtensionRequest::OpenSidePanel {
            window,
            tab,
            page: "panel.html".to_string(),
        };
        assert_eq!(
            ask(&mut b, 1, open(WindowId(99), None)),
            vec![refused(1, "No window with id: 99.")]
        );
        assert_eq!(
            ask(&mut b, 2, open(window, Some(TabId(98)))),
            vec![refused(2, "No tab with id: 98.")]
        );
        // A tab of another window is no tab of this one.
        assert_eq!(
            ask(&mut b, 3, open(window, Some(t2))),
            vec![refused(3, &format!("No tab with id: {}.", t2.0))]
        );
        assert_eq!(
            ask(
                &mut b,
                4,
                ExtensionRequest::CloseSidePanel {
                    window: WindowId(99),
                    tab: None
                }
            ),
            vec![refused(4, "No window with id: 99.")]
        );
        assert_eq!(
            ask(
                &mut b,
                5,
                ExtensionRequest::CloseSidePanel {
                    window: w2,
                    tab: Some(tab)
                }
            ),
            vec![refused(5, &format!("No tab with id: {}.", tab.0))]
        );
        assert_eq!(b.windows, before, "a refused request changes nothing");
    }

    #[test]
    fn an_extension_closes_its_own_panel_and_no_other() {
        let (mut b, window, tab) = one_window();
        let for_window = open_panel(&mut b, window, None, "window.html");
        let for_tab = open_panel(&mut b, window, Some(tab), "tab.html");
        // Only the place named is closed.
        assert_eq!(
            ask(
                &mut b,
                1,
                ExtensionRequest::CloseSidePanel {
                    window,
                    tab: Some(tab)
                }
            ),
            vec![
                EngineAction::CloseView {
                    window,
                    view: for_tab
                },
                reply(1, ExtensionReply::Done),
            ]
        );
        assert!(b.tab(tab).unwrap().panel.is_none());
        assert!(b.window(window).unwrap().panel.is_some());
        // Closing what is not open is done, with nothing to do.
        assert_eq!(
            ask(
                &mut b,
                2,
                ExtensionRequest::CloseSidePanel {
                    window,
                    tab: Some(tab)
                }
            ),
            vec![reply(2, ExtensionReply::Done)]
        );
        // Another extension's panel stays.
        let actions = crate::event::apply(
            &mut b,
            &crate::event::EngineEvent::ExtensionRequest {
                token: 3,
                extension: "other".to_string(),
                request: ExtensionRequest::CloseSidePanel { window, tab: None },
            },
        );
        assert_eq!(actions, vec![reply(3, ExtensionReply::Done)]);
        assert_eq!(
            b.window(window).unwrap().panel.as_ref().unwrap().view,
            for_window
        );
        assert_eq!(
            ask(
                &mut b,
                4,
                ExtensionRequest::CloseSidePanel { window, tab: None }
            ),
            vec![
                EngineAction::CloseView {
                    window,
                    view: for_window
                },
                reply(4, ExtensionReply::Done),
            ]
        );
        assert_eq!(b.window(window).unwrap().shown_panel(), None);
    }

    #[test]
    fn closing_a_tab_closes_its_panel() {
        let (mut b, window, t) = window_with_tabs(3);
        let on_first = open_panel(&mut b, window, Some(t[0]), "first.html");
        let on_last = open_panel(&mut b, window, Some(t[2]), "last.html");
        // An inactive tab: its engine tab, then its panel's view.
        assert_eq!(
            dispatch(&mut b, &Command::CloseTab(t[0])),
            vec![
                EngineAction::CloseTab(t[0]),
                EngineAction::CloseView {
                    window,
                    view: on_first
                },
            ]
        );
        // The active tab: the same, then the tab that takes its place.
        assert_eq!(
            dispatch(&mut b, &Command::CloseTab(t[2])),
            vec![
                EngineAction::CloseTab(t[2]),
                EngineAction::CloseView {
                    window,
                    view: on_last
                },
                EngineAction::Show(t[1]),
            ]
        );
        let w = b.window(window).unwrap();
        assert!(!w.has_panel_view(on_first) && !w.has_panel_view(on_last));
        assert_eq!(w.shown_panel(), None);
    }

    #[test]
    fn a_tab_the_page_closed_or_that_moved_away_takes_its_panel_too() {
        let (mut b, window, t) = window_with_tabs(3);
        let on_first = open_panel(&mut b, window, Some(t[0]), "first.html");
        let on_second = open_panel(&mut b, window, Some(t[1]), "second.html");
        // The page closed its own tab (`window.close()`).
        assert_eq!(
            crate::event::apply(&mut b, &crate::event::EngineEvent::TabClosed(t[0])),
            vec![
                EngineAction::CloseTab(t[0]),
                EngineAction::CloseView {
                    window,
                    view: on_first
                },
            ]
        );
        // A tab moved to a new window is recreated there and closed
        // here: its panel does not follow.
        let actions = dispatch(&mut b, &Command::MoveTabsToNewWindow(vec![t[1]]));
        assert!(
            actions.contains(&EngineAction::CloseView {
                window,
                view: on_second
            }),
            "{actions:?}"
        );
        assert!(b.windows.iter().all(|w| w.panels().next().is_none()));
    }

    #[test]
    fn closing_the_last_tab_closes_both_panels_before_the_window() {
        let (mut b, window, tab) = one_window();
        let for_window = open_panel(&mut b, window, None, "window.html");
        let for_tab = open_panel(&mut b, window, Some(tab), "tab.html");
        assert_eq!(
            dispatch(&mut b, &Command::CloseTab(tab)),
            vec![
                EngineAction::CloseTab(tab),
                EngineAction::CloseView {
                    window,
                    view: for_tab
                },
                EngineAction::CloseView {
                    window,
                    view: for_window
                },
                EngineAction::CloseWindow(window),
                EngineAction::Quit,
            ]
        );
        assert!(b.windows.is_empty());
    }

    #[test]
    fn closing_a_window_closes_every_panel_in_it() {
        let (mut b, window, t) = window_with_tabs(2);
        let for_window = open_panel(&mut b, window, None, "window.html");
        let on_second = open_panel(&mut b, window, Some(t[1]), "second.html");
        let on_first = open_panel(&mut b, window, Some(t[0]), "first.html");
        assert_eq!(
            dispatch(&mut b, &Command::CloseWindow(window)),
            vec![
                EngineAction::CloseTab(t[0]),
                EngineAction::CloseTab(t[1]),
                // The tabs' panels in strip order, then the window's.
                EngineAction::CloseView {
                    window,
                    view: on_first
                },
                EngineAction::CloseView {
                    window,
                    view: on_second
                },
                EngineAction::CloseView {
                    window,
                    view: for_window
                },
                EngineAction::CloseWindow(window),
                EngineAction::Quit,
            ]
        );
    }

    #[test]
    fn the_close_command_closes_the_shown_panel_the_tabs_before_the_windows() {
        // side-panel.AC1.5
        let (mut b, window, t) = window_with_tabs(2);
        let for_window = open_panel(&mut b, window, None, "window.html");
        let for_tab = open_panel(&mut b, window, Some(t[1]), "tab.html");
        let close = Command::CloseSidePanel { window };
        // t[1] is active: its own panel is the one shown, and goes.
        assert_eq!(
            dispatch(&mut b, &close),
            vec![EngineAction::CloseView {
                window,
                view: for_tab
            }]
        );
        assert!(b.tab(t[1]).unwrap().panel.is_none());
        // The window's shows now, and goes next.
        assert_eq!(
            b.window(window).unwrap().shown_panel().map(|p| p.view),
            Some(for_window)
        );
        assert_eq!(
            dispatch(&mut b, &close),
            vec![EngineAction::CloseView {
                window,
                view: for_window
            }]
        );
        assert_eq!(b.window(window).unwrap().shown_panel(), None);
        // Nothing shown, or no such window: nothing.
        assert!(dispatch(&mut b, &close).is_empty());
        assert!(
            dispatch(
                &mut b,
                &Command::CloseSidePanel {
                    window: WindowId(99)
                }
            )
            .is_empty()
        );
        // A panel of a tab that is not active is not the shown one.
        let hidden = open_panel(&mut b, window, Some(t[0]), "hidden.html");
        assert!(dispatch(&mut b, &close).is_empty());
        assert!(b.window(window).unwrap().has_panel_view(hidden));
    }

    #[test]
    fn an_extension_opens_a_tab_active_or_behind_and_is_told_its_id() {
        // side-panel.AC3.3 at the model level
        let (mut b, window, t) = window_with_tabs(2);
        dispatch(&mut b, &Command::ActivateTab(t[0]));
        let actions = ask(
            &mut b,
            1,
            ExtensionRequest::OpenTab {
                window: Some(window),
                url: url("https://new.example/"),
                active: true,
            },
        );
        let opened = b.window(window).unwrap().active;
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(t[0]),
                EngineAction::CreateTab {
                    window,
                    tab: opened,
                    url: url("https://new.example/"),
                },
                reply(1, ExtensionReply::Tab(opened)),
            ]
        );
        assert_eq!(order(&b, window), vec![t[0], t[1], opened]);
        // Not active: created hidden at the end, the active tab kept.
        let actions = ask(
            &mut b,
            2,
            ExtensionRequest::OpenTab {
                window: Some(window),
                url: url("https://behind.example/"),
                active: false,
            },
        );
        let behind = *order(&b, window).last().unwrap();
        assert_eq!(
            actions,
            vec![
                EngineAction::CreateHiddenTab {
                    window,
                    tab: behind,
                    url: url("https://behind.example/"),
                },
                reply(2, ExtensionReply::Tab(behind)),
            ]
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.active, opened);
        assert_eq!(w.selected, vec![opened]);
        assert_eq!(order(&b, window), vec![t[0], t[1], opened, behind]);
        assert_eq!(b.tab(behind).unwrap().folder, None);
    }

    #[test]
    fn a_tab_an_extension_opens_on_a_blocked_site_shows_the_block_page() {
        let (mut b, window, tab) = one_window();
        b.blocked.add("b.example");
        for (token, active) in [(1, true), (2, false)] {
            let actions = ask(
                &mut b,
                token,
                ExtensionRequest::OpenTab {
                    window: Some(window),
                    url: url("https://www.b.example/x"),
                    active,
                },
            );
            let opened = *order(&b, window).last().unwrap();
            // The engine loads the blank page; the tab keeps the address.
            let created = actions
                .iter()
                .find_map(|a| match a {
                    EngineAction::CreateTab { tab, url, .. }
                    | EngineAction::CreateHiddenTab { tab, url, .. } => Some((*tab, url.as_str())),
                    _ => None,
                })
                .expect("a tab is created");
            assert_eq!(created, (opened, "about:blank"));
            assert_eq!(
                actions.last(),
                Some(&reply(token, ExtensionReply::Tab(opened)))
            );
            let t = b.tab(opened).unwrap();
            assert_eq!(t.url.as_str(), "https://www.b.example/x");
            assert_eq!(t.blocked.as_ref().unwrap().site, "b.example");
        }
        assert_eq!(order(&b, window).len(), 3);
        assert!(b.tab(tab).unwrap().blocked.is_none());
    }

    #[test]
    fn a_tab_without_a_window_opens_in_the_focused_one_and_an_unknown_window_is_refused() {
        let (mut b, w1, _) = one_window();
        let other = dispatch(&mut b, &Command::NewWindow);
        let EngineAction::OpenWindow { window: w2, .. } = other[0].clone() else {
            panic!("NewWindow returns OpenWindow");
        };
        let open = |window| ExtensionRequest::OpenTab {
            window,
            url: url("https://new.example/"),
            active: false,
        };
        // Nobody has said which window is focused: the first.
        ask(&mut b, 1, open(None));
        assert_eq!(order(&b, w1).len(), 2);
        b.focused = Some(w2);
        ask(&mut b, 2, open(None));
        assert_eq!(order(&b, w2).len(), 2);
        let before = b.windows.clone();
        assert_eq!(
            ask(&mut b, 3, open(Some(WindowId(99)))),
            vec![refused(3, "No window with id: 99.")]
        );
        assert_eq!(b.windows, before);
        // With no window at all there is nothing to land in: the extension
        // API's words, which end with no full stop.
        b.windows.clear();
        assert_eq!(
            ask(&mut b, 4, open(None)),
            vec![refused(4, "No current window")]
        );
        assert!(b.windows.is_empty());
    }

    #[test]
    fn an_extension_opens_a_window_on_a_url_or_the_new_window_page() {
        let (mut b, _, _) = one_window();
        b.settings.new_tab_url = "https://start.example/".to_string();
        b.blocked.add("b.example");
        let actions = ask(&mut b, 1, ExtensionRequest::OpenWindow { url: None });
        let opened = b.windows.last().unwrap();
        assert_eq!(
            actions,
            vec![
                EngineAction::OpenWindow {
                    window: opened.id,
                    tab: opened.active,
                    url: url("https://start.example/"),
                },
                reply(1, ExtensionReply::Window(opened.id)),
            ]
        );
        // A blocked site opens on its block page, as the user's would.
        let actions = ask(
            &mut b,
            2,
            ExtensionRequest::OpenWindow {
                url: Some(url("https://b.example/")),
            },
        );
        let opened = b.windows.last().unwrap();
        assert_eq!(
            actions,
            vec![
                EngineAction::OpenWindow {
                    window: opened.id,
                    tab: opened.active,
                    url: url("about:blank"),
                },
                reply(2, ExtensionReply::Window(opened.id)),
            ]
        );
        assert!(opened.active_tab().blocked.is_some());
        assert_eq!(b.windows.len(), 3);
    }

    #[test]
    fn an_extension_closes_tabs_or_none_of_them_when_one_is_unknown() {
        let (mut b, window, t) = window_with_tabs(4);
        let before = b.windows.clone();
        // One unknown id among known ones: nothing closes.
        assert_eq!(
            ask(
                &mut b,
                1,
                ExtensionRequest::CloseTabs(vec![t[0], TabId(99), t[1]])
            ),
            vec![refused(1, "No tab with id: 99.")]
        );
        assert_eq!(b.windows, before);
        assert!(b.closed_tabs.is_empty());
        // All known: each closes as the user's close would, and can be
        // reopened. t[3] is active, so the tab on its left is shown.
        assert_eq!(
            ask(&mut b, 2, ExtensionRequest::CloseTabs(vec![t[0], t[3]])),
            vec![
                EngineAction::CloseTab(t[0]),
                EngineAction::CloseTab(t[3]),
                EngineAction::Show(t[2]),
                reply(2, ExtensionReply::Done),
            ]
        );
        assert_eq!(order(&b, window), vec![t[1], t[2]]);
        assert_eq!(b.closed_tabs.len(), 2);
        // No tabs at all is done.
        assert_eq!(
            ask(&mut b, 3, ExtensionRequest::CloseTabs(Vec::new())),
            vec![reply(3, ExtensionReply::Done)]
        );
    }

    #[test]
    fn an_extension_activates_a_tab_and_a_deferred_one_wakes() {
        let mut b = browser();
        restore_session(&mut b, &three_sites_session(1));
        let w = &b.windows[0];
        let (window, a, bb, c) = (w.id, w.tabs[0].id, w.tabs[1].id, w.tabs[2].id);
        assert!(b.tab(a).unwrap().deferred);
        // A restored tab the engine does not have yet is created now, as
        // a click on it would.
        assert_eq!(
            ask(&mut b, 1, ExtensionRequest::ActivateTab(a)),
            vec![
                EngineAction::Hide(bb),
                EngineAction::CreateTab {
                    window,
                    tab: a,
                    url: url("https://a.example/"),
                },
                reply(1, ExtensionReply::Done),
            ]
        );
        assert!(!b.tab(a).unwrap().deferred);
        assert_eq!(b.window(window).unwrap().active, a);
        // A live tab is shown; the active one is done with nothing to do.
        assert_eq!(
            ask(&mut b, 2, ExtensionRequest::ActivateTab(bb)),
            vec![
                EngineAction::Hide(a),
                EngineAction::Show(bb),
                reply(2, ExtensionReply::Done),
            ]
        );
        assert_eq!(
            ask(&mut b, 3, ExtensionRequest::ActivateTab(bb)),
            vec![reply(3, ExtensionReply::Done)]
        );
        // A deferred tab on a site blocked meanwhile wakes on the block
        // page.
        b.blocked.add("c.example");
        assert_eq!(
            ask(&mut b, 4, ExtensionRequest::ActivateTab(c)),
            vec![
                EngineAction::Hide(bb),
                EngineAction::CreateTab {
                    window,
                    tab: c,
                    url: url("about:blank"),
                },
                reply(4, ExtensionReply::Done),
            ]
        );
        assert!(b.tab(c).unwrap().blocked.is_some());
        assert_eq!(
            ask(&mut b, 5, ExtensionRequest::ActivateTab(TabId(99))),
            vec![refused(5, "No tab with id: 99.")]
        );
        assert_eq!(b.window(window).unwrap().active, c);
    }

    #[test]
    fn an_extension_moves_a_tab_to_an_index_of_its_strip() {
        let (mut b, window, t) = window_with_tabs(4);
        let moved = |b: &mut Browser, token, tab, index| {
            ask(b, token, ExtensionRequest::MoveTab { tab, index })
        };
        assert_eq!(
            moved(&mut b, 1, t[3], 0),
            vec![reply(1, ExtensionReply::Done)]
        );
        assert_eq!(order(&b, window), vec![t[3], t[0], t[1], t[2]]);
        moved(&mut b, 2, t[3], 2);
        assert_eq!(order(&b, window), vec![t[0], t[1], t[3], t[2]]);
        // Past the end is the end.
        moved(&mut b, 3, t[0], usize::MAX);
        assert_eq!(order(&b, window), vec![t[1], t[3], t[2], t[0]]);
        // The active tab and the engine are left alone.
        assert_eq!(b.window(window).unwrap().active, t[3]);
        assert_eq!(
            moved(&mut b, 4, TabId(99), 0),
            vec![refused(4, "No tab with id: 99.")]
        );
        assert_eq!(order(&b, window), vec![t[1], t[3], t[2], t[0]]);
    }

    /// What an engine's `windows.remove` relies on: closing every tab of
    /// a window closes the window, as the user's close of its last tab
    /// does. (The last tab of the last window goes through the quit
    /// guard instead: it quits, or asks first while downloads are going
    /// and then closes nothing; the reply is `Done` either way.)
    #[test]
    fn an_extension_closing_every_tab_of_a_window_closes_the_window() {
        let (mut b, first, t) = window_with_tabs(2);
        ask(&mut b, 1, ExtensionRequest::OpenWindow { url: None });
        assert_eq!(b.windows.len(), 2);
        let actions = ask(&mut b, 2, ExtensionRequest::CloseTabs(t.clone()));
        assert!(b.window(first).is_none());
        assert_eq!(b.windows.len(), 1);
        assert!(
            actions.contains(&EngineAction::CloseWindow(first)),
            "{actions:?}"
        );
        assert_eq!(actions.last(), Some(&reply(2, ExtensionReply::Done)));
    }

    #[test]
    fn an_extension_closing_the_last_tab_while_downloads_go_asks_first_and_closes_nothing() {
        let (mut b, window, tab) = one_window();
        crate::apply(
            &mut b,
            &crate::EngineEvent::DownloadUpdated {
                id: 42,
                report: download_report("f.zip", DownloadState::InProgress),
            },
        );
        let actions = ask(&mut b, 1, ExtensionRequest::CloseTabs(vec![tab]));
        assert_eq!(
            actions,
            vec![
                EngineAction::ShowPrompt {
                    window,
                    prompt: Prompt::QuitWithDownloads { count: 1 },
                },
                reply(1, ExtensionReply::Done),
            ]
        );
        assert!(b.window(window).is_some());
        assert_eq!(order(&b, window), vec![tab]);
    }

    #[test]
    fn grouping_tabs_makes_an_expanded_folder_named_after_the_extension() {
        // side-panel.AC4.1
        let (mut b, window, t) = window_with_tabs(5);
        installed(&mut b, "ext", true, true);
        b.extensions[0].name = "Claude".to_string();
        let actions = ask(
            &mut b,
            1,
            ExtensionRequest::GroupTabs {
                tabs: vec![t[3], t[1]],
                folder: None,
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.folders.len(), 1);
        let folder = w.folders[0].id;
        assert_eq!(actions, vec![reply(1, ExtensionReply::Folder(folder))]);
        assert_eq!(w.folders[0].name, "Claude Session");
        assert!(w.folders[0].expanded, "an extension's folder starts open");
        // Gathered where the first of them is, in strip order.
        assert_eq!(w.folder_tabs(folder), vec![t[1], t[3]]);
        assert_eq!(order(&b, window), vec![t[0], t[1], t[3], t[2], t[4]]);
        // The user's own new folder still starts collapsed.
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[4]],
                folder: FolderTarget::New {
                    name: String::new(),
                },
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.folders[1].name, "Folder A");
        assert!(!w.folders[1].expanded);
        // An extension the model does not list lends its id.
        let actions = crate::event::apply(
            &mut b,
            &crate::event::EngineEvent::ExtensionRequest {
                token: 2,
                extension: "unlisted".to_string(),
                request: ExtensionRequest::GroupTabs {
                    tabs: vec![t[0]],
                    folder: None,
                },
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(
            actions,
            vec![reply(2, ExtensionReply::Folder(w.folders[2].id))]
        );
        assert_eq!(w.folders[2].name, "unlisted Session");
    }

    #[test]
    fn grouping_into_an_existing_folder_adds_the_tabs_and_keeps_its_state() {
        // side-panel.AC4.2
        let (mut b, window, t) = window_with_tabs(5);
        // A folder the user made, collapsed and named by them.
        dispatch(
            &mut b,
            &Command::MoveTabsToFolder {
                tabs: vec![t[1]],
                folder: FolderTarget::New {
                    name: "Work".to_string(),
                },
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        let actions = ask(
            &mut b,
            1,
            ExtensionRequest::GroupTabs {
                tabs: vec![t[4], t[2]],
                folder: Some(folder),
            },
        );
        assert_eq!(actions, vec![reply(1, ExtensionReply::Folder(folder))]);
        let w = b.window(window).unwrap();
        assert_eq!(w.folders.len(), 1);
        assert_eq!(w.folder_tabs(folder), vec![t[1], t[2], t[4]]);
        assert_eq!(order(&b, window), vec![t[0], t[1], t[2], t[4], t[3]]);
        assert_eq!(w.folders[0].name, "Work");
        assert!(!w.folders[0].expanded, "an existing folder is not opened");
    }

    #[test]
    fn grouping_is_refused_whole_for_an_unknown_tab_or_folder_or_another_windows_tab() {
        let (mut b, window, t) = window_with_tabs(3);
        let other = dispatch(&mut b, &Command::NewWindow);
        let EngineAction::OpenWindow { tab: far, .. } = other[0].clone() else {
            panic!("NewWindow returns OpenWindow");
        };
        ask(
            &mut b,
            0,
            ExtensionRequest::GroupTabs {
                tabs: vec![t[0]],
                folder: None,
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        let before = b.windows.clone();
        let group = |tabs, folder| ExtensionRequest::GroupTabs { tabs, folder };
        assert_eq!(
            ask(&mut b, 1, group(vec![t[1], TabId(99)], None)),
            vec![refused(1, "No tab with id: 99.")]
        );
        assert_eq!(
            ask(&mut b, 2, group(vec![t[1]], Some(FolderId(98)))),
            vec![refused(2, "No group with id: 98.")]
        );
        // A page cannot change windows, so neither can a tab join a
        // group of another window, or a new group span two.
        assert_eq!(
            ask(&mut b, 3, group(vec![t[1], far], Some(folder))),
            vec![refused(3, "Tabs of another window cannot join this group.")]
        );
        assert_eq!(
            ask(&mut b, 4, group(vec![t[1], far], None)),
            vec![refused(4, "Tabs of another window cannot join this group.")]
        );
        assert_eq!(
            ask(&mut b, 5, group(Vec::new(), None)),
            vec![refused(5, "No tabs to group.")]
        );
        assert_eq!(b.windows, before, "a refused request changes nothing");
    }

    #[test]
    fn ungrouping_takes_tabs_out_and_an_emptied_folder_goes() {
        // side-panel.AC4.2, side-panel.AC4.4
        let (mut b, window, t) = window_with_tabs(5);
        ask(
            &mut b,
            0,
            ExtensionRequest::GroupTabs {
                tabs: vec![t[1], t[2], t[3]],
                folder: None,
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        // The middle tab leaves: the folder's tabs stay together, the
        // tab that left after them.
        assert_eq!(
            ask(&mut b, 1, ExtensionRequest::UngroupTabs(vec![t[2]])),
            vec![reply(1, ExtensionReply::Done)]
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.folder_tabs(folder), vec![t[1], t[3]]);
        assert_eq!(w.tab(t[2]).unwrap().folder, None);
        assert_eq!(order(&b, window), vec![t[0], t[1], t[3], t[2], t[4]]);
        // A tab in no folder is done with nothing to do.
        assert_eq!(
            ask(&mut b, 2, ExtensionRequest::UngroupTabs(vec![t[0]])),
            vec![reply(2, ExtensionReply::Done)]
        );
        // One unknown tab: nobody leaves.
        assert_eq!(
            ask(
                &mut b,
                3,
                ExtensionRequest::UngroupTabs(vec![t[1], TabId(99)])
            ),
            vec![refused(3, "No tab with id: 99.")]
        );
        assert_eq!(
            b.window(window).unwrap().folder_tabs(folder),
            vec![t[1], t[3]]
        );
        // The last tabs leave: the folder is removed.
        ask(&mut b, 4, ExtensionRequest::UngroupTabs(vec![t[3], t[1]]));
        let w = b.window(window).unwrap();
        assert!(w.folders.is_empty());
        assert!(w.tabs.iter().all(|t| t.folder.is_none()));
        assert_eq!(b.window_of_folder(folder), None);
        // So is a folder whose last tab an extension closes.
        ask(
            &mut b,
            5,
            ExtensionRequest::GroupTabs {
                tabs: vec![t[4]],
                folder: None,
            },
        );
        assert_eq!(b.window(window).unwrap().folders.len(), 1);
        ask(&mut b, 6, ExtensionRequest::CloseTabs(vec![t[4]]));
        assert!(b.window(window).unwrap().folders.is_empty());
    }

    #[test]
    fn an_extension_renames_collapses_and_expands_a_folder() {
        // side-panel.AC4.1 (the rename), side-panel.AC4.2 (collapsed)
        let (mut b, window, t) = window_with_tabs(3);
        installed(&mut b, "ext", true, true);
        b.extensions[0].name = "Claude".to_string();
        ask(
            &mut b,
            0,
            ExtensionRequest::GroupTabs {
                tabs: vec![t[1]],
                folder: None,
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        let update = |b: &mut Browser, token, name: Option<&str>, expanded| {
            ask(
                b,
                token,
                ExtensionRequest::UpdateFolder {
                    folder,
                    name: name.map(str::to_string),
                    expanded,
                },
            )
        };
        let state = |b: &Browser| {
            let f = b.window(window).unwrap().folder(folder).unwrap();
            (f.name.clone(), f.expanded)
        };
        assert_eq!(
            update(&mut b, 1, Some("  Trip planning "), None),
            vec![reply(1, ExtensionReply::Done)]
        );
        assert_eq!(state(&b), ("Trip planning".to_string(), true));
        // Later renames follow.
        update(&mut b, 2, Some("Flights"), None);
        assert_eq!(state(&b), ("Flights".to_string(), true));
        // Collapsing and expanding leave the name.
        update(&mut b, 3, None, Some(false));
        assert_eq!(state(&b), ("Flights".to_string(), false));
        update(&mut b, 4, None, Some(true));
        assert_eq!(state(&b), ("Flights".to_string(), true));
        // A title taken away is the name the folder started with.
        update(&mut b, 5, Some("   "), Some(false));
        assert_eq!(state(&b), ("Claude Session".to_string(), false));
        // Neither given: done, nothing changed.
        update(&mut b, 6, None, None);
        assert_eq!(state(&b), ("Claude Session".to_string(), false));
        assert_eq!(
            ask(
                &mut b,
                7,
                ExtensionRequest::UpdateFolder {
                    folder: FolderId(99),
                    name: Some("x".to_string()),
                    expanded: Some(true),
                }
            ),
            vec![refused(7, "No group with id: 99.")]
        );
    }

    #[test]
    fn every_request_is_answered_once_with_its_token_last() {
        let (mut b, window, t) = window_with_tabs(4);
        ask(
            &mut b,
            0,
            ExtensionRequest::GroupTabs {
                tabs: vec![t[0]],
                folder: None,
            },
        );
        let folder = b.window(window).unwrap().folders[0].id;
        let page = || "panel.html".to_string();
        // Each request, and whether it is refused.
        let requests = vec![
            (
                ExtensionRequest::OpenTab {
                    window: None,
                    url: url("https://new.example/"),
                    active: true,
                },
                false,
            ),
            (
                ExtensionRequest::OpenTab {
                    window: Some(WindowId(99)),
                    url: url("https://new.example/"),
                    active: true,
                },
                true,
            ),
            (ExtensionRequest::ActivateTab(t[1]), false),
            (ExtensionRequest::ActivateTab(TabId(99)), true),
            (
                ExtensionRequest::MoveTab {
                    tab: t[2],
                    index: 0,
                },
                false,
            ),
            (
                ExtensionRequest::MoveTab {
                    tab: TabId(99),
                    index: 0,
                },
                true,
            ),
            (
                ExtensionRequest::GroupTabs {
                    tabs: vec![t[1]],
                    folder: Some(folder),
                },
                false,
            ),
            (
                ExtensionRequest::GroupTabs {
                    tabs: vec![TabId(99)],
                    folder: None,
                },
                true,
            ),
            (
                ExtensionRequest::UpdateFolder {
                    folder,
                    name: Some("Named".to_string()),
                    expanded: None,
                },
                false,
            ),
            (
                ExtensionRequest::UpdateFolder {
                    folder: FolderId(99),
                    name: None,
                    expanded: None,
                },
                true,
            ),
            (ExtensionRequest::UngroupTabs(vec![t[1]]), false),
            (ExtensionRequest::UngroupTabs(vec![TabId(99)]), true),
            (
                ExtensionRequest::OpenSidePanel {
                    window,
                    tab: Some(t[1]),
                    page: page(),
                },
                false,
            ),
            (
                ExtensionRequest::OpenSidePanel {
                    window: WindowId(99),
                    tab: None,
                    page: page(),
                },
                true,
            ),
            (
                ExtensionRequest::CloseSidePanel {
                    window,
                    tab: Some(t[1]),
                },
                false,
            ),
            (
                ExtensionRequest::CloseSidePanel {
                    window: WindowId(99),
                    tab: None,
                },
                true,
            ),
            (ExtensionRequest::OpenWindow { url: None }, false),
            (ExtensionRequest::CloseTabs(vec![t[3]]), false),
            (ExtensionRequest::CloseTabs(vec![TabId(99)]), true),
        ];
        for (n, (request, refusal)) in requests.into_iter().enumerate() {
            let token = 100 + n as u64;
            let actions = ask(&mut b, token, request.clone());
            let replies: Vec<&EngineAction> = actions
                .iter()
                .filter(|a| matches!(a, EngineAction::ExtensionReply { .. }))
                .collect();
            assert_eq!(replies.len(), 1, "{request:?}: {actions:?}");
            let Some(EngineAction::ExtensionReply { token: got, reply }) = actions.last() else {
                panic!("the reply comes last: {request:?}: {actions:?}");
            };
            assert_eq!(*got, token, "{request:?}");
            assert_eq!(
                matches!(reply, ExtensionReply::Refused(_)),
                refusal,
                "{request:?}: {reply:?}"
            );
            // A refusal comes alone: nothing was asked of the engine.
            if refusal {
                assert_eq!(actions.len(), 1, "{request:?}: {actions:?}");
            }
        }
    }

    #[test]
    fn an_extension_command_needs_a_window_and_an_enabled_extension_that_declares_it() {
        // side-panel.AC2.2 at the model level
        let (mut b, window, _) = one_window();
        let declare =
            |b: &mut Browser, id: &str, enabled| {
                installed(b, id, enabled, false);
                b.extensions.last_mut().unwrap().commands.push(
                    crate::extension::ExtensionCommand {
                        name: "toggle".to_string(),
                        description: "Toggle the panel".to_string(),
                        // The chord is the key router's business, not dispatch's.
                        chord: None,
                    },
                );
            };
        declare(&mut b, "a", true);
        declare(&mut b, "off", false);
        let run = |b: &mut Browser, w, id: &str, name: &str| {
            dispatch(
                b,
                &Command::ExtensionCommand {
                    window: w,
                    extension: id.to_string(),
                    name: name.to_string(),
                },
            )
        };
        assert_eq!(
            run(&mut b, window, "a", "toggle"),
            vec![EngineAction::ExtensionCommand {
                window,
                extension: "a".to_string(),
                name: "toggle".to_string(),
            }]
        );
        assert!(run(&mut b, window, "a", "undeclared").is_empty());
        assert!(run(&mut b, window, "off", "toggle").is_empty());
        assert!(run(&mut b, window, "missing", "toggle").is_empty());
        assert!(run(&mut b, WindowId(99), "a", "toggle").is_empty());
    }
}
