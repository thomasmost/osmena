# Blocked Sites Design

## Summary

Blocked Sites lets a user keep, per profile, a list of websites the
browser refuses to show. The list is keyed by registrable domain
(`youtube.com` covers `www.youtube.com` and `m.youtube.com`), reduced
from any URL with the public suffix list so that `bbc.co.uk` is a site
and `co.uk` is not. A site is added from a tab's context menu after a
confirmation dialog, or from a new "Blocked sites" section in the
settings window, which is also the only place a site can be removed.
The list lives in a `blocked.toml` file in the profile directory,
following the pattern the per-site zoom table already uses.

The approach checks every navigation twice. In the shared osmena
model, the pure `dispatch` function runs each navigation the user
starts (typed address, new tab, search, session restore) through one
guard; a blocked destination marks the tab and loads `about:blank` in
the engine instead, keeping the attempted URL for the address bar. In
each browser's engine, a before-navigate hook (CEF's `on_before_browse`
in Nautilus, Servo's `request_navigation` in Paladin) catches what the
model never sees, namely links, scripts and redirects, cancels them
before any request is sent, and reports back an event the model folds
into the same tab state. The engine learns the list up front through a
new defaulted trait method, so a browser that has not implemented it
still builds. The block page itself is never a web page: the chrome
paints it with egui in the page area, as it paints the crash panel
today, with one of three lines picked at random over the profile's
colour gradient. Blocking a site replaces the history entry of every
open tab on it so Back returns to the previous page; unblocking
navigates those tabs straight back to their own URLs.

## Definition of Done

1. Each profile keeps its own list of blocked sites, matched by
   registrable domain including subdomains.
2. Right-clicking a tab offers "Block on this profile", which asks
   "Are you sure? Blocked sites can only be unblocked from profile
   settings" and on Yes adds the tab's site and sends every open tab on
   that site, in all of the profile's windows, to the block page.
3. Any navigation to a blocked site, typed, clicked or redirected, is
   cancelled by the engine before a request goes out and the tab shows a
   modern, cheeky block page with one of the three lines in big text,
   picked at random: "Nice Try ;)", "Shouldn't you be doing something
   else?", "You don't want you to be going there".
4. The settings window gains a "Blocked sites" section to add, view and
   remove sites, the only place a site can be unblocked.
5. This works in Nautilus and Paladin, with the shared part in osmena
   and a before-navigate hook in each engine; tests, clippy, fmt,
   CHANGELOGs and READMEs updated per each repository's Definition of
   Done.

Out of scope: blocking subresources or iframes, schedules or time
limits, a password on unblocking, syncing lists between profiles.

## Acceptance Criteria

### blocked-sites.AC1: The list and the matching
- **blocked-sites.AC1.1 Success:** `www.`, `m.` and deeper subdomains of a site reduce to one entry (`youtube.com`).
- **blocked-sites.AC1.2 Success:** Multi-label public suffixes hold: `bbc.co.uk` stays `bbc.co.uk`, never `co.uk`.
- **blocked-sites.AC1.3 Edge:** IPv4, IPv6 and `localhost` hosts are kept whole.
- **blocked-sites.AC1.4 Failure:** `about:`, `data:`, `view-source:`, `file:` and other host-less URLs reduce to nothing and are never blockable.
- **blocked-sites.AC1.5 Success:** `blocked.toml` round-trips; a missing file is an empty list; an unreadable file logs a warning, starts empty, and the browser still starts.

### blocked-sites.AC2: Blocking from the tab menu
- **blocked-sites.AC2.1 Success:** The tab menu shows "Block on this profile" (or "Block {n} sites on this profile"), enabled only when a target has a blockable site not yet blocked.
- **blocked-sites.AC2.2 Success:** The prompt lists the targets' distinct sites, skipping already-blocked ones; with nothing to ask, no prompt is shown.
- **blocked-sites.AC2.3 Failure:** Cancel or Escape adds nothing.
- **blocked-sites.AC2.4 Success:** Confirming adds the sites and saves the file.
- **blocked-sites.AC2.5 Success:** Every live tab on the site, in every window of the profile, shows the block page, with its history entry replaced so Back returns to the previous page.
- **blocked-sites.AC2.6 Edge:** A deferred tab on the site is marked and shows the block page when activated.
- **blocked-sites.AC2.7 Success:** The dialog reads "Block <site> on this profile? Are you sure? Blocked sites can only be unblocked from profile settings." with buttons Block (danger colour) and Cancel.

### blocked-sites.AC3: Navigation and the block page
- **blocked-sites.AC3.1 Success:** A typed address on a blocked site shows the block page with no request sent; the previous page is behind it in history.
- **blocked-sites.AC3.2 Success:** A new tab, a Launch Panel answer or a search landing on a blocked site shows the block page.
- **blocked-sites.AC3.3 Success:** A session-restored tab on a blocked site shows the block page when shown.
- **blocked-sites.AC3.4 Success:** A link click or script navigation to a blocked site is cancelled by the engine before any request; the tab shows the block page.
- **blocked-sites.AC3.5 Edge:** A redirect onto a blocked site is cancelled by the engine, or re-blocked by the model if the engine let it through.
- **blocked-sites.AC3.6 Success:** Back from a block page returns to the previous page and the block page clears.
- **blocked-sites.AC3.7 Success:** Reload on a block page re-rolls the line and sends the engine nothing.
- **blocked-sites.AC3.8 Success:** The page shows one of the three lines, large, on the profile-colour gradient, with the site named beneath; the address bar shows the attempted URL.
- **blocked-sites.AC3.9 Success:** The line does not change between frames.
- **blocked-sites.AC3.10 Failure:** No control on the page unblocks; pointer and key input stay in the chrome while it shows.

### blocked-sites.AC4: Settings
- **blocked-sites.AC4.1 Success:** The "Blocked sites" section lists sites alphabetically; the empty state reads "No blocked sites."
- **blocked-sites.AC4.2 Success:** Remove unblocks the site and saves, with no confirmation.
- **blocked-sites.AC4.3 Success/Failure:** The add field takes a bare host or a full URL and reduces it; junk shows "Not a site" and adds nothing.
- **blocked-sites.AC4.4 Success:** Unblocking brings every tab blocked on that site back to its own URL.
- **blocked-sites.AC4.5 Success:** Adding from settings blocks open tabs on that site at once.
- **blocked-sites.AC4.6 Edge:** Adding a site already in the list changes nothing and saves nothing.

### blocked-sites.AC5: Both browsers
- **blocked-sites.AC5.1 Success:** Nautilus: a scripted run shows a blocked link produces no network request in the log.
- **blocked-sites.AC5.2 Success:** Nautilus: `about:`, `data:`, `view-source:`, `chrome:`, `devtools:` and subframe navigations pass the hook.
- **blocked-sites.AC5.3 Success:** Paladin: `request_navigation` denies a blocked URL and reports it; Paladin builds and its tests pass against the pinned osmena.
- **blocked-sites.AC5.4 Success:** CHANGELOGs, READMEs (manual checklist), osmena `CLAUDE.md`/`AGENTS.md` and the PRD index are updated; CI is green in all three repositories.

## Glossary

- **osmena**: The shared browser shell both browsers are built on: the model, the engine contract, the egui chrome and the winit application. Browser-neutral; it never names an engine.
- **Nautilus**: The browser built on Chromium through CEF. Depends on osmena by git revision.
- **Paladin**: The browser built on Servo (forks of Servo and stylo). Depends on osmena by git revision.
- **CEF**: Chromium Embedded Framework, the library that lets Nautilus embed Chromium and intercept its callbacks, such as `on_before_browse`.
- **Servo**: The Rust browser engine Paladin embeds. Its `request_navigation` callback is where Paladin can deny a navigation.
- **egui**: The immediate-mode Rust GUI library the chrome is drawn with. "egui in place of the page" means the chrome paints a panel where the web page would be.
- **The chrome**: Everything the user sees above and around the page: tab strip, toolbar, menus, prompts, overlays. In osmena it is the `osmena-chrome` crate, which emits `Command`s and never calls an engine.
- **The model**: The in-memory `Browser`, `Window` and `Tab` state in `osmena-core`, changed only by `dispatch` and `apply`.
- **The shell**: `osmena-shell`, the winit application that owns the windows, loads and saves files, and joins the chrome to an engine.
- **Registrable domain**: The part of a hostname one registers with a registrar plus the public suffix beneath it: `youtube.com` for `www.youtube.com`, `bbc.co.uk` for `news.bbc.co.uk`. Also called eTLD+1.
- **Public suffix list**: Mozilla's maintained list of suffixes under which names are registered (`com`, `co.uk`, `github.io`). It is what tells `co.uk` apart from `bbc.co.uk`.
- **`psl`**: The Rust crate that compiles the public suffix list into the binary and answers "what is this host's registrable domain" with no network access.
- **Command**: A value naming something the user asked for (`BlockSites`, `UnblockSite`, `Navigate`). Every chrome control, shortcut and menu entry produces one; nothing calls an engine directly.
- **`dispatch`**: The pure function in `osmena-core` that takes a `Command`, updates the model, and returns `EngineAction`s. Unit-tested without a window.
- **EngineAction**: An instruction `dispatch` returns for the shell to carry out, such as navigating a tab, showing a prompt, or saving a file. The shell forwards engine-facing ones to the engine.
- **EngineEvent**: A report from the engine (a URL changed, a navigation was blocked) that the shell hands to `apply`.
- **`apply`**: The pure function in `osmena-core` that folds an `EngineEvent` into the model; the counterpart of `dispatch` for the engine's side of the conversation.
- **Prompt**: A question to the user, modelled as data (`Prompt::BlockSites`) that `dispatch` asks for through `EngineAction::ShowPrompt` and the chrome draws over the page. The answer is an ordinary `Command`.
- **`Engine` trait**: The contract in `osmena-engine` an engine crate implements for process-wide concerns: start-up, policies handed down from the shell such as `configure_downloads` and the new `configure_blocked_sites`.
- **`TabHost` trait**: The per-window part of the engine contract: create, navigate, paint and close engine tabs. `replace_with_blank` is added here.
- **Defaulted method**: A trait method with a body in the trait itself, so an implementor that ignores it still compiles. Both new contract methods are defaulted so neither browser breaks on the pin bump.
- **Deferred tab**: A tab restored from a session that has no engine tab yet; the engine tab is created the first time the tab is activated (`show_tab`). Actions for a deferred tab reach no engine.
- **Session restore**: Rebuilding the windows and tabs of the previous run from the session file at start-up.
- **Profile**: A directory holding one user's settings, bookmarks, history, zoom table and now `blocked.toml`. A guest profile is a profile deleted at exit.
- **Palette**: The chrome's colour set derived from the profile's chrome colour; `Palette::strip_at` is the gradient curve the tab strip and now the block page follow.
- **History entry**: One item in a tab's back/forward list. Replacing the current entry with `about:blank` means Back skips the blocked page; pushing one adds it instead.
- **`about:blank`**: The empty page every engine can show without a request. The engine shows it behind the chrome's block page.
- **Launch Panel**: The new-tab panel that asks for an address, search or question before a tab is created; its answers are navigations the guard must check.
- **Main frame / subframe**: The top-level document of a tab versus an iframe inside it. The hooks filter only main-frame navigations.
- **Before-navigate hook**: An engine callback fired before a navigation starts a request, which can cancel it. `on_before_browse` (CEF) and `request_navigation` (Servo) are the two.
- **`RefCell`**: A Rust cell giving interior mutability checked at run time; used to store the list inside the single-threaded engine delegate so a hook can read it synchronously.

## Architecture

Blocked sites are the model's list; the engine refuses, the chrome
reminds. Three rules hold the design together:

1. **The list is per profile and keyed by registrable domain.**
   `osmena_core::blocked::BlockedSites` (new module
   `crates/osmena-core/src/blocked.rs`) is a `BTreeSet<String>` of
   registrable domains (`youtube.com`, never `www.youtube.com`),
   stored as `blocked.toml` in the profile directory
   (`Profile::blocked_path`), held on `Browser::blocked`. The public
   suffix list, through the `psl` crate (compiled in, no network),
   reduces any URL to its registrable domain in
   `BlockedSites::site_for(&Url) -> Option<String>`; an IP address or
   `localhost` is kept whole; a URL without a host (`about:`, `data:`,
   `view-source:`, `file:`) reduces to `None` and is never blockable.
   `BlockedSites::blocks(&Url) -> Option<&str>` names the blocked site
   a URL falls under, if any.
2. **Every navigation is checked twice: in the model, and in the
   engine.** `dispatch` runs every embedder-started navigation
   (`Navigate`, `NewTab { url }`, `Search`, the Launch Panel's
   answers, `restore_session`, `show_tab`) through one guard before
   issuing an `EngineAction`; a blocked destination marks the tab and
   loads `about:blank` instead. The engine's before-navigate hook
   (CEF `on_before_browse` in Nautilus, Servo `request_navigation` in
   Paladin) covers what the model never sees: links, scripts and
   redirects. It cancels the navigation before any request is sent and
   reports `EngineEvent::NavigationBlocked { tab, url }`. The engine
   learns the list through `Engine::configure_blocked_sites`, called at
   start and after every change, as `configure_downloads` is.
3. **A blocked tab's engine page is `about:blank`; the chrome paints
   the reminder.** `Tab::blocked: Option<BlockedPage>` is the state;
   `BlockedPage { site, line }` names the site and which of the three
   sentences the tab shows. `BrowserWindow::frame` paints
   `chrome::draw_blocked_page` in the page area instead of the engine's
   content, as it paints the crash panel today, and routes input to
   egui. `Tab::url` keeps the attempted URL, so the address bar shows
   where the user tried to go. Nothing is ever requested from a
   blocked site.

### Contracts

The engine contract, in `crates/osmena-engine/src/lib.rs`, grows two
defaulted methods:

```rust
pub trait Engine {
    /// The profile's blocked sites, at start and after every change.
    /// A navigation to one of them is to be cancelled before any
    /// request goes out and reported as `EngineEvent::NavigationBlocked`.
    fn configure_blocked_sites(&self, _sites: BlockedSites) {}
}

pub trait TabHost {
    /// Replace the tab's current history entry with `about:blank`, so
    /// Back from there goes to the page before it. The default
    /// navigates, which adds an entry instead.
    fn replace_with_blank(&self, tab: TabId) { self.navigate(tab, about_blank()) }
}
```

Core types, in `crates/osmena-core/src/`:

```rust
// blocked.rs
pub struct BlockedSites { pub sites: BTreeSet<String> }
impl BlockedSites {
    pub fn site_for(url: &Url) -> Option<String>;      // registrable domain
    pub fn blocks(&self, url: &Url) -> Option<&str>;    // the site it falls under
    pub fn add(&mut self, site: &str) -> bool;          // changed?
    pub fn remove(&mut self, site: &str) -> bool;       // changed?
    pub fn load(path: &Path) -> Result<Self, BlockedError>;   // NotFound => empty
    pub fn save(&self, path: &Path) -> Result<(), BlockedError>;
}
pub const BLOCKED_LINES: [&str; 3] = [
    "Nice Try ;)",
    "Shouldn't you be doing something else?",
    "You don't want you to be going there",
];
pub fn pick_line(seed: u64) -> u8;
pub struct BlockedPage { pub site: String, pub line: u8 }

// model.rs
pub struct Tab { /* … */ pub blocked: Option<BlockedPage> }
pub struct Browser { /* … */ pub blocked: BlockedSites }

// command.rs
Command::PromptBlockSites { window: WindowId, tabs: Vec<TabId> }
Command::BlockSites(Vec<String>)
Command::UnblockSite(String)
Prompt::BlockSites { sites: Vec<String> }
EngineAction::ConfigureBlockedSites(BlockedSites)
EngineAction::ReplaceWithBlank(TabId)
EngineAction::SaveBlockedSites

// event.rs
EngineEvent::NavigationBlocked { tab: TabId, url: Url }
```

### Data flow

**Blocking from the tab menu.** The strip records `MenuRequest::Tab`;
`BrowserWindow::show_menu` builds `menus::tab_menu` with the blocked
list and the menu returns `Command::PromptBlockSites`. `dispatch`
reduces the targets' URLs to distinct registrable domains, drops any
already blocked, and answers `EngineAction::ShowPrompt` with
`Prompt::BlockSites`; the chrome's `draw_block_sites_prompt` asks the
question and answers `Command::BlockSites`. `dispatch` adds the sites
and, for every tab in every window whose URL falls under one: sets
`blocked` with a freshly picked line, and for a live tab emits
`ReplaceWithBlank` (the tab is showing the site, so its entry is
replaced rather than buried); a deferred tab is only marked, and
`show_tab` honours the mark when it creates the engine tab. Then
`SaveBlockedSites` and `ConfigureBlockedSites`.

**Blocking at navigation time.** The guard in `dispatch` turns a
navigation to a blocked URL into `Tab::blocked`, `Tab::url` = the
attempted URL, and `Navigate { about:blank }` (a push: the previous
page stays behind it, so Back returns there). The engine hook does the
same from the other side: it cancels, reports `NavigationBlocked`, and
`apply` sets the state and pushes `about:blank`.

**Living on a blocked tab.** `apply` ignores `UrlChanged` to
`about:blank` on a blocked tab, so the address bar keeps the attempted
URL. Any other `UrlChanged` clears `blocked`, unless the new URL is
itself blocked, in which case the tab is re-blocked (the safety net
for redirects and history traversal that reach the engine without a
hook, which Servo's history traversal does). `Reload` on a blocked tab
re-rolls the line and sends the engine nothing. `Back` and `Forward`
go to the engine unchanged. Known limitation: Forward after Back from a
block page lands on the bare `about:blank` the block page sat over, since
the model keeps no per-entry state.

**Unblocking.** Only `Command::UnblockSite` from the settings window
removes a site. `dispatch` saves and reconfigures, and every tab
blocked on that site has its `blocked` cleared and is navigated back
to its own URL, so the page returns without the user doing anything.

**The settings window** receives the list read-only from the app (as
it receives the extensions list) and pushes `BlockSites` and
`UnblockSite` into its command queue, which the app runs through
`dispatch`.

### Module map

| Where | What |
| ----- | ---- |
| `osmena-core/src/blocked.rs` | `BlockedSites`, `BlockedPage`, `BLOCKED_LINES`, `pick_line`, the file |
| `osmena-core/src/model.rs` | `Tab::blocked`, `Browser::blocked` |
| `osmena-core/src/profile.rs` | `blocked_path` |
| `osmena-core/src/command.rs` | the commands, the prompt, the actions, the guard, block and unblock |
| `osmena-core/src/event.rs` | `NavigationBlocked`, the `UrlChanged` rules |
| `osmena-engine/src/lib.rs` | `configure_blocked_sites`, `replace_with_blank` |
| `osmena-chrome/src/menus.rs` | the tab menu entry |
| `osmena-chrome/src/chrome.rs` | `draw_block_sites_prompt`, `draw_blocked_page` |
| `osmena-shell/src/lib.rs`, `app.rs`, `window.rs` | load at start, the three actions, the paint arm, the menu's new argument |
| `osmena-shell/src/settings_window.rs` | the "Blocked sites" section |
| `nautilus-engine/src/delegate.rs`, `engine.rs`, `host.rs` | `on_before_browse`, the stored list, `replace_with_blank` |
| `paladin-engine/src/delegate.rs`, `engine.rs`, `host.rs` | `request_navigation`, the stored list, `replace_with_blank` |

## Existing Patterns

Investigation of the osmena workspace and both engine crates found a
precedent for every part of this design; it introduces no new kind of
thing.

- **A per-site table in a TOML file of its own**: `zoom.rs` and
  `SiteZoom` (`zoom.toml`, `Profile::zoom_path`, loaded in
  `osmena-shell/src/lib.rs` `run` with a warning on a bad file, saved
  through `EngineAction::SaveZoom` only when the table changed).
  `BlockedSites` copies it, including the error enum, `parse`,
  `to_toml`, `load` with `NotFound` as empty, and `save` through
  `settings::write_atomically`.
- **A policy handed to the engine up front**: `Engine::configure_downloads(DownloadPolicy)`,
  defaulted to nothing, called by the shell after the engine starts
  (`app.rs`, after `open_host`) and in `settings_changed`. Nautilus
  stores the policy in its `DelegateState` behind a `RefCell` and
  reads it synchronously inside `on_before_download`.
  `configure_blocked_sites` is the same shape; both engine delegates
  run on the main thread, so a `RefCell` suffices.
- **Decide in the hook, report after**: Nautilus's `on_before_popup`
  and `on_open_urlfrom_tab` already cancel a load (return 1) and push
  an `EngineEvent`; `on_before_browse` does the same.
- **egui in place of the page**: `Tab::crashed: Option<String>` and
  `chrome::draw_crash_panel`, painted by `BrowserWindow::frame` where
  the page would be, with `content_is_egui` keeping input in egui.
  `Tab::blocked` and `draw_blocked_page` take the same arm.
- **A question to the user is a `Prompt`**: `Prompt::DeleteProfile`
  and `draw_delete_profile_prompt` (an `egui::Window` with id
  `chrome-prompt`, anchored centre-top, a danger-coloured confirm and
  Cancel, Escape cancels). `Prompt::BlockSites` and
  `draw_block_sites_prompt` are the same, registered in `open_prompt`
  and the `draw_prompt` lists like the others.
- **A list with per-row Remove and an add field** in the settings
  window: `draw_extensions` (striped grid, Remove pushing a `Command`,
  a text field with a hint and Enter to submit, a red notice on bad
  input kept in a buffer on `SettingsWindow`, the empty state in weak
  text). The "Blocked sites" section follows it.
- **Fan-out over every tab on a site**: `set_zoom` in `command.rs`
  iterates `browser.windows.iter_mut().flat_map(|w| w.tabs.iter_mut())`
  and matches each tab's URL; blocking and unblocking do the same with
  `BlockedSites::blocks`.
- **The strip's gradient**: `Palette::strip_at` is the curve the
  block page's background follows; `Palette` already picks light or
  dark text for the swatch.
- **Menus are data with tests**: `menus::tab_menu` returns
  `Vec<MenuEntry>`, tested by `labels` and `enabled` vectors in
  `tab_menu_for_one_tab_of_three` and
  `tab_menu_counts_the_selection_and_disables_what_cannot_apply`.

One divergence, deliberate: the existing `site_key` is the exact
lowercased host and stays so for zoom and favicons. Blocking keys by
registrable domain because the user asked for "a site and its
subdomains", which the exact host cannot express; `psl` is a new
dependency for that one purpose.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: The list and the matching
**Goal:** The per-profile data: what a blocked site is, how a URL is
reduced to one, and the file it lives in.

**Components:**
- `crates/osmena-core/src/blocked.rs`: `BlockedSites` (`site_for`,
  `blocks`, `add`, `remove`, `parse`, `to_toml`, `load`, `save`,
  `BlockedError`), `BLOCKED_LINES`, `pick_line`, `BlockedPage`;
  exported from `lib.rs`.
- `psl` added to the workspace `Cargo.toml` and `osmena-core`.
- `Profile::blocked_path` in `crates/osmena-core/src/profile.rs`.
- `Tab::blocked` and `Browser::blocked` in `crates/osmena-core/src/model.rs`,
  defaulted; `Tab::blocked` left out of the session file.

**Dependencies:** none.

**Done when:** unit tests pass for the reduction (www and deep
subdomains to one site, `co.uk`, IPv4 and IPv6, `localhost`,
punycode, no-host schemes), add and remove reporting change, the TOML
round trip including a missing file, and `pick_line` over fixed seeds
landing on every line. Covers blocked-sites.AC1.1 to AC1.5.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: The commands and the events
**Goal:** Every way a tab becomes blocked or unblocked, in the pure
model, with the engine's part expressed as actions and events.

**Components:**
- `crates/osmena-core/src/command.rs`: `PromptBlockSites`,
  `BlockSites`, `UnblockSite`, `Prompt::BlockSites`,
  `EngineAction::{ConfigureBlockedSites, ReplaceWithBlank, SaveBlockedSites}`;
  the guard applied to `Navigate`, `NewTab { url }`, `Search`,
  `SearchInNewTab`, `AskInNewTab`, `restore_session` and `show_tab`;
  `BlockSites` marking live tabs (replace) and deferred tabs (mark
  only); `UnblockSite` restoring tabs; `Reload` re-rolling the line.
- `crates/osmena-core/src/event.rs`: `NavigationBlocked`; the
  `UrlChanged` rules (ignore `about:blank` while blocked, clear
  otherwise, re-block a blocked URL).

**Dependencies:** Phase 1.

**Done when:** `dispatch` and `apply` tests pass for each flow named
above, including: the prompt's deduplication and the empty case; a
live tab getting `ReplaceWithBlank` and a deferred tab only the mark;
the guard on every listed command; unblock navigating a blocked tab
back to its URL; `NavigationBlocked` pushing `about:blank`; the three
`UrlChanged` rules. Covers blocked-sites.AC2.2 to AC2.6, AC3.1 to
AC3.3, AC3.6, AC3.7, AC4.4 and AC4.5.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: The engine contract and the shell
**Goal:** The engine can be told the list and asked to replace an
entry; the shell loads, saves and forwards.

**Components:**
- `crates/osmena-engine/src/lib.rs`: `Engine::configure_blocked_sites`,
  `TabHost::replace_with_blank`, both defaulted.
- `crates/osmena-shell/src/lib.rs`: load `blocked.toml` in `run`
  beside the zoom table.
- `crates/osmena-shell/src/app.rs`: handle the three actions;
  configure the engine after `open_host` and whenever the list
  changes; route `ReplaceWithBlank` by tab like `Navigate`.
- `README.md` engine-contract section and `CHANGELOG.md` entries.

**Dependencies:** Phase 2.

**Done when:** the workspace builds and its tests pass; a browser with
an engine that implements neither method still builds against the
revision (the defaults compile); the shell configures the engine at
start and after `BlockSites`/`UnblockSite`. Operational verification;
no new ACs of its own.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: The chrome
**Goal:** The user can block from a tab's menu, is asked to confirm,
and sees the block page.

**Components:**
- `crates/osmena-chrome/src/menus.rs`: the "Block on this profile" /
  "Block {n} sites on this profile" entry in `tab_menu`, taking the
  blocked list; `crates/osmena-shell/src/window.rs` passes it.
- `crates/osmena-chrome/src/chrome.rs`: `draw_block_sites_prompt`
  (title "Block site", the two-line body, "Block" in the danger
  colour, "Cancel", Escape), registered in `open_prompt` and the
  `draw_prompt` lists; `draw_blocked_page` (the profile-colour
  gradient, the line in Inter Medium sized to the width, the muted
  site line and "Settings › Blocked sites").
- `crates/osmena-shell/src/window.rs`: the paint arm for
  `Tab::blocked` beside the crash panel, `content_is_egui`.

**Dependencies:** Phase 2 (the commands), Phase 3 (the plumbing, for
the menu's list).

**Done when:** the menu tests pass with the new entry (label, enabled
state, command, disabled for an already-blocked or host-less target);
the prompt's answers are the right commands; the block page draws
without panicking at a narrow and a wide rect (an egui test context).
Covers blocked-sites.AC2.1, AC2.7, AC3.8, AC3.9, AC3.10.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: The settings section
**Goal:** The only place a site is unblocked, and a second way to add
one.

**Components:**
- `crates/osmena-shell/src/settings_window.rs`: the "Blocked sites"
  heading between Bookmarks and Downloads; the list passed in
  read-only; the striped grid with Remove per row; the empty state;
  the add field with hint, Enter, lenient parsing through `site_for`,
  and the "Not a site" notice; the hint line.
- `crates/osmena-shell/src/app.rs`: passing the list to the window.

**Dependencies:** Phase 2, Phase 3.

**Done when:** the input parsing is a pure function with tests (bare
host, full URL, junk); Remove and Block push the right commands;
the section is seen by eye in a debug build. Covers
blocked-sites.AC4.1 to AC4.3, AC4.6.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: Nautilus
**Goal:** Chromium never requests a blocked page.

**Components:**
- `nautilus/crates/nautilus-engine/src/delegate.rs`: `on_before_browse`
  on `TabRequestHandler` (main frame only; the URL filter as a pure
  function; return 1 and push `NavigationBlocked`); the list in
  `DelegateState` behind a `RefCell`.
- `nautilus/crates/nautilus-engine/src/engine.rs`:
  `configure_blocked_sites`.
- `nautilus/crates/nautilus-engine/src/host.rs`: `replace_with_blank`
  through `execute_java_script("location.replace('about:blank')")`
  on the main frame.
- Nautilus `Cargo.toml` pin bump, `CHANGELOG.md`, `README.md` manual
  checklist item.

**Dependencies:** Phases 1 to 5 pushed to osmena.

**Done when:** the URL filter's tests pass (blocked host and
subdomain cancelled; `about:`, `data:`, `view-source:`, `chrome:`
and subframes allowed); a scripted run of the debug build on Windows
shows: block from the tab menu, confirm, the page, Back to the
previous page, a typed address blocked, a link from another page
blocked with no request in the log, settings removal bringing the
page back. Covers blocked-sites.AC3.4, AC3.5, AC5.1, AC5.2.
<!-- END_PHASE_6 -->

<!-- START_PHASE_7 -->
### Phase 7: Paladin and the record
**Goal:** Servo refuses too; every document says what was built.

**Components:**
- `paladin/crates/paladin-engine/src/delegate.rs`: `request_navigation`
  denying a blocked URL and pushing `NavigationBlocked`; the list in
  `DelegateState`.
- `paladin/crates/paladin-engine/src/engine.rs`:
  `configure_blocked_sites`; `host.rs`: `replace_with_blank` through
  `WebView::evaluate_javascript`.
- Paladin `Cargo.toml` pin bump, `CHANGELOG.md`, `README.md` checklist
  item.
- osmena `CLAUDE.md`/`AGENTS.md`: the convention paragraph and the
  stamp; `docs/prd/README.md` in the parent workspace: a row for
  Blocked Sites pointing at this design plan.

**Dependencies:** Phase 6 (the osmena revision both browsers pin).

**Done when:** Paladin builds and its tests pass against the pinned
revision; the delegate's filter has the same tests as Nautilus's;
the hook is verified by hand if a Paladin build is to hand, otherwise
recorded as not verified in its handoff; the docs are updated. Covers
blocked-sites.AC5.3, AC5.4.
<!-- END_PHASE_7 -->

## Additional Considerations

**History entries that are themselves blocked.** A cancelled history
navigation leaves Chromium's index where it was, so a blocked entry
deeper in a tab's history (a site visited earlier, blocked later from
elsewhere) cannot be stepped past with Back: the block page shows and
Back goes no further. Replacing the entry at block time
(`replace_with_blank`) keeps the common case right, where the site is
the current page; the deeper case is accepted and recorded here.
Chromium offers no way to skip an entry without letting its request
start, and the Definition of Done forbids the request.

**The engine's own pages.** The filter in each engine allows
`about:`, `data:`, `view-source:`, `blob:` and the engine's internal
schemes (`chrome:`, `devtools:`), and subframes. The model's
`site_for` returns `None` for all of these, so the two filters agree
by construction.

**Browsers outside tabs.** In Nautilus, the extension manager's hidden
browser, action popups and Chromium-opened stray browsers have no
request handler and are not filtered; a stray that becomes a tab is
filtered when the tab loads. Popups are turned into tab requests
before any hook runs; the new tab's first load is where they are
caught.

**Randomness in a pure model.** `pick_line` takes a seed; `dispatch`
and `apply` seed it from the clock as `history::unix_now` already
does. Tests pass seeds.

**Deferred tabs.** The shell routes tab actions by asking each engine
host whether it has the tab, so actions for a deferred tab are
dropped. The model only marks a deferred tab; `show_tab` creates its
engine tab at `about:blank` when it wakes. The session file does not
record `blocked`; a restored tab on a blocked site is caught by the
guard in `show_tab`.

**Guest profiles** have a list like any profile; it dies with the
directory.

**Shared bookmarks** are unaffected; a bookmark to a blocked site
opens the block page, which is the point.
