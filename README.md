# osmena

The shared shell of [Paladin](https://github.com/thomasmost/paladin) (Servo)
and [Nautilus](https://github.com/thomasmost/nautilus) (Chromium): the
browser model, the egui chrome, and the winit application, written once and
built on top of any engine that implements a small trait. Named after the
nautilus shell's iridescent inner layer, which is also the paladin's armour.

A browser built on osmena is four things:

1. a `Brand` constant (name, data directory, homepage, icon, update
   source, and the Google client id for the Launch Panel's Drive tile, if
   the product has one);
2. an `EngineInfo` constant (engine name and version, its data directory,
   what it can do);
3. an engine crate that implements `Engine`, `TabHost`, and `Surface` from
   `osmena-engine`;
4. a `main` that parses its command line and calls `osmena_shell::run`.

Everything else, including the multi-tab selection, the tab moves between
windows and profiles, the native menus, the profile colours, the settings
window, and the keymap, is here and shared.

## Crates

Dependencies point only downwards.

| Crate | Tier | Holds | Depends on |
| --- | --- | --- | --- |
| `osmena-core` | model | `Browser`/`Window`/`Tab`, `Command` and `dispatch`, `EngineEvent` and `apply`, `EngineInput`, keymap, settings, profiles, `Brand`, `EngineInfo`. Pure and unit-tested. | serde, url, keyboard-types |
| `osmena-engine` | engine contract | The `Engine`, `TabHost`, and `Surface` traits, `PagePaint`, `Waker`, `EngineError`; with the `glutin` feature, a `Surface` for engines that paint into memory. | osmena-core, glow, raw-window-handle |
| `osmena-chrome` | chrome | The egui tab strip, toolbar, status overlay and crash panel; the theme (Inter, bundled under `assets/fonts` with its SIL Open Font License, and the shared style); the chrome's own glyphs and the profile avatars, drawn as SVGs under `assets/icons` and `assets/avatars` and built into two small fonts; the window, tab and page menus and the macOS menu bar as data and as native menus; pointer and key routing; the icon and the fatal-error dialog. | osmena-core, egui, winit, muda |
| `osmena-shell` | shell | The winit `App`, `BrowserWindow`, `SettingsWindow`, the process glue, and `run`. Generic over `Engine`. | osmena-chrome, osmena-engine, egui_glow |
| `osmena-xtask` | tool | `icons`: renders a browser's `assets/logo.svg` into its icon set, the `.icns` from `assets/logo-macos.svg` when the browser has one (macOS icons sit on Apple's grid, inset in a transparent canvas, which would look small on Windows). | resvg |

An engine crate depends on `osmena-core` and `osmena-engine` only; it never
sees egui or winit. A browser's binary depends on `osmena-shell` and its
engine crate.

## How a browser uses it

```rust
use osmena_core::update::{UpdateSource, build_number};
use osmena_core::{Brand, ChromeColor, EngineInfo, ExtensionSupport};

static BRAND: Brand = Brand {
    name: "Nautilus",
    binary: "nautilus",
    data_dir: "Nautilus",
    homepage: "https://www.wikipedia.org/",
    chrome_color: ChromeColor::Shell,
    icon_png: include_bytes!("../../../assets/icons/nautilus-256.png"),
    version: env!("CARGO_PKG_VERSION"),
    // CI sets OSMENA_BUILD_NUMBER when it packages a build; a local
    // build has none and never updates itself.
    build: build_number(option_env!("OSMENA_BUILD_NUMBER")),
    // Where newer builds are announced (see "Updates" below), or None.
    updates: Some(UpdateSource {
        manifest_url: "https://github.com/thomasmost/nautilus/releases/latest/download/latest.toml",
    }),
};

// In the engine crate:
pub static INFO: EngineInfo = EngineInfo {
    name: "Chromium",
    version: "154",
    data_dir: "chromium",
    supports_stop: true,
    wheel_line_px: 100.0,
    // `None` for an engine without extensions.
    extensions: Some(ExtensionSupport {
        store_name: "Chrome Web Store",
        store_url: "https://chromewebstore.google.com/",
        store_item_id: web_store_item_id,
    }),
    // One process runs every open profile (see "Profiles and
    // processes" below); `false` makes each profile a process.
    profiles_in_process: true,
};

// In main, after the engine-specific preamble (helper-process dispatch,
// logging) and clap parsing:
osmena_shell::run(osmena_shell::Launch {
    brand: &BRAND,
    engine: &nautilus_engine::INFO,
    args: cli.base,
    // Called for each profile the shell opens: once, or once per open
    // profile when `profiles_in_process` is true.
    start_engine: Box::new(move |profile, waker| chromium.start(profile, waker)),
})
```

### Profiles and processes

`EngineInfo::profiles_in_process` says which of two shapes the browser
has.

- **True: one process, every profile.** The shell calls `start_engine`
  again for each profile it opens, while the engines of the profiles
  opened before still live, and keeps one `Browser` model, one engine
  and one set of windows per open profile. All of them are windows of
  the one application: on macOS they share the Dock icon, the menu bar
  (which follows the profile in front) and the window cycle. Closing a
  profile's last window drops that profile's engine and leaves the
  others; the process ends with the last one. Every engine is dropped
  before the closure, inside the event loop, so what the engines share
  (a runtime that may start once per process) belongs in the closure
  and shuts down in its `Drop`. `spin` is called on every open engine
  after each event, so an engine whose work is one loop for the whole
  process runs it from each. A second start of the browser does not
  become a second process: the first holds a lock in the brand's data
  directory (`instance.lock`, with `instance.toml` beside it naming a
  loopback port and a token only the user can read) and the later one
  sends its command line there and exits; the running browser opens
  the profile (or brings it forward) with the addresses as tabs, or a
  new window when none were given. `Profile::shared_engine_dir` (the
  default profile's engine directory) is where such an engine may have
  to keep every profile, and `Profile::shared_engine_data`
  (`profile-<name>` inside it) one profile's part of it; the shell
  removes that with the profile.
- **False: one profile, one process.** `start_engine` is called once;
  "open in another profile" starts the executable again with
  `--profile`, and nothing above applies.

`--data-dir <dir>` (hidden) keeps every profile under `<dir>` instead of
the per-user data directory: a browser apart from the installed one,
with its own lock and its own process, for scripted runs and for trying
a build beside the one in use.

The engine contract, in `osmena-engine`:

- `Engine`: `spin`, `drain_events`, `discard_pending`, `open_surface` (a
  GL context on a native window), `open_host` (the tabs of one window),
  and, for engines with extensions, `install_extension`,
  `set_extension_enabled`, and `uninstall_extension` (defaulted to
  nothing); `configure_downloads` (for engines whose
  `EngineInfo::downloads` is set) and `configure_blocked_sites`, the
  policies the shell hands down at start and on every change (after an
  unblock the new list comes before the navigations that return tabs to
  their pages, so it must take effect before the next `navigate`; both
  defaulted: an engine's before-navigate hook is to cancel a main-frame
  navigation to a blocked site before any request (an engine that cannot
  tell frames apart may cancel those too) and report
  `EngineEvent::NavigationBlocked`; the model blocks what the user types
  whether or not the engine has a hook).
  For an engine that runs extensions (`EngineInfo::extensions` is
  `Some`, with or without `ExtensionSupport::side_panels`),
  `strip_changed` hands over every window's strip (`WindowStrip`: the
  tabs in strip order with the active one flagged, deferred tabs
  included, the folders, the side panel the window shows, and which
  window has the focus) once after the first windows open and then
  whenever it differs from the last one sent, after the commands and
  events of a turn of the event loop; `extension_reply` answers an
  `EngineEvent::ExtensionRequest` by its token, exactly once, and always
  before the `strip_changed` that shows the request's effect (both
  defaulted to nothing). `StripSnapshot` is the same data owned
  (`StripSnapshot::of(&browser, browser.focused)`, `lend`), for an
  engine that keeps the last strip to compare the next against. The
  shell keeps `Browser::focused`: the browser window with the focus,
  `None` while none has it.
- `TabHost`: create, adopt, close, navigate, history, show and hide,
  focus, resize, input (every pointer event carries the keyboard modifiers
  held at that moment, from the window's own state; an engine never infers
  them from the key events it saw), `paint_page`, which hands the shell
  either a blit function (the engine painted into GL) or a frame of BGRA
  pixels with the rectangles that changed (the engine paints into memory;
  the shell uploads those and draws either under the toolbar),
  `open_extension_action` and `close_extension_popups` (defaulted to
  nothing), and `page_command` and `context_menu_closed` for the page's
  context menu: the engine reports what is under the pointer with
  `EngineEvent::ContextMenuRequested` (a `PageContext`: link, media,
  selection, editable state, misspelling), the chrome shows the menu, and
  what the user picks that only the engine can do (copy, paste, save,
  print, inspect, …) comes back as a `PageCommand`.
  `EngineInfo::page_tools` says which of those the engine offers.
  `answer_media_access` (defaulted to nothing) answers a page's request
  for the microphone or the camera, which the engine reported as
  `EngineEvent::MediaAccessRequested` and holds open meanwhile: the
  shell asks the user in a dialog of the system's, and an origin
  allowed once is answered without asking for the rest of the run.
  `replace_with_blank` replaces the tab's current history entry with
  `about:blank` when the page it shows has just been blocked, so Back
  skips it (defaulted to a plain navigation, which adds an entry instead,
  so Back from a tab blocked in place stays on the block page; an engine
  that can replace an entry should). Forward after Back from a block page
  then lands on the bare `about:blank` the block page sat over; the model
  keeps no per-entry state, so the page cannot be restored and this is
  accepted. A side panel is a second view in the window, under a `TabId`
  the model allocates and that is in no strip: `create_view` makes it
  (hidden and without focus, showing a page of an extension), `close_view`
  ends it, `set_view_origin` says where it sits on screen as
  `set_content_origin` does for the page, and `extension_command` runs an
  extension's keyboard command for the window's active tab (all four
  defaulted to nothing). Between `create_view` and `close_view` the view
  answers `has_tab`, `resize_content`, `paint_page`, `input`,
  `set_focused`, `show` and `hide` under its id as a tab does: it has a
  size of its own, its pointer positions count from its own top left, the
  shell shows it while its panel is drawn (not in full screen, not in a
  window too narrow for page and panel) and hides it otherwise, and it
  has the focus from a click in it until a click in the page or a tab
  switch. A view that ends on its own reports `EngineEvent::TabClosed` or
  `TabCrashed` with its id. An engine whose `ExtensionSupport::side_panels`
  is false sees none of this.
- `Surface`: make current, resize, the glow handle, begin frame, present.

## What the user gets

- **A look of its own: the theme.** Inter for every label, pills for
  the address bar and the Launch Panel's field, widgets and tabs
  rounded at 8 px, cards at 14 and popups at 12 on a wide soft shadow.
  Each profile's colour gives the chrome an accent, the deep shade of
  the swatch's family (or its pastel in a dark system theme): the ring
  around a focused field, the text selection, the picked suggestion,
  the lit chips and the links. The toolbar's Back, Forward, Reload and
  Stop are painted glyphs in round buttons that fill only under the
  pointer, faded while they cannot act, like every other toolbar
  button; the address bar is a filled pill a shade darker than the
  toolbar, the page's colour with the accent ring while it has the
  keyboard. Every popup sits on a surface tinted a little by the
  swatch.
- **A frameless window.** Browser windows open without the system title
  bar: the tab strip is the title bar (drag its empty space to move the
  window, double-click it to maximize, right-click it on Windows for the
  system menu) and, except on macOS, ends in minimize, maximize, and close
  controls; the edges still resize. On macOS the title bar turns
  transparent and the strip leaves room for the traffic lights; a press
  on the strip is the chrome's, never the title bar's
  (`osmena-shell`'s `titlebar.rs`), so tabs drag from anywhere on them
  and only the empty stretch moves the window. The
  setting "Use the system title bar and borders" (`system_title_bar` in
  `settings.toml`) restores the system frame; on macOS it applies to
  windows opened afterwards.
- **A pinch zooms.** Two fingers spread or pinched on a touchpad
  over the page step its zoom in and out, as Ctrl (Cmd) and the wheel
  do: the same per-site level, shown in the address bar's chip.
- **A tab that is recording says so.** While a page uses the
  microphone or the camera its tab carries a red dot left of the
  icon, slowly fading out and in; the pointer over the tab says which
  device. The dot stays however narrow the tab gets, and a collapsed
  folder's chip carries it for its tabs. The engine reports the use
  (`EngineEvent::MediaCaptureChanged`); an engine that does not shows
  no dot.
- **A new tab asks first: the Launch Panel.** Ctrl+T and the "+"
  button open a wide panel over the window instead of a blank page.
  At its top, a large field: type an address or a search and press
  Enter, or pick a page from the history and bookmark matches that
  drop down under it; Escape or a click outside opens nothing. Before
  the first space the field completes the typing to the most visited
  page whose address starts with it (`gi` becomes `github.com`, the
  rest selected after the caret, so typing on replaces it and Enter
  opens the page); Backspace drops the completion and leaves the
  typing, and a space makes the text a phrase, so Enter then searches.
  The icon at the field's left end is what Enter will open: the
  site's favicon for a page, a magnifier for a search, sparkles for a
  question.
  Shift tapped twice turns the field from searching to asking (the
  chip then names who is asked, Claude or ChatGPT as the Launch
  Settings' "Ask with" row says, and Enter opens the question
  there), and twice again turns it back.
  The completion is always a page, without the `?query` and
  `#fragment` an address in the history may carry. A typing that is
  an address already (`docs.rs`, a whole URL) opens
  as typed: the completion may finish its host but never carries it to
  a page deeper in the site.
  The first row of the dropdown is always what Enter opens (the
  completed page, the typed address with "Open", or the words with
  "Search Google"), and the engine's name sits in a chip at the
  field's right end while the text is a search. Under
  the field, a mosaic of tiles: **Recent Drive files** (the five files
  the connected Google account opened last; "Connect Google Drive"
  signs in through Google in a tab, only in a build whose brand has a
  Google client id), **Weather** (today's conditions, high and low
  where this device is: the panel asks the system for the location,
  which is what puts the question to the user, names it through
  BigDataCloud and asks Open-Meteo for the forecast, both without a
  key; a place can be named on the tile instead, and is then used
  without asking the system anything. A report stands fifteen
  minutes and a location an hour),
  **Ask Claude** and **Ask ChatGPT** (a question typed there opens the
  service with it in a new tab), and **Most visited** (a shelf across
  the panel of the five hosts with the most visits, each a disc with
  its favicon, or its initial, and the host under it; a close control
  at the disc's corner, under the pointer, hides that host). The panel
  fades and rises in, on a surface tinted by the profile's colour; a
  row of tiles is as tall as its tallest tile, and the field, a
  filled pill with a magnifier, turns the page's colour with the
  profile's accent around it while it has the keyboard. The gear at the bottom right opens the Launch
  Settings in the panel: the search engine as presets or a custom
  template (the same setting the address bar uses), the two question
  templates, the weather location and whether temperatures read in
  Fahrenheit or Celsius (unchosen, they follow the country the tile is
  showing), the Google account, and the tiles in edit mode, dragged to reorder and closed to remove, with
  "Restore default widgets". The same matches drop down under the
  address bar of an existing tab as you type. The history
  (`history.toml` in the profile) keeps every page that finished
  loading, with its title and visit count, up to 5000 entries.
- **Tabs that read at any width.** Favicons are fitted to the pixels
  their slot covers and drawn on pixel boundaries, so they are crisp
  at every tab width and display scale; a loading tab shows a turning
  arc there, a page without an icon a globe. As tabs narrow they give
  up parts in Chromium's order: the active tab keeps its close control
  and then its icon, an inactive one keeps its icon (centred when it
  is all that fits) and shows the close control only under the
  pointer; a title that does not fit fades out rather than ending in
  an ellipsis, and the tooltip has the whole of it. The active tab
  opens into the toolbar with no line between them; the strip's edge
  runs under the others.
- **Tabs that drag.** A tab (or the selected tabs) dragged along the
  strip slides between the others, which move out of its way, and lands
  where it is let go; dropped between two tabs of a folder it joins the
  folder, dropped anywhere else it leaves one. Dragged out of the window
  it moves to the window under the pointer, or to a new one. The drag
  holds while the pointer is outside the window, and Escape calls it
  off. Tabs also slide to their places after a close or a reorder.
- **Chromium's shortcuts**, with Cmd for Ctrl on macOS: tabs (`Ctrl+T`,
  `Ctrl+W`/`Ctrl+F4`, `Ctrl+Shift+T` reopens the last closed, `Ctrl+Tab`
  and `Ctrl+PgDn`/`Ctrl+PgUp` cycle, `Ctrl+1`–`Ctrl+8` and `Ctrl+9` jump),
  navigation (`Alt+Left`/`Alt+Right` or `Cmd+[`/`Cmd+]`, `Ctrl+R`/`F5`,
  `Ctrl+.` stops, `Alt+Home` or `Cmd+Shift+H` goes home), `Ctrl+L`/`Alt+D`
  for the address bar, `Ctrl+N` and `Ctrl+Shift+W` for windows, `F11` or
  `Ctrl+Cmd+F` for full screen (the page alone), `Ctrl+,` for settings,
  `Ctrl+Q` to quit. Chords use physical keys, so they are the same on every
  layout.
- **Remappable shortcuts.** The settings window lists every action with a
  primary and a secondary slot, like a game's controls screen: click a
  slot, press the new chord (Esc keeps the old one, Backspace clears it,
  right-click clears it); a chord in use moves to the action it is given
  to, and a note says what lost it. "Reset to defaults" restores the
  platform's bindings. The result is `keymap.toml` in the profile:

  ```toml
  [bindings]
  new-tab = ["Ctrl+KeyT"]
  reload = ["Ctrl+KeyR", "F5"]
  stop = []            # unbound
  ```

  Chords are modifiers (`Ctrl`, `Alt`, `Shift`, `Meta`; `Cmd`, `Win`,
  `Option` are accepted) and a W3C key code joined with `+`. An action left
  out keeps its default; a chord that does not parse is a startup error,
  like a malformed `settings.toml`.
- **Tab folders.** A tab's context menu (which acts on the selection
  when the tab is part of it: Ctrl-click adds, Shift-click extends)
  offers "Close all selected", "Move to folder…", and "Bookmark all
  selected…". A folder is named in a prompt ("Folder A", "Folder B", …
  by default, or an existing folder) and its tabs sit together in the
  strip behind one chip: collapsed, the chip shows the name and count
  and a click lists the tabs; expanded (from its menu), its tabs follow
  it. The chip's menu also renames, ungroups, or closes the folder.
  Folders are not kept across launches.
- **Session restore.** The windows and tabs are written to
  `session.toml` in the profile a moment after they change and at exit
  (their place on screen, their order, folders, the active tab, the
  closed-tab list). With "Continue where you left off" in Settings
  (`restore_windows_on_launch`), the next start opens them again where
  they were: each window's active tab loads; the others wait, drawn
  faint, until first shown, but are warmed one after another half a
  second apart (each loaded out of sight for its title and favicon,
  then let go); a URL on the command line opens as a tab in the last
  window. "Restart to update" comes back the same way whatever
  the setting says (`--restore`). After a crash (the profile's claim
  names no live process), a bar under the toolbar offers the last
  session: Restore brings it back and closes the home-page window,
  Dismiss forgets it. "Reopen closed tab" works across a restart.
- **Downloads**, when the engine downloads (`EngineInfo::downloads`).
  A file goes to the downloads directory (the system's Downloads folder,
  or the one Settings names) under its suggested name, made unique, or
  the system's save dialog asks when "Ask where to save each file" is
  on. A button appears at the toolbar's right end while the list is not
  empty, with a ring that fills as the downloads in progress do; it
  (or `Ctrl+J`, `Cmd+Shift+J` on macOS, or the ☰ menu's Downloads)
  opens a popover listing them newest first with their size, progress
  and speed, and the buttons for each one's state: Pause, Resume and
  Cancel while it goes, Open and Show in folder when done, Retry after
  a failure, and × to forget; "Clear all" and "Open folder" below.
  Quitting with a download going asks first. The list is
  `downloads.toml` in the profile (the newest 200 finished ones); what
  was still going when the browser closed is marked interrupted.
- **Page zoom.** `Ctrl+Plus` (`Ctrl+Equal`, or the numpad's plus),
  `Ctrl+Minus` and `Ctrl+0` (`Cmd` on macOS), `Ctrl` with the wheel over
  the page, the ☰ menu's Zoom submenu or the macOS View menu zoom the
  page through Chromium's steps (25% to 500%). Every tab on the same
  host follows, the site remembers its level in `zoom.toml` in the
  profile, and a chip beside the address bar shows the level while it
  is not the default, with a popover for −, +, and Reset. "Page zoom"
  in Settings (`page_zoom` in `settings.toml`) is the level of sites
  without one of their own.
- **Blocked sites.** A profile keeps a list of websites it refuses to
  show, keyed by registrable domain, so blocking `youtube.com` covers
  `www.youtube.com` and `m.youtube.com` (the public suffix list keeps
  `bbc.co.uk` a site and `co.uk` not; IP addresses and `localhost` stand
  whole; `about:`, `data:` and `view-source:` pages have no site). "Block
  on this profile" in a tab's menu asks first and warns that only the
  settings window unblocks; the "Blocked sites" section there lists the
  sites with Remove on each and a field that takes a host or a URL. The
  model stops what the user types before any request, and the engine's
  before-navigate hook stops links, scripts and redirects before any
  request where it sees them. A load the hook misses (an engine without
  one, or a redirect, a popup's first page, or a step Back or Forward it
  does not see) may be requested once, and is then replaced by the block
  page through the `UrlChanged` rule. The tab shows a block page the
  chrome paints over `about:blank`, one of three lines on the profile's
  colour with the site named beneath; the address bar keeps the attempted
  address, Back returns to the page before, Reload re-rolls the line.
  Blocking replaces the history entry of every open tab on the site;
  unblocking sends them back. The list is `blocked.toml` in the profile.
- **Find in page**, when the engine searches pages (`EngineInfo::find`).
  `Ctrl+F` (or "Find in page…" in the ☰ menu, "Find…" in the macOS Edit
  menu) opens a pill at the top right of the page: the text is searched
  as it is typed, every match is highlighted and the active one scrolled
  into view, the count reads "3 of 41" or "No matches", Enter and `F3`
  (`Ctrl+G`) step forward, Shift+Enter and `Shift+F3` (`Ctrl+Shift+G`)
  back, wrapping at the ends, "Aa" matches case, and Escape or × closes
  the bar leaving the active match selected. Switching tabs runs the
  search again in the new tab. An engine without it has the entries
  disabled.
- **Bookmarks.** The star at the address bar's right end (or `Ctrl+D`)
  bookmarks the page into the group the last bookmark went to ("Bookmarks
  bar" to begin with) and opens a popover under the star to name it,
  move it to another group (or a new one), or remove it; the star is
  filled on a bookmarked page, and clicking it again edits. "Bookmark
  all selected…" in the tab menu (and "Bookmark all tabs…" in the
  Bookmarks menu) asks for a group name and saves the pages together.
  Bookmarks live in `bookmarks.toml` in the profile. The bookmarks bar
  (`Ctrl+Shift+B`, or the Bookmarks menu's check item, or Settings) sits
  under the toolbar: the "Bookmarks bar" group's pages as chips with
  the site's favicon (a click loads the page, a middle click opens it
  behind, a right click offers open in new tab or window, copy, edit
  and delete, and a drag reorders), a "»" chip for the ones that did not
  fit, and the other groups as drop-down chips at the right end. The
  Bookmarks menu (in the ☰ menu and the macOS menu bar) has the same
  actions, import and export of an HTML bookmarks file (the format
  every browser reads and writes), and a submenu per group with "Open
  all", the pages, rename and delete. Bookmarks also appear among the
  suggestions under the address bar and in the launcher, with a star,
  ranked above history matches of the same kind. Favicons of visited
  sites are kept as PNGs under `favicons/` in the profile so the bar
  shows them before a site is open.
- **Updates over the air**, when the brand names a source and the build
  has a number. The browser checks shortly after it starts and every
  six hours, downloads a newer build in the background, and offers
  "Restart to update" in the toolbar, the window menu, and the settings
  window; the restart installs it and reopens the same profile. See
  "Updates" below for what a browser's CI publishes.
- **Extensions**, when the engine runs them (`EngineInfo::extensions`).
  The toolbar ends in a puzzle-piece button: its menu lists the installed
  extensions (click one to run its action: its popup under the button, or
  the extension's own click when it has no popup), has a "Pin to
  toolbar" submenu with a check per extension, offers to install the
  extension on the active tab's store page, opens the store, loads an
  unpacked directory, and opens the settings window, whose Extensions
  section switches each extension on or off, pins it, removes it, installs
  by store URL or id, and lists each extension's shortcuts. One that will
  not run its command is marked "Not active" with the reason:
  the keymap holds its keys, another command holds them, or the extension
  is off. Pinned extensions sit as icons beside the
  address bar; clicking one opens the popup under it, or runs the action
  when it has no popup. On an engine with `side_panels`, a panel an
  extension opens sits at the page's right edge under a header with its
  icon, its name (or its id) and a close control; the puzzle menu's
  "Close side panel" closes it. The model only
  holds the list (`Browser::extensions`) and the pinned ids
  (`pinned_extensions` in `settings.toml`); installing, enabling, and
  running them is the engine's, through `Engine::install_extension`
  and its siblings, and the engine reports every change with
  `EngineEvent::ExtensionsChanged`.
- **The page's context menu**, shaped like Chromium's: a link opens in
  a new tab (behind the current one, beside it) or a new window, is
  saved, or has its address copied; an image opens in a new tab, is
  saved, copied, or has its address copied; a video or audio loops,
  shows its controls, is saved, or has its address copied; selected text
  is copied, searched, or printed; an editable field gets spelling
  suggestions, Undo, Redo, Cut, Copy, Paste, Paste as plain text,
  Delete, and Select all; a plain page gets Back, Forward, Reload, Save
  as, Print, and View page source; Inspect comes last. What the engine
  cannot do is left out. Middle-click and Ctrl-click open a link in a
  background tab beside the current one, Shift-click in a new window.
- **Search.** Address-bar text that is not a URL is searched, with
  Google by default; `[search]` in `settings.toml` (and the settings
  window) names the provider and its URL, where `{query}` stands for the
  text, so another engine, or something other than a search engine, can
  take its place.
- **Profiles.** A profile is a directory of its own (settings,
  bookmarks, history, the engine's cookies and storage; `--profile
  <dir>` picks one, `default` otherwise) with a name, a colour and an
  avatar. The toolbar ends in the avatar; its menu, the same Profiles
  menu the macOS menu bar has, is shaped like Chromium's: the current
  profile and "Edit…", the other profiles under an "Other <brand>
  Profiles" heading (each opens that profile's window, or brings it
  forward when it is open: with an engine that runs them together the
  profiles are windows of one application, otherwise each is a
  process), then "Add <brand> Profile…", "Open Guest Profile" and
  "Manage Profiles…" (a dialog with a row per profile: open, edit or
  delete it, and the two ways to add one). The dialog asks for
  the name, the theme colour and the avatar (the name's initial, or a
  plain silhouette drawn to read on the toolbar's small disc, from
  three tabs: sea creatures, the 24 most used emoji, whose faces are
  their features on the disc itself, and an assorted set; the colour
  is one of seven hues, each in a pastel, a middle and a deep shade)
  and,
  for any profile but the default one, whether to share
  the default profile's bookmarks (one list both keep) and whether to
  copy its extensions (installed here, then independent; only when
  the engine runs extensions). History is never shared. A new
  profile's directory is the slug of its name (`work-stuff`, then
  `work-stuff-2`); deleting one asks first, refuses a profile that is
  open, and for the current profile closes its windows (handing over
  to the default one when nothing else is open) and removes the
  directory as it goes. A guest
  profile is a throwaway: "Open Guest Profile" makes one (`guest`,
  then `guest-2`, marked by an empty `guest` file; named "Guest",
  grey, a ghost avatar, nothing shared) and opens it; its directory
  goes when its last window closes. A guest is listed in no menu,
  cannot be edited, and one a crash left behind is swept at the next
  start once it is a minute old and no browser claims it. The name of
  a profile deleted while it was open is not given out again until the
  browser has quit, and what its engine left on disk is swept at the
  next start. With every profile in one process, Quit quits them all
  (each asking first while it has downloads going), and the next start
  that names no profile opens the one that was in front, with the
  others that were open behind it when their settings bring their
  windows back (`open-profiles.toml` in the brand's data directory).
- **The default browser (macOS).** While the browser is not the
  system's default for web links, a bar under the toolbar offers to
  make it so (Make default, Don't ask again, Not now); the settings
  window's Default browser section shows the status with the same
  button and the switch for the bar. "Make default" asks the system,
  which asks the user. Links clicked in other apps arrive as `GURL`
  Apple Events and pages opened from the Finder as `odoc`: each opens
  as a tab of the focused window, brought to the front, and a link the
  browser was launched for takes the start's home-page tab instead. A
  browser gets all this by claiming `http` in its bundle's
  `Info.plist` (`CFBundleURLTypes`; also `https`, and HTML pages in
  `CFBundleDocumentTypes` for the system to list it); without the claim,
  or outside a bundle, nothing of it shows. Not on Windows yet.
- **Settings** (`settings.toml`): homepage, new-tab URL, the search
  provider, a chrome colour per profile, the title bar switch, the
  pinned extensions, the session-restore switch, the default-browser
  offer (`check_default_browser`), the bookmarks bar switch and the
  group the last bookmark went to, the default page zoom, `[downloads]`
  (the directory and whether to ask each time), `[launch]` (the
  panel's widgets in order, the question templates, the weather
  location and units, hidden sites), and `[profile]` (name, avatar, shared
  bookmarks, extensions to copy at the next start).

## Updates

The shell updates a browser over the air from a manifest its CI publishes
with every build. The browser's side is three fields of its `Brand`: the
crate version, the build number CI stamped (`OSMENA_BUILD_NUMBER` in the
environment of the release build, read with `option_env!` through
`osmena_core::update::build_number`; a build without one never updates
itself), and the manifest's URL. The manifest is a TOML document,
`osmena_core::update::Manifest`:

```toml
version = "0.1.0"
build = 150
commit = "0123abcd"
published = "2026-10-29T12:00:00Z"
notes_url = "https://github.com/owner/repo/releases/tag/build-150"

[assets.windows-x86_64]
name = "nautilus_0.1.0_x64-setup.exe"
url = "https://github.com/owner/repo/releases/download/build-150/nautilus_0.1.0_x64-setup.exe"
sha256 = "…"
size = 123456789

[assets.macos-aarch64]
name = "Nautilus-macos-arm64.zip"
url = "…"
sha256 = "…"
size = 456789012
```

A build is newer when its version is higher, or its build number is
within the same version. The asset keys are `<os>-<arch>` as Rust names
them (`windows-x86_64`, `macos-aarch64`, `macos-x86_64`); a platform
without an asset gets nothing from that build. On Windows the asset is
an NSIS installer that takes `/S` (cargo-packager's does); on macOS it is
a zip of the application bundle (`ditto -c -k --keepParent`). The
browsers publish a GitHub release per green push to `main`, tagged
`build-<run number>`, with the installers, the bundles, the symbols and
`latest.toml`; `releases/latest/download/latest.toml` is then the
manifest URL, which GitHub redirects to the newest release.

The shell checks 15 seconds after the engine starts and every six hours
after that (`check_for_updates` in `settings.toml` turns the automatic
checks off; "Check for updates…" still works by hand), downloads into
`<brand data dir>/updates/<build>/`, verifies the SHA-256, and shows the
"Restart to update" pill. The restart quits and installs: on Windows a
hidden PowerShell waits for the process to exit, runs the installer with
`/S` (which closes every other window of the browser too) and starts
the browser again; on macOS the running bundle is moved into the
updates directory, the unpacked one takes its place, and a shell reopens
it once the process is gone. What comes back is every profile that was
open when they share the process, the one profile otherwise, each with
its windows (`--restore`). A check that finds the
browser up to date removes what the updates directory holds for builds
up to the running one. To try the flow against a local server, build
with `OSMENA_BUILD_NUMBER=1` and start the browser with the hidden
`--update-manifest http://127.0.0.1:8000/latest.toml`.

## The Launch Panel's Google client

The Drive tile signs in with OAuth 2.0 for installed apps (PKCE, a
loopback redirect served by the shell for one request, the consent
page in an ordinary tab). The browser's side is one field of its
`Brand`, `google_client_id`: the id of a Google Cloud "Desktop app"
OAuth client, which is not a secret. A brand reads it at compile time
from an environment variable (`option_env!`) so CI stamps it from a
repository variable and nothing is committed; `None` leaves the Drive
tile out of the panel and the Google row out of the Launch Settings.
The refresh token lives in the profile's `google.toml`
(`Profile::google_path`), never shared between profiles, revoked and
removed by Disconnect. The scopes are `drive.metadata.readonly` and
`userinfo.email`, nothing else.

## Depending on osmena

Browsers pin a revision, never a branch, so any checkout builds the same:

```toml
[workspace.dependencies]
osmena-core = { git = "https://github.com/thomasmost/osmena", rev = "<sha>" }
osmena-engine = { git = "https://github.com/thomasmost/osmena", rev = "<sha>" }
osmena-shell = { git = "https://github.com/thomasmost/osmena", rev = "<sha>" }
```

The four crates version together; bump all pins at once. To work on osmena
and a browser side by side, patch the git source to a checkout in
`~/.cargo/config.toml` (config patches override manifests):

```toml
[patch."https://github.com/thomasmost/osmena"]
osmena-core = { path = "/Users/you/vibes/browser-project/osmena/crates/osmena-core" }
osmena-engine = { path = "/Users/you/vibes/browser-project/osmena/crates/osmena-engine" }
osmena-chrome = { path = "/Users/you/vibes/browser-project/osmena/crates/osmena-chrome" }
osmena-shell = { path = "/Users/you/vibes/browser-project/osmena/crates/osmena-shell" }
osmena-xtask = { path = "/Users/you/vibes/browser-project/osmena/tools/osmena-xtask" }
```

Cargo still resolves the pinned revision from GitHub even when every crate
is patched, so the repository must be reachable. Then commit in osmena,
push, and move the browser's `rev` to the new sha.

## Development

- `cargo test --workspace --features osmena-engine/glutin`
- `cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings`
- `cargo fmt --all`

Nothing here needs an engine, so the whole workspace builds in under a
minute. The pins on winit, egui, and glow are exact because the engines
share a GL context and window handles with them; a browser resolves to the
same versions through its dependency on these crates.

### Glyphs

The chrome's glyphs and the profile avatars are SVG drawings on a
16-unit grid, `crates/osmena-chrome/assets/icons` and `assets/avatars`,
one file per variant of `icons::Icon` and of `osmena_core::Avatar`
(`StarFilled` is `star-filled.svg`). They reach the chrome as two
fonts, which the text renderer rasterises at whatever scale the
display has. After changing or adding a drawing:

- `python tools/glyphs/build.py` rebuilds `OsmenaIcons.ttf` and
  `OsmenaAvatars.ttf` (it needs `fonttools` and `skia-pathops`; the
  script's own header says what a drawing may contain and has the
  `--preview` option). Commit the fonts with the drawings.
- `cargo test -p osmena-chrome sheet -- --ignored` writes pictures of
  the chrome with the new fonts to `target/sheets`, magnified ones
  among them: look at the glyph at its real size before believing it.

An icon is drawn at 16 points, where a grid unit is a pixel on an
unscaled display: a line that should be sharp runs along a row of
pixels (its centre on a half unit), in the set's weight of a unit and a
half with round ends. An avatar is a solid silhouette inside the circle
its disc leaves it, with nothing finer than a pixel; a face is its
features only. The tests fail for a variant without a drawing, a
drawing without a variant, a glyph missing from its font, an icon that
leaves its box, and an avatar that reaches the rim of its disc.

## Licence

MIT or Apache-2.0, at your option. `osmena-chrome/src/keyutils.rs` is
adapted from Servo's reference shell and keeps its MPL-2.0 header.
`OsmenaAvatars.ttf` is under the SIL Open Font License, because its
whale is Noto Emoji's (`assets/fonts/LICENSE-OsmenaAvatars.txt`); the
other drawings in it are this project's own.
