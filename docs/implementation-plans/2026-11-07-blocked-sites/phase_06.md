# Blocked Sites Implementation Plan

**Goal:** Chromium never requests a blocked page: Nautilus cancels blocked main-frame navigations in CEF's `on_before_browse`, replaces a blocked tab's entry with `about:blank`, takes the list from the shell, pins the osmena revision, and records it.

**Architecture:** `nautilus-engine`'s `DelegateState` holds the list behind a `RefCell` (like the download policy); `TabRequestHandler` gains `on_before_browse`, which asks a pure, CEF-free filter whether a main-frame URL is blocked and, if so, returns 1 and pushes `EngineEvent::NavigationBlocked`; `Engine::configure_blocked_sites` stores the list; `TabHost::replace_with_blank` runs `location.replace('about:blank')` on the main frame. The pin moves to the revision Phase 5 pushed.

**Tech Stack:** Rust 1.97.1, cef 154.5.0 (`wrap_request_handler!`, `ImplRequest::url`, `ImplFrame::{is_main, execute_java_script}`), osmena-core `BlockedSites`.

**Scope:** 7 phases from the design `docs/design-plans/2026-11-07-blocked-sites.md`. This file is Phase 6 of 7. Depends on Phases 1-5 pushed to osmena (the sha noted at the end of Phase 5).

**Codebase verified:** 2026-11-07 (nautilus `1df5bc8`; cef crate `154.5.0+154.0.34` in `~/.cargo/registry`).

---

## Acceptance Criteria Coverage

This phase implements and tests:

### blocked-sites.AC3: Navigation and the block page
- **blocked-sites.AC3.4 Success:** A link click or script navigation to a blocked site is cancelled by the engine before any request; the tab shows the block page.
- **blocked-sites.AC3.5 Edge:** A redirect onto a blocked site is cancelled by the engine, or re-blocked by the model if the engine let it through.

### blocked-sites.AC5: Both browsers
- **blocked-sites.AC5.1 Success:** Nautilus: a scripted run shows a blocked link produces no network request in the log.
- **blocked-sites.AC5.2 Success:** Nautilus: `about:`, `data:`, `view-source:`, `chrome:`, `devtools:` and subframe navigations pass the hook.

(AC5.4's Nautilus part, CHANGELOG, README checklist and `CLAUDE.md`/`AGENTS.md`, is Task 2.)

---

## Context for the engineer

All paths in this phase are under `C:\Users\tomis\vibes\browser-project\nautilus\`. Commit to the current branch (`main`), as its `CLAUDE.md:365` says. The verification gate (`CLAUDE.md:61-64`, CI the same) needs `CEF_PATH` set and CMake and Ninja on PATH, and always `--workspace` (the workspace's `default-members` is the binary alone):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --all-targets
cargo test --workspace
```

Conventional Commits; the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. The pin-bump idiom is `chore: osmena <sha> (<summary>)`; a bump that carries a feature is `feat: <what the user gets> over Chromium's <mechanism>` with a body (`1d01042 feat: page zoom over Chromium's zoom levels`).

**The lock-file rule** (`CLAUDE.md` tail, and osmena's README): while `~/.cargo/config.toml`'s `[patch."https://github.com/thomasmost/osmena"]` table is active, cargo rewrites `Cargo.lock` with path sources for the osmena crates. Before committing: rename the table's key (for example to `[patch."https://github.com/thomasmost/osmena-off"]`), run `cargo metadata --format-version 1 > $null` to put the git sources back into the lock, commit, then restore the key. Cargo fetches the pinned revision from GitHub during that `cargo metadata`, so the revision must be pushed (Phase 5, Task 4).

**Facts about the code as it stands:**
- `crates/nautilus-engine/src/delegate.rs`: the imports at `:15-35` are `use cef::{...}` with `Frame as CefFrame`; `Request` and `ImplRequest` are **not** imported (`ImplRequest` is used in `extensions.rs:155`). `DelegateState` (`:68-99`, `#[derive(Default)]`) has `download_policy: RefCell<Option<DownloadPolicy>>` and `downloads: RefCell<HashMap<u32, DownloadItemCallback>>` as its last fields; `impl DelegateState` has `pub(crate) fn push(&self, event: EngineEvent)` at `:106` and `pub(crate) fn set_download_policy(&self, policy: DownloadPolicy) { *self.download_policy.borrow_mut() = Some(policy); }` at `:175-177`. `ViewState` (`:252-279`) has `pub tab: TabId` and `pub delegate: Rc<DelegateState>`, and `fn push(&self, event)` at `:282`. `TabRequestHandler` is a `wrap_request_handler! { struct TabRequestHandler { view: Rc<ViewState>, } impl RequestHandler { fn on_open_urlfrom_tab(...) -> c_int {...} fn on_render_process_terminated(...) {...} } }` at `:950-1001`; it is registered by `client_for` (`:306-337`), so a new method needs no registration. `on_address_change` (`:817-832`) already does `frame.is_some_and(|f| f.is_main() == 0)`. The test module is at `:1554-1619`; tests that touch CEF types carry `#[cfg(not(target_os = "macos"))]`, so a decision function takes plain `&str`/`&Url`.
- The cef signature (`~/.cargo/registry/src/*/cef-154.5.0+154.0.34/src/bindings/x86_64_pc_windows_msvc.rs:26779-26791`): `fn on_before_browse(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, user_gesture: c_int, is_redirect: c_int) -> c_int` (0 allows, 1 cancels). `ImplRequest::url(&self) -> CefStringUserfree` (read with `CefString::from(&request.url()).to_string()`); `ImplFrame::is_main(&self) -> c_int`; `ImplFrame::execute_java_script(&self, code: Option<&CefString>, script_url: Option<&CefString>, start_line: c_int)`, called elsewhere as `frame.execute_java_script(Some(&CefString::from(script.as_str())), None, 0)` (`delegate.rs:741`, `host.rs:758`).
- `crates/nautilus-engine/src/engine.rs`: `impl osmena_engine::Engine for Engine` at `:228-308` with `fn configure_downloads(&self, policy: DownloadPolicy) { self.delegate.set_download_policy(policy); }` at `:287-289`; imports at the top include `use osmena_core::{...}`.
- `on_load_start` (`delegate.rs:710-722`, in `TabLoadHandler`) logs `"tab {:?}: load started"` for every main-frame load with no URL, so today the blank page the block page sits over logs one too.
- `crates/nautilus-engine/src/host.rs`: `fn with(&self, tab: TabId, f: impl FnOnce(&TabEntry))` at `:105-110`; `fn navigate(&self, tab: TabId, url: Url) { self.with(tab, |entry| { if let Some(frame) = entry.browser.main_frame() { frame.load_url(Some(&CefString::from(url.as_str()))); } }); }` at `:394-400` inside `impl TabHostApi for TabHost` (`:318-733`).
- `Cargo.toml:19-22`: the four `osmena-* = { git = "https://github.com/thomasmost/osmena", rev = "d0e3124abff27f7885c2b68913c21e26f2d54c33" }` lines.
- Logging and scripted runs (`CLAUDE.md:71-80`): `RUST_LOG=nautilus_engine=debug` logs per tab `"tab {:?}: load started"` (`delegate.rs:719`) and `"tab {tab:?}: loading = {}"` (`:744`); `NAUTILUS_CEF_LOG=verbose` raises Chromium's own log; `NAUTILUS_TEST_PAGE_SCRIPT` runs a script in a page when its load finishes (`delegate.rs:736-742`); a page's `console.log` is logged under the target `nautilus_engine::console` (`:875-897`); osmena's `--exit-after <seconds>` quits after logging the tabs. Nothing special-cases `about:blank` in the tab handlers; `view-source:` pages open as new tabs through `request_tab` (`host.rs:663-671`), so such a URL reaches the hook with its prefix (and `Url::parse` gives it no host).
- `CHANGELOG.md`: `## [Unreleased]` at `:7`, `### Fixed` at `:9`, no `### Added` yet. `README.md`: `## Features` bullets `:12-109` (the **Page zoom** bullet at `:73-75`), `## Manual checklist` `:205-444` (numbered bold-titled items; item 16 at `:402-407` is **Page zoom**; `## Packaging` follows at `:445`), `## Known limitations` `:552`. `CLAUDE.md`: stamp `:9-15`, `## Status and handoff (2026-11-06)` at `:373` (the newest; a new section goes above it). `AGENTS.md` mirrors `CLAUDE.md`.

---

<!-- START_SUBCOMPONENT_A (tasks 1) -->
<!-- START_TASK_1 -->
### Task 1: The hook, the list, and `replace_with_blank`

**Verifies:** blocked-sites.AC5.2 (the filter's tests), blocked-sites.AC3.4 and blocked-sites.AC3.5 (the hook; seen in Task 3)

**Files:**
- Modify: `crates/nautilus-engine/src/delegate.rs` (imports `:15-35`, `DelegateState` `:68-99` and its impl, `TabRequestHandler` `:950-1001`, a new pure function, tests `:1554+`)
- Modify: `crates/nautilus-engine/src/engine.rs` (imports, the `impl Engine` block `:228-308`)
- Modify: `crates/nautilus-engine/src/host.rs` (the `impl TabHostApi` block)
- Test: `crates/nautilus-engine/src/delegate.rs`

**Implementation:**

1. `delegate.rs` imports: add `ImplRequest, Request` to the `use cef::{...}` list; add `BlockedSites` to the `use osmena_core::{...}` list.

2. `DelegateState`: after the `downloads` field add
   ```rust
       /// The sites the profile blocks, as the shell last said; a
       /// main-frame navigation to one is cancelled in `on_before_browse`.
       blocked: RefCell<BlockedSites>,
   ```
   and after `set_download_policy` add
   ```rust
       pub(crate) fn set_blocked_sites(&self, sites: BlockedSites) {
           *self.blocked.borrow_mut() = sites;
       }
   ```

3. The pure filter, placed near `fn text(s: CefStringUserfree)` (`:1117`):
   ```rust
   /// The navigation `on_before_browse` cancels: a main-frame load of a
   /// URL on a blocked site. A subframe passes (nothing is blocked
   /// inside a page), and so does anything that is no web page, since
   /// `BlockedSites::site_for` gives nothing outside `http` and
   /// `https`: `about:`, `data:`, `view-source:`, Chromium's own
   /// `chrome:` and `devtools:` pages, extension pages, and text that
   /// is no URL. Over plain text so tests need no CEF.
   pub(crate) fn blocked_navigation(
       sites: &BlockedSites,
       url: &str,
       main_frame: bool,
   ) -> Option<Url> {
       if !main_frame {
           return None;
       }
       let url = Url::parse(url).ok()?;
       sites.blocks(&url)?;
       Some(url)
   }
   ```

4. `TabRequestHandler`: inside its `impl RequestHandler { ... }`, before `on_open_urlfrom_tab`, add
   ```rust
           /// Every navigation of this tab, before any request goes out.
           /// A main-frame load of a blocked site is cancelled here and
           /// reported, and the model puts the block page up; the blank
           /// page it sits over has no site and passes.
           fn on_before_browse(
               &self,
               _browser: Option<&mut Browser>,
               frame: Option<&mut CefFrame>,
               request: Option<&mut Request>,
               _user_gesture: ::std::os::raw::c_int,
               is_redirect: ::std::os::raw::c_int,
           ) -> ::std::os::raw::c_int {
               let main_frame = frame.is_some_and(|f| f.is_main() != 0);
               let Some(request) = request else {
                   return 0;
               };
               let url = text(request.url());
               let blocked =
                   blocked_navigation(&self.view.delegate.blocked.borrow(), &url, main_frame);
               match blocked {
                   Some(url) => {
                       log::info!(
                           "tab {:?}: blocked navigation to {url}{}",
                           self.view.tab,
                           if is_redirect != 0 { " (a redirect)" } else { "" }
                       );
                       self.view.push(EngineEvent::NavigationBlocked {
                           tab: self.view.tab,
                           url,
                       });
                       1
                   }
                   None => 0,
               }
           }
   ```
   (`text` is the existing `fn text(s: CefStringUserfree) -> String` helper at `:1117-1120`. The `RefCell` borrow is a temporary that ends with the `let blocked` statement, before `push` borrows the events queue.)

5. `delegate.rs`, `on_load_start` (`:710-722`, in `TabLoadHandler`): make its debug line name what is loading, so a scripted run can tell the blank page behind a block page from a real load. Replace `let main = frame.is_some_and(|f| f.is_main() != 0);` and the `if main { log::debug!("tab {:?}: load started", self.view.tab); }` that follows with
   ```rust
           let main_url = frame
               .as_deref()
               .filter(|f| f.is_main() != 0)
               .map(|f| text(f.url()));
           let main = main_url.is_some();
           if let Some(url) = &main_url {
               log::debug!("tab {:?}: load started at {url}", self.view.tab);
           }
   ```
   (`frame` is only borrowed, so any later use of it in the method stands.)

6. `engine.rs`: add `BlockedSites` to the `osmena_core::{...}` import; in `impl osmena_engine::Engine for Engine`, after `configure_downloads` (`:287-289`), add
   ```rust
       fn configure_blocked_sites(&self, sites: BlockedSites) {
           self.delegate.set_blocked_sites(sites);
       }
   ```

7. `host.rs`: in `impl TabHostApi for TabHost`, after `navigate`, add
   ```rust
       /// `location.replace` swaps the current entry for the blank page;
       /// `load_url` would add one and leave the blocked page behind Back.
       fn replace_with_blank(&self, tab: TabId) {
           self.with(tab, |entry| {
               if let Some(frame) = entry.browser.main_frame() {
                   frame.execute_java_script(
                       Some(&CefString::from("location.replace('about:blank')")),
                       None,
                       0,
                   );
               }
           });
       }
   ```

**Testing** (in `delegate.rs`'s `mod tests`; these touch no CEF type, so no `cfg`):

```rust
    use osmena_core::BlockedSites;
    use super::blocked_navigation;

    fn blocking(site: &str) -> BlockedSites {
        let mut b = BlockedSites::default();
        b.add(site);
        b
    }

    #[test]
    fn a_main_frame_load_of_a_blocked_site_or_its_subdomain_is_cancelled() {
        let b = blocking("example.com");
        let hit = blocked_navigation(&b, "https://example.com/page", true).unwrap();
        assert_eq!(hit.as_str(), "https://example.com/page");
        assert!(blocked_navigation(&b, "https://www.example.com/", true).is_some());
        assert!(blocked_navigation(&b, "http://m.example.com:8080/x?y", true).is_some());
        assert!(blocked_navigation(&b, "https://notexample.com/", true).is_none());
        assert!(blocked_navigation(&b, "https://example.org/", true).is_none());
    }

    #[test]
    fn subframes_and_pages_without_a_site_pass() {
        let mut b = blocking("example.com");
        assert!(blocked_navigation(&b, "https://example.com/", false).is_none());
        // Even with the engine pages' "hosts" in the list, they pass:
        // only web pages have sites.
        b.add("extensions");
        b.add("devtools");
        b.add("abcdefghijklmnop");
        for url in [
            "about:blank",
            "data:text/html,<a href='https://example.com/'>x</a>",
            "view-source:https://example.com/",
            "chrome://extensions/",
            "devtools://devtools/bundled/inspector.html",
            "chrome-extension://abcdefghijklmnop/popup.html",
            "not a url",
        ] {
            assert!(blocked_navigation(&b, url, true).is_none(), "{url}");
        }
    }
```

**Verification:**

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --all-targets
cargo test --workspace
```

Expected: clean and green (the patch table is active, so this builds against the osmena checkout).

Do not commit yet: Task 4 commits code, pin and docs together after the run in Task 3.
<!-- END_TASK_1 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_2 -->
### Task 2: CHANGELOG, README, `CLAUDE.md`/`AGENTS.md`

**Verifies:** blocked-sites.AC5.4 (Nautilus's part)

**Files:**
- Modify: `CHANGELOG.md:7-9`
- Modify: `README.md` (Features `:73-75`, Manual checklist after item 16 at `:402-407`)
- Modify: `CLAUDE.md:9-15` (stamp), `:373` (a new handoff section above)
- Copy: `AGENTS.md`

**Step 1: CHANGELOG**

Under `## [Unreleased]`, before `### Fixed`, add a `### Added` section whose first bullet is:

```
### Added

- Blocked sites. A profile keeps websites it refuses to show, by
  registrable domain, so blocking `youtube.com` covers every subdomain:
  "Block on this profile" in a tab's menu (behind a confirmation) or
  the "Blocked sites" section of the settings window, which is also
  the only place a site is unblocked. Typed addresses are stopped by
  the shell; links, scripts and redirects are cancelled in Chromium's
  before-browse hook before any request goes out, and the tab shows a
  block page over `about:blank` with the attempted address in the
  address bar. Back returns to the page before; Reload re-rolls the
  page's line; unblocking brings the tabs back (osmena, with
  `on_before_browse` and `location.replace` here).
```

**Step 2: README features**

After the **Page zoom** feature bullet (`:73-75`) add:

```
- **Blocked sites.** A per-profile list of websites the browser refuses
  to show, by registrable domain; a tab's menu blocks a site behind a
  confirmation, the settings window lists and unblocks them. Chromium's
  before-browse hook cancels links, scripts and redirects to a blocked
  site before any request; the shell stops typed addresses. The tab
  shows a block page instead (osmena).
```

**Step 3: README manual checklist**

After item 16 (**Page zoom**) insert a new item and renumber the ones after it:

```
17. **Blocked sites.** On example.com, right-click the tab and pick
    "Block on this profile": the dialog names example.com and warns that
    only the settings window unblocks; Block turns the page into the
    profile colour's gradient with one of three lines and "example.com
    is blocked on this profile" beneath; the address bar still reads
    `https://example.com/`. Reload changes the line. Back goes to the
    page before. Type `example.com/other` and press Enter: the block
    page at once, and with `RUST_LOG=info,nautilus_engine=debug` the only
    `load started` for the tab after it is `at about:blank`. On another
    site, click a link to example.com: the block page, with `blocked
    navigation to https://example.com/` in the log, then one `load
    started at about:blank` and none naming example.com (with
    `NAUTILUS_CEF_LOG=verbose`, no request to it either). Open the
    settings window: "Blocked sites"
    lists `example.com`; Remove brings the tab back to example.com
    without asking, and `blocked.toml` in the profile is empty.
    `view-source:` of an unblocked page and the extension manager
    (`chrome://extensions`) still open.
```

**Step 4: `CLAUDE.md`**

Stamp: prepend to the parenthesis `blocked sites over Chromium's before-browse hook (osmena's list and block page); before that ` and move the date to `2026-11-07`.

Handoff: above `## Status and handoff (2026-11-06)` add `## Status and handoff (2026-11-07)` describing: the hook in `TabRequestHandler::on_before_browse` with the pure `blocked_navigation` filter, the list in `DelegateState`, `replace_with_blank` through `location.replace`; what Task 3's scripted run showed (write what happened); not verified: macOS, a redirect by hand unless one was tried, DevTools on a blocked tab.

Mirror, in PowerShell: `Copy-Item CLAUDE.md AGENTS.md; fc.exe /b CLAUDE.md AGENTS.md` (no differences).

No commit yet (Task 4).
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: The scripted run

**Verifies:** blocked-sites.AC5.1, blocked-sites.AC3.4, blocked-sites.AC3.5 (by observation), blocked-sites.AC5.2 (`view-source:` and `chrome://extensions` by hand; the rest by the filter's tests)

**Files:** none changed.

**Step 1: A profile that blocks example.com, and a page with a link to it**

In PowerShell, from `C:\Users\tomis\vibes\browser-project\nautilus` with `CEF_PATH` set. First find where this build keeps profiles: run `cargo run -- --profile blocked-probe --exit-after 3 about:blank 2>&1 | Select-String "profile"`; the `profile "blocked-probe" at <dir>` line names the directory. Then:

```
$dir = "<that directory>"
Set-Content -Encoding utf8 "$dir\blocked.toml" 'sites = ["example.com"]'
$page = "$env:TEMP\blocked-link.html"
Set-Content -Encoding utf8 $page '<!doctype html><title>Blocked link test</title><a id="l" href="https://example.com/">go</a>'
$env:RUST_LOG = "info,nautilus_engine=debug"
$env:NAUTILUS_TEST_PAGE_SCRIPT = "console.log('clicking'); document.getElementById('l').click();"
cargo run -- --profile blocked-probe --exit-after 20 "file:///$($page -replace '\\','/')" > "$env:TEMP\blocked-run.log" 2>&1
Remove-Item Env:NAUTILUS_TEST_PAGE_SCRIPT
Select-String -Path "$env:TEMP\blocked-run.log" -Pattern "load started|blocked navigation|clicking|quitting with"
```

Expected, in order: `load started at file:///...blocked-link.html`, the console line `clicking`, `blocked navigation to https://example.com/`, then exactly one `load started at about:blank` (the blank page the model pushes behind the block page) and **no** `load started at https://example.com/`, up to the `--exit-after` line, which lists the tab `at https://example.com/` (the attempted address, which the model keeps). That is AC5.1: Chromium never started the blocked load. Two things are expected noise: the test page script runs again when `about:blank` finishes loading, so a second `clicking` and a console error about a null element may follow, and they must not be followed by another `blocked navigation` (the blank page has no link). For a second opinion run once more with `$env:NAUTILUS_CEF_LOG = "verbose"` and confirm the log has no request line naming `example.com`.

**Step 2: By hand**

Start `cargo run -- --profile blocked-probe https://example.org/` (the same profile, so example.com is still blocked and `RUST_LOG` still set) and:
1. Type `example.com` in the address bar and press Enter: the block page (the model's guard). Back: example.org returns.
2. The tab-menu path with the real `replace_with_blank` (AC2.5, AC2.7, AC3.6): type `example.net` and press Enter; the page loads. Right-click the tab, "Block on this profile", Block: the block page. Back: example.org returns straight away, and the log has no `load started at https://example.net/` after the block (the blocked entry was replaced, not buried). Forward: a plain blank page with `about:blank` in the address bar. That is the replaced entry itself; the model does not re-block a blank page on an unblocked tab, and nothing of example.net is requested. Record it in the handoff as known and accepted. Type `example.net` and press Enter: the block page (the guard). Open the settings window (Ctrl+,), Remove `example.net`: the tab returns to example.net on its own.
3. Right-click the page, "View page source": a `view-source:https://example.org/` tab opens (AC5.2).
4. Open the extension manager (the puzzle menu's "Manage extensions", or however the README's extensions item reaches `chrome://extensions`): it opens (AC5.2).
5. A redirect (AC3.5): type `https://httpbin.org/redirect-to?url=https%3A%2F%2Fexample.com%2F` and press Enter. Expected: the block page, by one of two paths, both acceptable: the log shows `blocked navigation to https://example.com/ (a redirect)` (CEF's hook saw the redirect, nothing of example.com was requested), or, if the redirect reached the engine, a `load started at https://example.com/` followed by the model's re-block (the address bar reads `https://example.com/`, the page is the block page). Record which path was seen; if httpbin is down, say the redirect was not observed.

Quit; delete the probe profile directory; `Remove-Item Env:RUST_LOG`.

Write the outcome into the handoff paragraph from Task 2 before committing.
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: Pin the osmena revision and commit

**Verifies:** None (the pin and the record).

**Files:**
- Modify: `Cargo.toml:19-22` (the four `rev = "..."` values)
- `Cargo.lock` (regenerated with git sources)

**Step 1: Move the pins**

Replace `d0e3124abff27f7885c2b68913c21e26f2d54c33` with the sha from Phase 5, Task 4 (`git -C C:\Users\tomis\vibes\browser-project\osmena rev-parse blocked-sites` prints it) in all four lines.

**Step 2: The lock, with the patch table off**

Edit `C:\Users\tomis\.cargo\config.toml`: change the table header `[patch."https://github.com/thomasmost/osmena"]` to `[patch."https://github.com/thomasmost/osmena-off"]`. Then:

```
cargo metadata --format-version 1 > $null
git diff --stat Cargo.lock
```

Expected: the lock's osmena entries now read `source = "git+https://github.com/thomasmost/osmena?rev=<sha>#<sha>"`, and `psl` and `psl-types` appear in it. Restore the header to `[patch."https://github.com/thomasmost/osmena"]` straight after.

**Step 3: Commit**

```
cargo fmt --all --check
git add Cargo.toml Cargo.lock crates/nautilus-engine/src/delegate.rs crates/nautilus-engine/src/engine.rs crates/nautilus-engine/src/host.rs CHANGELOG.md README.md CLAUDE.md AGENTS.md
git commit -m "feat: blocked sites over Chromium's before-browse hook" -m "Takes osmena's blocked sites (the per-profile list, the tab-menu entry and its confirmation, the block page, the settings section) and adds the engine's half: on_before_browse cancels a main-frame navigation to a blocked site before any request and reports it, the list lives in DelegateState, and a blocked tab's entry is swapped for about:blank with location.replace. Pins osmena <sha>." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

Do not push yet: Phase 7, Task 6 pushes `main` in all three repositories together once Paladin is done, as the user asked.

**Step 4: Verify once more against the pin**

With the patch table active again, `cargo build --workspace` still builds (it uses the checkout, which is at the pinned sha). `git status --short` shows only the rewritten `Cargo.lock`, which is expected while the table is active; do not commit that rewrite.
<!-- END_TASK_4 -->

---

## Phase 6 done when

- The filter's tests pass (blocked host and subdomain cancelled; `about:`, `data:`, `view-source:`, `chrome:`, `devtools:`, a non-URL and a subframe allowed); the gate is clean and green.
- The scripted run shows a clicked link to a blocked site produced `blocked navigation to https://example.com/`, exactly one `load started at about:blank` after it and none at `https://example.com/`; the by-hand steps (the typed address, the tab-menu block with Back and the settings removal, view-source, the extension manager, the redirect) went as described, or the handoff says which did not.
- `Cargo.toml` pins the pushed osmena revision with a lock holding git sources; CHANGELOG, README (feature and checklist) and `CLAUDE.md`/`AGENTS.md` are updated and identical.
