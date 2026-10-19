# Changelog

All notable changes to the osmena crates. The crates version together.

## Unreleased

### Added

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
