# osmena

The shared shell of Paladin (Servo) and Nautilus (Chromium): the browser
model, the egui chrome, and the winit application, generic over an engine
trait. The browsers are separate repositories that depend on these crates by
git revision. `README.md` has the crate table and the engine contract.

_Last updated: 2026-10-19 (project start)._

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
  direct engine call.
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
