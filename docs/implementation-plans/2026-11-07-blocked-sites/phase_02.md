# Blocked Sites Implementation Plan

**Goal:** Every way a tab becomes blocked or unblocked, in the pure model: the commands, the prompt, the actions, the guard on every navigation the user starts, and the events the engine reports.

**Architecture:** `dispatch` in `osmena-core` gains `PromptBlockSites`, `BlockSites` and `UnblockSite`, a guard on every path that puts a tab at a URL (`Navigate`, `new_tab`, `new_background_tab`, `open_window`, `show_tab`, `warm_tab`, `restore_session`), and a `Reload` that re-rolls the line; `apply` gains `NavigationBlocked` and three `UrlChanged` rules. The engine's side is expressed as `EngineAction::{ConfigureBlockedSites, ReplaceWithBlank, SaveBlockedSites}`. So that the workspace builds at the end of the phase, the two defaulted engine-trait methods, the shell's `perform` arms and the chrome's exhaustive `Prompt` matches are closed here too; Phase 3 wires loading, saving and configuring at start, Phase 4 draws the prompt and the page.

**Tech Stack:** Rust 1.97.1 (edition 2024, let-chains available), `url` 2.5, osmena-core/engine/shell/chrome.

**Scope:** 7 phases from the design `docs/design-plans/2026-11-07-blocked-sites.md`. This file is Phase 2 of 7. Depends on Phase 1.

**Codebase verified:** 2026-11-07 (osmena `1bdea01` on branch `blocked-sites`).

---

## Acceptance Criteria Coverage

This phase implements and tests:

### blocked-sites.AC2: Blocking from the tab menu
- **blocked-sites.AC2.2 Success:** The prompt lists the targets' distinct sites, skipping already-blocked ones; with nothing to ask, no prompt is shown.
- **blocked-sites.AC2.3 Failure:** Cancel or Escape adds nothing.
- **blocked-sites.AC2.4 Success:** Confirming adds the sites and saves the file.
- **blocked-sites.AC2.5 Success:** Every live tab on the site, in every window of the profile, shows the block page, with its history entry replaced so Back returns to the previous page.
- **blocked-sites.AC2.6 Edge:** A deferred tab on the site is marked and shows the block page when activated.

### blocked-sites.AC3: Navigation and the block page
- **blocked-sites.AC3.1 Success:** A typed address on a blocked site shows the block page with no request sent; the previous page is behind it in history.
- **blocked-sites.AC3.2 Success:** A new tab, a Launch Panel answer or a search landing on a blocked site shows the block page.
- **blocked-sites.AC3.3 Success:** A session-restored tab on a blocked site shows the block page when shown.
- **blocked-sites.AC3.5 Edge:** A redirect onto a blocked site is cancelled by the engine, or re-blocked by the model if the engine let it through. (The model's half; the engines' halves are Phases 6 and 7.)
- **blocked-sites.AC3.6 Success:** Back from a block page returns to the previous page and the block page clears.
- **blocked-sites.AC3.7 Success:** Reload on a block page re-rolls the line and sends the engine nothing.

### blocked-sites.AC4: Settings
- **blocked-sites.AC4.4 Success:** Unblocking brings every tab blocked on that site back to its own URL.
- **blocked-sites.AC4.5 Success:** Adding from settings blocks open tabs on that site at once.
- **blocked-sites.AC4.6 Edge:** Adding a site already in the list changes nothing and saves nothing.

(AC2.3 is the chrome's Cancel in Phase 4; here it is the model's half: no `BlockSites` command, no change. AC2.4's "saves the file" is `EngineAction::SaveBlockedSites`, performed by the shell in Phase 3.)

---

## Context for the engineer

All paths are under `C:\Users\tomis\vibes\browser-project\osmena\`. Branch `blocked-sites`. Verification and commit conventions are in `phase_01.md`'s "Context for the engineer"; the gate is

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

**How `dispatch` and `apply` are tested** (`crates/osmena-core/src/command.rs:2484-2497`, `event.rs:405-416`): the test modules import `use crate::model::test_support::{browser, url};` and define

```rust
    /// A browser with one window holding one tab. Returns the ids.
    fn one_window() -> (Browser, WindowId, TabId) {
        let mut b = browser();
        let actions = open_window(&mut b, url("https://a.example/"));
        let EngineAction::OpenWindow { window, tab, .. } = actions[0].clone() else {
            panic!("open_window returns OpenWindow");
        };
        (b, window, tab)
    }
```

More tabs come from `dispatch(&mut b, &Command::NewTab { window, url: Some(url("https://b.example/")) })`, read back as `b.window(window).unwrap().active`. Results are compared whole with `assert_eq!(actions, vec![...])` when the order is fixed, or with `assert!(matches!(actions.as_slice(), [...]))`. Model state is then read directly (`b.tab(tab).unwrap().blocked`). The zoom test `zoom_steps_spread_over_the_host_and_are_remembered` (`command.rs:4338-4415`) is the model for a per-site fan-out test. `dispatch(browser: &mut Browser, cmd: &Command) -> Vec<EngineAction>` and `apply(browser: &mut Browser, event: &EngineEvent) -> Vec<EngineAction>` take no clock; a seed is taken inside with `crate::blocked::line_seed()` (as `launch_fetches` calls `unix_now()`). Tests never assert which line was picked, only that `blocked` is `Some` with the right `site`.

**Facts about the code as it stands:**
- `Command` enum `command.rs:30-395`; `Prompt` `:482-548`; `EngineAction` `:556-781`; `dispatch` `:784-1587` with the `Navigate` arm at `:1338-1349`, `Reload` at `:1352-1359`, `Search`/`SearchInNewTab`/`AskInNewTab` at `:1475-1495`.
- Helpers: `restore_session` `:1589-1659` (pub), `warm_tab` `:1666-1681`, `show_tab(w: &mut Window, tab: TabId) -> EngineAction` `:1707-1727`, `open_window` `:1939-1946` (pub, used by event.rs), `new_tab` `:2091-2109`, `new_background_tab` `:2113-2127`, `activate_tab` `:2189-2206` (calls `show_tab`), `close_tab` `:2132-2187` (pub(crate), calls `show_tab` for the next tab). Imports at `:1-25` include `use crate::model::{Browser, FindState, Folder, FolderId, LoadState, Tab, TabId, Window, WindowId, site_key};`.
- `event.rs`: `EngineEvent` `:130-219`, `apply` `:221-...` with `UrlChanged` at `:227-242` and `LoadStateChanged` at `:253-272`; `with_tab(browser, tab, f)` `:394-398`.
- The shell's `tick_warms` (`crates/osmena-shell/src/app.rs:360-364`) dispatches `Command::WarmTab(tab)` and **skips a tab whose dispatch returns nothing**, so `warm_tab` may decline a blocked tab without stalling the queue.
- Exhaustive matches that a new `Prompt` variant breaks (osmena-chrome `chrome.rs`): `open_prompt` `:516-525` (a `String::new()` list), `draw_prompt`'s title tuple `:2028-2047` (a `return Vec::new()` list) and the generic answer match `:2134-2167` (a `Vec::new()` list). A new `EngineAction` variant breaks the shell's `perform` match (`app.rs:848-1360`). A new `EngineEvent` variant breaks the shell's `fn tab_of(event: &EngineEvent) -> Option<TabId>` (`app.rs:1673-1694`, which names the window to redraw after an event; its first group `UrlChanged | TitleChanged | ... | TabClosed(tab) => Some(*tab)`), and the shell's `session_changed` trigger is a non-exhaustive `matches!` at `app.rs:1408-1414` (`UrlChanged | TitleChanged | TabClosed | NewTabRequested`). A new `Command` variant breaks only `dispatch`.
- `osmena-engine/src/lib.rs`: `TabHost` `:130-196` with `fn navigate(&self, tab: TabId, url: Url);` at `:139` and defaulted methods ending with `start_download` at `:195`; `Engine` `:198-248` with `configure_downloads` at `:244` followed by the three download methods. `pub use osmena_core::DownloadPolicy;` at `:34`.

---

<!-- START_SUBCOMPONENT_A (tasks 1-3) -->
<!-- START_TASK_1 -->
### Task 1: The new commands, prompt and actions

**Verifies:** None (types; behaviour follows in Tasks 2-6).

**Files:**
- Modify: `crates/osmena-core/src/command.rs:30-395` (`Command`), `:482-548` (`Prompt`), `:556-781` (`EngineAction`), `:1-25` (imports)
- Modify: `crates/osmena-core/src/event.rs:130-219` (`EngineEvent`)

**Implementation:**

In `command.rs` add to the imports:

```rust
use crate::blocked::{BlockedPage, BlockedSites, line_seed};
use crate::model::about_blank;
```

(`about_blank` can also join the existing `use crate::model::{...}` list.)

Add to `pub enum Command`, directly after the `Quit` variant's predecessor group, i.e. before `Quit,`:

```rust
    /// Ask the user to confirm blocking the sites of these tabs on this
    /// profile (the tab menu's "Block on this profile"); sites already
    /// blocked are left out of the question.
    PromptBlockSites {
        window: WindowId,
        tabs: Vec<TabId>,
    },
    /// Block these sites (registrable domains) on this profile: every
    /// open tab on one shows the block page from now on.
    BlockSites(Vec<String>),
    /// Unblock `site`; the tabs blocked on it return to their pages.
    /// Only the settings window issues it.
    UnblockSite(String),
```

Add to `pub enum Prompt`, after `QuitWithDownloads { count: usize },`:

```rust
    /// Confirm blocking these sites on this profile.
    BlockSites {
        sites: Vec<String>,
    },
```

Add to `pub enum EngineAction`, after `SaveZoom,`:

```rust
    /// The profile's blocked sites, for the engine's before-navigate hook:
    /// at start and after every change.
    ConfigureBlockedSites(BlockedSites),
    /// Replace `tab`'s current history entry with `about:blank`: the page
    /// it shows is blocked now, and Back should skip it.
    ReplaceWithBlank(TabId),
    /// The blocked sites changed: persist them.
    SaveBlockedSites,
```

In `event.rs` add to `pub enum EngineEvent`, after `TabCrashed { tab, reason },`:

```rust
    /// The engine's before-navigate hook cancelled `tab`'s navigation to
    /// `url`, a blocked site, before any request went out. The tab is
    /// still on its previous page.
    NavigationBlocked {
        tab: TabId,
        url: Url,
    },
```

`dispatch` (`command.rs:785`) and `apply` (`event.rs:226`) are exhaustive matches with no `_` arm, so the crate does not build until the new variants have arms. Give them placeholders now; Task 3 replaces the command arms and Task 4 the event arm:

- in `dispatch`, before `Command::Quit`:
  ```rust
          Command::PromptBlockSites { .. } | Command::BlockSites(_) | Command::UnblockSite(_) => {
              Vec::new()
          }
  ```
- in `apply`, after the `TabCrashed { tab, reason }` arm:
  ```rust
          EngineEvent::NavigationBlocked { .. } => Vec::new(),
  ```

**Verification:**
Run: `cargo build -p osmena-core`
Expected: builds. `cargo build --workspace` does **not** yet (the shell's and chrome's exhaustive matches); Task 7 closes them. Do not commit until Task 3.
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: The guard on every navigation the user starts

**Verifies:** blocked-sites.AC3.1, blocked-sites.AC3.2, blocked-sites.AC3.3, blocked-sites.AC2.6 (the "shows when activated" half)

**Files:**
- Modify: `crates/osmena-core/src/command.rs`: the `Navigate` arm (`:1338-1349`), `Reload` (`:1352-1359`), `restore_session` (`:1589-1659`), `warm_tab` (`:1666-1681`), `show_tab` (`:1707-1727`) and its two callers in `activate_tab` (`:2189-2206`) and `close_tab` (`:2132-2187`), `open_window` (`:1939-1946`), `new_tab` (`:2091-2109`), `new_background_tab` (`:2113-2127`)
- Test: `crates/osmena-core/src/command.rs` (`mod tests`)

**Implementation:**

Add these helpers near `open_window` (they are the guard):

```rust
/// The block page a navigation to `url` lands on, if its site is blocked
/// on this profile.
fn block_for(browser: &Browser, url: &Url) -> Option<BlockedPage> {
    browser
        .blocked
        .blocks(url)
        .map(|site| BlockedPage::new(site, line_seed()))
}

/// A new tab at `url`, and what the engine loads for it: the page, or
/// `about:blank` behind the block page when the site is blocked. The
/// tab keeps `url` either way, so the address bar says where the user
/// meant to go.
fn tab_at(browser: &Browser, id: TabId, url: Url) -> (Tab, Url) {
    let mut tab = Tab::new(id, url);
    let load = match block_for(browser, &tab.url) {
        Some(page) => {
            tab.block(page);
            about_blank()
        }
        None => tab.url.clone(),
    };
    (tab, load)
}

/// The block page a deferred tab shows when it wakes, if its site is
/// blocked now (the session file records no block, and a site may have
/// been blocked or unblocked while the tab slept).
fn wake_mark(browser: &Browser, tab: TabId) -> Option<BlockedPage> {
    browser
        .tab(tab)
        .filter(|t| t.deferred)
        .and_then(|t| block_for(browser, &t.url))
}
```

**`Navigate`** (`:1338-1349`) becomes:

```rust
        Command::Navigate { tab, url } => {
            let page = block_for(browser, url);
            match browser.tab_mut(*tab) {
                Some(t) => {
                    t.url = url.clone();
                    t.load = LoadState::Loading;
                    t.crashed = None;
                    // A blocked destination: the engine loads the blank
                    // page (a push, so Back returns to the page before),
                    // and the chrome paints the block page over it.
                    let load = match page {
                        Some(page) => {
                            t.block(page);
                            about_blank()
                        }
                        None => {
                            t.blocked = None;
                            url.clone()
                        }
                    };
                    vec![EngineAction::Navigate {
                        tab: *tab,
                        url: load,
                    }]
                }
                None => Vec::new(),
            }
        }
```

**`Reload`** (`:1352-1359`) becomes:

```rust
        Command::Reload(tab) => match browser.tab_mut(*tab) {
            // A block page reloads into another of its lines; the engine
            // is asked nothing.
            Some(t) if t.blocked.is_some() => {
                if let Some(page) = t.blocked.as_mut() {
                    page.reroll(line_seed());
                }
                Vec::new()
            }
            Some(t) => {
                t.load = LoadState::Loading;
                t.crashed = None;
                vec![EngineAction::Reload(*tab)]
            }
            None => Vec::new(),
        },
```

**`open_window`** (`:1939-1946`) becomes:

```rust
pub fn open_window(browser: &mut Browser, url: Url) -> Vec<EngineAction> {
    let window = browser.allocate_window_id();
    let tab = browser.allocate_tab_id();
    let (t, load) = tab_at(browser, tab, url);
    browser.windows.push(Window::with_tab(window, t));
    vec![EngineAction::OpenWindow {
        window,
        tab,
        url: load,
    }]
}
```

**`new_tab`** (`:2091-2109`): after `let tab = browser.allocate_tab_id();` insert `let (t, load) = tab_at(browser, tab, url);` and change `w.tabs.push(Tab::new(tab, url.clone()));` to `w.tabs.push(t);` and the action to `EngineAction::CreateTab { window, tab, url: load }`. (The `url` parameter is moved into `tab_at`; nothing else uses it after.)

**`new_background_tab`** (`:2113-2127`): after `let tab = browser.allocate_tab_id();` insert `let (t, load) = tab_at(browser, tab, url.clone());`, change `w.insert_tab_after(opener, Tab::new(tab, url.clone()));` to `w.insert_tab_after(opener, t);` and the action's `url: url.clone()` to `url: load`.

**`show_tab`** (`:1707-1727`) gains the mark as a third argument:

```rust
/// Show `tab` in `w`: a deferred tab is created in the engine now (it
/// comes up visible), unless a warm has it there already, in which
/// case it is shown like any other. `mark` is the block page a deferred
/// tab wakes on when its site is blocked (`wake_mark`): it is created at
/// `about:blank` behind it.
fn show_tab(w: &mut Window, tab: TabId, mark: Option<BlockedPage>) -> EngineAction {
    let window = w.id;
    match w.tab_mut(tab) {
        Some(t) if t.deferred => {
            t.deferred = false;
            match mark {
                Some(page) => t.block(page),
                None => t.blocked = None,
            }
            if std::mem::take(&mut t.warming) {
                EngineAction::Show(tab)
            } else {
                let url = if t.blocked.is_some() {
                    about_blank()
                } else {
                    t.url.clone()
                };
                EngineAction::CreateTab { window, tab, url }
            }
        }
        _ => EngineAction::Show(tab),
    }
}
```

Its two callers compute the mark **before** borrowing the window mutably (the mark reads `browser`, the window borrow is `browser.window_mut(..)`):

- `activate_tab` (`:2189-2206`): after the `let Some(window_id) = browser.window_of_tab(tab) else { return Vec::new(); };` line insert `let mark = wake_mark(browser, tab);`, and change `show_tab(w, tab)` to `show_tab(w, tab, mark)`.
- `close_tab` (`:2132-2187`): it picks the next tab after removing the closed one and calls `show_tab(w, next)`. Restructure that spot so the id of the next tab is found first, the window borrow ends, `let mark = wake_mark(browser, next);` runs, and the window is re-borrowed with `browser.window_mut(window).expect("...")` to call `show_tab(w, next, mark)`. Keep every other line of `close_tab` as it is. Run `grep -n "show_tab(" crates/osmena-core/src/command.rs` afterwards: every call must have three arguments.

**`warm_tab`** (`:1666-1681`): a tab on a blocked site is never loaded, even out of sight. Insert, before the `match browser.tab_mut(tab)`:

```rust
    // A tab on a blocked site is not warmed: nothing is requested from
    // the site, and the shell moves on to the next in line.
    if browser
        .tab(tab)
        .is_some_and(|t| browser.blocked.blocks(&t.url).is_some())
    {
        return Vec::new();
    }
```

**`restore_session`** (`:1589-1659`): the active tab comes through `open_window`, which is guarded. The deferred tabs are marked at restore and left out of the warm list. Before `let w = browser.window_mut(window).expect("just opened");` insert

```rust
        // Deferred tabs on a blocked site are marked now so the strip
        // can say so; they are re-checked when shown.
        let marks: Vec<Option<BlockedPage>> =
            sw.tabs.iter().map(|st| block_for(browser, &st.url)).collect();
```

and inside the tab loop, in the deferred branch after `t.deferred = true;`, add `if let Some(page) = marks[i].clone() { t.block(page); }`. Then change the unconditional `t.title = st.title.clone();` to

```rust
            // A blocked tab shows its address, not the title it had.
            if t.blocked.is_none() {
                t.title = st.title.clone();
            }
```

(The active tab comes blocked from `open_window`; a deferred one from the mark just set. `TitleChanged` is ignored while blocked, so a title left here would stay for good.) Change the warm line to

```rust
        warm.extend(
            w.tabs
                .iter()
                .filter(|t| t.deferred && t.blocked.is_none())
                .map(|t| t.id),
        );
```

**Testing** (add to `mod tests` in `command.rs`; each test blocks a site by setting `b.blocked.add("b.example")` directly on the model, which is what Task 3's `BlockSites` does and what the shell loads at start):

- `a_typed_address_on_a_blocked_site_shows_the_block_page_over_blank` (AC3.1): `one_window()`, give the tab a title (`b.tab_mut(tab).unwrap().title = "Before".to_string()`), `b.blocked.add("b.example")`, dispatch `Navigate { tab, url: url("https://www.b.example/x") }`; assert actions `== vec![EngineAction::Navigate { tab, url: url("about:blank") }]`, the tab's `url` is `https://www.b.example/x`, `blocked.as_ref().unwrap().site == "b.example"`, `load == Loading`, `title.is_empty()` and `favicon.is_none()` (so the strip shows the address). Then dispatch `Navigate` to `https://c.example/`: actions name that URL and `blocked` is `None`.
- `a_new_tab_a_search_and_a_question_on_a_blocked_site_are_blocked` (AC3.2): block `b.example`; `NewTab { window, url: Some(url("https://b.example/")) }` returns `[Hide(_), CreateTab { url, .. }]` with `url == about:blank` and the new active tab blocked with `url` `https://b.example/`; set `b.settings.search` to a template whose `url_for("q")` lands on `b.example` (look at how `a_search_loads_the_search_url_in_the_tab_or_a_new_tab` at `command.rs:2500` builds one) and check `Search { tab, query }` gives `Navigate { url: about:blank }` with the tab's `url` the search URL and `blocked` set; `SearchInNewTab` likewise gives `CreateTab` at `about:blank`; so does `AskInNewTab { window, kind, query }` once `b.settings.launch` holds an ask template for `kind` whose `url_for` lands on `b.example` (see how the launch tests at `command.rs:5190-5400` set one with `SetAskTemplate`). `NewBackgroundTab { opener, url }` on the site gives `CreateHiddenTab` at `about:blank` and the inserted tab is blocked. `OpenInNewWindow(url)` gives `OpenWindow` at `about:blank` and the new window's tab is blocked.
- `a_restored_tab_on_a_blocked_site_wakes_on_the_block_page_and_is_not_warmed` (AC3.3, AC2.6): build a `Session` with one window of three tabs (`a.example`, `b.example`, `c.example`, each with a saved title, active 0) as `saved_session()` does at `command.rs:4014-4050` (copy its shape), block `b.example`, call `restore_session(&mut b, &session)`; assert the `WarmTabs` action lists only the `c.example` tab, the `b.example` tab is `deferred` with `blocked` `Some` and an empty `title` while the other two keep their saved titles; on a second session with the `b.example` tab active (`active: 1`), the `OpenWindow` action's `url` is `about:blank` and the active tab is blocked with an empty title; dispatch `WarmTab(b_tab)` returns `[]`; dispatch `ActivateTab(b_tab)` returns `[Hide(_), CreateTab { url, .. }]` with `url == about:blank`, and the tab is no longer deferred and still blocked. Also: unblock with `b.blocked.remove("b.example")` on another restored copy before activating; then `ActivateTab` creates the tab at its own URL with `blocked == None` (the wake re-check).
- `reload_on_a_block_page_rerolls_the_line_and_asks_the_engine_nothing` (AC3.7): block, navigate, read `line`, dispatch `Reload(tab)` and assert the actions are empty and `line` changed; a second `Reload` changes it again; `Reload` on an unblocked tab still returns `[EngineAction::Reload(tab)]`.

**Verification:**
Run: `cargo test -p osmena-core`
Expected: the new tests and every existing test pass (existing tests that assert `OpenWindow`/`CreateTab` URLs are unaffected because nothing is blocked in them).
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: Blocking and unblocking: the prompt, `BlockSites`, `UnblockSite`

**Verifies:** blocked-sites.AC2.2, blocked-sites.AC2.3 (model half), blocked-sites.AC2.4, blocked-sites.AC2.5, blocked-sites.AC2.6 (the "marked" half), blocked-sites.AC4.4, blocked-sites.AC4.5, blocked-sites.AC4.6

**Files:**
- Modify: `crates/osmena-core/src/command.rs` (`dispatch` arms and two helpers)
- Test: `crates/osmena-core/src/command.rs`

**Implementation:**

Replace Task 1's placeholder arm in `dispatch` with these three, before `Command::Quit`:

```rust
        Command::PromptBlockSites { window, tabs } => {
            if browser.window(*window).is_none() {
                return Vec::new();
            }
            let urls = tabs.iter().filter_map(|id| browser.tab(*id)).map(|t| &t.url);
            let sites = browser.blocked.sites_to_block(urls);
            if sites.is_empty() {
                return Vec::new();
            }
            vec![EngineAction::ShowPrompt {
                window: *window,
                prompt: Prompt::BlockSites { sites },
            }]
        }
        Command::BlockSites(sites) => block_sites(browser, sites),
        Command::UnblockSite(site) => unblock_site(browser, site),
```

Add the helpers near `set_zoom` (the fan-out precedent):

```rust
/// Block `sites` on this profile. Every tab on one of them, in every
/// window, shows the block page from now on: a live tab has its history
/// entry replaced with `about:blank` (so Back returns to the page
/// before it), a deferred tab is only marked and wakes on the block
/// page, and a tab warming out of sight has its engine tab closed. Then
/// the file is saved and the engine told. Nothing when no site is new.
fn block_sites(browser: &mut Browser, sites: &[String]) -> Vec<EngineAction> {
    let mut changed = false;
    for site in sites {
        changed |= browser.blocked.add(site);
    }
    if !changed {
        return Vec::new();
    }
    let mut actions = Vec::new();
    for t in browser.windows.iter_mut().flat_map(|w| w.tabs.iter_mut()) {
        if t.blocked.is_some() {
            continue;
        }
        let Some(site) = browser.blocked.blocks(&t.url) else {
            continue;
        };
        t.block(BlockedPage::new(site, line_seed()));
        if t.deferred {
            if std::mem::take(&mut t.warming) {
                t.load = LoadState::Idle;
                actions.push(EngineAction::CloseTab(t.id));
            }
        } else {
            actions.push(EngineAction::ReplaceWithBlank(t.id));
        }
    }
    actions.push(EngineAction::SaveBlockedSites);
    actions.push(EngineAction::ConfigureBlockedSites(browser.blocked.clone()));
    actions
}

/// Unblock `site`: every tab blocked on it goes back to its own page
/// (a deferred one simply loses the mark), the file is saved and the
/// engine told. Nothing for a site that was not blocked.
fn unblock_site(browser: &mut Browser, site: &str) -> Vec<EngineAction> {
    if !browser.blocked.remove(site) {
        return Vec::new();
    }
    let mut actions = Vec::new();
    for t in browser.windows.iter_mut().flat_map(|w| w.tabs.iter_mut()) {
        if t.blocked.as_ref().is_some_and(|p| p.site == site) {
            t.blocked = None;
            if !t.deferred {
                t.load = LoadState::Loading;
                actions.push(EngineAction::Navigate {
                    tab: t.id,
                    url: t.url.clone(),
                });
            }
        }
    }
    actions.push(EngineAction::SaveBlockedSites);
    actions.push(EngineAction::ConfigureBlockedSites(browser.blocked.clone()));
    actions
}
```

(`browser.windows.iter_mut()` and `browser.blocked.blocks(..)` borrow two different fields of `browser` directly, which Rust allows side by side; do not route either through a `&mut self` method inside the loop.)

**Testing** (in `command.rs`'s `mod tests`):

- `the_block_prompt_names_the_distinct_unblocked_sites_or_nothing` (AC2.2): `three_tabs()`-style setup with tabs at `https://www.a.example/`, `https://m.a.example/`, `https://b.example/`; dispatch `PromptBlockSites { window, tabs: all three }` → `[ShowPrompt { window, prompt: Prompt::BlockSites { sites } }]` with `sites == ["a.example", "b.example"]`; with `b.blocked.add("a.example")` beforehand only `b.example` remains; with both blocked, or with only an `about:blank` tab, the result is empty; an unknown window is empty.
- `blocking_replaces_every_live_tab_on_the_site_in_every_window` (AC2.4, AC2.5, AC4.5): two windows (`open_window` twice), tabs on `https://a.example/`, `https://m.a.example/x` (second window), `https://b.example/`; dispatch `BlockSites(vec!["a.example"])`; assert the actions are exactly `[ReplaceWithBlank(t1), ReplaceWithBlank(t2), SaveBlockedSites, ConfigureBlockedSites(sites)]` where `sites.sites` contains only `a.example` (tab order is strip order, window by window); both tabs have `blocked.site == "a.example"` and keep their URLs; the `b.example` tab is untouched; `b.blocked.sites.contains("a.example")`.
- `blocking_a_deferred_tab_only_marks_it` (AC2.6): restore a session with a deferred tab on `b.example` (nothing blocked yet), then dispatch `BlockSites(["b.example"])`; the actions carry no `ReplaceWithBlank` for it, only `SaveBlockedSites` and `ConfigureBlockedSites`; the tab is `deferred` and `blocked`. Then dispatch `WarmTab(tab)` on an un-marked copy first, so it is warming, and block: the actions include `CloseTab(tab)` and the tab is no longer warming.
- `blocking_a_site_already_blocked_changes_nothing` (AC4.6): `b.blocked.add("a.example")`, dispatch `BlockSites(["a.example"])` → `[]`.
- `unblocking_sends_every_tab_blocked_on_the_site_back_to_its_page` (AC4.4): block `a.example` with two live tabs and one deferred on it; dispatch `UnblockSite("a.example")`; assert `[Navigate { tab: t1, url: t1's url }, Navigate { tab: t2, url: ... }, SaveBlockedSites, ConfigureBlockedSites(empty)]`, every `blocked` is `None`, the deferred one is still deferred; `UnblockSite("a.example")` again → `[]`.

**Verification:**
Run: `cargo test -p osmena-core`
Expected: all pass.

**Commit (Tasks 1-3):** `cargo build --workspace` still fails here (the shell and chrome matches); commit the core crate anyway, since the core's tests are green, and close the build in Task 7 of this phase before pushing anything:

```bash
cargo fmt --all
git add crates/osmena-core/src/command.rs crates/osmena-core/src/event.rs
git commit -m "feat: block and unblock sites in the model, and guard every navigation" -m "PromptBlockSites asks about the distinct sites not yet blocked; BlockSites marks every tab on them (live tabs get their entry replaced, deferred ones wake on the block page) and asks for the file and the engine's list; UnblockSite sends the tabs back. Every navigation the user starts goes through one guard: a blocked destination keeps the URL and loads about:blank behind the chrome's page. Reload on a block page re-rolls its line." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_3 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_SUBCOMPONENT_B (tasks 4-5) -->
<!-- START_TASK_4 -->
### Task 4: `apply`: `NavigationBlocked`

**Verifies:** blocked-sites.AC3.5 (model half), and the engine-hook path of AC3.4 that Phases 6 and 7 complete

**Files:**
- Modify: `crates/osmena-core/src/event.rs` (`apply`, imports)
- Test: `crates/osmena-core/src/event.rs`

**Implementation:**

Add to the imports in `event.rs`:

```rust
use crate::blocked::{BlockedPage, line_seed};
use crate::model::about_blank;
```

Replace Task 1's placeholder `NavigationBlocked` arm in `apply` (after the `TabCrashed { tab, reason }` arm) with:

```rust
        // The engine refused a navigation to a blocked site before any
        // request: the tab shows the block page for the address it was
        // going to, over a blank page pushed behind it so Back returns to
        // the page it was on.
        EngineEvent::NavigationBlocked { tab, url } => {
            if browser.tab(*tab).is_none() {
                return Vec::new();
            }
            let Some(site) = browser.blocked.blocks(url).map(str::to_owned) else {
                // The engine's list is behind the model's (the site was
                // just unblocked): the page is wanted after all.
                return vec![EngineAction::Navigate {
                    tab: *tab,
                    url: url.clone(),
                }];
            };
            with_tab(browser, *tab, |t| {
                t.url = url.clone();
                t.block(BlockedPage::new(site, line_seed()));
                t.load = LoadState::Loading;
                t.crashed = None;
            });
            vec![EngineAction::Navigate {
                tab: *tab,
                url: about_blank(),
            }]
        }
```

**Testing** (in `event.rs`'s `mod tests`, which has its own `one_window()`):

- `a_navigation_the_engine_blocked_puts_the_tab_on_the_block_page`: block `b.example` (`b.blocked.add`), apply `NavigationBlocked { tab, url: url("https://b.example/p") }` → `[Navigate { tab, url: url("about:blank") }]`; the tab's `url` is `https://b.example/p`, `blocked.site == "b.example"`; an unknown tab gives `[]`.
- `a_navigation_blocked_for_a_site_no_longer_blocked_is_loaded_after_all`: nothing blocked, apply `NavigationBlocked { tab, url: url("https://b.example/") }` → `[Navigate { tab, url: url("https://b.example/") }]` and `blocked` stays `None`.

**Verification:**
Run: `cargo test -p osmena-core event`
Expected: pass.
<!-- END_TASK_4 -->

<!-- START_TASK_5 -->
### Task 5: `apply`: the three `UrlChanged` rules and no visit for a block page

**Verifies:** blocked-sites.AC3.5 (model half), blocked-sites.AC3.6

**Files:**
- Modify: `crates/osmena-core/src/event.rs:227-242` (`UrlChanged`) and `:253-272` (`LoadStateChanged`)
- Test: `crates/osmena-core/src/event.rs`

**Implementation:**

Replace the `UrlChanged` arm with:

```rust
        EngineEvent::UrlChanged { tab, url } => {
            let Some(t) = browser.tab(*tab) else {
                return Vec::new();
            };
            // The blank page behind a block page: the address bar keeps
            // the address the user was going to.
            if t.blocked.is_some() && url.as_str() == "about:blank" {
                return Vec::new();
            }
            // A redirect, or a history step, landing on a blocked site
            // without the engine's hook catching it: blocked here, and
            // the entry replaced so Back does not land on it again.
            if let Some(site) = browser.blocked.blocks(url).map(str::to_owned) {
                let page = BlockedPage::new(site, line_seed());
                with_tab(browser, *tab, |t| {
                    t.url = url.clone();
                    t.block(page);
                });
                return vec![EngineAction::ReplaceWithBlank(*tab)];
            }
            // A page on another site takes that site's zoom level; any
            // block page the tab showed is over.
            let level = browser.zoom.level_for(url, browser.settings.page_zoom);
            let mut actions = Vec::new();
            with_tab(browser, *tab, |t| {
                t.url = url.clone();
                t.blocked = None;
                if t.zoom != level {
                    t.zoom = level;
                    actions.push(EngineAction::SetZoom {
                        tab: t.id,
                        percent: level,
                    });
                }
            });
            actions
        }
```

In the `LoadStateChanged` arm, the visit guard `Some(t) if *state == LoadState::Complete && !t.deferred => {` becomes `Some(t) if *state == LoadState::Complete && !t.deferred && t.blocked.is_none() => {` (a block page is nobody's visit; the attempted URL must not enter the history).

The blank page behind a block page reports a title (empty, or "about:blank") and may report a favicon; neither belongs to the tab, which shows its attempted address instead. Add a guard at the top of the `TitleChanged` arm and of the `FaviconChanged` arm:

```rust
            if browser.tab(*tab).is_some_and(|t| t.blocked.is_some()) {
                return Vec::new();
            }
```

(In `TitleChanged` this also keeps `history.retitle` from running for the attempted URL.)

**Testing** (in `event.rs`'s `mod tests`):

- `the_blank_page_behind_a_block_page_leaves_the_tab_alone`: block `b.example`, dispatch `Command::Navigate { tab, url: url("https://b.example/") }` (`dispatch` and `Command` are already imported in this test module), then apply `UrlChanged { tab, url: url("about:blank") }` → `[]`, the tab's `url` is still `https://b.example/` and `blocked` is `Some`. Apply `TitleChanged { tab, title: "about:blank".to_string() }` → `[]` and `title` stays empty; apply `FaviconChanged { tab, favicon: <any Favicon; build it as the existing favicon tests in event.rs do> }` → `[]` and `favicon` stays `None`. Then `LoadStateChanged { tab, state: Complete }` → `[]` and `assert!(b.history.entries().iter().all(|e| e.url.host_str() != Some("b.example")))` (`History::entries(&self) -> &[HistoryEntry]` at `history.rs:57`).
- `going_back_from_a_block_page_clears_it` (AC3.6): same setup, then apply `UrlChanged { tab, url: url("https://a.example/") }` → `[]` (same zoom), `url` is `https://a.example/` and `blocked == None`.
- `a_url_change_onto_a_blocked_site_is_blocked_again` (AC3.5): block `b.example`, apply `UrlChanged { tab, url: url("https://m.b.example/r") }` on an unblocked tab → `[ReplaceWithBlank(tab)]`, `blocked.site == "b.example"`, `url == https://m.b.example/r`.
- Keep `url_changed_updates_tab_url` passing unchanged.

**Verification:**
Run: `cargo test -p osmena-core`
Expected: all pass.

**Commit (Tasks 4-5):**

```bash
cargo fmt --all
git add crates/osmena-core/src/event.rs
git commit -m "feat: fold the engine's blocked navigations and the blank page behind a block page into the model" -m "NavigationBlocked puts the tab on the block page for the address it was going to and pushes about:blank behind it. UrlChanged ignores that blank page while the tab is blocked, re-blocks a URL on a blocked site that reached the engine without a hook (a redirect, a history step) by replacing the entry, and clears the block otherwise. A block page is no visit." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_5 -->
<!-- END_SUBCOMPONENT_B -->

<!-- START_TASK_6 -->
### Task 6: The engine contract's two defaulted methods

**Verifies:** None (contract; the design listed it under Phase 3, moved here so the shell can forward the new actions and the workspace builds).

**Files:**
- Modify: `crates/osmena-engine/src/lib.rs:34` (re-export), `:130-196` (`TabHost`), `:198-248` (`Engine`)

**Implementation:**

After `pub use osmena_core::DownloadPolicy;` (line 34) add:

```rust
pub use osmena_core::BlockedSites;
```

In `pub trait TabHost`, after `fn start_download(&self, _tab: TabId, _url: Url) {}` add:

```rust
    /// Replace the tab's current history entry with `about:blank`, so
    /// Back from there goes to the page before it: the page it shows
    /// has just been blocked, and the chrome paints the block page over
    /// the blank one. The default navigates instead, which adds an
    /// entry; an engine that can replace should.
    fn replace_with_blank(&self, tab: TabId) {
        self.navigate(tab, osmena_core::about_blank());
    }
```

In `pub trait Engine`, after `fn resume_download(&self, _id: u32) {}` add:

```rust
    /// The profile's blocked sites, at start and after every change. The
    /// engine's before-navigate hook cancels any main-frame navigation
    /// to one of them before a request goes out and reports it as
    /// `EngineEvent::NavigationBlocked`; the model's own guard covers
    /// what the user types, so an engine without a hook still blocks
    /// typed addresses. Default: nothing.
    fn configure_blocked_sites(&self, _sites: BlockedSites) {}
```

**Verification:**
Run: `cargo build -p osmena-engine --features glutin`
Expected: builds.
<!-- END_TASK_6 -->

<!-- START_TASK_7 -->
### Task 7: Close the workspace build: the shell's arms and the chrome's prompt matches

**Verifies:** None (compile closure; the behaviour behind the arms is Phase 3's and Phase 4's).

**Files:**
- Modify: `crates/osmena-shell/src/app.rs:1027-1032` (after the `SaveZoom` arm in `perform`), `:1408-1414` (the `session_changed` trigger), `:1673-1694` (`tab_of`)
- Modify: `crates/osmena-chrome/src/chrome.rs:516-525`, `:2028-2047`, `:2134-2167`

**Implementation:**

In `app.rs`, after the `EngineAction::SaveZoom => { ... }` arm add:

```rust
            EngineAction::SaveBlockedSites => {
                let path = self.browser.profile.blocked_path();
                if let Err(e) = self.browser.blocked.save(&path) {
                    log::error!("cannot save the blocked sites to {}: {e}", path.display());
                }
            }
            EngineAction::ConfigureBlockedSites(sites) => engine.configure_blocked_sites(sites),
            EngineAction::ReplaceWithBlank(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().replace_with_blank(tab);
                    // The block page is the chrome's: paint it now rather
                    // than when the engine next produces a frame.
                    w.request_redraw();
                }
            }
```

(`engine` is the `let Some(engine) = self.engine.as_ref()` binding at the top of `perform`; the other two arms borrow `self` mutably after `engine`'s last use, as the `Navigate` and `SetZoom` arms do.)

In `tab_of` (`app.rs:1673-1694`) add `| EngineEvent::NavigationBlocked { tab, .. }` to the first group, the one ending `| EngineEvent::TabClosed(tab) => Some(*tab)`, so the window showing the tab redraws with the block page as soon as the event lands. In the `session_changed` trigger (`app.rs:1408-1414`) add `| EngineEvent::NavigationBlocked { .. }` to the `matches!` list, since the event changes `Tab::url`.

In `chrome.rs`:
- `open_prompt` (`:516-525`): add `| Prompt::BlockSites { .. }` to the alternatives that map to `String::new()`.
- `draw_prompt`'s title tuple match (`:2028-2047`): add `| Prompt::BlockSites { .. }` to the alternatives that `return Vec::new()`.
- the generic answer match (`:2134-2167`): add `| Prompt::BlockSites { .. }` to the alternatives that yield `Vec::new()`.

(Phase 4 adds the real `Some(Prompt::BlockSites { .. }) => return draw_block_sites_prompt(ui, state),` line to `draw_prompt`'s dispatch at `:2013-2021`; until then the prompt opens as an empty state and closes with nothing, which no user can reach because no menu entry issues `PromptBlockSites` yet.)

**Verification:**

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

Expected: all clean and green.

**Commit:**

```bash
cargo fmt --all
git add crates/osmena-engine/src/lib.rs crates/osmena-shell/src/app.rs crates/osmena-chrome/src/chrome.rs
git commit -m "feat: an engine can be told the blocked sites and asked to replace an entry with about:blank" -m "Two defaulted methods on the engine contract, configure_blocked_sites and replace_with_blank, so a browser that implements neither still builds; the shell saves the list, forwards it, and routes ReplaceWithBlank by tab like Navigate." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_7 -->

---

## Phase 2 done when

- The gate is clean and green for the whole workspace.
- `dispatch` and `apply` tests pass for each flow: the prompt's deduplication and the empty case; a live tab getting `ReplaceWithBlank` and a deferred tab only the mark; the guard on `Navigate`, `NewTab`, `Search`, `SearchInNewTab`, `AskInNewTab`, `NewBackgroundTab`, `OpenInNewWindow`, `restore_session`, `WarmTab` and `ActivateTab` of a deferred tab; `Reload` re-rolling; unblock navigating a blocked tab back to its URL; `NavigationBlocked` pushing `about:blank`; the three `UrlChanged` rules; title and favicon events ignored while blocked; no visit for a block page.
- No user can yet reach any of it (no menu entry, no settings section): that is Phases 4 and 5.
