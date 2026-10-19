# Changelog

All notable changes to the osmena crates. The crates version together.

## Unreleased

### Added

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
