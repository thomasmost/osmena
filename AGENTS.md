# osmena

The shared shell of Paladin (Servo) and Nautilus (Chromium): the browser
model, the egui chrome, and the winit application, generic over an engine
trait. The browsers are separate repositories that depend on these crates by
git revision. `README.md` has the crate table and the engine contract.

_Last updated: 2026-11-08 (blocked sites: a per-profile list keyed by
registrable domain, the guard in `dispatch`, the engine hook contract,
the block page, the settings section; before that restored tabs warmed
one by one for their titles and favicons, tab titles centred by their
capitals; before that the weather tile follows this device, asking
the system for the location, with Fahrenheit and Celsius a row of their
own in the Launch Settings and the units following the system's locale
or, failing that, the country the tile is showing; before that the strip on the pixel grid,
icons fitted to their pixels, Chromium's order as tabs narrow, the
active tab open into the toolbar; earlier that day the Profiles menu shaped like Chromium's,
guest profiles and the Manage Profiles dialog; before that the default
browser on macOS: the status, the offer bar, the settings section, the
shell's Apple Event handlers for links and pages other apps hand over;
modifier keys carry their own modifier; the Dock keeps a bundle's icon;
osmena-xtask's macOS icon source; the Launch Panel, session restore,
downloads, page zoom, find in page, bookmarks, updates over the air)._

## Architecture

Cargo workspace (Rust 1.97.1 in `rust-toolchain.toml`, edition 2024), five
crates, dependencies pointing only downwards:

`osmena-core` (model, pure) ← `osmena-engine` (the `Engine`/`TabHost`/
`Surface` traits) and `osmena-chrome` (egui chrome, menus, routing) ←
`osmena-shell` (winit app, windows, the command line with its hidden
`--exit-after <seconds>` that quits through the normal path after
logging the open tabs, for smoke tests, `run`). `tools/osmena-xtask` is a
library the browsers' `xtask` binaries call for icon generation.

Data flow: the chrome and the keymap emit `Command`s; `osmena_core::dispatch`
updates the model and returns `EngineAction`s, which the shell forwards to
the window's `TabHost` or handles itself. Engine callbacks become
`EngineEvent`s that `osmena_core::apply` folds into the model. Both
functions are pure and unit-tested; core ids never meet engine ids outside
an engine crate.

## Commands

- `cargo test --workspace --features osmena-engine/glutin`
- `cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings`
- `cargo fmt --all` (CI runs `--check`).

No engine is needed; the workspace builds in under a minute.

## Conventions

- **Nothing here imports an engine.** The engine crates live in the
  browsers' repositories and implement the traits in `osmena-engine`.
  Everything that crosses the boundary is a core type: `Command`,
  `EngineAction`, `EngineEvent`, `EngineInput`, `PagePaint`.
- **Branding is injected, never embedded.** A product string, path,
  icon, or a fresh profile's chrome colour comes from the `Brand` the
  binary passes in; an engine fact (name, version, data directory,
  what it supports, wheel distance) comes from `EngineInfo`. Neither
  is a global: functions that need one take a reference. Grep for
  "Paladin", "Nautilus", "Servo", "Chromium" before committing; only
  the README and this file may name them.
- **The look is `osmena_chrome::theme`.** The typeface (Inter, bundled
  under `assets/fonts` with its licence), the text sizes, the widget
  rounding (`CORNER`, which the tabs share), the input padding
  (`INPUT_MARGIN`) and the pill fields (`pill_fields`) are set there
  once per egui context (`install`, called by both windows). A new
  field takes `INPUT_MARGIN`; a new colour goes through `Palette`, which
  picks dark or light text by the swatch's luminance. A new swatch
  joins a hue of `ChromeColor::FAMILIES` (pastel, middle, deep; a test
  checks they darken in that order). Avatar glyphs come from
  `theme::AVATARS` only, a cut of Noto Emoji: a new avatar needs its
  code point added to that cut (the recipe is on `AVATARS`; a test
  fails otherwise).
- **Every user action is a `Command`.** New chrome controls and shortcuts
  add a `Command` (and a `CommandKind` if it needs a hotkey), never a
  direct engine call. A new `CommandKind` needs a `name` (stable: it is
  the key in `keymap.toml`), a `label`, a place in `CommandKind::ALL`
  (the editor's row order), and a default chord on both platforms (a test
  checks); the editor and the keymap file pick it up from there.
- **The keymap file is an overlay.** `Keymap::parse` lays `keymap.toml`
  over the platform defaults: an action named in the file gets exactly
  the chords listed, an action left out keeps its default. The editor
  always writes every action, so a saved file is complete.
- **Extensions are the engine's; the model only lists them.** An engine
  that runs extensions sets `EngineInfo::extensions` (the store's name,
  URL, and how to read an item id off a store page) and implements
  `Engine::{install_extension, set_extension_enabled,
  uninstall_extension}` and `TabHost::{open_extension_action,
  close_extension_popups}`; it reports the list, after every change,
  with `EngineEvent::ExtensionsChanged`. The chrome draws the puzzle menu
  and the pinned actions only when `extensions` is `Some`; pinned ids
  live in `Settings::pinned_extensions`, kept even for an extension that
  is gone. An engine without extensions (`None`) needs none of this.
- **Bookmarks are the model's; the star, the bar and the menus are
  views of it.** `Bookmarks` (`bookmarks.rs`) is flat groups of pages;
  the group named `Bookmarks::BAR` is what the bar shows, and
  `Settings::last_bookmark_group` is where the star puts the next page.
  Every edit is a `Command` through `dispatch` (`ToggleBookmark` adds
  then asks `Prompt::Bookmark`; `EditBookmark` is the popover's answer),
  which answers `EngineAction::SaveBookmarks`; the shell reloads shared
  bookmarks before any command for which `Command::changes_bookmarks`
  is true. Import and export are `bookmarks_html` (pure, tested); the
  shell only shows the file dialog (`EngineAction::PickFile`, `rfd`)
  and writes the file (`EngineAction::ExportBookmarks`). Suggestions
  come from `suggest::suggestions`, never from `History::matches`
  directly, so bookmarks and history rank together. Site favicons are
  the shell's (`favicons.rs`, `Profile::favicons_dir`), handed to the
  chrome as `ChromeContext::site_icons` by host (`Bookmarks::site_key`).
- **The session is the shell's file; restoring is the model's.**
  `Session::capture` reads the model plus each window's geometry; the
  shell writes it on a timer after any command, URL or title change,
  move or resize, and at exit (`save_session`), and reads it in `run`.
  `command::restore_session` rebuilds the windows: `open_window` for
  each with its active tab, the other tabs pushed as `Tab::deferred`
  (no engine tab yet), folders by name, and a `PlaceWindow` action
  ahead of the `OpenWindow`, which the shell turns into window
  attributes (`pending_geometry`, dropped off-monitor). The active tab
  is never deferred: `show_tab` creates a deferred tab the moment it is
  activated (also when the active tab closes and it is next), and
  `close_tab`/`close_window` ask the engine nothing for one. A crash is
  a `running.pid` that names no live process at start; the session is
  then `Browser::crashed_session` and the chrome offers it.
- **Downloads are the engine's work and the model's list.** The engine
  saves the file where `Engine::configure_downloads` said (the shell
  resolves `Settings::downloads` to a `DownloadPolicy` at engine start
  and on every settings change) and reports every change as
  `EngineEvent::DownloadUpdated` under its own id; `apply` upserts
  `Browser::downloads` by that id and asks `SaveDownloads` only when a
  state changes (progress is not worth a write). Commands name the
  model's `DownloadId`; `dispatch` maps them to the engine's id
  (`engine_download`) and only while the state fits. Opening and
  revealing files is the shell's (`open_path`, `reveal_path`), as is
  the folder dialog. `quit_guard` in `dispatch` turns Quit, the last
  window's close and the last tab's close into `Prompt::QuitWithDownloads`
  while anything is going; `ForceQuit` is the answer. On exit the shell
  marks what was going as interrupted and writes the list.
- **Zoom is per site, kept by the model, applied by the engine.**
  `Tab::zoom` is the level; `set_zoom` in `command.rs` changes every
  tab on the host at once and records the site in `Browser::zoom`
  (`SiteZoom`; the default, `Settings::page_zoom`, is recorded as no
  entry) and `apply` gives a tab its site's level when its URL changes,
  so an engine only ever receives `EngineAction::SetZoom` and never
  remembers anything itself (Chromium keeps its own per-host zoom, which
  agrees because the model re-applies after every navigation). The
  chord table has two chords per zoom kind (the row and the numpad);
  the shell turns `Ctrl`+wheel over the page into `ZoomIn`/`ZoomOut`
  a notch at a time. The settings window's default goes through
  `command::apply_default_zoom`.
- **Blocked sites are the model's list; the engine refuses, the chrome
  reminds.** `Browser::blocked` (`BlockedSites`, `blocked.toml`) is a
  set of registrable domains: `BlockedSites::site_for` reduces a URL
  through the `psl` crate (an IP or a single label stays whole, a bare
  public suffix and a host-less URL are no site) and `blocks` names the
  site a URL falls under. Every navigation the user starts goes through
  one guard in `command.rs` (`block_for`, `tab_at`, `wake_mark`: the
  `Navigate` arm, `new_tab`, `new_background_tab`, `open_window`,
  `show_tab`, `restore_session`; `warm_tab` declines a blocked tab): a
  blocked destination keeps `Tab::url`, sets `Tab::blocked`
  (`BlockedPage`, the site and which of `BLOCKED_LINES` it shows) and
  loads `about:blank` behind the chrome's page (`chrome::draw_blocked_page`,
  painted by `BrowserWindow::frame` like the crash panel, with
  `content_is_egui` keeping input in the chrome; while an open prompt is
  up, egui takes every key (`ChromeState::takes_every_key`, which is how
  Escape reaches a buttons-only prompt; the downloads popover and the
  Launch Panel count too), and page-bound key presses are dropped while
  the chrome owns the content area). What the model never
  sees, links, scripts and redirects, the engine's before-navigate hook
  cancels before any request and reports as
  `EngineEvent::NavigationBlocked`; `apply` sets the same state and
  pushes `about:blank`. `UrlChanged` ignores `about:blank` while a tab
  is blocked, re-blocks a blocked URL that reached the engine without a
  hook (`ReplaceWithBlank`), and clears the block otherwise; a block
  page is no visit. `BlockSites` marks every tab on the sites (a live
  tab's entry is replaced through `TabHost::replace_with_blank`, a
  deferred one wakes on the page), saves the file and hands the list to
  `Engine::configure_blocked_sites`, which the shell also calls at start.
  `UnblockSite`, issued only by the settings window, saves the file and
  hands the new list to the engine before it sends the tabs back, so the
  engine's hook never judges those navigations by the old list; each
  tab blocked on the site then navigates back to its own page. Both
  `configure_blocked_sites` and `TabHost::replace_with_blank` are
  defaulted. The line is picked
  by `pick_line` from `line_seed()`, never in the chrome, so it holds
  between frames; `Reload` re-rolls it.
- **The find bar is the window's state; the engine does the finding.**
  `Window::find` (`FindState`) is open or not; the chrome draws the bar
  from it and edits a copy of the text, sending `Command::FindText` as
  it changes. `dispatch` turns the commands into `EngineAction::Find`
  and `StopFinding` for the active tab and moves the search when the
  active tab changes (`find_follows_switch`); `apply` folds
  `EngineEvent::FindResult` into the state for the active tab only,
  letting interim counts only grow. An engine says whether it can with
  `EngineInfo::find`; the shell drops the find commands otherwise and
  the menus disable the entries, so a chord on such an engine does
  nothing.
- **A question to the user is a `Prompt`.** A menu entry that needs
  input (a folder or group name) issues a `Command::Prompt…`, `dispatch`
  answers with `EngineAction::ShowPrompt` carrying what the question
  needs (the suggested name, the existing choices), the window hands it
  to `ChromeState::open_prompt`, and the chrome draws it over the page;
  the answer is an ordinary `Command`. Nothing in the chrome decides what
  to ask.
- **The strip's row is laid out by hand.** `strip_row` measures every
  tab and chip (`Slot`), places each at an animated offset
  (`Context::animate_value_with_time` keyed by the tab or folder id, so
  a tab slides to its place after any reorder), and draws the tabs being
  dragged (`ChromeState::tab_drag`) last, riding with the pointer, with
  a gap opened at the index `Command::MoveTabs` would get. A drop inside
  the window is that command; outside it is a `TabDrop` for the
  application. A tab's widget id is `("tab", id)` every frame, which is
  what lets egui keep the drag while the tab moves. Everything in
  the strip sits on the pixel grid: a slot's rect is rounded to
  pixels before it is drawn, the strip's edge is one pixel row
  (`strip_edge_y`), the active tab's body (`active_tab_body`) opens
  into the toolbar and every other tab (`inactive_tab_body`) stops
  a pixel above the edge. An icon is never drawn at its model size:
  `favicon::fit` resamples it to the pixels its slot covers, cached
  per size in `CachedFavicon` through `icon_texture`, and
  `favicon::snap` gives the box to draw it in. A tab's parts are
  placed by `tab_layout` (Chromium's order as a tab narrows), a
  pure function with tests. A label in the strip is centred by
  `text_top_centered`, never by `galley.size().y / 2.0`: a galley's
  box has room over the capitals and none under the baseline, so the
  box centres the text about two pixels low beside a favicon. The band
  that is centred instead runs from the top of a capital to the foot
  of a descender, measured from `TEXT_BAND` rather than from the
  label, so titles without descenders keep the same height as the
  rest.
- **Folders are contiguous runs of tabs.** `Tab::folder` names the
  folder; `Window::gather_folder` keeps a folder's tabs together where
  its first tab is, `prune_folders` drops a folder whose last tab went
  (every tab removal calls it), and `strip_items` is the strip as drawn.
  New tabs go to the end, outside any folder.
- **The page's context menu is the engine's report and the chrome's
  menu.** An engine never shows its own: it reports what is under the
  pointer as `EngineEvent::ContextMenuRequested` with a `PageContext`
  (as much as it knows: link, media, selection, editable state,
  misspelling), the window shows `menus::page_menu` from it (shaped like
  Chromium's, with the entries `EngineInfo::page_tools` allows), and the
  choice is a `Command`: what the model can do itself (open a link in a
  tab or window, copy an address, search) stays in the model, what only
  the engine can do (copy, paste, save, print, inspect, …) is a
  `Command::Page` with a `PageCommand`, which reaches
  `TabHost::page_command`; `ContextMenuClosed` follows either way so the
  engine lets go of what it held. The point is the engine's to remember.
- **Address-bar text is a URL or a search.** `location_input_to_url`
  decides; what is not a URL is `Command::Search`, and `dispatch`
  resolves it with `Settings::search` (`url_for`), the one place that
  knows the provider. Nothing else builds a search URL.
- **A new tab asks for its address first: the Launch Panel.**
  `Command::NewTab` without a URL is `EngineAction::ShowLauncher`,
  never a tab; the chrome's panel (`osmena-chrome/src/launch.rs`,
  `ChromeState::open_launcher`) answers with `NewTab { url: Some(..) }`,
  `SearchInNewTab` or `AskInNewTab`, or with nothing. The `new_tab_url`
  setting is what a new *window* opens on. The tiles under the field
  are `osmena_core::launch`: `WidgetKind` names them, `Settings::launch`
  (`LaunchSettings`) holds the order and what they need, and
  `Browser::launch` (`LaunchData`) holds what the shell fetched, as
  `Fetched<T>` (`Empty`, `Loading`, `Ready { at }`, `Failed`). Nothing
  is fetched while the panel is closed: `dispatch` adds the fetch
  actions to `ShowLauncher` (and to `RefreshLaunch`, the place and
  units commands) only for a tile that is shown, never while a fetch
  is going, and only when the data is older than its lifetime; the
  data moves to `Loading` in the same step. The shell's `launch/`
  module answers on threads through `AppEvent::Launch(LaunchUpdate)`,
  which `LaunchData::apply` folds in. The Drive tile exists only when
  `Brand::google_client_id` is `Some` (`DriveState::Unavailable`
  otherwise, and `visible_widgets` drops it); the sign-in is the
  shell's loopback listener plus a normal tab, the refresh token the
  profile's `google.toml`. A question tile's URL is built in
  `dispatch` from its `UrlTemplate`, like a search. The Launch
  Settings are drawn in the panel, not the settings window; the
  settings window's search rows edit the same `Settings::search`.
- **The weather tile follows the device unless a place is named.**
  `WeatherSettings::location` of `None` means "where this device is":
  `launch_fetches` asks for `EngineAction::FetchLocation`, the shell's
  `launch::location::current` asks the system (Windows: WinRT's
  `Geolocator`, whose access request is what the user answers; other
  systems say they cannot yet), and the same thread names the position
  through BigDataCloud and goes straight on to the forecast, so one
  opening costs one wait. The answer is `LaunchUpdate::Location` into
  `LaunchData::located`, which stands for `LOCATION_LIFETIME`. A
  `Some(place)` is the user's own choice and asks the system nothing.
  Two rules keep the permission honest: nothing falls back to guessing
  the position from the network, because a guess behind a refused
  permission is what the permission exists to prevent; and a refusal
  is re-asked only on `force` (Retry, or handing a named place back),
  never at every opening. A fresh location is reused even when forced,
  so changing the units costs no new fix. No test asks the system for
  a position: `RequestAccessAsync` waits for the consent prompt to be
  answered, and with nobody at the keyboard it waits for as long as it
  is given, so the test that calls `current` is `#[ignore]`d and CI
  checks the sentences instead. Anything else that asks the system for
  a permission goes the same way.
- **A fresh profile follows the system's locale, which the shell
  reads.** `Settings::for_brand` asks `settings::system_locale`, which
  is what the shell left in `set_system_locale` at startup or, failing
  that, `LC_ALL`/`LC_MESSAGES`/`LANG`. The environment covers Linux
  and a Mac started from a terminal; Windows keeps its locale nowhere
  an environment variable can see it, so `osmena-shell`'s `locale.rs`
  asks `GetUserDefaultLocaleName`. A Mac app bundle started from the
  Dock has no locale in its environment either and would need
  `NSLocale`. So far it decides one thing, the weather tile's units,
  and only for a profile with no settings file yet.
- **Units nobody has chosen follow the country on show.**
  `WeatherSettings::units` is an `Option<Units>`: `None` means neither
  the user nor the locale has said. `WeatherSettings::units(&located)`
  resolves it from the country of the place the tile is showing (the
  named place first, then the device's own fix), through
  `Units::for_country`, and Celsius when nothing is known. `Place`
  carries that country as an ISO code, from either provider. The one
  wrinkle is ordering: when the device's location is still being
  asked for, nobody knows the country yet, so `EngineAction::
  FetchLocation` carries the *stored* `Option<Units>` and the
  fetcher — which learns the country first — resolves it with the same
  core function. A choice stands wherever the device goes; following
  does not.
- **A profile is a directory; its face is in its settings.** The
  directory name is the identity (`--profile <dir>`, `Profile::name`);
  the display name, avatar and colour live in that profile's
  `settings.toml` (`ProfileSettings`, `chrome_color`), which is what
  `Profile::siblings` reads to list the others (`Browser::profiles`,
  refreshed by the shell when a window gains focus and after a profile
  is made or deleted). The menus and `dispatch` work from that list;
  only `osmena-shell::profiles` touches the directories (make, delete,
  the `running.pid` claim). A new profile's directory is
  `directory_name_for(name)`. Sharing bookmarks is a path switch
  (`Browser::bookmarks_path`); copying extensions goes through the
  `extensions.toml` snapshot the shell writes on every
  `ExtensionsChanged` and `Engine::install_extension` by store id.
  A guest profile is an ordinary directory with an empty `guest`
  file in it (`GUEST_MARKER_NAME`): `Profile::guest` is read from
  that file by `resolve` and `sibling`, `others`/`siblings` leave
  guests out and `guests` lists them, `dispatch` refuses to edit or
  delete the current profile when it is one, the shell starts with
  `delete_on_exit` set for it (so no session is written either) and
  sweeps abandoned guests at every start (`profiles::sweep_guests`,
  a minute's grace for one whose browser is still starting). The
  directory of a profile on its way out (a guest, or the current
  profile the user deleted) goes through `profiles::delete_at_exit`:
  Chromium still holds a file after the engine is dropped, so after a
  short retry a detached helper (outside the job object, like the
  updater's) removes it once the process has exited. The
  Profiles menu (`menus::profiles_menu`, Chromium's shape, the brand
  name in its labels) and the Manage Profiles dialog
  (`Prompt::ManageProfiles`, drawn by `draw_manage_profiles_prompt`)
  issue the same commands; deletion lives in that dialog and the Edit
  dialog, not in the menu.
- **The history is the model's, written by the shell.** `apply` records
  a visit when a load completes and a title when one arrives, answering
  `EngineAction::SaveHistory`; the shell writes `history.toml` a few
  seconds after the last change (`App::about_to_wait`) and at exit.
  `History::matches` ranks suggestions; the chrome draws them under the
  address bar (`address_suggestions`) and in the launcher, with
  `step_pick` for the arrow keys. Nothing else reads or writes the file.
- **The default browser is the system's; the model holds the answer.**
  `osmena-shell::default_browser` asks the system (macOS: the `http`
  handler's bundle id against the running bundle's) at start and on
  every browser window's `Focused(true)`, and writes
  `Browser::default_browser`; `Unsupported` (not macOS, no bundle, or a
  bundle whose `Info.plist` does not claim `http`) hides everything.
  The bar is `Browser::offers_default_browser` (not the default,
  `Settings::check_default_browser`, not dismissed this run); its
  buttons and the settings window's are `Command`s, and only
  `MakeDefaultBrowser` reaches the shell (`EngineAction`), which calls
  `NSWorkspace` and reads the status again when the system answers.
  The same module installs the `GURL` and `odoc` Apple Event handlers
  (C functions of CoreServices, installed before the run loop and again
  when AppKit posts "will finish launching", since AppKit replaces an
  earlier `odoc` handler then); what they bring is `AppEvent::OpenUrls`,
  opened by `App::open_urls` as tabs, the launch's link in the start's
  home-page tab (`App::startup_tab`, within `LAUNCH_URL_WINDOW`).
  winit's application delegate implements no open-URL method, so do
  not move this into one.
- **Updates are the shell's; the model carries the status.** A brand
  that can update names its manifest URL (`Brand::updates`) and its
  build number (`Brand::build`, stamped by CI through
  `OSMENA_BUILD_NUMBER`; `None` for a local build, which never checks).
  `osmena_shell::update` fetches `latest.toml`
  (`osmena_core::update::Manifest`), compares `BuildId`s, downloads
  into `Profile::updates_dir` and verifies the hash on a thread that
  reports `AppEvent::Update(UpdateStatus)`; the app writes the status to
  `Browser::update` the way it writes `profiles`, and the chrome, the
  menus (`menus::UpdateMenu`) and the settings window draw from it.
  The user's actions are commands (`CheckForUpdates`, `InstallUpdate`,
  `DismissUpdateFailure`); `dispatch` moves the status (Checking,
  Installing) and emits `EngineAction::{CheckForUpdates, InstallUpdate}`.
  Installing is per platform in `update.rs` (the NSIS installer's `/S`
  behind a PowerShell that waits for the exit on Windows; a bundle swap
  and `open` on macOS) and ends in `Quit`. Nothing in osmena names a
  repository: the URL comes from the brand.
- **The native window is asked, never driven, by the chrome.** The strip
  records a `WindowRequest` (drag, minimize, maximize, system menu) the
  way it records a `MenuRequest`; `BrowserWindow` acts on it after the
  frame. Frameless windows handle their own resize borders
  (`routing::resize_edge`), except on macOS where the system keeps them.
- **Menus are native, the rest of the chrome is egui.** A menu is a
  `Vec<MenuEntry>` from `menus.rs`, shown by `native_menu::NativeMenu`
  after the frame is painted; the chrome only records a `MenuRequest`.
  Add entries in `menus.rs` with a test. The macOS menu bar is the same
  data (`menus::menu_bar`, one `Submenu` per menu, `System` entries for
  what the system does itself), shown by `native_menu::MenuBar`: the
  shell rebuilds the entries in `about_to_wait` for the focused window
  and the bar changes only when they differ. Every pick, from a popup
  or the bar, lands in one queue (`install_event_handler`,
  `take_menu_ids`); the bar's reach the shell as `AppEvent::Menu`. Its
  Edit entries are `Command::Page`s with the standard chords, which
  the bar intercepts before the window sees the keys: the shell hands
  them to a focused egui text field as the key events they stand for
  (`edit_events`), to the page otherwise.
- **Key routing has one rule.** `osmena_chrome::input::route_key`, applied
  by `routing::KeyRouter`. Change it there, with a unit test.
- **A held key or button belongs to the tab that got its press.**
  `KeyRouter` and `PointerCapture` both record the tab, so a release
  after a keyboard tab switch goes where the press went, and a hold
  whose tab closed is dropped. On `Focused(false)` the window releases
  what the page holds (`release_held`): the system sends no release
  for a key held across Alt+Tab, and the real one would be dropped as
  a release nobody saw pressed. Pointer events carry the modifiers from
  the window's winit state for the same reason; an engine must not keep
  its own.
- **egui's repaint requests go through `repaint::RepaintSchedule`.** A
  due request becomes `request_redraw`, a delayed one (caret blink,
  tooltip) the event loop's `WaitUntil`, set in `App::about_to_wait`.
  Never read `Context::has_requested_repaint` to decide on a redraw: it
  counts delayed requests, and the window would redraw continuously
  while a text field has focus.
- **The page is painted two ways.** `TabHost::paint_page` hands the shell a
  `PagePaint::Blit` (the engine drew into the window's GL context; the
  shell blits from egui's paint callback) or a `PagePaint::Frame` (pixels
  with the `dirty` rectangles since the last hand-out; the shell keeps a
  `PageTexture` per tab, uploads only those rectangles, and registers it
  with egui after the UI pass that created it). Keep both paths working:
  Paladin uses the first, Nautilus the second.
- **Version pins move together.** `winit`, `egui`, `egui-winit`,
  `egui_glow`, and `glow` are pinned exactly in the workspace `Cargo.toml`
  because the engines share a GL context and window handles with them. A
  bump here is a bump in both browsers; coordinate with Paladin's Servo
  version, which dictates them.
- **The crates version together.** One version in `[workspace.package]`;
  tag releases `v<version>`; browsers pin a `rev`.
- **Format before every commit:** `cargo fmt --all`.
- **`AGENTS.md` mirrors this file.** Keep them identical.

## Definition of Done

1. `cargo test`, `cargo clippy -D warnings`, and `cargo fmt --check` are
   clean for the whole workspace.
2. A change to a trait in `osmena-engine` or to the shell's use of it was
   built against both browsers (path-patched, see README) before it is
   pushed, and their pins move in the same change set.
3. `CHANGELOG.md` has an entry under `Unreleased` for any change a browser
   would notice, and this file's stamp is updated when the architecture or
   commands change.

## Workflow

- Commit to `main` unless asked for a branch. Conventional Commits.
- The browsers' own docs (`README.md`, `CLAUDE.md`) describe what each one
  adds; the shared behaviour is documented here only.

## Status and handoff (2026-11-08)

Added the blocked sites feature (see the convention bullet on blocked
sites above, and the README's bullet of the same name): a per-profile
list in `blocked.toml` keyed by registrable domain; the guard that every
user-started navigation passes through in `dispatch`; the block page the
chrome paints over `about:blank`, with its three lines; "Block on this
profile" in a tab's menu, with its prompt; and the settings window's
"Blocked sites" section between Bookmarks and Downloads. The engine side
of the contract is in place (`EngineEvent::NavigationBlocked`,
`Engine::configure_blocked_sites`, both defaulted). The Nautilus and
Paladin changes in this change set added the engine's before-navigate
hook the same day (see the last paragraph). Without an engine hook, the
model's own behaviour is this: a link, script or redirect onto a blocked
site is caught only by the `UrlChanged` rule, which re-blocks the tab
after the request has gone out, and Back on a tab blocked in place stays
on the block page, because the default `replace_with_blank` pushes an
entry.

Verified: `cargo test` (395 tests, one ignored: chrome 110, core 229,
shell 50, xtask 6), clippy with `-D warnings`, and `cargo fmt --check`.
The tests cover the reduction through the public suffix list
(subdomains, `co.uk`, IP addresses, `localhost`, punycode, host-less
schemes); the `blocked.toml` round trip and a missing or bad file; every
`dispatch` flow (the typed address, a new tab, a search, a question, a
restored tab waking on the block page without being warmed, the
close-tab wake, the reload re-roll, the prompt's distinct sites,
blocking across windows, deferred tabs only marked, an already blocked
site changing nothing, and unblocking sending tabs back with the list
configured first); the `apply` rules (the engine's blocked navigation,
the blank page behind a block page, Back clearing it, a URL change onto
a blocked site re-blocked); the tab menu's entry and its enabled rule;
the prompt's words and answers; the block page's sizing rule and its
drawing at narrow and wide rects; a WCAG contrast test of the block
page's three lines against the painted mesh, across the swatches (floors
3:1 and 4.5:1, the worst cases 13.06 and 8.90 on light, 8.47 and 6.66 on
dark); the strip's gradient mesh, asserted byte-identical to its state
before the refactor; the settings field's parser and its notice; and
that an open prompt takes every key.

Verified by hand in a debug Nautilus built against this checkout on
Windows, by scripted input, twice: once after the block page landed and
again after the settings section and the Escape fix. The tab menu's
"Block on this profile" is enabled on example.com and disabled on
about:blank in a new window. The prompt reads "Block site" with its
words, a red Block and Cancel; Cancel and Escape add nothing. Escape
failed on the first run, because a buttons-only prompt left egui without
keyboard focus, so the shell never sent it the key; the
`takes_every_key` change fixed it and the re-run passed. Block turns the
page into the gradient with one of the three lines, the site line and
the settings hint; the address bar keeps `https://example.com/`; the
strip title is the address, with no favicon. Clicks on the page and
plain keys do nothing, and Ctrl+L focuses the address bar. Reload
re-rolls the line (twice in the run). The line fits one row at 420 px
wide. A typed `example.com/other` is blocked at once, with exactly one
`load started` (about:blank) in the log. A second tab navigated onto
example.com is blocked, and Back returns it to example.org with the page
cleared. `blocked.toml` holds `sites = ["example.com"]`. The look was
checked in the default, "wine" and "blush" swatches: the colour fades
out above the big line, and the three lines stand on the toolbar's fill.
In the settings window, the section reads "No blocked sites." when
empty; junk gives the red "Not a site."; `https://WWW.Example.COM/x`
adds `example.com` and blocks the open tab at once; a duplicate leaves
`blocked.toml`'s modification time unchanged; `news.bbc.co.uk` and
`127.0.0.1` list as `127.0.0.1`, `bbc.co.uk`, `example.com`; Remove
unblocks without a question and the tab returns to example.com on its
own; `blocked.toml` lists the other two. Ctrl+Q exited cleanly on every
run. A profile whose `blocked.toml` reads `sites = 3` logged the warning
and started normally (checked when the blocked.toml loader landed).

Not verified: macOS; a held key across the moment a block page appears;
the Block button by click (Enter was used); a resize by the window
border (`SetWindowPos` was used). Observed and left alone: a rejected
entry keeps its text in the field, as the extensions field does; the
Remove buttons sit a pixel or two below their row labels, the grid the
extensions section uses; CEF logs "Timeout of new browser info response"
after a block load, which has not been traced.

Known limitation, recorded in the README's TabHost bullet: Forward after
Back from a block page lands on the bare `about:blank`.

Both browsers adopted revision a5fc846 on 2026-11-08, each on its own
`main`. Nautilus cancels a main-frame navigation to a blocked site in
`on_before_browse` and reports `NavigationBlocked`, and swaps the tab's
entry for `about:blank` through `location.replace`. Paladin denies a
navigation in `request_navigation`, frames included since Servo does
not name the frame (its S15), and swaps the entry through
`evaluate_javascript`. Both were checked by hand on Windows with
scripted input: a page script's link to a blocked site was refused
before any request for it (Nautilus logs `blocked navigation to
https://example.com/`, Paladin the same line), the tab menu's block
replaced the page, Remove returned it, and Back went to the page
before.
Not verified in Paladin: a redirect onto a blocked site (it reaches the
model's `UrlChanged` rule, S15), an `<iframe>` onto one by hand, DevTools
on a blocked tab; macOS in either browser.

Two follow-ups are open. `apply`'s blank-page exemption compares the
exact string `about:blank`, so a URL change to `about:blank#x` on a
blocked tab clears the block; it should match the scheme and path and
ignore the fragment, with a test. A typed `view-source:https://<blocked
site>/` is exempt from the guard and from both hooks by AC1.4 and AC5.2,
so the engine fetches the blocked site to show its source; whether
`site_for` should refuse it is a design decision still open.

## Status and handoff (2026-11-06, evening)

Restored tabs are warmed: after a session restore the deferred tabs
load one after another in hidden engine tabs, half a second apart in
strip order, each let go once its page has loaded and reported its
favicon (`App::tick_warms` in osmena-shell; `WARM_SPACING` 500 ms,
`WARM_GRACE` 3 s after the load with no favicon, `WARM_LIMIT` 20 s).
The model side is `Tab::warming`, only ever set with `deferred`:
`show_tab` shows rather than creates a warming tab, `close_tab` and
`close_window` close its engine tab, `apply` counts a warming tab's
load as no visit and ends the warm on `TabClosed` or `TabCrashed`
(`command::end_warm`). Before its turn a restored tab shows the icon
on disk for its site (`App::warm_tabs`). Verified with the debug
Nautilus on a six-tab session: the five warms started 0.5 s apart,
four ended in under a second with title and favicon, example.com (no
favicon) after the 3 s grace, and the strip showed every icon; the
run wrote the five icons to the profile's `favicons/`. Paladin was
not run; its `close_tab` unregisters the webview before Servo
reports it closed, so no `TabClosed` echoes back either.

Tab titles centre by the band from a capital's top to the baseline
(`TEXT_BAND` is "H", not "Hg"): with descenders counted in, the
capitals sat a pixel or two above the favicon's middle. Measured off
a capture of the same build: capitals on rows 19–27 against a 16-px
favicon on 15–30, half a pixel below its centre, which is as close
as an 8-px cap height gets.

## Status and handoff (2026-11-06, later still)

CI had been red since "the weather tile follows this device", for two
reasons neither of which a Windows checkout shows: on macOS clippy
read the `match` in `locale.rs`'s test as a `single_match`, because
the `None` arm is empty once its `#[cfg(windows)] panic!` is compiled
out; and on Windows the job hit the 30-minute cap because
`launch::location`'s test waited on `RequestAccessAsync` for a consent
prompt no runner can answer (the log says "has been running for over
60 seconds", then nothing). Both fixed. Platform-conditional code
needs its other arm linted: the recipe that found the first is a
scratch crate holding a copy of the module with `#[cfg(windows)]` and
`#[cfg(not(windows))]` rewritten to two Cargo features, clippied with
only the non-Windows one on. It reproduces CI's error on the old file
exactly, so a clean run means something; `cfg(all())` as the stand-in
does not work, since clippy lints that as `non_minimal_cfg`.

Units nobody has chosen follow the country the tile is showing (see
Conventions), which is what covers a system with no locale to read:
macOS from the Dock, some Linux sessions. Verified on Windows by
driving the debug Nautilus against a profile whose `settings.toml`
names no units: the forecast went out as `temperature_unit=fahrenheit`
with nothing stored, the tile drew 67°F in New York, the Launch
Settings lit "Fahrenheit (°F)", and nothing was written to the file.
Not verified by hand: a device in a Celsius country (the resolution
is unit-tested both ways), and the named-place branch end to end.

## Status and handoff (2026-11-06, later)

Fahrenheit and Celsius now have a labelled row of their own in the
Launch Settings ("Temperature", the two named in words), where they
were two bare degree signs under the weather place that did not read
as a control. The default they start from was wrong on Windows:
`Units::for_locale` was fed `LC_ALL`/`LC_MESSAGES`/`LANG`, which
Windows never sets, so every fresh profile measured in Celsius
wherever it was. The shell reads the locale from the system
(`locale.rs`, `GetUserDefaultLocaleName`) and hands it to the model
before any profile loads. Verified on Windows by driving the debug
Nautilus with a fresh profile: `en-US`, and the tile opened on 67°F,
H 69 L 61; clicking "Celsius (°C)" refetched and drew 19°C, H 20
L 16, with the second Open-Meteo call in the log. Not verified: the
locale on macOS and Linux (both still read the environment; a Mac
bundle started from the Dock has none and would need `NSLocale`).

## Status and handoff (2026-11-06)

The weather tile follows this device (see Conventions), with the
Launch Settings gear redrawn as a cog and two drawing faults in the
tile fixed (the cloud's overlapping translucent circles, the dangling
unit after the high and low). Verified: `cargo test`, clippy, fmt,
and the debug Nautilus built against this checkout on Windows, driven
to open the panel and captured with `PrintWindow`: the tile found
"New York City, New York" through the system and BigDataCloud and
drew 19°C with the drizzle glyph, the high and low; the log shows
both services answering 200. The gear and the cloud were judged from
magnified crops of that capture. Not verified: the refusal path by
hand (this machine allows location, so the "Try again" and place-field
branch was never drawn), a location that ages out, macOS and Linux
(where `location::current` is a stub that says so), and the Launch
Settings view's new rows by eye. One flake seen and not a bug: an
earlier run drew "Open-Meteo is having trouble; try again later" from
a genuine 5xx; `get_text` now logs every service's URL and status at
debug, which is how that was settled.

## Status and handoff (2026-10-19)

Added 2026-11-05 (night, on the branch `worktree-tab-presentation`,
not merged): the strip on the pixel grid with icons fitted to their
pixels (`favicon.rs`), Chromium's order as tabs narrow (`tab_layout`),
the title fade, the throbber and the globe, and the active tab open
into the toolbar with egui's panel separator off (see Conventions and
the CHANGELOG). Verified: `cargo test` (the resampler's identity,
premultiplied averaging and letterboxing, the layout rules, the edge
row), clippy, fmt, and a debug Nautilus built from a copy of its
source against this checkout, on Windows at 100% scale: twelve tabs
of 70 points show crisp centred favicons, no titles and the edge line
under them; the active tab's body meets the toolbar with no line;
example.com shows the globe; a page served slowly shows the turning
arc; five tabs of 165 points show titles fading out before the close
cross. Not verified: hover (no synthetic input was sent), a
fractional display scale (only the edge-row test covers its
arithmetic), the bookmarks bar and Launch Panel icons by eye, macOS,
Paladin (nothing it implements changed). To see the GL window in a
capture, `PrintWindow` with `PW_RENDERFULLCONTENT` on the window of
the started process works; `CopyFromScreen` shows whatever is over it.

Added 2026-11-05: the Profiles menu in Chromium's shape (the "Other
<brand> Profiles" heading, "Add <brand> Profile…", "Open Guest
Profile", "Manage Profiles…"; the Delete submenu gone), guest profiles
and the Manage Profiles dialog (see Conventions). Verified: `cargo
test` (the menu's three shapes, the guest marker and listing, the
dispatch of the new commands and the guest guards, `create_guest`
and the sweep with aged markers), clippy, fmt, and scripted runs of
Nautilus on Windows: a staged guest directory started with
`--profile guest --exit-after 10` shows "Nautilus – Guest", and the
directory is gone a moment after the exit (the first try showed the
in-process delete failing on a file Chromium holds until the process
ends; the detached helper now finishes it). Not verified: macOS and
its menu bar, the Unix helper, a real "Delete profile" of the current
profile through the same helper, and the menu and the Manage Profiles
dialog by eye unless the commit says otherwise. Later the same day the
"☰" button moved from the strip to the toolbar's right end, right of
the avatar (`menu_button`, strokes like the window controls).
Added 2026-11-03, from Nautilus's macOS reports (see its handoff for
the causes): the default browser (see Conventions), the modifier
keys' own flag (`keyutils::with_own_modifier`), the Dock left to a
bundle's icon (`icon::set_dock_icon`), and osmena-xtask's
`logo-macos.svg`. Verified: `cargo test` (the offer's commands, the
setting, the URL filter, the modifier flags, the macOS icon source),
clippy, fmt, and Nautilus's bundle on Apple silicon built against this
checkout: the status reads "No" with Comet the default, the Dock keeps
the bundle's icon, `open -a` with a URL loads it in place of the home
page at launch and as a new tab while running, an `.html` file opens.
Not verified: "Make default" and the system's question (it would
change the user's default browser), the bar and the settings section by
eye, Paladin (not built against this revision; nothing it implements
changed, but its pin moves with Nautilus's), the Windows build (no
Windows target here; the non-macOS branches are stubs, CI will tell).

Added 2026-10-31: the Launch Panel (see Conventions): the wide modal,
the field's dropdown over the tiles, the five tiles, the edit mode
with drag and remove, the Launch Settings, the Open-Meteo and Google
fetchers, `Brand::google_client_id`. Verified: `cargo test` (the
widget order and settings round trip, `top_sites`, the fetch policy,
the commands, the grid and drop index, the Open-Meteo and Drive
parsers on captured JSON, the PKCE vectors, the redirect parse, the
account file), clippy, fmt, and scripted runs of Nautilus on Windows
(the panel with the field focused, the matches dropping over the
tiles, Most visited with favicons, a place lookup, the settings view).
Not verified: the Connect click opening the consent tab (a Windows
Firewall prompt raised by a page load sat over the test window; the
listener's I/O has a loopback unit test), a real Google sign-in (no
client id was registered when this shipped; `docs/LaunchPanelOps.md`
in the parent workspace says how), the Drive list against a live
account, Paladin by hand (type-checked), macOS (the aarch64 clippy
pass is clean).
Known: the panel's scroll box is sized from the screen, so a very
short window scrolls the tiles rather than shrinking them.

Added 2026-10-30 (fifth): session restore (see Conventions). Verified:
`cargo test` (the file's round trip, the rebuild with deferred tabs
and folders, showing and closing deferred tabs, the crash offer),
clippy, fmt, and a scripted run of Nautilus on Windows: two tabs and a
moved window come back at the same place on the next start with the
second tab faint until clicked, when it loads; a killed run leaves the
offer bar on the next start. Not verified: the update restart's
`--restore` end to end, folders in a restored strip by eye, macOS, a
window on a monitor that is gone.

Added 2026-10-30 (fourth): downloads (see Conventions). Verified:
`cargo test` (the list's upserts, trimming and TOML, the commands, the
quit guard), clippy, fmt, and a scripted run of Nautilus on Windows: a
release asset served as an attachment lands in the configured
directory under its name, a second copy as "latest (2).toml", the
toolbar button appears, Ctrl+J opens the popover with both rows and
their sizes, `downloads.toml` records both as complete. Not verified:
pause, resume and cancel on a long download, the quit prompt, Open and
Show in folder (they spawn the system's programs), the folder dialog,
macOS.

Added 2026-10-30 (later still): page zoom (see Conventions). Verified:
`cargo test` (the steps, the site table, the fan-out over a host, the
reset, the default), clippy, fmt, and a scripted run of Nautilus on
Windows: Ctrl+= twice shows the 125% chip and a larger page, Ctrl and
the wheel step in and out, the chip's popover offers −, + and Reset,
a new tab on the same site opens at the site's level, Ctrl+0 resets
both tabs and forgets the site in `zoom.toml`. Not verified: Paladin's
`set_page_zoom` in a running Paladin, the settings window's "Page zoom"
by hand, macOS.

Added 2026-10-30 (later): find in page (see Conventions). Verified:
`cargo test` (the commands, the tab switch, the result folding, the
menus), clippy, fmt, and a scripted run of Nautilus on Windows: Ctrl+F
opens the bar, typing highlights the matches with the count, Enter and
Shift+Enter step with the active match turning orange, a miss reads
"No matches", Escape closes. Not verified: F3 while the field has the
keyboard, the macOS Edit menu entries, Paladin (whose engine reports
`find: false`, so the entries are disabled).

Added 2026-10-30: bookmarks (see Conventions): the star in the toolbar
and its popover, the bookmarks bar with favicon chips, overflow and
group drop-downs, the Bookmarks menu with import and export of HTML
bookmarks files and per-group rename and delete, bookmarks among the
address-bar and launcher suggestions, the favicon cache. Verified:
`cargo test` (the bookmark operations, the HTML round-trip with a
Chrome export, the merged ranking, the commands and prompts, the
menus, the cache), clippy, fmt. Not verified by hand yet: the popover's
placement under the star and under a chip, chip dragging, the "»"
overflow, the file dialogs on either platform, the bar on macOS.

Added 2026-10-29 (later): updates over the air (see Conventions). The
browsers' CI publishes a GitHub release per green push to `main` with a
`latest.toml` manifest (`ci/update-manifest.sh` in each browser); the
shell fetches it with `ureq` (rustls, platform trust store), downloads
the platform's asset, verifies it with `sha2`, and the chrome offers the
restart. Verified: `cargo test` (the manifest, the build order, the
commands, the menu entry), clippy, fmt, the macOS target check
(`cargo clippy --target aarch64-apple-darwin` with a stand-in C compiler
for `ring`), and a scripted run of Nautilus on Windows against a local
server: the check 15 s after start, the download and verification, the
pill in the toolbar, the click quitting the browser, the helper running
the (fake) installer with `/S` after the exit and relaunching the same
profile; a second run reused the verified download. Not verified: the
real NSIS installer in silent mode over a running install (its template
kills every `<binary>.exe`, then installs over the previous location),
anything on macOS (the bundle swap and `open -n … --args --profile`),
the settings window's Updates section by hand, the "Release notes"
button, and the release job itself before the first push.

What exists: the five crates, extracted in one day from Paladin and Nautilus,
both of which now build on them and pin the same revision. The repository is
public at <https://github.com/thomasmost/osmena>; CI (fmt, clippy, tests on
macOS and Windows runners, no engine needed) was green on every push.

Added the same day: frameless browser windows (the tab strip as title bar,
window controls, resize borders, a setting to restore the system frame),
Chromium-style default shortcuts with the new commands behind them (reopen
closed tab, tab 1–8 and last, home, full screen), the shortcut editor
in the settings window with `keymap.toml` persistence, and the extension
UI (puzzle menu, pinned actions beside the address bar, the Extensions
section of the settings window) over an engine contract Nautilus
implements and Paladin leaves at its defaults.

Added later the same day: tab folders (chip in the strip, drop-down
list, folder menu) and bookmark groups (`bookmarks.toml`, the window
menu's Bookmarks submenu), both reached from the tab context menu
through prompts drawn by the chrome.

Added 2026-10-20: the page's context menu (`PageContext` from the
engine, `menus::page_menu`, `PageCommand` back to the engine, `PageTools`
on `EngineInfo`), background tabs beside their opener and new windows
from a page's request, the address-bar search (`Settings::search`),
clipboard writes in the shell, and egui-winit's `clipboard` feature so
the address bar pastes from the system clipboard.

Verified 2026-10-20 with Nautilus on Windows, by scripted input on a
local test page: the link, image, text-field, selection, and plain-page
menus show the expected entries and shortcuts; "Open link in new tab"
adds a tab beside the current one without activating it; "Copy link
address" then Ctrl+V in the address bar pastes the URL; "Copy image"
puts the image on the clipboard; Print opens the system dialog; "View
page source" opens a `view-source:` tab; Inspect opens the developer
tools; address-bar text that is not a URL loads a Google search; Ctrl+Q
afterwards exits cleanly. Paladin builds and its tests pass against this
revision; its page menu was not run by hand. Not verified: the media
entries (no video was tried), spelling suggestions, "Paste as plain
text", Shift-click and middle-click on a link, the Search fields in the
settings window by hand.

Verified: `cargo test` (205 tests), clippy with `-D warnings`, formatting;
both browsers build against this revision; Nautilus on Windows launches
frameless with the controls at the strip's right end, opens the settings
window on `Ctrl+,`, captures a chord in the editor (New tab rebound to
`Ctrl+Y`), writes it to `keymap.toml`, and the new chord opens a tab;
with an extension installed and pinned, its icon and the puzzle button
appear in the toolbar and clicking the icon opens the popup under it;
the tab context menu shows the folder and bookmark entries, the folder
prompt prefills "Folder A", the chip appears collapsed with its count
and expanded with its tab after it, its drop-down lists the tab, its
menu opens, the bookmark prompt writes `bookmarks.toml`, and the window
menu has the Bookmarks submenu. Not verified: picking an existing folder
or group in a prompt, Rename, Ungroup, "Open all", the puzzle menu and
the settings window's Extensions section by hand, dragging and
resizing a frameless window by hand, Aero snap, full screen, the macOS
title bar style (nothing has run on macOS since the extraction), and the
browsers' manual checklists beyond launch.

Decisions, with reasons:
- Separate repositories, not a monorepo: Paladin's Servo build (60 git crates,
  a patch table, Servo's toolchain pin) must not be a cost of building Nautilus.
- Four crates rather than three tiers: `osmena-engine` holds the traits so an
  engine crate implements them without depending on egui or winit.
- Branding by injection: `Brand` and `EngineInfo` statics owned by the binary
  and the engine crate, passed by reference, no globals.
- Two page-paint paths, `PagePaint::Blit` (Servo) and `PagePaint::Frame` (CEF),
  both in one `BrowserWindow`; the frame path registers its texture with egui
  after the UI pass that created it (the painter is not reachable inside it).
- Licence MIT OR Apache-2.0 with "the osmena contributors" as the holder; the
  owner has not chosen a name. Nothing is published to crates.io; the names
  are free and the manifests are ready.

Added 2026-10-22 (later): the browsing history (`osmena-core::history`,
`history.toml`, 5000 entries, least recently visited evicted) recorded
in `apply`; the new-tab launcher in place of a blank page (`Ctrl+T` and
"+" open it; Enter, Up/Down, Escape, click outside); history matches
under the address bar. Verified: `cargo test` (the ranking, the arrow
pick, recording and persistence), clippy, formatting, and a scripted
run in Nautilus (see its handoff). Not verified by hand on macOS. Open:
the launcher has no bookmarks or open-tab matches yet, only history;
the history has no UI to browse or clear it.

Added 2026-10-22, from a review of click capture and the shell/page
boundary: pointer events carry the keyboard modifiers (the fix for a
link click turning into a download after Alt+Tab, when Nautilus kept the
Alt of the last key event), the pointer capture follows the tab that
got the press, held keys and buttons are released in the page on focus
loss, the folder drop-down closes on a page click and prompts are modal
(`ChromeState::popup_open`), the back and forward mouse buttons
navigate, egui's delayed repaints are waited for instead of redrawn at
once, `FrameRef::dirty` with partial texture uploads, and page focus
gated on the window's. Verified: `cargo test`, clippy, formatting, and
both browsers built against the revision. Not verified by hand: any of
it in a running browser; the Alt+Tab download path is the one to try
first (hold Alt, switch away, release, click a link).

Added 2026-10-26: profiles as the user sees them. The toolbar's avatar
button and the menu bar's Profiles menu share `menus::profiles_menu`;
the profile dialog (`chrome::draw_profile_prompt`) and the delete
confirmation are prompts like the folder and bookmark ones;
`osmena-shell::profiles` makes and deletes directories and keeps the
`running.pid` claim. Verified: `cargo test` (the slug, the siblings
listing, the dialog's prefill and answers, the menu), clippy, fmt, the
macOS type-check, and a scripted run in Nautilus on Windows (see its
handoff). Not verified by hand: the macOS menu bar's Profiles menu
(never run on a Mac), the delete-current-profile handover on macOS
(the directory is removed after `cef::shutdown`, which on macOS has not
been watched), and copying extensions from a profile whose snapshot
lists an unpacked extension (the store install fails and the engine
reports it). Known: two browsers sharing the default profile's
bookmarks each keep their own copy in memory and reload it on focus and
before adding; a change made in both within the same moment loses one.

Added 2026-10-28: the profile dialog's avatars come in two palettes,
`Avatar::PALETTES` ("Sea creatures", ten glyphs, then "Assorted", the
first round's 24), drawn as one labelled six-column grid each under the
initial; five sea glyphs were added (`SpoutingWhale`, `Dolphin`, `Fish`,
`TropicalFish`, `Blowfish`), chosen from what egui's NotoEmoji fallback
font covers (crab, shark, squid, jellyfish, seal, and the other newer
sea emoji are not in it; check the font's cmap before adding a glyph).
Verified: `cargo test`, clippy, fmt, and a scripted run in Nautilus on
Windows (the dialog with both palettes, a pick). Not verified: macOS.

Added 2026-10-29: the avatars come in three tabs (`Avatar::PALETTES`:
"Sea creatures", 22, with shark, seal, otter, squid, jellyfish, crab,
lobster, shrimp, oyster, coral, crocodile and merperson new; "Emoji",
the 24 most used in Unicode's frequency ranking; "Assorted"), each
tab as tall as the longest so the buttons stay put, opening on the
current avatar's tab. They are drawn from their own font,
`assets/fonts/NotoEmoji-Avatars.ttf` (46 KB: Google Fonts' current
`NotoEmoji[wght]` at weight 400, subset to the 65 avatar glyphs with
fontTools), because egui's NotoEmoji is old enough to lack most of
them. The chrome colours are seven hues of three shades
(`ChromeColor::FAMILIES`, 21 swatches, the eight old names unchanged),
drawn by `chrome::swatch_grid` in both the settings window and the
dialog. Verified: `cargo test`, clippy, fmt; Nautilus builds against
it.

Known and left alone: a native menu (`native_menu`, `TrackPopupMenu` on
Windows) blocks inside the event loop, so pages freeze under any
context menu and the engine's wake requests queue up until it closes.
Acceptable for a menu; do not add anything long-running there.

Open items, most useful first:
1. Run both browsers' manual checklists against the current revision, with
   the frameless window first: move, resize from every edge, maximize by
   double-click and by the control, snap, full screen and back; then
   Nautilus's extensions checklist item.
2. An integration test of `osmena-shell` with a fake `Engine` (no window
   system is needed for the model and routing, but the window code itself has
   no test).
3. IME for page fields (E1 in the browsers' `docs/upstream-issues.md`).
4. macOS: first built and run there on 2026-10-20 (with Nautilus); the
   native menus' position was fixed from that run but not yet re-checked
   by hand, nor has the rest of the chrome on macOS.

All three checkouts live side by side under `vibes/browser-project/`
(`osmena`, `paladin`, `nautilus`); Paladin's Servo and stylo fork checkouts
stay at `~/vibes/servo` and `~/vibes/stylo`. `~/.cargo/config.toml` on the
development Windows box carries a `[patch."https://github.com/thomasmost/osmena"]`
table pointing at the osmena checkout, so both browsers build against
uncommitted osmena changes; while it is active `cargo` rewrites the
browsers' `Cargo.lock` with path sources for the osmena crates. Before
committing a browser's lock, disable the table (rename its key), run
`cargo metadata` to put the git sources back, commit, and re-enable it.
Cargo fetches the pinned revision from GitHub even when the table is
active, so GitHub must be reachable.
