# osmena

The shared shell of [Paladin](https://github.com/thomasmost/paladin) (Servo)
and [Nautilus](https://github.com/thomasmost/nautilus) (Chromium): the
browser model, the egui chrome, and the winit application, written once and
built on top of any engine that implements a small trait. Named after the
nautilus shell's iridescent inner layer, which is also the paladin's armour.

A browser built on osmena is four things:

1. a `Brand` constant (name, data directory, homepage, icon);
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
| `osmena-chrome` | chrome | The egui tab strip, toolbar, status overlay and crash panel; the theme (Inter, bundled under `assets/fonts` with its SIL Open Font License, and the shared style); the window, tab and page menus and the macOS menu bar as data and as native menus; pointer and key routing; the icon and the fatal-error dialog. | osmena-core, egui, winit, muda |
| `osmena-shell` | shell | The winit `App`, `BrowserWindow`, `SettingsWindow`, the process glue, and `run`. Generic over `Engine`. | osmena-chrome, osmena-engine, egui_glow |
| `osmena-xtask` | tool | `icons`: renders a browser's `assets/logo.svg` into its icon set. | resvg |

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
};

// In main, after the engine-specific preamble (helper-process dispatch,
// logging) and clap parsing:
osmena_shell::run(osmena_shell::Launch {
    brand: &BRAND,
    engine: &nautilus_engine::INFO,
    args: cli.base,
    start_engine: Box::new(|dir, waker| nautilus_engine::Engine::new(dir, waker)),
})
```

The engine contract, in `osmena-engine`:

- `Engine`: `spin`, `drain_events`, `discard_pending`, `open_surface` (a GL
  context on a native window), `open_host` (the tabs of one window), and,
  for engines with extensions, `install_extension`,
  `set_extension_enabled`, and `uninstall_extension` (defaulted to
  nothing).
- `TabHost`: create, adopt, close, navigate, history, show and hide, focus,
  resize, input (every pointer event carries the keyboard modifiers held
  at that moment, from the window's own state; an engine never infers
  them from the key events it saw), `paint_page`, which hands the shell
  either a blit function (the engine painted into GL) or a frame of BGRA
  pixels with the rectangles that changed (the engine paints into
  memory; the shell uploads those and draws either under the toolbar),
  `open_extension_action` and `close_extension_popups` (defaulted to
  nothing), and `page_command` and `context_menu_closed` for the page's
  context menu: the engine reports what is under the pointer with
  `EngineEvent::ContextMenuRequested` (a `PageContext`: link, media,
  selection, editable state, misspelling), the chrome shows the menu,
  and what the user picks that only the engine can do (copy, paste,
  save, print, inspect, …) comes back as a `PageCommand`.
  `EngineInfo::page_tools` says which of those the engine offers.
- `Surface`: make current, resize, the glow handle, begin frame, present.

## What the user gets

- **A frameless window.** Browser windows open without the system title
  bar: the tab strip is the title bar (drag its empty space to move the
  window, double-click it to maximize, right-click it on Windows for the
  system menu) and, except on macOS, ends in minimize, maximize, and close
  controls; the edges still resize. On macOS the title bar turns
  transparent and the strip leaves room for the traffic lights. The
  setting "Use the system title bar and borders" (`system_title_bar` in
  `settings.toml`) restores the system frame; on macOS it applies to
  windows opened afterwards.
- **A new tab asks first.** Ctrl+T and the "+" button open the
  launcher, a large field over the window, instead of a blank page:
  type an address or a search and press Enter, or pick a page from the
  history's matches with Up and Down; Escape or a click outside opens
  nothing. The same matches drop down under the address bar of an
  existing tab as you type. The history (`history.toml` in the
  profile) keeps every page that finished loading, with its title and
  visit count, up to 5000 entries.
- **Tabs that drag.** A tab (or the selected tabs) dragged along the
  strip slides between the others, which move out of its way, and lands
  where it is let go; dropped between two tabs of a folder it joins the
  folder, dropped anywhere else it leaves one. Dragged out of the window
  it moves to the window under the pointer, or to a new one. Tabs also
  slide to their places after a close or a reorder.
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
  extensions (click one to open its action's popup), has a "Pin to
  toolbar" submenu with a check per extension, offers to install the
  extension on the active tab's store page, opens the store, loads an
  unpacked directory, and opens the settings window, whose Extensions
  section switches each extension on or off, pins it, removes it, and
  installs by store URL or id. Pinned extensions sit as icons beside the
  address bar; clicking one opens the popup under it. The model only
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
  menu the macOS menu bar has, shows the current profile and "Edit…",
  the other profiles (each starts a browser for it: a profile is a
  process), "Add Profile…" and "Delete Profile". The dialog asks for
  the name, the theme colour and the avatar (the name's initial, or a
  flat monochrome glyph from three tabs: sea creatures, the 24 most
  used emoji, and an assorted set; the colour is one of seven hues, each
  in a pastel, a middle and a deep shade) and,
  for any profile but the default one, whether to share
  the default profile's bookmarks (one list both keep) and whether to
  copy its extensions (installed here, then independent; only when
  the engine runs extensions). History is never shared. A new
  profile's directory is the slug of its name (`work-stuff`, then
  `work-stuff-2`); deleting one asks first, refuses a profile open in
  another browser, and for the current profile hands over to the
  default one and removes the directory on the way out.
- **Settings** (`settings.toml`): homepage, new-tab URL, the search
  provider, a chrome colour per profile, the title bar switch, the
  pinned extensions, the restore flag, the bookmarks bar switch and the
  group the last bookmark went to, the default page zoom, `[downloads]`
  (the directory and whether to ask each time), and `[profile]` (name,
  avatar, shared bookmarks, extensions to copy at the next start).

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
the same profile again; on macOS the running bundle is moved into the
updates directory, the unpacked one takes its place, and a shell reopens
it for the profile once the process is gone. A check that finds the
browser up to date removes what the updates directory holds for builds
up to the running one. To try the flow against a local server, build
with `OSMENA_BUILD_NUMBER=1` and start the browser with the hidden
`--update-manifest http://127.0.0.1:8000/latest.toml`.

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

## Licence

MIT or Apache-2.0, at your option. `osmena-chrome/src/keyutils.rs` is
adapted from Servo's reference shell and keeps its MPL-2.0 header.
