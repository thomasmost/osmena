# osmena

The shared shell of Paladin (Servo) and Nautilus (Chromium): the browser
model, the egui chrome, and the winit application, generic over an engine
trait. The browsers are separate repositories that depend on these crates by
git revision. `README.md` has the crate table and the engine contract.

_Last updated: 2026-10-19 (tab folders, bookmark groups, prompts)._

## Architecture

Cargo workspace (Rust 1.97.1 in `rust-toolchain.toml`, edition 2024), five
crates, dependencies pointing only downwards:

`osmena-core` (model, pure) ← `osmena-engine` (the `Engine`/`TabHost`/
`Surface` traits) and `osmena-chrome` (egui chrome, menus, routing) ←
`osmena-shell` (winit app, windows, `run`). `tools/osmena-xtask` is a
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
- **Branding is injected, never embedded.** A product string, path, or
  icon comes from the `Brand` the binary passes in; an engine fact (name,
  version, data directory, what it supports, wheel distance) comes from
  `EngineInfo`. Neither is a global: functions that need one take a
  reference. Grep for "Paladin", "Nautilus", "Servo", "Chromium" before
  committing; only the README and this file may name them.
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
- **Folders are contiguous runs of tabs.** `Tab::folder` names the
  folder; `Window::gather_folder` keeps a folder's tabs together where
  its first tab is, `prune_folders` drops a folder whose last tab went
  (every tab removal calls it), and `strip_items` is the strip as drawn.
  New tabs go to the end, outside any folder.
- **The native window is asked, never driven, by the chrome.** The strip
  records a `WindowRequest` (drag, minimize, maximize, system menu) the
  way it records a `MenuRequest`; `BrowserWindow` acts on it after the
  frame. Frameless windows handle their own resize borders
  (`routing::resize_edge`), except on macOS where the system keeps them.
- **Menus are native, the rest of the chrome is egui.** A menu is a
  `Vec<MenuEntry>` from `menus.rs`, shown by `native_menu::NativeMenu`
  after the frame is painted; the chrome only records a `MenuRequest`.
  Add entries in `menus.rs` with a test.
- **Key routing has one rule.** `osmena_chrome::input::route_key`, applied
  by `routing::KeyRouter`. Change it there, with a unit test.
- **The page is painted two ways.** `TabHost::paint_page` hands the shell a
  `PagePaint::Blit` (the engine drew into the window's GL context; the
  shell blits from egui's paint callback) or a `PagePaint::Frame` (pixels;
  the shell keeps a `PageTexture` per tab and registers it with egui after
  the UI pass that created it). Keep both paths working: Paladin uses the
  first, Nautilus the second.
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

Verified: `cargo test` (186 tests), clippy with `-D warnings`, formatting;
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

Open items, most useful first:
1. Run both browsers' manual checklists against the current revision, with
   the frameless window first: move, resize from every edge, maximize by
   double-click and by the control, snap, full screen and back; then
   Nautilus's extensions checklist item.
2. An integration test of `osmena-shell` with a fake `Engine` (no window
   system is needed for the model and routing, but the window code itself has
   no test).
3. IME for page fields (E1 in the browsers' `docs/upstream-issues.md`).
4. macOS: nothing has been built there since the extraction.

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
