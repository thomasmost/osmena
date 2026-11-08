# Blocked Sites Implementation Plan

**Goal:** The settings window's "Blocked sites" section, the only place a site is unblocked and a second way to add one; then the osmena record (README, `CLAUDE.md`/`AGENTS.md`) and the push the browsers pin.

**Architecture:** `SettingsWindow` gains an input buffer and a notice for the section; `handle_event` and `frame` take the list read-only from the app (as they take the extensions list) and the section pushes `Command::BlockSites` and `Command::UnblockSite` into the queue the app runs through `dispatch`. The input parser is a pure function over `location_input_to_url` and `BlockedSites::site_for`. The osmena `CLAUDE.md` gains the convention paragraph and the handoff before the branch is pushed, so the revision the browsers pin carries them.

**Tech Stack:** egui 0.34.3, osmena-shell, osmena-core.

**Scope:** 7 phases from the design `docs/design-plans/2026-11-07-blocked-sites.md`. This file is Phase 5 of 7. Depends on Phases 2 and 3 (and 4 for the by-eye check).

**Codebase verified:** 2026-11-07 (osmena `1bdea01` on branch `blocked-sites`).

---

## Acceptance Criteria Coverage

This phase implements and tests:

### blocked-sites.AC4: Settings
- **blocked-sites.AC4.1 Success:** The "Blocked sites" section lists sites alphabetically; the empty state reads "No blocked sites."
- **blocked-sites.AC4.2 Success:** Remove unblocks the site and saves, with no confirmation.
- **blocked-sites.AC4.3 Success/Failure:** The add field takes a bare host or a full URL and reduces it; junk shows "Not a site" and adds nothing.
- **blocked-sites.AC4.6 Edge:** Adding a site already in the list changes nothing and saves nothing.

(AC4.1's order comes from `BTreeSet` iteration; the list, the empty state and Remove are seen by eye in Task 3 (the settings window has no drawing tests, only pure helpers). AC4.2's "saves" and AC4.6 are `dispatch`'s, tested in Phase 2; the section only issues the commands. AC4.3's parser is tested here.)

---

## Context for the engineer

All paths are under `C:\Users\tomis\vibes\browser-project\osmena\`. Branch `blocked-sites`. The gate and the commit conventions are in `phase_01.md`.

**Facts about the code as it stands (`crates/osmena-shell/src/settings_window.rs`):**
- `pub struct SettingsWindow<S: Surface>` at `:50-75` has `store_input: String`, `store_notice: Option<String>`, `commands: Vec<Command>`; `SettingsWindow::open` initialises them at `:132-134` (`store_input: String::new(), store_notice: None, commands: Vec::new(),`).
- `pub fn handle_event(&mut self, event, settings: &mut Settings, keymap: &mut Keymap, extensions: &[Extension], support: Option<&ExtensionSupport>, update: &UpdateStatus, can_update: bool, can_download: bool, default_browser: DefaultBrowser) -> SettingsOutcome` at `:191-202`, `#[allow(clippy::too_many_arguments)]`; on `RedrawRequested` (`:219-234`) it calls `self.frame(settings, keymap, extensions, support, updates, can_download, default_browser)`.
- `fn frame(&mut self, settings, keymap, extensions, support, updates: UpdatesSection<'_>, can_download, default_browser) -> FormActivity` at `:320-329`; it destructures `let Self { egui, window, capture, notice, store_input, store_notice, commands, .. } = self;` (`:332-341`), builds `extensions_section` borrowing `commands`, runs `draw_settings(ui, settings, keymap, capture, notice, extensions_section.as_mut(), updates, &mut update_commands, can_download, default_browser)` (`:349-361`), then `self.commands.extend(update_commands)` (`:365`). The Blocked section must push into `update_commands`, since `commands` is lent to the extensions section.
- `struct ExtensionsSection<'a> { extensions: &'a [Extension], support: &'a ExtensionSupport, store_input: &'a mut String, store_notice: &'a mut Option<String>, commands: &'a mut Vec<Command> }` at `:497-504`.
- `fn draw_settings(ui, settings, keymap, capture, notice, extensions: Option<&mut ExtensionsSection<'_>>, updates, update_commands: &mut Vec<Command>, can_download, default_browser) -> FormActivity` at `:518-585`, `#[allow(clippy::too_many_arguments)]`; the Bookmarks and Downloads headings are at `:552-559`:
  ```rust
                  ui.heading("Bookmarks");
                  ui.add_space(8.0);
                  draw_bookmarks(ui, settings, &mut activity, update_commands);
                  ui.add_space(20.0);
                  ui.heading("Downloads");
  ```
- `draw_extensions` at `:980-1082` is the template (striped `egui::Grid` with `num_columns`, `.spacing([16.0, 6.0])`, a fixed 240-wide first column, Remove pushing a command, a `TextEdit::singleline` with `.margin(theme::INPUT_MARGIN).desired_width(300.0).hint_text(..)`, `submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))`, the red notice through `ui.visuals().error_fg_color`, the weak empty-state label).
- `location_input_to_url` is already imported (`:8-26`: `use osmena_chrome::chrome::{self, describe_chord, location_input_to_url};`). It turns `youtube.com` into `https://youtube.com/`, `localhost:8080` into `http://localhost:8080/`, leaves a full URL as is, and returns `None` for text with whitespace or no dot.
- The tests module starts at `:1200` with `use super::*;`; `the_store_field_takes_a_store_page_url_or_a_bare_id` (`:1388-1412`) is the parser-test precedent.
- `crates/osmena-shell/src/app.rs:1770-1781` is the call: `sw.handle_event(&event, &mut self.browser.settings, &mut self.keymap, &self.browser.extensions, self.engine_info.extensions.as_ref(), &self.browser.update, self.brand.can_update(), self.engine_info.downloads, self.browser.default_browser)`.
- `README.md:232-240` is the **Page zoom** bullet under "What the user gets"; the **Find in page** bullet follows at `:241`.
- `CLAUDE.md:8-21` is the italic stamp (newest first, "before that" chain); the zoom convention bullet is `:138-149` and the find-bar bullet starts at `:150`; the newest handoff heading is `## Status and handoff (2026-11-06, evening)` at `:427`. `AGENTS.md` must stay byte-identical to `CLAUDE.md`.

---

<!-- START_SUBCOMPONENT_A (tasks 1-2) -->
<!-- START_TASK_1 -->
### Task 1: The input parser

**Verifies:** blocked-sites.AC4.3

**Files:**
- Modify: `crates/osmena-shell/src/settings_window.rs` (a new pure function beside `store_item_from_input` at `:1095-1106`; the `osmena_core::{...}` import at `:8-26` gains `BlockedSites`)
- Test: `crates/osmena-shell/src/settings_window.rs`

**Implementation:**

```rust
/// The site the "Blocked sites" field names: a bare host or a full URL,
/// reduced to its registrable domain (`www.youtube.com/watch` is
/// `youtube.com`); `None` for text that is no site.
fn site_from_input(input: &str) -> Option<String> {
    location_input_to_url(input)
        .as_ref()
        .and_then(BlockedSites::site_for)
}

/// What Enter or the Block button does with the field's text: the
/// command to issue, or the notice to show under the field.
fn blocked_field_submit(input: &str) -> Result<Command, String> {
    match site_from_input(input) {
        Some(site) => Ok(Command::BlockSites(vec![site])),
        None => Err("Not a site.".to_string()),
    }
}
```

**Testing** (in `mod tests`):

```rust
    #[test]
    fn the_blocked_sites_field_takes_a_host_or_a_url_and_refuses_junk() {
        assert_eq!(site_from_input("youtube.com").as_deref(), Some("youtube.com"));
        assert_eq!(
            site_from_input(" https://www.youtube.com/watch?v=1 ").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(site_from_input("news.bbc.co.uk").as_deref(), Some("bbc.co.uk"));
        assert_eq!(site_from_input("localhost:8080").as_deref(), Some("localhost"));
        assert_eq!(site_from_input("co.uk"), None);
        assert_eq!(site_from_input("not a site"), None);
        assert_eq!(site_from_input("about:blank"), None);
        assert_eq!(site_from_input(""), None);
    }

    #[test]
    fn the_blocked_sites_field_submits_a_block_or_a_notice() {
        assert_eq!(
            blocked_field_submit("https://www.youtube.com/"),
            Ok(Command::BlockSites(vec!["youtube.com".to_string()]))
        );
        assert_eq!(blocked_field_submit("junk text"), Err("Not a site.".to_string()));
    }
```

**Verification:**
Run: `cargo test -p osmena-shell settings_window`
Expected: pass.
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: The section, and the list handed in

**Verifies:** blocked-sites.AC4.1, blocked-sites.AC4.2 (the commands; the saving is `dispatch`'s), blocked-sites.AC4.3 (the notice)

**Files:**
- Modify: `crates/osmena-shell/src/settings_window.rs`: the struct (`:50-75`), `open` (`:132-134`), `handle_event` (`:191-234`), `frame` (`:320-366`), `draw_settings` (`:518-585`), a new `BlockedSection` and `draw_blocked_sites`
- Modify: `crates/osmena-shell/src/app.rs:1770-1781`

**Implementation:**

1. Fields. In `SettingsWindow`, after `store_notice: Option<String>,` add:
   ```rust
       /// The blocked-sites section's add field.
       block_input: String,
       /// What that field's last attempt said, if it failed.
       block_notice: Option<String>,
   ```
   In `open`, after `store_notice: None,` add `block_input: String::new(), block_notice: None,`.

2. The section's view, beside `ExtensionsSection`:
   ```rust
   /// What the blocked-sites section draws from and into.
   struct BlockedSection<'a> {
       blocked: &'a BlockedSites,
       input: &'a mut String,
       notice: &'a mut Option<String>,
   }
   ```

3. `handle_event` gains `blocked: &BlockedSites` after `support: Option<&ExtensionSupport>,`, and passes it to `self.frame(settings, keymap, extensions, support, blocked, updates, can_download, default_browser)`. Update the doc comment above it: "`blocked` is the profile's blocked sites, read-only; the section's changes are commands."

4. `frame` gains `blocked: &BlockedSites` after `support`. Add `block_input, block_notice` to the `let Self { ... } = self;` destructure, build
   ```rust
           let mut blocked_section = BlockedSection {
               blocked,
               input: block_input,
               notice: block_notice,
           };
   ```
   before `egui.run`, and pass `&mut blocked_section` to `draw_settings` after `extensions_section.as_mut()`.

5. `draw_settings` gains `blocked: &mut BlockedSection<'_>` after `extensions: Option<&mut ExtensionsSection<'_>>`. Between the Bookmarks section's `ui.add_space(20.0);` and `ui.heading("Downloads");` insert:
   ```rust
                   ui.heading("Blocked sites");
                   ui.add_space(8.0);
                   draw_blocked_sites(ui, blocked, update_commands);
                   ui.add_space(20.0);
   ```

6. The section, after `draw_extensions`:
   ```rust
   /// The sites this profile refuses to show, alphabetically, each with
   /// Remove (the only way to unblock one), and a field to add another.
   /// Both go to the model as commands; nothing here asks twice.
   fn draw_blocked_sites(
       ui: &mut egui::Ui,
       section: &mut BlockedSection<'_>,
       commands: &mut Vec<Command>,
   ) {
       let input: &mut String = section.input;
       let notice = &mut *section.notice;
       if section.blocked.sites.is_empty() {
           ui.label(egui::RichText::new("No blocked sites.").weak());
       }
       egui::Grid::new("blocked-sites-grid")
           .num_columns(2)
           .striped(true)
           .spacing([16.0, 6.0])
           .show(ui, |ui| {
               // A BTreeSet: the sites come out in alphabetical order.
               for site in &section.blocked.sites {
                   ui.vertical(|ui| {
                       ui.set_min_width(240.0);
                       ui.set_max_width(240.0);
                       ui.label(site);
                   });
                   if ui.button("Remove").clicked() {
                       commands.push(Command::UnblockSite(site.clone()));
                   }
                   ui.end_row();
               }
           });
       ui.add_space(8.0);
       ui.horizontal(|ui| {
           let field = ui.add(
               egui::TextEdit::singleline(input)
                   .margin(theme::INPUT_MARGIN)
                   .desired_width(300.0)
                   .hint_text("A site, such as youtube.com"),
           );
           let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
           if (ui.button("Block").clicked() || submitted) && !input.trim().is_empty() {
               match blocked_field_submit(input) {
                   Ok(command) => {
                       commands.push(command);
                       input.clear();
                       *notice = None;
                   }
                   Err(text) => *notice = Some(text),
               }
           }
       });
       if let Some(text) = notice.as_deref() {
           let color = ui.visuals().error_fg_color;
           ui.label(egui::RichText::new(text).small().color(color));
       }
       ui.label(
           egui::RichText::new(
               "A blocked site covers its subdomains. Open tabs on it show a block page at once; \
                this is the only place a site is unblocked.",
           )
           .small()
           .weak(),
       );
   }
   ```

7. `app.rs:1770-1781`: pass `&self.browser.blocked,` right after `self.engine_info.extensions.as_ref(),`.

**Verification:**

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

Expected: clean and green.

**Commit (Tasks 1-2):**

```bash
cargo fmt --all
git add crates/osmena-shell/src/settings_window.rs crates/osmena-shell/src/app.rs
git commit -m "feat: a Blocked sites section in the settings window" -m "The list alphabetically with Remove on each row (the only way to unblock a site, no confirmation), an add field that takes a bare host or a full URL and reduces it to the site (junk says \"Not a site.\"), and the empty state. The section reads the list and issues BlockSites and UnblockSite; the model does the rest." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_2 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_3 -->
### Task 3: See the section in a debug build

**Verifies:** blocked-sites.AC4.1, blocked-sites.AC4.2, blocked-sites.AC4.3, blocked-sites.AC4.4, blocked-sites.AC4.5, blocked-sites.AC4.6 by eye (the model's behaviour behind each is unit-tested in Phase 2).

**Files:** none changed; nothing committed in a browser.

**Steps** (PowerShell, patch table active, `CEF_PATH` set):

```
cd C:\Users\tomis\vibes\browser-project\nautilus
cargo run -- --profile blocked-probe https://example.com/
```

1. Ctrl+, opens the settings window: between Bookmarks and Downloads sits "Blocked sites" reading "No blocked sites." with the field and a Block button.
2. Type `not a site` and press Enter: the red "Not a site." appears, nothing is added. Type `https://WWW.Example.COM/x` and Enter: the row `example.com` appears, the field clears, the notice goes, and the browser window's example.com tab shows the block page at once (AC4.5).
3. Type `example.com` again and Enter: nothing changes, and the profile's `blocked.toml` modification time does not move (AC4.6).
4. Add `news.bbc.co.uk` and `127.0.0.1`: the rows read `127.0.0.1`, `bbc.co.uk`, `example.com` in that order (AC4.1).
5. Click Remove on `example.com`: the row goes without a question; the tab returns to example.com on its own (AC4.2, AC4.4); `blocked.toml` lists the other two.
6. Quit; delete the probe profile; `git checkout -- Cargo.lock` in nautilus.

Record what was seen for the handoff in Task 4.
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: The osmena record, and the push

**Verifies:** blocked-sites.AC5.4 (osmena's part: README, `CLAUDE.md`/`AGENTS.md`; the browsers' and the PRD index are Phases 6 and 7).

**Files:**
- Modify: `README.md:232-240` (a new bullet after **Page zoom**)
- Modify: `CLAUDE.md:8-21` (stamp), `:138-149` (a new convention bullet after the zoom bullet), `:427` (a new handoff section above the newest)
- Copy: `AGENTS.md` from `CLAUDE.md`

**Step 1: README, "What the user gets"**

After the **Page zoom** bullet (which ends "is the level of sites without one of their own.") insert:

```
- **Blocked sites.** A profile keeps a list of websites it refuses to
  show, keyed by registrable domain, so blocking `youtube.com` covers
  `www.youtube.com` and `m.youtube.com` (the public suffix list keeps
  `bbc.co.uk` a site and `co.uk` not; IP addresses and `localhost`
  stand whole; `about:`, `data:` and `view-source:` pages have no site).
  "Block on this profile" in a tab's menu asks first and warns that
  only the settings window unblocks; the "Blocked sites" section there
  lists the sites with Remove on each and a field that takes a host or
  a URL. A navigation to a blocked site is never requested: the model
  stops what the user types and the engine's before-navigate hook
  stops links, scripts and redirects. The tab shows a block page the
  chrome paints over `about:blank`, one of three lines on the
  profile's colour with the site named beneath; the address bar keeps
  the attempted address, Back returns to the page before, Reload
  re-rolls the line. Blocking replaces the history entry of every open
  tab on the site; unblocking sends them back. The list is
  `blocked.toml` in the profile.
```

**Step 2: `CLAUDE.md` convention**

After the zoom bullet (ending "`command::apply_default_zoom`.") and before "**The find bar is the window's state; the engine does the finding.**" insert:

```
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
  `content_is_egui` keeping input in the chrome). What the model never
  sees, links, scripts and redirects, the engine's before-navigate hook
  cancels before any request and reports as
  `EngineEvent::NavigationBlocked`; `apply` sets the same state and
  pushes `about:blank`. `UrlChanged` ignores `about:blank` while a tab
  is blocked, re-blocks a blocked URL that reached the engine without a
  hook (`ReplaceWithBlank`), and clears the block otherwise; a block
  page is no visit. `BlockSites` marks every tab on the sites (a live
  tab's entry is replaced through `TabHost::replace_with_blank`, a
  deferred one wakes on the page) and `UnblockSite`, issued only by
  the settings window, sends them back; both save the file and hand
  the list to `Engine::configure_blocked_sites`, which the shell also
  calls at start. Both engine methods are defaulted. The line is picked
  by `pick_line` from `line_seed()`, never in the chrome, so it holds
  between frames; `Reload` re-rolls it.
```

**Step 3: `CLAUDE.md` stamp**

Change the opening of the stamp from

```
_Last updated: 2026-11-06 (restored tabs warmed one by one for their
titles and favicons, tab titles centred by their capitals; before that
```

to

```
_Last updated: 2026-11-07 (blocked sites: a per-profile list keyed by
registrable domain, the guard in `dispatch`, the engine hook contract,
the block page, the settings section; before that restored tabs warmed
one by one for their titles and favicons, tab titles centred by their
capitals; before that
```

**Step 4: `CLAUDE.md` handoff**

Above `## Status and handoff (2026-11-06, evening)` insert a section `## Status and handoff (2026-11-07)` with: what was added (one paragraph pointing at the convention), what was verified (`cargo test` with the test families named: the reduction, the file, the commands' flows, the events' rules, the menu, the prompt text, the block page's sizing and drawing, the settings parser; clippy; fmt; the Nautilus walk-throughs of Phase 4 Task 5 and Phase 5 Task 3 as they went), and what was not verified (Paladin by hand; macOS; an engine's hook, which Phases 6 and 7 add; anything from the walk-throughs that was not reached). Write what actually happened, not this plan's expectation.

**Step 5: Mirror and verify**

In PowerShell:

```
Copy-Item CLAUDE.md AGENTS.md
fc.exe /b CLAUDE.md AGENTS.md
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

Expected: `fc` reports no differences; the gate clean and green. Also `grep -rn "Paladin\|Nautilus\|Servo\|Chromium" crates` must show nothing new (the branding rule; the convention text above names neither).

**Step 6: Commit and push the branch**

```bash
git add README.md CLAUDE.md AGENTS.md
git commit -m "docs: blocked sites in the README and the conventions, with the handoff" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
git push -u origin blocked-sites
git rev-parse HEAD
```

Expected: the push succeeds; note the sha printed last. It is the revision Phases 6 and 7 pin (Cargo fetches a pinned revision from GitHub even with the patch table active, so the branch must be on the remote). Merging `blocked-sites` into `main` is decided at the end of the work, not here.
<!-- END_TASK_4 -->

---

## Phase 5 done when

- The gate is clean and green; `site_from_input` is tested over a bare host, a full URL, a multi-label suffix, `localhost:port`, a bare suffix, junk, a host-less URL and empty text; `blocked_field_submit` is tested for the command and the notice.
- Remove pushes `UnblockSite(site)` (one line, read in review) and Block pushes what `blocked_field_submit` returns; the section was seen by eye in a debug build with the steps of Task 3.
- README, `CLAUDE.md` (convention, stamp, handoff) and `AGENTS.md` are updated and identical; the branch is pushed and its sha noted for the pins.
