# Blocked Sites Test Requirements

What each acceptance criterion of the Blocked Sites design is checked by:
the automated tests the phase files name, and the by-hand steps for what
no test in these repositories can see. It is the list the test analyst
checks the finished work against.

- **Date:** 2026-11-07
- **Design:** `osmena/docs/design-plans/2026-11-07-blocked-sites.md`
- **Plan:** `osmena/docs/implementation-plans/2026-11-07-blocked-sites/phase_01.md` … `phase_07.md`

Test types:

- **unit**: an inline `#[cfg(test)] mod tests` function, run by `cargo test` in that repository and in its CI.
- **integration**: none. No phase adds a test that crosses crates or drives a window; the closest is the egui test context in `the_block_page_draws_narrow_and_wide_without_changing_its_line`, which is a unit test of one drawing function.
- **scripted run**: a debug browser started from PowerShell with fixed arguments, environment and, where needed, a page script; the commands are in the phase file named, the outcome is read off the log. A tester runs it on Windows; it is not in CI.

Paths are repository-relative, prefixed by the repository (`osmena/`, `nautilus/`, `paladin/`). Test names are the phase files' own.

## Decisions taken in planning that this mapping follows

- **The engine contract moved from Phase 3 to Phase 2.** `Engine::configure_blocked_sites`, `TabHost::replace_with_blank` (both defaulted), the shell's `perform` arms for the three new actions and the chrome's exhaustive `Prompt` matches are Phase 2, Tasks 6 and 7, so the workspace builds at the end of that phase. Phase 3 is left with loading `blocked.toml` at start, configuring the engine at start, the osmena README and CHANGELOG, and the build against both browsers. Neither Task 6 nor Task 7 of Phase 2 verifies a criterion of its own.
- **The osmena `CLAUDE.md`/`AGENTS.md` convention paragraph is Phase 5, Task 4**, not Phase 7 as the design has it, so that the revision the browsers pin carries it. Phase 7, Task 5 only appends the browsers' adoption to the osmena handoff.
- **Paladin denies and reports every navigation to a blocked site, iframes included.** Servo 0.7.0's `NavigationRequest` does not say which frame is navigating (recorded as S15 in `paladin/docs/upstream-issues.md`). Paladin's filter `blocked_navigation(sites, url)` therefore has no main-frame argument. AC5.2's "subframe navigations pass the hook" applies to Nautilus only. Paladin's AC3.4 and AC3.5 coverage is the unit-tested filter, the model's `NavigationBlocked` and `UrlChanged` tests, and the by-hand check of Phase 7, Task 3.
- **Servo's hook does not see redirects, a popup's first page, or Back and Forward.** For Paladin, AC3.5 is met by the model's re-block on `UrlChanged` (the second half of the criterion), after the redirected page has been requested once; the README's known limitations and S15 say so.
- **Forward onto the replaced blank entry in Nautilus shows a plain blank page** with `about:blank` in the address bar. Known and accepted (Phase 6, Task 3, Step 2.2); the tester records it in the handoff, not as a failure of AC2.5 or AC3.6.
- **The `psl` behaviour the AC1 tests assert was established by a probe** (`psl` 2.1.248 on Rust 1.97.1, recorded in `phase_01.md`'s context): it does not lowercase, gives no domain for a bare known suffix (`co.uk`) or a single unknown label (`localhost`), and mis-parses IP literals, so `site_for` lowercases first and keeps IPs whole through `url::Host` before asking `psl`. The expected values in the AC1 tests rest on that probe.

---

## blocked-sites.AC1: The list and the matching

### AC1.1

> **blocked-sites.AC1.1 Success:** `www.`, `m.` and deeper subdomains of a site reduce to one entry (`youtube.com`).

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `subdomains_reduce_to_the_registrable_domain` | Phase 1, Task 2 |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `a_blocked_site_covers_its_subdomains_and_nothing_else` | Phase 1, Task 2 |

Human verification: none needed.

### AC1.2

> **blocked-sites.AC1.2 Success:** Multi-label public suffixes hold: `bbc.co.uk` stays `bbc.co.uk`, never `co.uk`.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `multi_label_public_suffixes_hold` | Phase 1, Task 2 |

Human verification: none needed.

### AC1.3

> **blocked-sites.AC1.3 Edge:** IPv4, IPv6 and `localhost` hosts are kept whole.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `addresses_and_single_labels_are_kept_whole` | Phase 1, Task 2 |

Human verification: none needed.

### AC1.4

> **blocked-sites.AC1.4 Failure:** `about:`, `data:`, `view-source:`, `file:` and other host-less URLs reduce to nothing and are never blockable.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `host_less_urls_are_no_site` | Phase 1, Task 2 |

Human verification: none needed.

### AC1.5

> **blocked-sites.AC1.5 Success:** `blocked.toml` round-trips; a missing file is an empty list; an unreadable file logs a warning, starts empty, and the browser still starts.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `round_trips_through_toml_and_files` (round trip, missing file is empty, `sites = 3` is `Err(BlockedError::Parse(_))`) | Phase 1, Task 3 |
| unit | `osmena/crates/osmena-core/src/profile.rs` | `the_blocked_sites_file_sits_in_the_profile` | Phase 1, Task 3 |
| scripted run | none (commands in `phase_03.md`) | Nautilus with `--profile osmena-blocked-probe --exit-after 8` over a `blocked.toml` holding `sites = 3`, `RUST_LOG=warn` | Phase 3, Task 4, Step 2 |

The shell's "logs a warning and starts empty" (`run` in `osmena-shell/src/lib.rs`, Phase 3, Task 1) has no unit test: `run` starts an engine and a window. The scripted run covers it; the tester sees `the blocked sites file is not valid` and `starting with no blocked sites` on stderr, the window opens, the page loads, and the process exits 0.

---

## blocked-sites.AC2: Blocking from the tab menu

### AC2.1

> **blocked-sites.AC2.1 Success:** The tab menu shows "Block on this profile" (or "Block {n} sites on this profile"), enabled only when a target has a blockable site not yet blocked.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-chrome/src/menus.rs` | `tab_menu_for_one_tab_of_three` (updated: label, enabled, `PromptBlockSites` command) | Phase 4, Task 1 |
| unit | `osmena/crates/osmena-chrome/src/menus.rs` | `tab_menu_counts_the_selection_and_disables_what_cannot_apply` (updated: "Block 3 sites on this profile") | Phase 4, Task 1 |
| unit | `osmena/crates/osmena-chrome/src/menus.rs` | `the_block_entry_is_disabled_when_nothing_can_be_blocked` | Phase 4, Task 1 |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `the_sites_to_block_are_distinct_and_not_yet_blocked` (the count the label uses) | Phase 1, Task 2 |

Human verification: the native popup that shows the entries is not exercised by tests (menus are tested as data only). Phase 4, Task 5, Step 1: the tester right-clicks the example.com tab and sees a separator and "Block on this profile", enabled; on a new window's `about:blank` tab the entry is disabled.

### AC2.2

> **blocked-sites.AC2.2 Success:** The prompt lists the targets' distinct sites, skipping already-blocked ones; with nothing to ask, no prompt is shown.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `the_block_prompt_names_the_distinct_unblocked_sites_or_nothing` | Phase 2, Task 3 |
| unit | `osmena/crates/osmena-chrome/src/chrome.rs` | `the_block_prompt_names_one_site_and_lists_several` (the listing of several) | Phase 4, Task 2 |

Human verification: the drawn dialog has no test (egui drawing has no input-driving tests in this codebase). Phase 4, Task 5, Step 2 shows the one-site prompt naming example.com. The several-sites listing is not walked by hand; it rests on the text test.

### AC2.3

> **blocked-sites.AC2.3 Failure:** Cancel or Escape adds nothing.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-chrome/src/chrome.rs` | `the_block_prompt_answers_block_sites_on_block_and_nothing_on_cancel` | Phase 4, Task 2 |

The model's half (Phase 2, Task 3) has no test of its own: Cancel and Escape issue no command, so `dispatch` is never reached and the list cannot change. The chrome test checks that the cancelled answer is an empty command list.

Human verification: that the Cancel button and the Escape key set `cancelled` is in `draw_block_sites_prompt`, which no test drives. Phase 4, Task 5, Step 2: Escape closes the dialog with nothing changed; reopened, Cancel closes it with nothing changed (the page still shows, the tab menu still offers the entry).

### AC2.4

> **blocked-sites.AC2.4 Success:** Confirming adds the sites and saves the file.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `blocking_replaces_every_live_tab_on_the_site_in_every_window` (the site is added; `SaveBlockedSites` and `ConfigureBlockedSites` are emitted) | Phase 2, Task 3 |
| unit | `osmena/crates/osmena-chrome/src/chrome.rs` | `the_block_prompt_answers_block_sites_on_block_and_nothing_on_cancel` (Block answers `BlockSites(sites)`) | Phase 4, Task 2 |

The shell's `SaveBlockedSites` arm (Phase 2, Task 7) writes the file through `BlockedSites::save`, whose write is covered by `round_trips_through_toml_and_files`; the arm itself has no test.

Human verification: Phase 4, Task 5, Step 6: after Block, the profile's `blocked.toml` holds `sites = ["example.com"]`.

### AC2.5

> **blocked-sites.AC2.5 Success:** Every live tab on the site, in every window of the profile, shows the block page, with its history entry replaced so Back returns to the previous page.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `blocking_replaces_every_live_tab_on_the_site_in_every_window` (two windows, `ReplaceWithBlank` for each live tab on the site, the other site untouched) | Phase 2, Task 3 |
| unit | `osmena/crates/osmena-core/src/event.rs` | `the_blank_page_behind_a_block_page_leaves_the_tab_alone` (the replaced blank page does not clear the block) | Phase 2, Task 5 |

Human verification: the entry replacement is the engine's (`location.replace('about:blank')` in each browser's `replace_with_blank`), which needs a running browser. Phase 6, Task 3, Step 2.2 (Nautilus): load example.net, block it from the tab menu, see the block page; Back returns to example.org at once and the log has no `load started at https://example.net/` after the block. Forward then shows a plain blank page with `about:blank` in the address bar: known and accepted, recorded in the handoff. Phase 7, Task 3, Step 1 (Paladin): the tab-menu block shows the block page with the address bar on `https://example.com/`; Back after a tab-menu block is not walked on Paladin. "Every window" is covered by the unit test only.

### AC2.6

> **blocked-sites.AC2.6 Edge:** A deferred tab on the site is marked and shows the block page when activated.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `blocking_a_deferred_tab_only_marks_it` (marked, no `ReplaceWithBlank`; a warming one gets `CloseTab`) | Phase 2, Task 3 |
| unit | `osmena/crates/osmena-core/src/command.rs` | `a_restored_tab_on_a_blocked_site_wakes_on_the_block_page_and_is_not_warmed` (`ActivateTab` creates it at `about:blank`, still blocked) | Phase 2, Task 2 |

Human verification: none in the walk-throughs; the deferred-tab paths are model state and actions, which the unit tests cover.

### AC2.7

> **blocked-sites.AC2.7 Success:** The dialog reads "Block <site> on this profile? Are you sure? Blocked sites can only be unblocked from profile settings." with buttons Block (danger colour) and Cancel.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-chrome/src/chrome.rs` | `the_block_prompt_names_one_site_and_lists_several` (title "Block site" and the exact sentence) | Phase 4, Task 2 |

Human verification: the buttons and their colour are drawn by egui, which has no drawing assertions in this codebase. Phase 4, Task 5, Step 2: the dialog titled "Block site" reads "Block example.com on this profile? Are you sure? Blocked sites can only be unblocked from profile settings." with a red "Block" and a "Cancel". Seen again in Nautilus's checklist item 17 and Paladin's item 14.

---

## blocked-sites.AC3: Navigation and the block page

### AC3.1

> **blocked-sites.AC3.1 Success:** A typed address on a blocked site shows the block page with no request sent; the previous page is behind it in history.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `a_typed_address_on_a_blocked_site_shows_the_block_page_over_blank` (`Navigate` to `about:blank`, the URL kept, the block set, title and favicon cleared) | Phase 2, Task 2 |

The unit test shows the engine is asked for `about:blank` and nothing else; that the engine sends no request for it, and that the blank page is a push behind which the previous page stays, need a running browser.

Human verification: Phase 4, Task 5, Step 5 (Nautilus): type `example.com/other`, see the block page at once, one `load started` for the tab after the block, and with `NAUTILUS_CEF_LOG=verbose` no request naming example.com; on a second tab, navigate to example.com, then Back returns to the other site. Phase 6, Task 3, Step 2.1 (Nautilus, with the hook in): type `example.com`, the block page; Back returns to example.org. Paladin: the typed-address step of checklist item 14 (Phase 7, Task 2).

### AC3.2

> **blocked-sites.AC3.2 Success:** A new tab, a Launch Panel answer or a search landing on a blocked site shows the block page.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `a_new_tab_a_search_and_a_question_on_a_blocked_site_are_blocked` (`NewTab`, `Search`, `SearchInNewTab`, `AskInNewTab`, `NewBackgroundTab`, `OpenInNewWindow`) | Phase 2, Task 2 |

Human verification: none in the walk-throughs. The Launch Panel's answers are the `NewTab { url: Some(..) }`, `SearchInNewTab` and `AskInNewTab` commands the test dispatches, and the block page they lead to is the same paint arm the other steps see.

### AC3.3

> **blocked-sites.AC3.3 Success:** A session-restored tab on a blocked site shows the block page when shown.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `a_restored_tab_on_a_blocked_site_wakes_on_the_block_page_and_is_not_warmed` (active and deferred cases, the warm list, the wake re-check after an unblock) | Phase 2, Task 2 |

Human verification: none in the walk-throughs.

### AC3.4

> **blocked-sites.AC3.4 Success:** A link click or script navigation to a blocked site is cancelled by the engine before any request; the tab shows the block page.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/event.rs` | `a_navigation_the_engine_blocked_puts_the_tab_on_the_block_page` | Phase 2, Task 4 |
| unit | `osmena/crates/osmena-core/src/event.rs` | `a_navigation_blocked_for_a_site_no_longer_blocked_is_loaded_after_all` | Phase 2, Task 4 |
| unit | `nautilus/crates/nautilus-engine/src/delegate.rs` | `a_main_frame_load_of_a_blocked_site_or_its_subdomain_is_cancelled` | Phase 6, Task 1 |
| unit | `paladin/crates/paladin-engine/src/delegate.rs` | `a_load_of_a_blocked_site_or_its_subdomain_is_denied` | Phase 7, Task 1 |
| scripted run | none (commands in `phase_06.md`) | Nautilus on a `file:///` page whose `NAUTILUS_TEST_PAGE_SCRIPT` clicks a link to example.com | Phase 6, Task 3, Step 1 |

The hooks themselves (`on_before_browse`, `request_navigation`) take CEF and Servo types that cannot be built in a test, so the tests cover the pure filters and the model's fold of the event; the hook's cancel is seen in a running browser.

Human verification: Phase 6, Task 3, Step 1 (Nautilus): the log reads, in order, `load started at file:///…blocked-link.html`, `clicking`, `blocked navigation to https://example.com/`, one `load started at about:blank`, and no `load started at https://example.com/`. Phase 7, Task 3, Step 3 (Paladin): with example.com blocked from the settings window, click the link on the `file:///` page; the block page shows and the log has `blocked navigation to https://example.com/`. If Paladin does not start, the handoff records the deny as not verified by hand.

### AC3.5

> **blocked-sites.AC3.5 Edge:** A redirect onto a blocked site is cancelled by the engine, or re-blocked by the model if the engine let it through.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/event.rs` | `a_url_change_onto_a_blocked_site_is_blocked_again` (`UrlChanged` onto a blocked site answers `ReplaceWithBlank`) | Phase 2, Task 5 |
| unit | `osmena/crates/osmena-core/src/event.rs` | `a_navigation_the_engine_blocked_puts_the_tab_on_the_block_page` (the engine-cancel path) | Phase 2, Task 4 |
| unit | `nautilus/crates/nautilus-engine/src/delegate.rs` | `a_main_frame_load_of_a_blocked_site_or_its_subdomain_is_cancelled` (the filter `on_before_browse` asks, redirects included) | Phase 6, Task 1 |

Paladin's redirect path is the model's re-block only: Servo's `request_navigation` does not fire for HTTP redirects (S15), so `a_url_change_onto_a_blocked_site_is_blocked_again` is its coverage, and the redirected page is requested once before the block page replaces it (recorded in Paladin's README known limitations).

Human verification: Phase 6, Task 3, Step 2.5 (Nautilus): type `https://httpbin.org/redirect-to?url=https%3A%2F%2Fexample.com%2F`; the block page shows, and the tester records which path was taken: `blocked navigation to https://example.com/ (a redirect)` in the log (the engine cancelled), or `load started at https://example.com/` followed by the model's re-block. If httpbin is down, the handoff says the redirect was not observed. Paladin has no redirect step in Phase 7, Task 3.

### AC3.6

> **blocked-sites.AC3.6 Success:** Back from a block page returns to the previous page and the block page clears.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/event.rs` | `going_back_from_a_block_page_clears_it` | Phase 2, Task 5 |

Back is the engine's history step; the test covers the model's side (the `UrlChanged` that follows clears `blocked`).

Human verification: Phase 4, Task 5, Step 5 (Nautilus, second tab: Back returns to the other site and the page clears); Phase 6, Task 3, Steps 2.1 and 2.2 (Nautilus, after a typed address and after a tab-menu block with the real `replace_with_blank`; Forward onto the blank entry is the known, accepted plain blank page); Phase 7, Task 3, Step 4 (Paladin, Back after the denied link returns to the file page).

### AC3.7

> **blocked-sites.AC3.7 Success:** Reload on a block page re-rolls the line and sends the engine nothing.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `reload_on_a_block_page_rerolls_the_line_and_asks_the_engine_nothing` | Phase 2, Task 2 |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `seeds_land_on_every_line_and_a_reroll_always_moves` | Phase 1, Task 4 |

Human verification: Phase 4, Task 5, Step 4: F5 or the reload button changes the line, and again.

### AC3.8

> **blocked-sites.AC3.8 Success:** The page shows one of the three lines, large, on the profile-colour gradient, with the site named beneath; the address bar shows the attempted URL.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `seeds_land_on_every_line_and_a_reroll_always_moves` (every line is reachable; `text()` is the line) | Phase 1, Task 4 |
| unit | `osmena/crates/osmena-chrome/src/chrome.rs` | `a_block_line_shrinks_to_its_width_and_no_further` (the sizing rule) | Phase 4, Task 3 |
| unit | `osmena/crates/osmena-chrome/src/chrome.rs` | `the_block_page_draws_narrow_and_wide_without_changing_its_line` (draws at 320×200 and 1600×900 without panicking) | Phase 4, Task 3 |
| unit | `osmena/crates/osmena-core/src/command.rs` | `a_typed_address_on_a_blocked_site_shows_the_block_page_over_blank` (`Tab::url` is the attempted URL) | Phase 2, Task 2 |
| unit | `osmena/crates/osmena-core/src/event.rs` | `the_blank_page_behind_a_block_page_leaves_the_tab_alone` (`UrlChanged` to `about:blank` leaves `Tab::url`) | Phase 2, Task 5 |

Human verification: the gradient, the size, the placement and the address bar are drawn by egui, which no test here inspects. Phase 4, Task 5, Step 3: the page area is the profile colour's gradient with one of the three lines large in the middle, "example.com is blocked on this profile" beneath, the settings hint under that; the address bar reads `https://example.com/`; the strip shows the address with no favicon. Step 4: narrowed, the line shrinks to one row.

### AC3.9

> **blocked-sites.AC3.9 Success:** The line does not change between frames.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-chrome/src/chrome.rs` | `the_block_page_draws_narrow_and_wide_without_changing_its_line` (each page drawn twice, its `line` unchanged) | Phase 4, Task 3 |

The line is `Tab::blocked.line`, model state that only `Reload` changes; the chrome takes the page by shared reference.

Human verification: Phase 4, Task 5, Steps 3 and 4: the line holds while the window repaints (pointer moves, a resize) and changes only on Reload.

### AC3.10

> **blocked-sites.AC3.10 Failure:** No control on the page unblocks; pointer and key input stay in the chrome while it shows.

No automated test. `draw_blocked_page` adds no widgets (read in review), and the input routing is `BrowserWindow::content_is_egui` in `osmena-shell/src/window.rs` (Phase 4, Task 4), which no test drives: egui drawing and the window's pointer routing have no input-driving tests in this codebase.

Human verification: Phase 4, Task 5, Step 3: clicking anywhere on the block page does nothing; keys go to the chrome (Ctrl+L focuses the address bar). The only unblock is the settings window (Phase 5, Task 3, Step 5).

---

## blocked-sites.AC4: Settings

### AC4.1

> **blocked-sites.AC4.1 Success:** The "Blocked sites" section lists sites alphabetically; the empty state reads "No blocked sites."

No automated test. The order is the iteration order of `BlockedSites::sites`, a `BTreeSet`; the section is drawn by `draw_blocked_sites` in `osmena-shell/src/settings_window.rs`, and the settings window has no drawing tests, only pure helpers.

Human verification: Phase 5, Task 3, Step 1: between Bookmarks and Downloads sits "Blocked sites" reading "No blocked sites." with the field and a Block button. Step 4: after adding `news.bbc.co.uk` and `127.0.0.1`, the rows read `127.0.0.1`, `bbc.co.uk`, `example.com`, in that order.

### AC4.2

> **blocked-sites.AC4.2 Success:** Remove unblocks the site and saves, with no confirmation.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `unblocking_sends_every_tab_blocked_on_the_site_back_to_its_page` (`UnblockSite` removes the site and emits `SaveBlockedSites`) | Phase 2, Task 3 |

That the Remove button pushes `Command::UnblockSite(site)` with no prompt in between is one line in `draw_blocked_sites`, read in review (Phase 5, "done when").

Human verification: Phase 5, Task 3, Step 5: Remove on `example.com` removes the row without a question, and `blocked.toml` lists the other two.

### AC4.3

> **blocked-sites.AC4.3 Success/Failure:** The add field takes a bare host or a full URL and reduces it; junk shows "Not a site" and adds nothing.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-shell/src/settings_window.rs` | `the_blocked_sites_field_takes_a_host_or_a_url_and_refuses_junk` | Phase 5, Task 1 |
| unit | `osmena/crates/osmena-shell/src/settings_window.rs` | `the_blocked_sites_field_submits_a_block_or_a_notice` | Phase 5, Task 1 |

Human verification: the field, Enter and the red notice are drawn by egui. Phase 5, Task 3, Step 2: `not a site` and Enter shows the red "Not a site." and adds nothing; `https://WWW.Example.COM/x` and Enter adds the row `example.com`, clears the field and the notice.

### AC4.4

> **blocked-sites.AC4.4 Success:** Unblocking brings every tab blocked on that site back to its own URL.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `unblocking_sends_every_tab_blocked_on_the_site_back_to_its_page` (live tabs get `Navigate` to their own URL, a deferred one loses the mark only) | Phase 2, Task 3 |
| unit | `osmena/crates/osmena-core/src/command.rs` | `a_restored_tab_on_a_blocked_site_wakes_on_the_block_page_and_is_not_warmed` (a deferred tab unblocked before it wakes is created at its own URL) | Phase 2, Task 2 |

Human verification: Phase 5, Task 3, Step 5 (the tab returns to example.com on its own); Phase 6, Task 3, Step 2.2 (Nautilus with the hook in: Remove `example.net`, the tab returns to it); Phase 7, Task 3, Step 2 (Paladin: Remove `example.com`, the page comes back).

### AC4.5

> **blocked-sites.AC4.5 Success:** Adding from settings blocks open tabs on that site at once.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `blocking_replaces_every_live_tab_on_the_site_in_every_window` | Phase 2, Task 3 |
| unit | `osmena/crates/osmena-shell/src/settings_window.rs` | `the_blocked_sites_field_submits_a_block_or_a_notice` (the field issues `BlockSites`, the same command the tab menu's prompt answers) | Phase 5, Task 1 |

Human verification: Phase 5, Task 3, Step 2: after adding `https://WWW.Example.COM/x`, the browser window's example.com tab shows the block page at once.

### AC4.6

> **blocked-sites.AC4.6 Edge:** Adding a site already in the list changes nothing and saves nothing.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `osmena/crates/osmena-core/src/command.rs` | `blocking_a_site_already_blocked_changes_nothing` (`BlockSites` of a blocked site returns no actions, so no `SaveBlockedSites`) | Phase 2, Task 3 |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `a_blocked_site_covers_its_subdomains_and_nothing_else` (`add` of a present site reports no change) | Phase 1, Task 2 |

Human verification: Phase 5, Task 3, Step 3: `example.com` again and Enter changes nothing, and the modification time of the profile's `blocked.toml` does not move.

---

## blocked-sites.AC5: Both browsers

### AC5.1

> **blocked-sites.AC5.1 Success:** Nautilus: a scripted run shows a blocked link produces no network request in the log.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| scripted run | none (commands in `phase_06.md`) | Nautilus, `--profile blocked-probe --exit-after 20`, `RUST_LOG=info,nautilus_engine=debug`, `NAUTILUS_TEST_PAGE_SCRIPT` clicking a link to example.com on a `file:///` page; log filtered with `Select-String "load started\|blocked navigation\|clicking\|quitting with"` | Phase 6, Task 3, Step 1 |

The criterion is a scripted run by definition; it is not in CI, since CI has no display and no network page to load. The tester runs it and reads the filtered log: `blocked navigation to https://example.com/`, exactly one `load started at about:blank` after it, no `load started at https://example.com/`, and the `--exit-after` line listing the tab `at https://example.com/`. A second `clicking` and a null-element console error after the blank page loads are expected noise and must not be followed by another `blocked navigation`. A second run with `NAUTILUS_CEF_LOG=verbose` has no request line naming example.com. The `load started at <url>` wording comes from Phase 6, Task 1, item 5.

### AC5.2

> **blocked-sites.AC5.2 Success:** Nautilus: `about:`, `data:`, `view-source:`, `chrome:`, `devtools:` and subframe navigations pass the hook.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `nautilus/crates/nautilus-engine/src/delegate.rs` | `subframes_and_pages_without_a_site_pass` (a subframe of a blocked site, `about:`, `data:`, `view-source:`, `chrome:`, `devtools:`, `chrome-extension:`, a non-URL) | Phase 6, Task 1 |
| unit | `osmena/crates/osmena-core/src/blocked.rs` | `host_less_urls_are_no_site` (the model's `site_for` gives nothing for the same schemes, so the two filters agree) | Phase 1, Task 2 |

Nautilus only. Paladin denies navigations to a blocked site in any frame (S15), so its filter has no subframe case; its `pages_without_a_site_pass` covers the scheme half for Paladin under AC5.3.

Human verification: that CEF calls the hook with these URLs and the hook lets them through needs a running Nautilus. Phase 6, Task 3, Step 2.3: "View page source" opens a `view-source:https://example.org/` tab. Step 2.4: the extension manager (`chrome://extensions`) opens. `about:blank` passing is seen in every block (the blank page behind the block page loads). `data:`, `devtools:` and a real subframe are covered by the filter's tests only.

### AC5.3

> **blocked-sites.AC5.3 Success:** Paladin: `request_navigation` denies a blocked URL and reports it; Paladin builds and its tests pass against the pinned osmena.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| unit | `paladin/crates/paladin-engine/src/delegate.rs` | `a_load_of_a_blocked_site_or_its_subdomain_is_denied` | Phase 7, Task 1 |
| unit | `paladin/crates/paladin-engine/src/delegate.rs` | `pages_without_a_site_pass` (`about:`, `data:`, `view-source:`, `file:`) | Phase 7, Task 1 |
| unit | `osmena/crates/osmena-core/src/event.rs` | `a_navigation_the_engine_blocked_puts_the_tab_on_the_block_page` (what the reported event does) | Phase 2, Task 4 |
| build and suite | `paladin` workspace | `cargo build --workspace --all-targets`, `cargo xtask angle-dlls debug`, `cargo test --workspace` (with `headless` and `web_animations`) against the checkout, then against the pinned revision | Phase 7, Tasks 1 and 4; CI in Task 6 |

`request_navigation` takes a `WebView` and a `NavigationRequest`, neither of which can be built in a test, so the deny and the report are seen by hand. Phase 3, Task 4, Step 3 type-checks Paladin against the defaulted contract before any Paladin change.

Human verification: Phase 7, Task 3, Steps 1 to 4 (block from the tab menu, Remove from settings, the denied link with `blocked navigation to https://example.com/` in the log, Back). If Paladin does not start on the machine, the handoff records "Not verified: the deny by hand; the filter is unit-tested and the build and tests pass", which the design allows. The iframe case (a frame pointing at a blocked site takes the whole tab to the block page, S15) is not walked by hand.

### AC5.4

> **blocked-sites.AC5.4 Success:** CHANGELOGs, READMEs (manual checklist), osmena `CLAUDE.md`/`AGENTS.md` and the PRD index are updated; CI is green in all three repositories.

| Type | File | Test | Phase/Task |
| ---- | ---- | ---- | ---------- |
| scripted run | `osmena/CLAUDE.md`, `osmena/AGENTS.md` | `fc.exe /b CLAUDE.md AGENTS.md` reports no differences | Phase 5, Task 4, Step 5; Phase 7, Task 5, Step 2 |
| scripted run | `nautilus/CLAUDE.md`, `nautilus/AGENTS.md` | `fc.exe /b CLAUDE.md AGENTS.md` | Phase 6, Task 2, Step 4 |
| scripted run | `paladin/CLAUDE.md`, `paladin/AGENTS.md` | `fc.exe /b CLAUDE.md AGENTS.md` | Phase 7, Task 2, Step 4 |
| CI | all three repositories | `gh run list --branch main --commit <HEAD>` then `gh run watch <id>` to completion | Phase 7, Task 6, Step 3 |

Human verification: the content of the documents is read, not tested; CI does not check READMEs. The tester checks each was written as the phase says:

- osmena: `CHANGELOG.md` `Unreleased`/`Added` bullet and `README.md` engine-contract text (Phase 3, Task 3); `README.md` "Blocked sites" bullet under "What the user gets", `CLAUDE.md` convention paragraph after the zoom bullet, stamp dated 2026-11-07, handoff section `## Status and handoff (2026-11-07)` (Phase 5, Task 4); the adoption sentence in that handoff (Phase 7, Task 5). osmena has no manual checklist; the checklists are the browsers'.
- Nautilus: `CHANGELOG.md` `### Added` bullet, `README.md` feature bullet and checklist item 17 with the later items renumbered, `CLAUDE.md` stamp and handoff with what the scripted run showed (Phase 6, Task 2).
- Paladin: `CHANGELOG.md` bullet, `README.md` feature bullet, checklist item 14, the known-limitation line, `docs/upstream-issues.md` S15, `CLAUDE.md` stamp and "Added 2026-11-07" paragraph (Phase 7, Task 2).
- Parent workspace: row 7 in `docs/prd/README.md`, "content blocking" left in "Next in line", the stamp in `CLAUDE.md` (Phase 7, Task 5).
- CI: all three runs green; a run still going at hand-back is reported as still going, not as green (Phase 7, Task 6).

---

## Human verification plan

The by-hand and scripted steps in the order a tester runs them, on Windows, Nautilus first and Paladin second. Each step names the criteria it covers. All runs use the debug build against the osmena checkout with the `~/.cargo/config.toml` patch table active; afterwards the probe profile directory is deleted, `RUST_LOG` (and any other variable set) is removed, and `git checkout -- Cargo.lock` restores each browser's lock unless the phase is committing it.

### Nautilus

**N1. The bad file (Phase 3, Task 4, Steps 1 and 2).** Build Nautilus with `cargo build --workspace --all-targets`. Run once with `--profile osmena-blocked-probe --exit-after 3 about:blank` to learn the profile directory; write `sites = 3` to its `blocked.toml`; run with `RUST_LOG=warn` and `--exit-after 8 https://example.com/`.
Must see: `the blocked sites file is not valid` and `starting with no blocked sites` on stderr; the window opens and loads the page; exit code 0. (AC1.5, scripted run.)

**N2. The tab menu, the prompt and the block page (Phase 4, Task 5).** `RUST_LOG=info,nautilus_engine=debug`, `cargo run -- --profile blocked-probe https://example.com/`.
1. Right-click the tab: a separator then "Block on this profile", enabled. Ctrl+N; on the new window's `about:blank` tab the entry is disabled. (AC2.1)
2. Pick it: "Block site", "Block example.com on this profile? Are you sure? Blocked sites can only be unblocked from profile settings.", a red "Block" and "Cancel". Escape: nothing changes. Reopen, Cancel: nothing changes. (AC2.2, AC2.3, AC2.7)
3. Block: the profile-colour gradient, one of the three lines large in the middle, "example.com is blocked on this profile" beneath, the settings hint under it; the address bar reads `https://example.com/`; the strip shows the address with no favicon. Clicking the page does nothing; Ctrl+L focuses the address bar. (AC3.8, AC3.9, AC3.10)
4. Reload twice: the line changes each time. Narrow the window: the line shrinks to one row. Between reloads the line does not change as the window repaints. (AC3.7, AC3.8, AC3.9)
5. Type `example.com/other`, Enter: the block page at once, one `load started` for the tab after the block, no request naming example.com with `NAUTILUS_CEF_LOG=verbose`. A second tab on another site, navigated to example.com: the block page; Back: the other site, the page cleared. (AC3.1, AC3.6)
6. The profile's `blocked.toml` holds `sites = ["example.com"]`. (AC2.4)
7. Quit; clean up.

**N3. The settings section (Phase 5, Task 3).** `cargo run -- --profile blocked-probe https://example.com/` on a fresh probe profile.
1. Ctrl+,: "Blocked sites" between Bookmarks and Downloads, "No blocked sites.", the field and Block. (AC4.1)
2. `not a site`, Enter: red "Not a site.", nothing added. `https://WWW.Example.COM/x`, Enter: row `example.com`, field and notice cleared, the browser's example.com tab on the block page at once. (AC4.3, AC4.5)
3. `example.com`, Enter: nothing changes; `blocked.toml`'s modification time unchanged. (AC4.6)
4. Add `news.bbc.co.uk` and `127.0.0.1`: rows `127.0.0.1`, `bbc.co.uk`, `example.com`. (AC4.1)
5. Remove on `example.com`: the row goes with no question; the tab returns to example.com on its own; `blocked.toml` lists the other two. (AC4.2, AC4.4)
6. Quit; clean up.

**N4. The engine hook, scripted (Phase 6, Task 3, Step 1).** With Phase 6, Task 1 built: a probe profile whose `blocked.toml` is `sites = ["example.com"]`, the `blocked-link.html` page, `RUST_LOG=info,nautilus_engine=debug`, `NAUTILUS_TEST_PAGE_SCRIPT` clicking the link, `--exit-after 20`, output to `blocked-run.log`, filtered with `Select-String`.
Must see, in order: `load started at file:///…blocked-link.html`, `clicking`, `blocked navigation to https://example.com/`, exactly one `load started at about:blank`, no `load started at https://example.com/`, and the exit line listing the tab `at https://example.com/`. A repeat `clicking` and a null-element console error may follow and must not be followed by another `blocked navigation`. Rerun with `NAUTILUS_CEF_LOG=verbose`: no request line naming example.com. (AC5.1, AC3.4, scripted run.)

**N5. The engine hook, by hand (Phase 6, Task 3, Step 2).** `cargo run -- --profile blocked-probe https://example.org/`, same profile.
1. Type `example.com`, Enter: the block page; Back: example.org. (AC3.1, AC3.6)
2. Type `example.net`, Enter, it loads; tab menu, Block: the block page; Back: example.org at once, no `load started at https://example.net/` in the log after the block. Forward: a plain blank page with `about:blank` in the address bar (known and accepted; record it). Type `example.net`, Enter: the block page. Settings, Remove `example.net`: the tab returns to it. (AC2.5, AC2.7, AC3.6, AC4.4)
3. Page menu, "View page source": a `view-source:https://example.org/` tab opens. (AC5.2)
4. The extension manager (`chrome://extensions`) opens. (AC5.2)
5. Type `https://httpbin.org/redirect-to?url=https%3A%2F%2Fexample.com%2F`: the block page; record whether the log shows `blocked navigation to https://example.com/ (a redirect)` or a `load started at https://example.com/` followed by the model's re-block; if httpbin is down, record the redirect as not observed. (AC3.5)
6. Quit; clean up. Write the outcome into Nautilus's handoff (Phase 6, Task 2).

### Paladin

**P1. Build and suite (Phase 3, Task 4, Step 3; Phase 7, Task 1).** `cargo check --workspace` against the defaulted contract in Phase 3; after Phase 7, Task 1, the full gate including `cargo xtask angle-dlls debug` and `cargo test --workspace`.
Must see: no errors; all tests pass, `headless` and `web_animations` among them. (AC5.3)

**P2. The deny, by hand (Phase 7, Task 3).** After `cargo xtask angle-dlls debug`, `RUST_LOG=info`, `cargo run -- --profile blocked-probe https://example.com/`.
1. Tab menu, "Block on this profile", Block: the block page, the address bar on `https://example.com/`. (AC2.5, AC3.8)
2. Settings, Remove `example.com`: the page comes back. (AC4.4)
3. Open the `file:///` link page from N4, block example.com from the settings window, click the link: the block page, and `blocked navigation to https://example.com/` in the log. (AC5.3, AC3.4)
4. Back: the file page. (AC3.6)
5. Quit; delete the directory the `profile … at` line named. Write the outcome into Paladin's handoff (Phase 7, Task 2). If Paladin does not start, record "Not verified: the deny by hand; the filter is unit-tested and the build and tests pass".

Not walked on Paladin: a redirect (the model's re-block is its only path, unit-tested), an iframe pointing at a blocked site (S15), Back after a tab-menu block.

### Records and CI

**R1. The documents (Phases 3, 5, 6, 7).** Read each document listed under AC5.4 against its phase task; run `fc.exe /b CLAUDE.md AGENTS.md` in each repository. (AC5.4)

**R2. CI (Phase 7, Task 6).** After the three pushes to `main`, watch each repository's run for the pushed commit to completion. Must see: three green runs; any still running at hand-back is reported as running. (AC5.4)

---

## Coverage summary

"Automated" means unit tests (and, for AC1.5 and AC5.1, a scripted run whose expected log lines are fixed in the phase file) with no by-eye step. "Human" means by-hand verification only. "Both" means an automated test of the model, filter or text plus a by-hand or by-eye step for what the test cannot see.

| Group | Criteria | Automated | Human | Both |
| ----- | -------- | --------- | ----- | ---- |
| AC1: The list and the matching | 5 | 5 (AC1.1, AC1.2, AC1.3, AC1.4, AC1.5) | 0 | 0 |
| AC2: Blocking from the tab menu | 7 | 1 (AC2.6) | 0 | 6 (AC2.1, AC2.2, AC2.3, AC2.4, AC2.5, AC2.7) |
| AC3: Navigation and the block page | 10 | 2 (AC3.2, AC3.3) | 1 (AC3.10) | 7 (AC3.1, AC3.4, AC3.5, AC3.6, AC3.7, AC3.8, AC3.9) |
| AC4: Settings | 6 | 0 | 1 (AC4.1) | 5 (AC4.2, AC4.3, AC4.4, AC4.5, AC4.6) |
| AC5: Both browsers | 4 | 1 (AC5.1) | 0 | 3 (AC5.2, AC5.3, AC5.4) |
| **Total** | **32** | **9** | **2** | **21** |

Every criterion maps to at least one automated test or a documented by-hand step. Partial coverage, by design: AC2.5's "every window" and AC2.2's several-site listing are unit-tested only; AC5.2's `data:`, `devtools:` and subframe cases are filter-tested only; Paladin's AC3.5 is the model's re-block only; Forward onto Nautilus's replaced blank entry is a known, accepted plain blank page.
