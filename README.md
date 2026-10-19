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
| `osmena-chrome` | chrome | The egui tab strip, toolbar, status overlay and crash panel; the window and tab menus as data and as native popups; pointer and key routing; the icon and the fatal-error dialog. | osmena-core, egui, winit, muda |
| `osmena-shell` | shell | The winit `App`, `BrowserWindow`, `SettingsWindow`, the process glue, and `run`. Generic over `Engine`. | osmena-chrome, osmena-engine, egui_glow |
| `osmena-xtask` | tool | `icons`: renders a browser's `assets/logo.svg` into its icon set. | resvg |

An engine crate depends on `osmena-core` and `osmena-engine` only; it never
sees egui or winit. A browser's binary depends on `osmena-shell` and its
engine crate.

## How a browser uses it

```rust
use osmena_core::{Brand, EngineInfo, ExtensionSupport};

static BRAND: Brand = Brand {
    name: "Nautilus",
    binary: "nautilus",
    data_dir: "Nautilus",
    homepage: "https://www.wikipedia.org/",
    icon_png: include_bytes!("../../../assets/icons/nautilus-256.png"),
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
  resize, input, `paint_page`, which hands the shell either a blit
  function (the engine painted into GL) or a frame of BGRA pixels (the
  engine paints into memory; the shell draws either under the toolbar),
  and `open_extension_action` and `close_extension_popups` (defaulted to
  nothing).
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
- **Bookmark groups.** "Bookmark all selected…" asks for a group name
  (or an existing group) and saves the pages to `bookmarks.toml` in the
  profile; the window menu's "Bookmarks" submenu opens a group's pages
  one at a time or all at once.
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
- **Settings** (`settings.toml`): homepage, new-tab URL, a chrome colour
  per profile, the title bar switch, the pinned extensions, and the
  restore flag.

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
