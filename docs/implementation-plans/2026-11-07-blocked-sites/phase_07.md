# Blocked Sites Implementation Plan

**Goal:** Servo refuses too, and every document says what was built: Paladin denies blocked navigations in `request_navigation`, replaces a blocked tab's entry, takes the list, pins the osmena revision and records a Servo limitation; the parent workspace's product-brief index gains the row.

**Architecture:** `paladin-engine`'s `DelegateState` holds the list behind a `RefCell`; `WebViewDelegate::request_navigation` (absent today; Servo's default allows) denies a request whose URL a pure filter says is blocked and pushes `EngineEvent::NavigationBlocked`; `Engine::configure_blocked_sites` stores the list; `TabHost::replace_with_blank` evaluates `location.replace('about:blank')`. Servo cannot tell a main-frame navigation from an `<iframe>`'s, so Paladin denies and reports both (the user's decision of 2026-11-07), recorded as an upstream limitation.

**Tech Stack:** Rust 1.97.1, servo 0.7.0 fork (`WebViewDelegate::request_navigation(&self, WebView, NavigationRequest)`, `NavigationRequest { pub url: Url }` with `allow(self)`/`deny(self)`, `WebView::evaluate_javascript(script, |result| ...)`), osmena-core `BlockedSites`.

**Scope:** 7 phases from the design `docs/design-plans/2026-11-07-blocked-sites.md`. This file is Phase 7 of 7. Depends on Phase 6 (the osmena revision both browsers pin is the one Phase 5 pushed).

**Codebase verified:** 2026-11-07 (paladin `cc9a89f`; the pinned servo fork is branch `dist/paladin-0.7.0` at `4bccebcb` per `Cargo.lock`; the local checkout `C:\Users\tomis\vibes\servo` is on `paladin/0.7.0` at `f6e38b0e`, and the files cited below are identical on both).

---

## Acceptance Criteria Coverage

This phase implements and tests:

### blocked-sites.AC5: Both browsers
- **blocked-sites.AC5.3 Success:** Paladin: `request_navigation` denies a blocked URL and reports it; Paladin builds and its tests pass against the pinned osmena.
- **blocked-sites.AC5.4 Success:** CHANGELOGs, READMEs (manual checklist), osmena `CLAUDE.md`/`AGENTS.md` and the PRD index are updated; CI is green in all three repositories.

(AC3.4 and AC3.5 for Paladin follow from AC5.3: a link or script navigation is denied and reported; a redirect never reaches `request_navigation` in Servo (it is handled inside the pipeline), so the model's `UrlChanged` re-block is Paladin's redirect path.)

---

## Context for the engineer

All paths are under `C:\Users\tomis\vibes\browser-project\paladin\` unless said otherwise. Commit to the current branch (`main`). The gate (`CLAUDE.md:45-48`, CI the same; always `--workspace`, and on Windows the ANGLE DLLs before the tests):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --all-targets
cargo xtask angle-dlls debug
cargo test --workspace
```

A cold Servo build takes most of an hour (`CLAUDE.md:292`); a `target/` from earlier builds exists, so expect an incremental build. The lock-file rule and the commit conventions are the same as Nautilus's (Phase 6's context): rename the patch table's key, `cargo metadata`, commit, restore.

**Facts about the code as it stands:**
- `crates/paladin-engine/src/delegate.rs` (363 lines): imports `:1-14` are `use osmena_core::{EditState, EngineEvent, Favicon, LoadState, Media, MediaKind, PageContext, PendingTab, TabId}; use servo::{ConsoleLogLevel, ContextMenu, ContextMenuElementInformationFlags, CreateNewWebViewRequest, EmbedderControl, EmbedderControlId, Image, LoadStatus, PixelFormat, ServoDelegate, ServoError, WebView, WebViewDelegate, WebViewId}; use url::Url;`. `DelegateState` (`:16-30`, `#[derive(Default)]`) has `events`, `tabs: RefCell<HashMap<WebViewId, TabId>>`, `pending`, `next_pending`, `context_menus`. Its impl (`:32-71`) has `pub(crate) fn push(&self, event: EngineEvent)` (`:61`) and `fn tab(&self, webview: &WebView) -> Option<TabId>` (`:65`). `impl WebViewDelegate for DelegateState` is `:73-221`, with `notify_url_changed` at `:74-78`; there is **no** `request_navigation`. The test module is at `:280-363` (pure helpers only; `WebView` and `NavigationRequest` cannot be built in a test).
- Servo (`C:\Users\tomis\vibes\servo\components\servo\webview_delegate.rs:30-71`): `pub struct NavigationRequest { pub url: Url, ... }` with `pub fn allow(mut self)`, `pub fn deny(mut self)`; dropping an unanswered request allows. `:983-985`: `fn request_navigation(&self, _webview: WebView, _navigation_request: NavigationRequest) {}` on `WebViewDelegate`, documented as "load a URL in its main frame or one of its nested `<iframe>`s". `servo::NavigationRequest` is re-exported. `WebView::evaluate_javascript<T: ToString>(&self, script: T, callback: impl FnOnce(Result<JSValue, JavaScriptEvaluationError>) + 'static)` (`webview.rs:731-741`; the callback is required). The hook fires only for script-initiated loads (links, `location.*`, forms) and `WebView::reload`; embedder loads (`WebView::load`, which `TabHost::navigate` uses), a new WebView's first URL, Back/Forward and HTTP redirects bypass it (constellation.rs `:1341-1367`, `:3372`, `:4476`, `:1830`).
- `crates/paladin-engine/src/engine.rs`: `impl osmena_engine::Engine for Engine` at `:137-177` (`spin`, `drain_events`, `discard_pending`, `open_surface`, `open_host`; no `configure_downloads`); imports `:8-14` include `use osmena_core::{EngineEvent, EngineInfo, PageTools, PendingTab};`; `Engine { servo: Servo, delegate: Rc<DelegateState> }`.
- `crates/paladin-engine/src/host.rs`: `fn with(&self, tab: TabId, f: impl FnOnce(&WebView))` at `:138-143`; `fn navigate(&self, tab: TabId, url: Url) { self.with(tab, |w| w.load(url)); }` at `:248-250` inside `impl TabHostApi for TabHost` (`:189-378`).
- `Cargo.toml:21-24`: the four `osmena-* = { git = ..., rev = "d0e3124abff27f7885c2b68913c21e26f2d54c33" }` lines.
- `CHANGELOG.md`: `## [Unreleased]` `:7`, `### Fixed` `:9`, `### Changed` `:17`, `### Added` `:29` (list from `:31`). `README.md`: `## Features` `:11-87` (**Page zoom** at `:60-62`), `## Manual checklist` `:171-320` (item 13 **Clean exit** at `:313-315` is the last), `## Known limitations` `:391-425` (style at `:404-407`: "- No find in page: … (Servo 0.7.0 has no API to search a page; S13)."). `CLAUDE.md`: stamp `:9-12`, `## Status and handoff (2026-10-19)` at `:256` with dated "Added …: … Verified: … Not verified: …" paragraphs prepended inside it (`:258-278`). `docs/upstream-issues.md` holds S-entries; S13 and S14 at `:218-247` show the format. `AGENTS.md` mirrors `CLAUDE.md`.
- Parent workspace: `C:\Users\tomis\vibes\browser-project\docs\prd\README.md` (32 lines) has a table of six rows and a "Next in line, not written up" paragraph; `C:\Users\tomis\vibes\browser-project\CLAUDE.md:7` is `_Last updated: 2026-10-30 (product briefs under `docs/prd`)._`. The parent directory is not a git repository.

---

<!-- START_SUBCOMPONENT_A (tasks 1) -->
<!-- START_TASK_1 -->
### Task 1: The deny, the list, and `replace_with_blank`

**Verifies:** blocked-sites.AC5.3 (the filter's tests, the build and the test suite; the deny by hand in Task 3 if a run is to hand)

**Files:**
- Modify: `crates/paladin-engine/src/delegate.rs` (imports `:1-14`, `DelegateState` `:16-30` and its impl `:32-71`, `impl WebViewDelegate` `:73-221`, a new pure function, tests `:280+`)
- Modify: `crates/paladin-engine/src/engine.rs` (imports `:8-14`, the `impl Engine` block `:137-177`)
- Modify: `crates/paladin-engine/src/host.rs` (the `impl TabHostApi` block)
- Test: `crates/paladin-engine/src/delegate.rs`

**Implementation:**

1. `delegate.rs` imports: add `BlockedSites` to the `osmena_core::{...}` list and `NavigationRequest` to the `servo::{...}` list.

2. `DelegateState`: after `context_menus` add
   ```rust
       /// The sites the profile blocks, as the shell last said; a page's
       /// navigation to one is denied in `request_navigation`.
       blocked: RefCell<BlockedSites>,
   ```
   and to the impl, after `push`:
   ```rust
       pub(crate) fn set_blocked_sites(&self, sites: BlockedSites) {
           *self.blocked.borrow_mut() = sites;
       }
   ```

3. The pure filter, placed before `impl WebViewDelegate for DelegateState`:
   ```rust
   /// Whether `request_navigation` denies a load of `url`: its site is
   /// blocked. Anything without a site (`about:blank`, which the block
   /// page sits over, `data:`, `view-source:`) passes. Over a plain URL
   /// so tests need no Servo.
   pub(crate) fn blocked_navigation(sites: &BlockedSites, url: &Url) -> bool {
       sites.blocks(url).is_some()
   }
   ```

4. `impl WebViewDelegate for DelegateState`: after `notify_url_changed` add
   ```rust
       /// Every navigation page content starts (a link, a script, a form),
       /// before any request. One to a blocked site is denied and
       /// reported, and the model puts the block page up. Servo does not
       /// say whether the main frame or an `<iframe>` is navigating, so a
       /// blocked frame inside an allowed page blocks the whole tab too
       /// (S15 in `docs/upstream-issues.md`). The loads the embedder
       /// starts (`WebView::load`, Back, Forward) never come here; the
       /// model's guard covers them.
       fn request_navigation(&self, webview: WebView, request: NavigationRequest) {
           let Some(tab) = self.tab(&webview) else {
               // Not a tab of ours (a popup not yet adopted): let it be.
               request.allow();
               return;
           };
           if blocked_navigation(&self.blocked.borrow(), &request.url) {
               let url = request.url.clone();
               log::info!("tab {tab:?}: blocked navigation to {url}");
               request.deny();
               self.push(EngineEvent::NavigationBlocked { tab, url });
           } else {
               request.allow();
           }
       }
   ```
   (The `RefCell` borrow is a temporary inside the `if` condition and ends before `push`.)

5. `engine.rs`: add `BlockedSites` to the `osmena_core::{...}` import; in `impl osmena_engine::Engine for Engine`, after `open_host`, add
   ```rust
       fn configure_blocked_sites(&self, sites: BlockedSites) {
           self.delegate.set_blocked_sites(sites);
       }
   ```

6. `host.rs`: in `impl TabHostApi for TabHost`, after `navigate`, add
   ```rust
       /// `location.replace` swaps the current entry for the blank page;
       /// `load` would add one and leave the blocked page behind Back. It
       /// is a script navigation, so it passes `request_navigation`
       /// (`about:blank` has no site).
       fn replace_with_blank(&self, tab: TabId) {
           self.with(tab, |w| w.evaluate_javascript("location.replace('about:blank')", |_| {}));
       }
   ```

**Testing** (in `delegate.rs`'s `mod tests`, the same cases as Nautilus's filter, over `Url`):

```rust
    use osmena_core::BlockedSites;
    use url::Url;

    use super::blocked_navigation;

    fn blocking(site: &str) -> BlockedSites {
        let mut b = BlockedSites::default();
        b.add(site);
        b
    }

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_load_of_a_blocked_site_or_its_subdomain_is_denied() {
        let b = blocking("example.com");
        assert!(blocked_navigation(&b, &u("https://example.com/page")));
        assert!(blocked_navigation(&b, &u("https://www.example.com/")));
        assert!(blocked_navigation(&b, &u("http://m.example.com:8080/x?y")));
        assert!(!blocked_navigation(&b, &u("https://notexample.com/")));
        assert!(!blocked_navigation(&b, &u("https://example.org/")));
    }

    #[test]
    fn pages_without_a_site_pass() {
        let b = blocking("example.com");
        for url in [
            "about:blank",
            "data:text/html,<a href='https://example.com/'>x</a>",
            "view-source:https://example.com/",
            "file:///C:/x.html",
        ] {
            assert!(!blocked_navigation(&b, &u(url)), "{url}");
        }
    }
```

**Verification:**

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --all-targets
cargo xtask angle-dlls debug
cargo test --workspace
```

Expected: clean and green against the osmena checkout (patch table active); `headless` and `web_animations` still pass. Do not commit yet (Task 4).
<!-- END_TASK_1 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_2 -->
### Task 2: Paladin's record: CHANGELOG, README, upstream issue, `CLAUDE.md`/`AGENTS.md`

**Verifies:** blocked-sites.AC5.4 (Paladin's part)

**Files:**
- Modify: `CHANGELOG.md:29-31` (top of `### Added`)
- Modify: `README.md` (Features after `:60-62`; Manual checklist after item 13 at `:313-315`; Known limitations after `:407`)
- Modify: `docs/upstream-issues.md` (a new S-entry after the last)
- Modify: `CLAUDE.md:9-12` (stamp), `:256-258` (a dated paragraph at the top of the handoff)
- Copy: `AGENTS.md`

**Step 1: CHANGELOG**, first bullet under `### Added`:

```
- Blocked sites. A profile keeps websites it refuses to show, by
  registrable domain, so blocking `youtube.com` covers every subdomain:
  "Block on this profile" in a tab's menu (behind a confirmation) or
  the "Blocked sites" section of the settings window, which is also
  the only place a site is unblocked. Typed addresses are stopped by
  the shell; links, scripts and forms are denied in Servo's navigation
  hook before any request goes out, and the tab shows a block page
  over `about:blank` with the attempted address in the address bar.
  Back returns to the page before; Reload re-rolls the page's line;
  unblocking brings the tabs back. Two limits of Servo's hook (S15):
  it does not tell the main frame from an `<iframe>`, so a frame
  pointing at a blocked site blocks its whole tab too; and it sees
  neither redirects, nor a popup's first page, nor Back and Forward,
  so a blocked site reached those ways is requested once before the
  block page replaces it (osmena, with `request_navigation` and
  `location.replace` here).
```

**Step 2: README**

Features, after **Page zoom**:

```
- **Blocked sites.** A per-profile list of websites the browser refuses
  to show, by registrable domain; a tab's menu blocks a site behind a
  confirmation, the settings window lists and unblocks them. Servo's
  navigation hook denies links, scripts and forms to a blocked site
  before any request; the shell stops typed addresses. The tab shows a
  block page instead (osmena).
```

Manual checklist, a new item 14 after **Clean exit** (renumber nothing else; it is last):

```
14. **Blocked sites.** On example.com, right-click the tab and pick
    "Block on this profile": the dialog names example.com and warns that
    only the settings window unblocks; Block turns the page into the
    profile colour's gradient with one of three lines and "example.com
    is blocked on this profile" beneath; the address bar still reads
    `https://example.com/`. Reload changes the line. Back goes to the
    page before. Type `example.com/other` and press Enter: the block
    page at once. On another page, click a link to example.com: the
    block page, with `blocked navigation to` in the log. Open the
    settings window: "Blocked sites" lists `example.com`; Remove brings
    the tab back to example.com without asking.
```

Known limitations, after the downloads line (`:406-407`):

```
- Blocked sites and Servo's navigation hook (S15): a blocked site
  inside an `<iframe>` blocks the whole tab, since `request_navigation`
  does not say which frame is navigating, so Paladin denies and reports
  every navigation to a blocked site; and a redirect, a popup's first
  page, or Back and Forward onto a blocked site are requested once
  before the block page replaces them, since the hook does not see
  them. Typed addresses, links, scripts and forms are never requested.
  Chromium-based Nautilus lets subframes through and cancels redirects.
```

**Step 3: `docs/upstream-issues.md`**

S14 is the last S-entry (`docs/upstream-issues.md:233`), so this one is S15. Add it after S14 in the file's format, saying: Servo 0.7.0's `WebViewDelegate::request_navigation` receives a `NavigationRequest` whose `pipeline_id` is `pub(crate)`, so an embedder cannot tell a top-level navigation from a nested frame's; Paladin therefore denies both when the site is blocked (a frame pointing at a blocked site takes the tab to the block page). And the hook fires only for script-initiated loads (`schedule_navigation`, reached from `ScriptToConstellationMessage::LoadUrl`) and `WebView::reload`: embedder loads (`WebView::load`), a new WebView's first URL (popups), history traversal and HTTP redirects bypass it, so a blocked site reached one of those ways is requested once and loaded before osmena's `UrlChanged` re-block replaces it with `about:blank` (typed addresses are stopped by osmena's model guard before Servo sees them). Wanted upstream: the frame (or at least "is top-level") on `NavigationRequest`, and the hook on redirects and popups' first loads.

**Step 4: `CLAUDE.md`**

Stamp: prepend `blocked sites over Servo's navigation hook (osmena's list and block page; frames too, S15); before that ` and move the date to `2026-11-07`.

Handoff: at the top of `## Status and handoff (2026-10-19)`'s body (before the newest "Added …" paragraph) add an "Added 2026-11-07:" paragraph naming `request_navigation` with the pure `blocked_navigation`, the list in `DelegateState`, `replace_with_blank` through `evaluate_javascript`; "Verified:" the gate and what Task 3 showed; "Not verified:" what it did not.

Mirror, in PowerShell: `Copy-Item CLAUDE.md AGENTS.md; fc.exe /b CLAUDE.md AGENTS.md` (no differences).

No commit yet (Task 4).
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: By hand, if a Paladin run is to hand

**Verifies:** blocked-sites.AC5.3 by observation (the design allows "recorded as not verified in its handoff" otherwise)

**Files:** none changed.

The debug build from Task 1 runs on Windows (`CLAUDE.md:287-288` records a launch rendering example.com). After `cargo xtask angle-dlls debug`:

```
$env:RUST_LOG = "info"
cargo run -- --profile blocked-probe https://example.com/
```

(`info` is Paladin's default level and keeps both `paladin_engine`'s `blocked navigation` line and osmena_shell's `profile "blocked-probe" at <dir>` line, which names the directory to delete afterwards.)

1. Right-click the tab, "Block on this profile", Block: the block page; the address bar keeps `https://example.com/`.
2. Open the settings window, Remove `example.com`: the page comes back.
3. Open a page with a link to example.com (the `file:///` page from Phase 6, Task 3 works), block example.com from the settings window, click the link: the block page, and `blocked navigation to https://example.com/` in the log. That is the deny.
4. Back: the file page.

Quit; delete the directory the `profile … at` line named; `Remove-Item Env:RUST_LOG`. Write what happened into Task 2's handoff paragraph. If Paladin does not start on this machine, write "Not verified: the deny by hand; the filter is unit-tested and the build and tests pass".
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: Pin the osmena revision and commit

**Verifies:** None (the pin and the record).

**Files:**
- Modify: `Cargo.toml:21-24`
- `Cargo.lock`

Exactly as Phase 6, Task 4, in this repository: replace the four `rev` values with the sha from Phase 5, Task 4; rename the patch table's key in `C:\Users\tomis\.cargo\config.toml`, run `cargo metadata --format-version 1 > $null`, check `git diff --stat Cargo.lock` shows git sources for the osmena crates and the new `psl` entries, restore the key. Then:

```
cargo fmt --all --check
git add Cargo.toml Cargo.lock crates/paladin-engine/src/delegate.rs crates/paladin-engine/src/engine.rs crates/paladin-engine/src/host.rs CHANGELOG.md README.md CLAUDE.md AGENTS.md docs/upstream-issues.md
git commit -m "feat: blocked sites over Servo's navigation hook" -m "Takes osmena's blocked sites and adds the engine's half: request_navigation denies a navigation to a blocked site before any request and reports it (frames too, since Servo does not name the frame; S15), the list lives in DelegateState, and a blocked tab's entry is swapped for about:blank with location.replace. Pins osmena <sha>." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_4 -->

<!-- START_TASK_5 -->
### Task 5: The parent workspace's record, and osmena's last word

**Verifies:** blocked-sites.AC5.4 (the PRD index; CI)

**Files:**
- Modify: `C:\Users\tomis\vibes\browser-project\docs\prd\README.md` (the table, and the "Next in line" paragraph)
- Modify: `C:\Users\tomis\vibes\browser-project\CLAUDE.md:7` (the stamp)
- Modify (docs only, in osmena): `C:\Users\tomis\vibes\browser-project\osmena\CLAUDE.md` and `AGENTS.md` (the 2026-11-07 handoff)

**Step 1: The PRD index**

Add a seventh row to the table:

```
| 7 | Blocked sites ([design plan](../../osmena/docs/design-plans/2026-11-07-blocked-sites.md)) | **Built 2026-11-07** in osmena and both browsers: the per-profile list by registrable domain, the tab menu's "Block on this profile" with its confirmation, the block page, the settings section; Nautilus cancels links, scripts and redirects in `on_before_browse`, Paladin denies them in `request_navigation` (frames too, its S15). The design plan stands in for a brief. |
```

Leave "content blocking" in the "Next in line, not written up" paragraph as it is: it names ad and tracker blocking, which this feature is not.

Change the parent `CLAUDE.md:7` stamp to `_Last updated: 2026-11-07 (blocked sites built; product briefs under `docs/prd`)._`.

**Step 2: osmena's handoff, docs only**

In `osmena/CLAUDE.md`'s `## Status and handoff (2026-11-07)` (Phase 5, Task 4) append a sentence saying both browsers adopted the revision on 2026-11-07, which engine hook each uses, what was verified by hand in each (from Phase 6, Task 3 and Phase 7, Task 3), and the known, accepted behaviour that Forward onto the replaced blank entry shows a plain blank page. Then:

```
cd C:\Users\tomis\vibes\browser-project\osmena
Copy-Item CLAUDE.md AGENTS.md; fc.exe /b CLAUDE.md AGENTS.md
git add CLAUDE.md AGENTS.md
git commit -m "docs: the browsers' adoption in the handoff" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
git push origin blocked-sites
```

The browsers' pins stay where they are: a documentation commit changes no crate.

No CI check here; Task 6 pushes and then watches it.
<!-- END_TASK_5 -->

<!-- START_TASK_6 -->
### Task 6: Update `main` in all three repositories, and watch CI

**Verifies:** blocked-sites.AC5.4 (CI green in all three repositories). The user asked (2026-11-07) that the `main` branches of all three repositories be pushed and brought up to date when the work is finished.

**Files:** none changed.

**Step 1: osmena: `main` takes the branch, and both go up**

Everything on `blocked-sites` sits on top of local `main` (the branch was cut from it), so the merge is a fast-forward and the revision the browsers pin stays the same sha:

```
cd C:\Users\tomis\vibes\browser-project\osmena
git status --short
git checkout main
git merge --ff-only blocked-sites
git push origin main
git push origin blocked-sites
git log --oneline -1
```

Expected: `git status --short` is empty before the merge (the plan files under `docs/implementation-plans/2026-11-07-blocked-sites/` were committed on the branch when the plan was written, before execution began, and every task since committed what it changed); the merge reports `Fast-forward`; both pushes succeed; the last line is the docs commit of Task 5. If `--ff-only` refuses, stop and report: it means `main` moved underneath, and how to reconcile is the user's call.

**Step 2: Nautilus and Paladin**

Each committed to `main` in Phase 6, Task 4 and Phase 7, Task 4. Push them:

```
cd C:\Users\tomis\vibes\browser-project\nautilus
git status --short
git push origin main
cd C:\Users\tomis\vibes\browser-project\paladin
git status --short
git push origin main
```

Expected: `git status --short` in each shows at most the `Cargo.lock` rewritten by the active patch table (do not commit that); both pushes succeed.

**Step 3: CI**

CI runs on every push to `main`. GitHub takes a few seconds to register a new run, so ask for the run of the commit just pushed rather than the newest run, and ask again if nothing comes back yet:

```
cd C:\Users\tomis\vibes\browser-project\osmena;   gh run list --branch main --commit (git rev-parse HEAD) --limit 1
cd C:\Users\tomis\vibes\browser-project\nautilus; gh run list --branch main --commit (git rev-parse HEAD) --limit 1
cd C:\Users\tomis\vibes\browser-project\paladin;  gh run list --branch main --commit (git rev-parse HEAD) --limit 1
```

Then `gh run watch <id>` on each to completion (Nautilus's job takes up to 90 minutes, Paladin's up to 180). Expected: all three green. A red run is reported with its log excerpt, not worked around; the Nautilus and Paladin `release` jobs also publish a release per green push, which is the normal path. Record the three outcomes in the final summary and, if any is still running when the work is handed back, say so rather than claiming green.
<!-- END_TASK_6 -->

---

## Phase 7 done when

- Paladin builds and its tests pass against the pinned revision; the delegate's filter has the same cases as Nautilus's; the deny was seen by hand or the handoff says it was not.
- Paladin's CHANGELOG, README (feature, checklist, known limitation), `docs/upstream-issues.md` (S15) and `CLAUDE.md`/`AGENTS.md` are updated and identical.
- The parent `docs/prd/README.md` has the row; osmena's handoff names both browsers' adoption.
- osmena's `main` has fast-forwarded to the branch and is pushed; Nautilus's and Paladin's `main` are pushed; the three CI runs' outcomes are recorded honestly.
