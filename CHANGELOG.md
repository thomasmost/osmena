# Changelog

All notable changes to the osmena crates. The crates version together.

## Unreleased

### Changed

- A new tab (Ctrl+T, the "+" button) no longer opens a blank page. It
  opens the launcher: a large field over the window in which the user
  types an address or a search, with the history's matches under it
  (Up and Down pick one). Enter opens the result in a new tab; Escape
  or a click outside opens nothing. `Command::NewTab` without a URL now
  answers with `EngineAction::ShowLauncher`; the `new_tab_url` setting
  still names what a new window opens on.
- `FrameRef` carries `dirty`, the rectangles (`PixelRect`) that changed
  since the frame was last handed out, and the shell uploads only those
  to the page texture when the frame's size is unchanged. An engine
  that does not track them passes an empty slice, which means all of
  it.
- `EngineInput::MouseMove`, `MouseButton`, and `Wheel` carry the
  keyboard `modifiers` held at that moment, from the window's own
  modifier state. An engine must use them instead of remembering the
  modifiers of the last key event it saw: those go stale when a
  modifier is released while the window is not focused (Alt+Tab), or
  when its release was never delivered (the tab that got the press
  closed), and a stale Alt or Ctrl turns the next click on a link into
  a download or a background tab.
- Tabs in the strip are all one standard width (200 points) instead of
  each taking its title's width; when they no longer fit they all
  shrink alike, down to a minimum (64 points), and only then does the
  strip scroll. A title that does not fit is elided with an ellipsis,
  and a narrow inactive tab shows its close control only while hovered.
- The tab strip fades from the chrome colour at its top into the
  toolbar's fill at its bottom edge, so strip and toolbar meet without
  a line; the active tab, which shares the toolbar's fill, now sits on
  the strip without an edge under it (`Palette::strip_at`).

### Fixed

- A tab shown or created in a window that did not have keyboard focus
  (a tab activated by a drag into a window behind another, a new tab in
  a window the user had left) was told it had focus, so two windows
  could each show a blinking caret and page key handlers thought they
  were foremost. Page focus now follows the window's.
- The window redrew continuously, spinning the engine each time, while
  the address bar or a prompt's field had focus (the caret blink asks
  egui for a repaint half a second ahead, and the shell treated that as
  a request for one now), and for as long as a tooltip was pending. The
  shell now takes egui's repaint requests through its callback and
  waits until the requested moment.
- A folder's drop-down list stayed open after a click on the page: the
  click went to the page, so the list never saw it. It now goes to the
  chrome, which closes the list, as a click outside any other popup
  does. A prompt (move to folder, bookmark, rename) is modal the same
  way: clicks on the page wait for the answer.
- A key or mouse button held in a page when the window lost focus
  (Alt+Tab, a click into another window) stayed down in the page: the
  system sends no release for it, and the real one, when it came, was
  dropped as a release nobody here saw pressed. The window now releases
  such keys and buttons in the page as it loses focus, each in the tab
  that got its press.
- A mouse button pressed in a page and released after a keyboard tab
  switch (Ctrl+Tab while dragging) released in the newly active tab,
  which had never seen the press, while the first tab kept the button
  down. The pointer capture now remembers the tab that got the press,
  as the key router does for keys, and sends it the moves and the
  release.
- macOS: the tab context menu, the page context menu, and the window
  menu opened along the bottom edge of the window instead of at the
  pointer. muda mirrors the y coordinate for an AppKit view with its
  origin at the bottom; winit's view has its origin at the top.

### Added

- Typing in the address bar drops the history's matches down under it;
  Up and Down put a match's address in the field, a click on one loads
  it.
- Browsing history: every page that finishes loading is remembered in
  the profile's `history.toml` with its title, visit count, and last
  visit (`History` in `osmena-core`; `EngineAction::SaveHistory` asks
  the shell to write it, which it does a few seconds after the last
  change and at exit). Blank, internal, and view-source pages are not
  kept; the least recently visited entry goes when there are 5000.
- Tabs are reordered by dragging them along the strip: the dragged
  tab (or selection) rides with the pointer, the others slide out of its
  way, and it lands where it is let go. Tabs also slide to their places
  after a close or any other reorder. A drag that leaves the window
  still moves the tabs to the window under the pointer or a new one.
- `Command::MoveTabs { tabs, index }` reorders tabs within their window;
  a tab dropped between two tabs of a folder joins it, one dropped
  anywhere else leaves its folder.
- The back and forward buttons of a mouse go back and forward in the
  active tab, wherever the pointer is over the window.
- `--exit-after <seconds>` (hidden from `--help`): the browser quits
  that long after its engine started, through the normal Quit path,
  after logging each open tab's title and URL. For scripted smoke tests
  of a build: a CI job can start the browser on a page, let it load,
  and check the log and the exit code.
- The page's context menu, shaped like Chromium's, over what the engine
  finds under the pointer: a link opens in a new tab (beside the opener,
  without taking its place) or a new window, is saved, or has its
  address copied; an image opens in a new tab, is saved, copied, or has
  its address copied; a video or audio loops, shows its controls, is
  saved, or has its address copied; selected text is copied, searched,
  or printed; an editable field gets spelling suggestions, undo, redo,
  cut, copy, paste, paste as plain text, delete, and select all, with
  their shortcuts; a plain page gets back, forward, reload, save, print,
  and view source; "Inspect" is last. Entries whose tool the engine
  lacks are left out (`EngineInfo::page_tools`).
- A search for address-bar text that is not a URL, and for selected
  text from the page menu: `[search]` in `settings.toml` names the
  provider and its URL (`{query}` stands for the text; a Google search
  by default), editable in the settings window's General section. A
  command-line argument that is not a URL is searched too.
- A page's request for a new tab says whether it should open in the
  background (a middle- or Ctrl-click) or in a new window (Shift-click);
  background tabs open beside the opener, in its folder, and leave the
  active tab alone.
- `osmena-core`: `PageContext`, `Media`, `MediaKind`, `EditState`,
  `Misspelling`, `PageCommand`, `PageTools` (`EngineInfo::page_tools`),
  `SearchSettings` (`Settings::search`), `RgbaImage` (`Favicon` is now
  an alias of it), `Command::{Search, SearchInNewTab, NewBackgroundTab,
  OpenInNewWindow, CopyText, Page, ContextMenuClosed}`,
  `EngineAction::{CreateHiddenTab, CopyText, Page, ContextMenuClosed}`,
  `EngineEvent::{NewWindowRequested, ContextMenuRequested,
  ClipboardImage}`.
- `osmena-engine`: `TabHost::{page_command, context_menu_closed}`,
  defaulted to nothing.
- `osmena-chrome`: `menus::page_menu`; menu labels show a literal `&`.
- `osmena-shell`: the clipboard (`arboard`) for "Copy link address",
  "Copy image address", and "Copy image".

- Tab folders and bookmark groups, from the tab context menu, which
  gains "Close all selected", "Move tab(s) to folder…", and "Bookmark
  all selected…". Moving to a folder asks for a name (prefilled "Folder
  A", "Folder B", … or an existing folder of the window); the folder's
  tabs sit together in the strip behind one chip that, collapsed, shows
  the name and count and lists its tabs on a click, and, expanded, is
  followed by its tabs. The chip's menu expands or collapses, renames,
  ungroups, or closes the folder. Bookmarking asks for a group name or
  an existing group; groups persist in `bookmarks.toml` in the profile
  and appear under "Bookmarks" in the window menu, each with "Open all"
  and its pages.
- `osmena-core`: `Folder`, `FolderId`, `StripItem`, `Tab::folder`,
  `Window::{folders, folder, folder_tabs, strip_items,
  suggested_folder_name}`, `Browser::{bookmarks, window_of_folder}`,
  `Bookmarks` (`bookmarks.rs`), `FolderTarget`, `Prompt`,
  `Command::{MoveTabsToFolder, SetFolderExpanded, RenameFolder, Ungroup,
  CloseFolder, BookmarkTabs, OpenBookmarkGroup, PromptMoveToFolder,
  PromptBookmarkTabs, PromptRenameFolder}`, `EngineAction::{SaveBookmarks,
  ShowPrompt}`, `Profile::bookmarks_path`.
- `osmena-chrome`: `MenuRequest::Folder`, `menus::folder_menu`,
  `ChromeState::{open_prompt, has_prompt}`; the chrome draws prompts as a
  small window over the page.

### Fixed

- The address bar copies and pastes through the system clipboard
  (egui-winit's `clipboard` feature); before, Ctrl+V in it read a
  private clipboard of egui's own and never what another program (or
  "Copy link address") had put there.

### Changed

- `menus::window_menu` takes the bookmarks; `tab_menu` has three more
  entries.
- `EngineEvent::NewTabRequested` carries `background`;
  `EngineAction::AdoptTab` carries `background`. The address bar issues
  `Command::Search` for text that is not a URL instead of ignoring it.
- Browser extensions, for engines that run them: the toolbar ends in a
  puzzle-piece button whose menu lists the installed extensions (click
  one to open its action's popup), a "Pin to toolbar" submenu, and the
  ways to add one (the extension on the active tab's store page, the
  store itself, an unpacked directory); pinned extensions sit as icons
  beside the address bar. The settings window gains an Extensions
  section: enabled, in toolbar, remove, load unpacked, install by store
  URL or id. Pinned ids persist as `pinned_extensions` in
  `settings.toml`.
- `osmena-core`: `Extension`, `ExtensionSource`, `Browser::extensions`
  (with `pinned_extensions`, `extension`, `is_pinned`),
  `Settings::pinned_extensions`, `EngineInfo::extensions` with
  `ExtensionSupport` (store name, URL, and the store page's item id),
  `Command::{OpenExtensionAction, PinExtension, SetExtensionEnabled,
  UninstallExtension, InstallExtension}`,
  `EngineAction::{SaveSettings, OpenExtensionAction, SetExtensionEnabled,
  UninstallExtension, InstallExtension}`, and
  `EngineEvent::ExtensionsChanged`.
- `osmena-engine`: `Engine::{install_extension, set_extension_enabled,
  uninstall_extension}` and `TabHost::{open_extension_action,
  close_extension_popups}`, all defaulted to nothing, and `ScreenRect`.
- `osmena-chrome`: `ChromeContext::pinned`, `MenuRequest::Extensions`,
  `menus::extensions_menu`, `MenuEntry::Check` (a check item, shown as a
  native check mark), and `ChromeState::{action_anchor,
  extensions_menu_anchor}`.

### Changed

- `EngineInfo` has a new field, `extensions`; an engine without them sets
  it to `None`.
- `SettingsWindow::handle_event` takes the extension list and support;
  `take_commands` returns what its Extensions section asked for.

- Browser windows open without the system title bar: the tab strip is
  the title bar (drag its empty space to move the window, double-click
  it to maximize, right-click it for the system menu on Windows) and,
  except on macOS, ends in minimize, maximize, and close controls; the
  window's edges still resize it. A new setting, "Use the system title
  bar and borders" (`system_title_bar` in `settings.toml`, off by
  default), restores the system's frame; on macOS it applies to windows
  opened afterwards.
- Chromium-style default shortcuts, and new commands behind them: reopen
  closed tab (`Ctrl+Shift+T`), switch to tab 1 to 8 and to the last tab
  (`Ctrl+1` to `Ctrl+9`), open homepage (`Alt+Home`; `Cmd+Shift+H` on
  macOS), full screen (`F11`; `Ctrl+Cmd+F`), close tab also on `Ctrl+F4`,
  next and previous tab also on `Ctrl+PgDn`/`Ctrl+PgUp`, and the address
  bar also on `Alt+D`. The window menu lists the new commands. The Command
  key stands in for Ctrl on macOS throughout.
- A keyboard shortcuts editor in the settings window, laid out like a
  game's controls screen: every action has a primary and a secondary
  slot; click one and press the new chord (Esc keeps the old one,
  Backspace clears it, right-click clears it); a chord in use moves to
  the action it is given to, and a note says what lost it. "Reset to
  defaults" restores the platform's bindings. Changes persist to
  `keymap.toml` in the profile (`[bindings]`, one array of chords per
  action; an action left out keeps its default), which hand edits may
  also change; a chord that does not parse is reported at startup like a
  malformed `settings.toml`.
- `osmena-core`: `Command::{ActivateTabAt, ActivateLastTab,
  ReopenClosedTab, Home, ToggleFullscreen}`, `EngineAction::SetFullscreen`,
  `Window::fullscreen`, `Browser::closed_tabs`; `CommandKind` has
  `name`, `label`, and `from_name`; `Keymap` has `chords_for`,
  `set_chords`, `set_slot`, `unbind`, `to_toml`, `parse`, `load`, and
  `save`; `KeyChord` displays and parses as `Ctrl+Shift+KeyT`.
- `osmena-chrome`: `ChromeContext::{frameless, maximized}`,
  `WindowRequest`, `routing::resize_edge`, and `chrome::describe_chord`
  is public; `menus::window_menu` takes whether a closed tab can be
  reopened.
- `osmena-core`: the browser model, commands, events, input, keymap,
  settings, and profiles extracted from Paladin and Nautilus, plus `Brand`
  and `EngineInfo` for the strings and facts that differed between them.
- `osmena-engine`: the `Engine`, `TabHost`, and `Surface` traits, the
  `PagePaint` handoff (blit or pixel frame), and a glutin `Surface` behind
  the `glutin` feature.
- `osmena-chrome`: the egui chrome, native menus, routing, key utilities,
  icon, and fatal dialog, parameterised by brand and engine.
- `osmena-shell`: the winit application, browser window, settings window,
  process glue, and `run`, generic over `Engine`.
- `osmena-xtask`: icon generation as a library, for vector logos and for
  SVGs that frame raster artwork.
