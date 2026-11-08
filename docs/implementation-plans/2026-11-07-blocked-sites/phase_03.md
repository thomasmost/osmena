# Blocked Sites Implementation Plan

**Goal:** The shell loads the profile's blocked sites at start, tells the engine, saves on change, and the engine contract is documented; both browsers still build against the revision.

**Architecture:** `osmena-shell`'s `run` loads `blocked.toml` beside the zoom table (a bad file logs a warning and starts empty); `App::resumed` hands the list to the engine right after `configure_downloads`; `perform` already saves and forwards (Phase 2, Task 7). The README's engine-contract bullets and the CHANGELOG name the change. The osmena Definition of Done requires a trait change to be built against both browsers before it is pushed; that build is this phase's last task.

**Tech Stack:** Rust 1.97.1, osmena-shell, `log`.

**Scope:** 7 phases from the design `docs/design-plans/2026-11-07-blocked-sites.md`. This file is Phase 3 of 7. Depends on Phase 2.

**Codebase verified:** 2026-11-07 (osmena `1bdea01` on branch `blocked-sites`; nautilus `1df5bc8`; paladin `cc9a89f`; `~/.cargo/config.toml`'s `[patch."https://github.com/thomasmost/osmena"]` table is active, pointing both browsers at this checkout).

---

## Acceptance Criteria Coverage

This phase implements and tests:

### blocked-sites.AC1: The list and the matching
- **blocked-sites.AC1.5 Success:** `blocked.toml` round-trips; a missing file is an empty list; an unreadable file logs a warning, starts empty, and the browser still starts.

(The round trip and the missing file were tested in Phase 1; this phase adds the shell's warning-and-start behaviour, verified operationally as the design's Phase 3 asks. No other ACs: the design says "Operational verification; no new ACs of its own".)

---

## Context for the engineer

All paths are under `C:\Users\tomis\vibes\browser-project\osmena\` unless a browser is named. Branch `blocked-sites`. The gate and the commit conventions are in `phase_01.md`.

**Facts about the code as it stands:**
- `crates/osmena-shell/src/lib.rs:40-43` imports `osmena_core::{Bookmarks, Brand, Browser, Command, Downloads, EngineInfo, History, Keymap, Platform, Profile, Session, Settings, SiteZoom, dispatch}`. `run` builds `Browser::new(profile, settings)` at `:161`, loads history (`:178-187`, `log::warn!` and default on error), zoom (`:188-198`, the same shape), downloads (`:199-209`), then assigns `browser.bookmarks`, `browser.history`, `browser.zoom`, `browser.downloads` at `:210-213`. `profile` is re-bound as `&browser.profile` at `:174`.
- `crates/osmena-shell/src/app.rs:1704-1709` (`ApplicationHandler::resumed`):
  ```rust
              match start_engine(&self.browser.profile.engine_dir, waker) {
                  Ok(engine) => {
                      if self.engine_info.downloads {
                          engine.configure_downloads(self.download_policy());
                      }
                      self.engine = Some(engine);
                  }
  ```
  The `SaveBlockedSites`, `ConfigureBlockedSites` and `ReplaceWithBlank` arms of `perform` exist since Phase 2, Task 7.
- `README.md:88-110` is the engine-contract list: the `Engine` bullet (`:90-94`) ends "`install_extension`, `set_extension_enabled`, and `uninstall_extension` (defaulted to nothing)."; the `TabHost` bullet (`:95-109`) ends "`EngineInfo::page_tools` says which of those the engine offers."
- `CHANGELOG.md:5-9`: `## Unreleased` then `### Added` whose first bullet begins "- Restored tabs get their titles and favicons without being opened." New bullets go at the top of the list.
- Nautilus (`C:\Users\tomis\vibes\browser-project\nautilus`) builds with `cargo build --workspace --all-targets` (needs `CEF_PATH`, CMake and Ninja on PATH, as its `CLAUDE.md:49-50` says). Paladin (`C:\Users\tomis\vibes\browser-project\paladin`) has a `target/` from earlier builds; `cargo check --workspace` there type-checks the engine against osmena without linking Servo's binaries (its CLAUDE.md warns a cold Servo build takes most of an hour; the check reuses the cache).

---

<!-- START_TASK_1 -->
### Task 1: Load `blocked.toml` at start

**Verifies:** blocked-sites.AC1.5 (the shell's half, operationally)

**Files:**
- Modify: `crates/osmena-shell/src/lib.rs:40-43` (imports) and `:199-213` (the loading block)

**Step 1: Import the type**

Insert `BlockedSites` as the first name in the `osmena_core::{...}` import list at `:40-43` (it sorts before `Bookmarks`).

**Step 2: Load beside the zoom table**

After the downloads block (which ends at `:209` with `};`) and before `browser.bookmarks = bookmarks;` insert:

```rust
    // And the blocked sites: a bad file is logged and the profile starts
    // with none, like the zoom levels.
    let blocked = match BlockedSites::load(&profile.blocked_path()) {
        Ok(b) => b,
        Err(e) => {
            log::warn!(
                "{e} (in {}); starting with no blocked sites",
                profile.blocked_path().display()
            );
            BlockedSites::default()
        }
    };
```

and after `browser.downloads = downloads;` add `browser.blocked = blocked;`.

**Step 3: Verify operationally**

Run: `cargo build --workspace --features osmena-engine/glutin`
Expected: builds.

The warning path is verified with a browser in Task 4 (a `blocked.toml` containing `sites = 3` in a test profile: the log shows "the blocked sites file is not valid … starting with no blocked sites" and the window opens).

**Step 4: Commit** (with Task 2).
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: Configure the engine at start

**Verifies:** None (operational: the design's "the shell configures the engine at start and after `BlockSites`/`UnblockSite`"; the "after" half is the `ConfigureBlockedSites` arm from Phase 2).

**Files:**
- Modify: `crates/osmena-shell/src/app.rs:1704-1709`

**Step 1: Hand the list to the engine**

Change the `Ok(engine)` arm of `resumed` to:

```rust
                Ok(engine) => {
                    if self.engine_info.downloads {
                        engine.configure_downloads(self.download_policy());
                    }
                    // Every engine gets the list; one without a hook
                    // ignores it (the method is defaulted) and still
                    // blocks what the user types through the model.
                    engine.configure_blocked_sites(self.browser.blocked.clone());
                    self.engine = Some(engine);
                }
```

**Step 2: Verify**

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

Expected: clean and green.

**Step 3: Commit**

```bash
cargo fmt --all
git add crates/osmena-shell/src/lib.rs crates/osmena-shell/src/app.rs
git commit -m "feat: the shell loads the blocked sites with the profile and hands them to the engine" -m "blocked.toml is read in run beside zoom.toml (a bad file is logged and the profile starts with none) and the engine learns the list as soon as it is up, as it learns the download policy." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: The README's engine contract and the CHANGELOG entry

**Verifies:** None (documentation, required by the osmena Definition of Done item 3).

**Files:**
- Modify: `README.md:90-94` (the `Engine` bullet) and `:95-109` (the `TabHost` bullet)
- Modify: `CHANGELOG.md:7-9` (top of `### Added` under `## Unreleased`)

**Step 1: README**

In the `Engine` bullet, change the ending

```
  for engines with extensions, `install_extension`,
  `set_extension_enabled`, and `uninstall_extension` (defaulted to
  nothing).
```

to

```
  for engines with extensions, `install_extension`,
  `set_extension_enabled`, and `uninstall_extension` (defaulted to
  nothing); `configure_downloads` and `configure_blocked_sites`, the
  policies the shell hands down at start and on every change (both
  defaulted: an engine's before-navigate hook is to cancel a main-frame
  navigation to a blocked site before any request and report
  `EngineEvent::NavigationBlocked`; the model blocks what the user
  types whether or not the engine has a hook).
```

In the `TabHost` bullet, after the sentence ending "`EngineInfo::page_tools` says which of those the engine offers." add:

```
  `replace_with_blank` replaces the tab's current history entry with
  `about:blank` when the page it shows has just been blocked, so Back
  skips it (defaulted to a plain navigation, which adds an entry
  instead).
```

Also add a known-limitations bullet, after the `TabHost` text above and before Step 2, for the README's blocked-sites notes:

```
- Known limitation: Forward after Back from a block page lands on the
  bare `about:blank` the block page sat over; the model keeps no
  per-entry state, so the page cannot be restored. Accepted.
```

**Step 2: CHANGELOG**

At the top of the `### Added` list under `## Unreleased` insert:

```
- Blocked sites. A profile keeps a list of websites the browser refuses
  to show, keyed by registrable domain so `youtube.com` covers
  `www.youtube.com` and `m.youtube.com` (the public suffix list keeps
  `bbc.co.uk` a site and `co.uk` not). A tab's context menu offers
  "Block on this profile" behind a confirmation; the settings window
  gains a "Blocked sites" section to add, view and remove them, the
  only place a site is unblocked. Every navigation the user starts is
  checked in the model (a typed address, a new tab, a search, a
  restored tab) and every other in the engine's before-navigate hook
  (a link, a script, a redirect); a blocked destination is never
  requested. The tab shows a block page the chrome paints over
  `about:blank`, one of three lines on the profile's colour, with the
  attempted address in the address bar; Back returns to the page
  before, Reload re-rolls the line. Blocking replaces the history entry
  of every open tab on the site; unblocking sends them back to their
  pages. The list is `blocked.toml` in the profile
  (`osmena_core::blocked`, `Tab::blocked`, `Command::{PromptBlockSites,
  BlockSites, UnblockSite}`, `EngineEvent::NavigationBlocked`,
  `Engine::configure_blocked_sites`, `TabHost::replace_with_blank`,
  both defaulted).
```

**Step 3: Commit**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: blocked sites in the engine contract and the changelog" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: Build against both browsers, and watch the warning path

**Verifies:** blocked-sites.AC1.5 (the warning, by observation); the osmena Definition of Done item 2 (a trait change built against both browsers before it is pushed).

**Files:** none changed in osmena. Nothing is committed in the browsers in this task (their pins move in Phases 6 and 7); the browsers' `Cargo.lock` files get rewritten with path sources while the patch table is active, so restore them afterwards with `git checkout -- Cargo.lock` in each browser.

**Step 1: Nautilus builds against the checkout**

In PowerShell, with `CEF_PATH` set as Nautilus's `CLAUDE.md:49-50` describes:

```
cd C:\Users\tomis\vibes\browser-project\nautilus
cargo build --workspace --all-targets
```

Expected: builds; the defaulted methods mean `nautilus-engine` needs no change yet.

**Step 2: The warning path, with Nautilus**

`--profile <name>` names a directory under the OS config directory, `Brand::data_dir`, `profiles\<name>` (`Profile::resolve_under`, `crates/osmena-core/src/profile.rs:519-531`), so let the browser create it and say where. In PowerShell:

```
cargo run -- --profile osmena-blocked-probe --exit-after 3 about:blank 2>&1 | Select-String "profile"
```

The line `profile "osmena-blocked-probe" at <dir>` names the directory. Then put a bad file there and start again for a few seconds:

```
$dir = "<that directory>"
Set-Content -Encoding utf8 "$dir\blocked.toml" "sites = 3"
$env:RUST_LOG = "warn"
cargo run -- --profile osmena-blocked-probe --exit-after 8 https://example.com/
Remove-Item Env:RUST_LOG
```

Expected: stderr shows a line containing `the blocked sites file is not valid` and `starting with no blocked sites`; the window opens and the page loads; the process exits 0 after the timeout. Remove the directory afterwards (`Remove-Item -Recurse -Force $dir`).

**Step 3: Paladin type-checks against the checkout**

```
cd C:\Users\tomis\vibes\browser-project\paladin
cargo check --workspace
```

Expected: finishes without errors (slow the first time it rebuilds Servo's crates for the check profile; this is the price the DoD sets for a trait change).

**Step 4: Put the lock files back**

```
cd C:\Users\tomis\vibes\browser-project\nautilus; git checkout -- Cargo.lock
cd C:\Users\tomis\vibes\browser-project\paladin; git checkout -- Cargo.lock
```

Expected: `git status --short` is empty in both browsers.

No commit: this task changes nothing.
<!-- END_TASK_4 -->

---

## Phase 3 done when

- The gate is clean and green in osmena.
- Nautilus builds and Paladin type-checks against the checkout with the patch table active; neither engine crate needed a change (the defaults compile).
- A profile with a bad `blocked.toml` logs the warning and starts.
- README (engine contract) and CHANGELOG carry the change.
