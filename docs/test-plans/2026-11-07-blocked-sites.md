# Blocked sites: human test plan

Generated on 2026-11-08 from `docs/implementation-plans/2026-11-07-blocked-sites/test-requirements.md` against osmena `3a3029f`, nautilus `8e89821` and paladin `b22418e`.

Coverage: 32 criteria, 28 needing automated tests, all 28 covered (PASS).

## Prerequisites

- Windows, with debug builds of Nautilus and Paladin. Both build against the osmena checkout through the `[patch."https://github.com/thomasmost/osmena"]` table in `~/.cargo/config.toml` (see osmena's README).
- **Building Nautilus:** follow the local setup in `nautilus/README.md` and the Commands section of `nautilus/CLAUDE.md`: `CEF_PATH` must be set, and CMake and Ninja must be on `PATH`.
- **Building Paladin:** follow the local setup in `paladin/README.md` and the Commands section of `paladin/CLAUDE.md`. Run `cargo xtask angle-dlls debug` after building.
- **Automated suites green first:**
  - osmena: `cargo test --workspace --features osmena-engine/glutin` (395 passed).
  - nautilus: `cargo test -p nautilus-engine --lib` (39 passed).
  - paladin: `cargo test -p paladin-engine --lib` (18 passed).
- **Test page.** Save `C:\temp\blocked-link.html` with this content:

  ```html
  <a id="l" href="https://example.com/">to example.com</a>
  ```

- **Test script.** Use this as the page script (it clicks once):

  ```powershell
  $env:NAUTILUS_TEST_PAGE_SCRIPT="if(location.protocol==='file:'&&!sessionStorage.c){sessionStorage.c=1;console.log('clicking');document.getElementById('l').click();}"
  ```

- **Finding the profile directory.** Start each probe profile once with `--exit-after 3 about:blank` and read the `profile … at <dir>` log line. That `<dir>` holds `blocked.toml`.
- **Cleanup after each run.**
  - Delete the probe profile directory.
  - Remove `RUST_LOG`, `NAUTILUS_TEST_PAGE_SCRIPT` and `NAUTILUS_CEF_LOG`.
  - Run `git checkout -- Cargo.lock` in each browser.

## Phase 1/3: List file and start-up (Nautilus)

| Step | Action | Expected |
|---|---|---|
| 1.1 | `cargo run -- --profile osmena-blocked-probe --exit-after 3 about:blank` | Exits 0. Note `<dir>` from the log. |
| 1.2 | Write `sites = 3` to `<dir>\blocked.toml`. Set `$env:RUST_LOG="warn"`. Run `cargo run -- --profile osmena-blocked-probe --exit-after 8 https://example.com/`. Then check `$LASTEXITCODE`. | stderr shows `the blocked sites file is not valid` and `… starting with no blocked sites`. The window opens and example.com renders. Exit code is 0. (AC1.5) |
| 1.3 | Delete `blocked.toml` and run 1.2 again. | No warning. The browser starts normally, so a missing file means an empty list. (AC1.5) |

## Phase 4: Tab menu, prompt, block page (Nautilus)

Set `$env:RUST_LOG="info,nautilus_engine=debug"`, then run `cargo run -- --profile blocked-probe https://example.com/`.

| Step | Action | Expected |
|---|---|---|
| 4.1 | Right-click the example.com tab. | The last entries are a separator, then "Block on this profile", enabled. (AC2.1) |
| 4.2 | Press Ctrl+N, then right-click the new window's `about:blank` tab. | "Block on this profile" is greyed out. Close that window. (AC2.1) |
| 4.3 | Back on example.com, pick "Block on this profile". | A dialog titled "Block site" reads exactly "Block example.com on this profile? Are you sure? Blocked sites can only be unblocked from profile settings." It has a red "Block" button and a "Cancel" button. (AC2.7, AC2.2) |
| 4.4 | Press Escape. | The dialog closes and example.com is still shown. The tab menu still offers the entry, and `blocked.toml` does not exist or is empty. (AC2.3) |
| 4.5 | Reopen the dialog and click Cancel. | Same as 4.4. (AC2.3) |
| 4.6 | Reopen the dialog and **click** Block. Use the mouse: the osmena handoff says the click was not verified, only Enter. | The page area becomes the profile-colour gradient with one of three lines in large type in the middle, "example.com is blocked on this profile" beneath it, and a settings hint under that. The address bar reads `https://example.com/`. The tab shows the address as its title with no favicon. (AC3.8, AC2.5) |
| 4.7 | Look at `<dir>\blocked.toml`. | It contains `sites = ["example.com"]`. (AC2.4) |
| 4.8 | Click several places on the block page. Type letters. Press Ctrl+L. | Clicks and letters do nothing. Ctrl+L focuses the address bar. Nothing on the page offers to unblock. (AC3.10) |
| 4.9 | Move the pointer around for a few seconds. | The line does not change. (AC3.9) |
| 4.10 | Press F5, then click the reload button. | The line changes each time, and the log shows no new `load started` for the tab. (AC3.7) |
| 4.11 | Drag the window edge until it is about 420 px wide. | The line shrinks to fit one row and stays the same line. (AC3.8, AC3.9) |
| 4.12 | Type `example.com/other` and press Enter. | The block page appears at once. The only `load started` for the tab after this is `at about:blank`. Rerun with `$env:NAUTILUS_CEF_LOG="verbose"`: the log has no request naming example.com. (AC3.1) |
| 4.13 | Ctrl+T, type `example.org` in the Launch Panel, Enter. Then type `example.com` in that tab and press Enter. Then press Back (Alt+Left). | The block page appears. Back returns to example.org with the block page cleared. (AC3.1, AC3.6, AC3.2) |
| 4.14 | Ctrl+T and type `https://www.example.com/` into the Launch Panel. | The new tab opens directly on the block page. (AC3.2) |
| 4.15 | Quit with Ctrl+Q. | Clean exit. Clean up. |

## Phase 5: Settings section (Nautilus, fresh probe profile)

Run `cargo run -- --profile blocked-probe https://example.com/`.

| Step | Action | Expected |
|---|---|---|
| 5.1 | Press Ctrl+, to open settings. | A "Blocked sites" section sits between Bookmarks and Downloads. It reads "No blocked sites." and has a field and a Block button. (AC4.1) |
| 5.2 | Type `not a site` and press Enter. | A red "Not a site." appears and nothing is added. (AC4.3) |
| 5.3 | Clear the field, type `https://WWW.Example.COM/x` and press Enter. | A row `example.com` appears and the field and notice clear. The browser window's example.com tab switches to the block page at once. (AC4.3, AC4.5) |
| 5.4 | Note the modification time of `<dir>\blocked.toml` (`(Get-Item blocked.toml).LastWriteTime`). Type `example.com` and press Enter. | Nothing changes and the modification time stays the same. (AC4.6) |
| 5.5 | Add `news.bbc.co.uk`, then `127.0.0.1`. | The rows read `127.0.0.1`, `bbc.co.uk`, `example.com`, in that order. (AC4.1, AC4.3) |
| 5.6 | Click Remove on `example.com`. | The row goes with no confirmation. The browser tab returns to example.com and renders it. `blocked.toml` lists only `127.0.0.1` and `bbc.co.uk`. (AC4.2, AC4.4) |
| 5.7 | Quit and clean up. | |

## Phase 6: Nautilus engine hook

**6.1 Scripted link click (AC5.1, AC3.4).**

1. Create a probe profile and write `sites = ["example.com"]` to its `blocked.toml`.
2. Set `$env:RUST_LOG="info,nautilus_engine=debug"` and the page script above.
3. Run `cargo run -- --profile blocked-probe --exit-after 20 file:///C:/temp/blocked-link.html *> blocked-run.log`.
4. Filter the log: `Select-String -Path blocked-run.log -Pattern "load started|blocked navigation|clicking|quitting with"`.

Expected, in this order:

- `load started at file:///C:/temp/blocked-link.html`
- `clicking`
- `blocked navigation to https://example.com/`
- exactly one `load started at about:blank`
- no `load started at https://example.com/` anywhere
- the exit line listing the tab `at https://example.com/`

Then rerun with `$env:NAUTILUS_CEF_LOG="verbose"`: no request line names example.com.

**6.2 By hand.** Use the same profile and run `cargo run -- --profile blocked-probe https://example.org/`.

| Step | Action | Expected |
|---|---|---|
| a | Type `example.com` and press Enter, then press Back. | The block page with no `blocked navigation` line (the model caught it). Back returns to example.org. (AC3.1, AC3.6) |
| b | Type `example.net` and press Enter; it loads. Use the tab menu to Block it. Then press Back. | The block page appears. Back returns to example.org at once, and the log has no `load started at https://example.net/` after the block. (AC2.5, AC3.6) |
| c | Press Forward. | A plain blank page with `about:blank` in the address bar. This is known and accepted: record it, do not fail it. Type `example.net` and press Enter: the block page. |
| d | In settings, click Remove on `example.net`. | The tab returns to example.net. (AC4.4) |
| e | On example.org, right-click the page and pick "View page source". | A `view-source:https://example.org/` tab opens. (AC5.2) |
| f | Open the extension manager (`chrome://extensions`) from a normal tab's address bar. | It opens and is not blocked. (AC5.2) Note that the puzzle menu's "Manage extensions" opens the settings window instead, by design. |
| g | Type `https://httpbin.org/redirect-to?url=https%3A%2F%2Fexample.com%2F` and press Enter. | The block page. Record which path ran: `blocked navigation to https://example.com/ (a redirect)` (the engine cancelled), or `load started at https://example.com/` followed by the re-block. If httpbin is down, record "redirect not observed". (AC3.5) |
| h | Quit, clean up, and record the outcome in the Nautilus handoff. | |

## Phase 7: Paladin

**Build and suite.**

1. `cargo build --workspace --all-targets`
2. `cargo xtask angle-dlls debug`
3. `cargo test --workspace`

Expected: all pass, including `headless` and `web_animations`. (AC5.3)

**By hand.** Set `$env:RUST_LOG="info"` and run `cargo run -- --profile blocked-probe https://example.com/`.

| Step | Action | Expected |
|---|---|---|
| P1 | Tab menu, "Block on this profile", Block. | The block page, with the address bar on `https://example.com/`. (AC2.1, AC2.5, AC2.7, AC3.8) |
| P2 | Press F5 twice. | The line changes each time. (AC3.7) |
| P3 | Type `example.com/other` and press Enter. | The block page at once. (AC3.1) |
| P4 | In settings, click Remove on `example.com`. | The page comes back and `blocked.toml` is empty. (AC4.2, AC4.4) |
| P5 | Open `file:///C:/temp/blocked-link.html`, block `example.com` from the settings window, then click the link. | The block page, and `blocked navigation to https://example.com/` in the log. (AC5.3, AC3.4) |
| P6 | Press Back. | The file page returns. (AC3.6) |
| P7 | Quit with Ctrl+Q and delete the profile directory. | Exits within a few seconds. If Paladin does not start, record "Not verified: the deny by hand; the filter is unit-tested and the build and tests pass". |

## End-to-End: Selecting several tabs

Purpose: check by hand the several-site listing, which today only has the text test (AC2.1, AC2.2).

Steps:

1. In Nautilus, open tabs on `https://www.example.com/`, `https://example.org/` and `https://example.net/`.
2. Block example.org from settings.
3. Ctrl-click to select all three tabs and right-click.

Expected:

- The entry reads "Block 2 sites on this profile".
- Picking it shows "Block sites", "Block these sites on this profile?", an indented list of `example.com` and `example.net`, and the closing warning sentence.
- Block switches both tabs to the block page.

## End-to-End: Session restore onto a blocked site

Purpose: check that a restored tab on a blocked site comes back blocked and is not warmed (AC3.3, AC2.6).

Steps:

1. With "restore session" on, open example.org (active) and example.com (in the background) and quit.
2. Write `sites = ["example.com"]` to `blocked.toml` and restart.

Expected:

- The example.com tab has no title or favicon, and the log shows no warm load for it.
- Clicking it shows the block page with `load started at about:blank` only.

## End-to-End: Block across two windows

Purpose: check AC2.5's "every window", which is unit-tested only.

Steps:

1. Open example.com in window 1 and `https://m.example.com/` or `https://www.example.com/` in window 2 (Ctrl+N).
2. Block from window 1's tab menu.

Expected: both windows show the block page.

## Edge cases for judgement (record, do not fail)

- **about:blank fragment:** on a blocked tab, the console or an opener setting `location='about:blank#x'` clears the block page. This is a known follow-up. The block page gives way to a plain blank page and nothing from the site is requested.
- **view-source of a blocked site:** typing `view-source:https://example.com/` while example.com is blocked shows the source. This is the open design question, not a failure.

## Human Verification Required

| Criterion | Why Manual | Steps |
|---|---|---|
| AC1.5 (shell half) | `run` starts an engine and a window | 1.2, 1.3 |
| AC2.1 | native popup menus are tested only as data | 4.1, 4.2, P1 |
| AC2.2 / AC2.7 | egui dialog drawing, button colour | 4.3, multi-tab E2E |
| AC2.3 | Cancel and Escape wiring in `draw_block_sites_prompt` | 4.4, 4.5 |
| AC2.4 | the shell's SaveBlockedSites arm | 4.7 |
| AC2.5 | the engine's `location.replace` history swap | 4.6, 6.2b–c, P1, two-window E2E |
| AC3.1 | no request, and the history push | 4.12, 4.13, 6.2a, P3 |
| AC3.4 / AC5.1 / AC5.3 | the CEF and Servo hooks take engine types | 6.1, P5 |
| AC3.5 | redirect path in a live engine | 6.2g |
| AC3.6 | engine Back | 4.13, 6.2a–b, P6 |
| AC3.7–AC3.9 | gradient, size, repaint stability | 4.6, 4.9–4.11, P2 |
| AC3.10 | input routing in `content_is_egui` has no test | 4.8 |
| AC4.1 | settings section drawing and order | 5.1, 5.5 |
| AC4.2–AC4.6 | settings field, Enter, notice, file mtime | 5.2–5.6, 6.2d, P4 |
| AC5.2 | CEF calling the hook with internal schemes | 6.2e–f |
| AC5.4 | docs and CI | Run `fc.exe /b CLAUDE.md AGENTS.md` in all three repositories (no differences). Read the CHANGELOG `Unreleased` bullets, the README feature bullets, Nautilus checklist item 17, Paladin checklist item 15, Paladin's known limitation and S15, the handoffs, and row 7 of `docs/prd/README.md`. Run `gh run list --branch main` and watch each repository's run to green. |

## Traceability

| Acceptance Criterion | Automated Test | Manual Step |
|---|---|---|
| AC1.1 | `subdomains_reduce_to_the_registrable_domain`, `a_blocked_site_covers_its_subdomains_and_nothing_else` | none (5.3 incidentally) |
| AC1.2 | `multi_label_public_suffixes_hold` | 5.5 incidentally |
| AC1.3 | `addresses_and_single_labels_are_kept_whole` | 5.5 incidentally |
| AC1.4 | `host_less_urls_are_no_site` | none |
| AC1.5 | `round_trips_through_toml_and_files`, `the_blocked_sites_file_sits_in_the_profile` | 1.2, 1.3 |
| AC2.1 | `tab_menu_for_one_tab_of_three`, `tab_menu_counts_the_selection_and_disables_what_cannot_apply`, `the_block_entry_is_disabled_when_nothing_can_be_blocked`, `the_sites_to_block_are_distinct_and_not_yet_blocked` | 4.1, 4.2, P1 |
| AC2.2 | `the_block_prompt_names_the_distinct_unblocked_sites_or_nothing`, `the_block_prompt_names_one_site_and_lists_several` | 4.3, multi-tab E2E |
| AC2.3 | `the_block_prompt_answers_block_sites_on_block_and_nothing_on_cancel` | 4.4, 4.5 |
| AC2.4 | `blocking_replaces_every_live_tab_on_the_site_in_every_window`, the prompt-answer test | 4.7 |
| AC2.5 | `blocking_replaces_every_live_tab_on_the_site_in_every_window`, `the_blank_page_behind_a_block_page_leaves_the_tab_alone` | 4.6, 6.2b–c, P1, two-window E2E |
| AC2.6 | `blocking_a_deferred_tab_only_marks_it`, `a_restored_tab_on_a_blocked_site_wakes_on_the_block_page_and_is_not_warmed` | session-restore E2E |
| AC2.7 | `the_block_prompt_names_one_site_and_lists_several` | 4.3 |
| AC3.1 | `a_typed_address_on_a_blocked_site_shows_the_block_page_over_blank` | 4.12, 4.13, 6.2a, P3 |
| AC3.2 | `a_new_tab_a_search_and_a_question_on_a_blocked_site_are_blocked` | 4.13, 4.14 |
| AC3.3 | `a_restored_tab_on_a_blocked_site_wakes_on_the_block_page_and_is_not_warmed` (+3 related tests) | session-restore E2E |
| AC3.4 | `a_navigation_the_engine_blocked_puts_the_tab_on_the_block_page`, `a_navigation_blocked_for_a_site_no_longer_blocked_is_loaded_after_all`, Nautilus `a_main_frame_load_of_a_blocked_site_or_its_subdomain_is_cancelled`, Paladin `a_load_of_a_blocked_site_or_its_subdomain_is_denied` | 6.1, P5 |
| AC3.5 | `a_url_change_onto_a_blocked_site_is_blocked_again`, the engine-blocked test, the Nautilus filter | 6.2g |
| AC3.6 | `going_back_from_a_block_page_clears_it` | 4.13, 6.2a–b, P6 |
| AC3.7 | `reload_on_a_block_page_rerolls_the_line_and_asks_the_engine_nothing`, `seeds_land_on_every_line_and_a_reroll_always_moves` | 4.10, P2 |
| AC3.8 | `seeds_land_…`, `a_block_line_shrinks_to_its_width_and_no_further`, `the_block_page_draws_narrow_and_wide_without_changing_its_line`, the typed-address and blank-page tests | 4.6, 4.11, P1 |
| AC3.9 | `the_block_page_draws_narrow_and_wide_without_changing_its_line` | 4.9, 4.11 |
| AC3.10 | none (by design) | 4.8 |
| AC4.1 | none (by design) | 5.1, 5.5 |
| AC4.2 | `unblocking_sends_every_tab_blocked_on_the_site_back_to_its_page` | 5.6, P4 |
| AC4.3 | `the_blocked_sites_field_takes_a_host_or_a_url_and_refuses_junk`, `the_blocked_sites_field_submits_a_block_or_a_notice` | 5.2, 5.3 |
| AC4.4 | `unblocking_sends_every_tab_blocked_on_the_site_back_to_its_page`, the restored-tab test | 5.6, 6.2d, P4 |
| AC4.5 | `blocking_replaces_every_live_tab_on_the_site_in_every_window`, `the_blocked_sites_field_submits_a_block_or_a_notice` | 5.3 |
| AC4.6 | `blocking_a_site_already_blocked_changes_nothing`, `a_blocked_site_covers_its_subdomains_and_nothing_else` | 5.4 |
| AC5.1 | scripted run (no unit test by definition) | 6.1 |
| AC5.2 | Nautilus `subframes_and_pages_without_a_site_pass`, `host_less_urls_are_no_site` | 6.2e, 6.2f |
| AC5.3 | Paladin `a_load_of_a_blocked_site_or_its_subdomain_is_denied`, `pages_without_a_site_pass`, the engine-blocked fold, the Paladin workspace suite | Paladin build and suite, P1–P7 |
| AC5.4 | `fc.exe /b` and CI runs | Human Verification table, AC5.4 row |
