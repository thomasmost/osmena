# osmena

The shared shell of Paladin (Servo) and Nautilus (Chromium): the browser
model, the egui chrome, and the winit application, generic over an engine
trait. The browsers are separate repositories that depend on these crates by
git revision. `README.md` has the crate table and the engine contract.

_Last updated: 2026-10-26 (profiles: name, colour, avatar, the Profiles
menu behind the toolbar's avatar and in the menu bar, the profile
dialog, adding and deleting profiles, shared bookmarks and copied
extensions; before that the theme and the macOS menu bar)._

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
  picks dark or light text by the swatch's luminance.
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
  what lets egui keep the drag while the tab moves.
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
- **A new tab asks for its address first.** `Command::NewTab` without a
  URL is `EngineAction::ShowLauncher`, never a tab; the chrome's
  launcher (`draw_launcher`, `ChromeState::open_launcher`) answers with
  `NewTab { url: Some(..) }` or `SearchInNewTab`, or with nothing. The
  `new_tab_url` setting is what a new *window* opens on.
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
- **The history is the model's, written by the shell.** `apply` records
  a visit when a load completes and a title when one arrives, answering
  `EngineAction::SaveHistory`; the shell writes `history.toml` a few
  seconds after the last change (`App::about_to_wait`) and at exit.
  `History::matches` ranks suggestions; the chrome draws them under the
  address bar (`address_suggestions`) and in the launcher, with
  `step_pick` for the arrow keys. Nothing else reads or writes the file.
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

## Status and handoff (2026-10-19)

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
