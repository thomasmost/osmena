# Blocked Sites Implementation Plan

**Goal:** The user can block from a tab's menu, is asked to confirm, and sees the block page where the page would be.

**Architecture:** `menus::tab_menu` takes the blocked list and adds "Block on this profile" (or "Block {n} sites on this profile"), enabled only when a target has a blockable site not yet blocked; `chrome::draw_block_sites_prompt` asks the question like `draw_delete_profile_prompt` and answers `Command::BlockSites`; `chrome::draw_blocked_page` paints the profile-colour gradient with one of the three lines in Inter Medium sized to the width; `BrowserWindow::frame` paints it in the page area beside the crash panel and keeps pointer input in egui.

**Tech Stack:** egui 0.34.3 (`Context::run_ui(RawInput, |ui: &mut Ui|)`, `Painter::layout_no_wrap(String, FontId, Color32) -> Arc<Galley>`, `Painter::galley(Pos2, Arc<Galley>, Color32)`, `Color32::gamma_multiply`), osmena-chrome, osmena-shell.

**Scope:** 7 phases from the design `docs/design-plans/2026-11-07-blocked-sites.md`. This file is Phase 4 of 7. Depends on Phases 2 and 3.

**Codebase verified:** 2026-11-07 (osmena `1bdea01` on branch `blocked-sites`).

---

## Acceptance Criteria Coverage

This phase implements and tests:

### blocked-sites.AC2: Blocking from the tab menu
- **blocked-sites.AC2.1 Success:** The tab menu shows "Block on this profile" (or "Block {n} sites on this profile"), enabled only when a target has a blockable site not yet blocked.
- **blocked-sites.AC2.3 Failure:** Cancel or Escape adds nothing.
- **blocked-sites.AC2.7 Success:** The dialog reads "Block <site> on this profile? Are you sure? Blocked sites can only be unblocked from profile settings." with buttons Block (danger colour) and Cancel.

### blocked-sites.AC3: Navigation and the block page
- **blocked-sites.AC3.8 Success:** The page shows one of the three lines, large, on the profile-colour gradient, with the site named beneath; the address bar shows the attempted URL.
- **blocked-sites.AC3.9 Success:** The line does not change between frames.
- **blocked-sites.AC3.10 Failure:** No control on the page unblocks; pointer and key input stay in the chrome while it shows.

(AC3.8's gradient, size and placement, AC3.9 by eye and AC3.10's input routing are seen in a debug build, recorded in the handoff; the tests cover the words, the sizing rule, and that the page draws at a narrow and a wide rect without panicking. The address bar shows `Tab::url`, which Phase 2 keeps at the attempted address.)

---

## Context for the engineer

All paths are under `C:\Users\tomis\vibes\browser-project\osmena\`. Branch `blocked-sites`. The gate and the commit conventions are in `phase_01.md`.

**Facts about the code as it stands:**
- `crates/osmena-chrome/src/menus.rs:1188-1281`: `pub fn tab_menu(window: &Window, targets: &[TabId], others: &[(WindowId, String)], profiles: &[ProfileEntry]) -> Vec<MenuEntry>`; it builds `plural(one, many)` closures and ends with the "Open tab in profile" submenu. `MenuEntry::item(label, enabled, command)` is the constructor. Imports at `:5-12` list `osmena_core::{Bookmark, ..., Window, WindowId}`. Its tests (`:1283-2442`) have `window_with(n)` (tab 1 at `https://a.example/`, tabs 2..=n at `https://{i}.example/`), `labels(&entries)` (a `Separator` is `"-"`) and `enabled(&entries)`. `tab_menu_for_one_tab_of_three` (`:1939-2003`) asserts a ten-label list and a ten-bool list and indexes `entries[1]`, `[4]`, `[5]`, `[8]`, `[9]`; `tab_menu_counts_the_selection_and_disables_what_cannot_apply` (`:2005-2027`) asserts labels `[0]`, `[2]`, `[4]`, `[5]`, `[7]`, a ten-bool list and `entries[2]`.
- `crates/osmena-shell/src/window.rs:1120-1131` is the `MenuRequest::Tab { targets, position }` arm calling `menus::tab_menu(model, &targets, &others, &browser.profiles)`.
- `crates/osmena-chrome/src/chrome.rs`: `Palette` at `:136-208` (`Palette::new(color: ChromeColor, dark_mode: bool)`, `ink_on(fill)`, `strip_at(t)`); `strip_background(rect, palette, pixels_per_point) -> egui::Shape` at `:216-243`, whose first half builds an `egui::Mesh` of `BANDS = 8` vertex-coloured bands of `strip_at` (so egui interpolates smoothly inside each band) and whose second half adds the one-pixel edge line; `blend` at `:380` (pub(crate)); `draw_crash_panel(ui, rect, reason) -> bool` at `:4274-4292`; `draw_delete_profile_prompt(ui, state) -> Vec<Command>` at `:3546-3600` (an `egui::Window` titled by the prompt, `.id(Id::new("chrome-prompt"))`, `.collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))`, `ui.set_min_width(360.0); ui.set_max_width(420.0);`, the danger button `Button::new(RichText::new("Delete").color(ui.visuals().error_fg_color))`, Cancel, `ui.input(|i| i.key_pressed(Key::Escape))` cancels, then `state.prompt = None` and the answer command); `draw_prompt`'s dispatch at `:2013-2021` (`Some(Prompt::QuitWithDownloads { .. }) => return draw_quit_prompt(ui, state),` is the last line before `Some(_) => {}`). The test module (`:4578-4960`) has `with_fonts() -> egui::Context` at `:4880-4885`.
- Fonts: `theme::MEDIUM` is the family name `"Inter-Medium"`; large text elsewhere uses `FontId::new(34.0, egui::FontFamily::Name(theme::MEDIUM.into()))` (`launch.rs:1123`).
- `crates/osmena-shell/src/window.rs:862-1012` is `BrowserWindow::frame`: `let crashed = model.active_tab().crashed.clone();` at `:872`, `let chrome_color = browser.settings.chrome_color;` at `:874`; the paint `match &crashed { Some(reason) => { if chrome::draw_crash_panel(ui, rect, reason) { commands.push(Command::Reload(active)); } } None => { ...host.paint_page... } }` at `:930-1008`; `self.content_is_egui = crashed.is_some();` at `:1011`. The field feeds `PointerContext::content_is_egui` at `:479`, which is what keeps pointer input in egui over the page area.

---

<!-- START_SUBCOMPONENT_A (tasks 1-2) -->
<!-- START_TASK_1 -->
### Task 1: "Block on this profile" in the tab menu

**Verifies:** blocked-sites.AC2.1

**Files:**
- Modify: `crates/osmena-chrome/src/menus.rs:5-12` (imports), `:1188-1281` (`tab_menu`), `:1939-2027` (the two tests)
- Modify: `crates/osmena-shell/src/window.rs:1128` (the call)
- Test: `crates/osmena-chrome/src/menus.rs`

**Implementation:**

Add `BlockedSites` to the `osmena_core::{...}` import in `menus.rs`.

Change `tab_menu`'s signature to

```rust
pub fn tab_menu(
    window: &Window,
    targets: &[TabId],
    others: &[(WindowId, String)],
    profiles: &[ProfileEntry],
    blocked: &BlockedSites,
) -> Vec<MenuEntry> {
```

extend its doc comment with "`blocked` is the profile's blocked sites, for the Block entry.", and before the `vec![` add

```rust
    // The sites the targets would block: the distinct ones not blocked
    // yet. A tab without a site (`about:blank`) adds nothing.
    let to_block = blocked.sites_to_block(
        targets
            .iter()
            .filter_map(|id| window.tab(*id))
            .map(|t| &t.url),
    );
    let block_label = if to_block.len() > 1 {
        format!("Block {} sites on this profile", to_block.len())
    } else {
        "Block on this profile".to_string()
    };
```

and append to the `vec![...]`, after the "Open tab in profile" submenu:

```rust
        MenuEntry::Separator,
        MenuEntry::item(
            block_label,
            !to_block.is_empty(),
            Command::PromptBlockSites {
                window: window.id,
                tabs: targets.to_vec(),
            },
        ),
```

In `window.rs:1128` the call becomes `menus::tab_menu(model, &targets, &others, &browser.profiles, &browser.blocked)`.

**Testing** (`menus.rs` tests):

- Update `tab_menu_for_one_tab_of_three`: pass `&BlockedSites::default()` as the fifth argument; append `"-"` and `"Block on this profile"` to the label list and `true, true` to the enabled list (twelve each). Add at the end:
  ```rust
          assert!(matches!(
              &entries[11],
              MenuEntry::Item { command: Command::PromptBlockSites { window: WindowId(1), tabs }, .. }
                  if *tabs == vec![TabId(2)]
          ));
  ```
- Update `tab_menu_counts_the_selection_and_disables_what_cannot_apply`: pass `&BlockedSites::default()`; `labels(&entries)[11]` is `"Block 3 sites on this profile"` (three tabs on `a.example`, `2.example`, `3.example`); append `true, true` to the enabled list.
- Add `the_block_entry_is_disabled_when_nothing_can_be_blocked`:
  - `window_with(3)` with `TabId(2)` as the target and a `BlockedSites` holding `2.example`: label `"Block on this profile"`, `enabled(&entries)[11] == false`.
  - a window whose target tab is at `about:blank` (push `Tab::new(TabId(4), Url::parse("about:blank").unwrap())` onto `window_with(3)` and target `TabId(4)`): disabled.
  - targets `[TabId(1), TabId(2), TabId(3)]` with `2.example` blocked: label `"Block 2 sites on this profile"`, enabled.
  - two targets on one site (push `TabId(4)` at `https://www.a.example/x` and target `[TabId(1), TabId(4)]`): label `"Block on this profile"` (one distinct site), enabled.

**Verification:**
Run: `cargo test -p osmena-chrome menus`
Expected: pass. Then `cargo build -p osmena-shell` (the call site).
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: The confirmation prompt

**Verifies:** blocked-sites.AC2.7, blocked-sites.AC2.3 (Cancel and Escape issue no command)

**Files:**
- Modify: `crates/osmena-chrome/src/chrome.rs`: `draw_prompt`'s dispatch (`:2013-2021`), a new `draw_block_sites_prompt` beside `draw_delete_profile_prompt` (`:3546-3600`), a new pure `block_sites_prompt_text`
- Test: `crates/osmena-chrome/src/chrome.rs`

**Implementation:**

Add, before `draw_delete_profile_prompt`:

```rust
/// What the block-sites prompt's buttons do: Block answers
/// `BlockSites`, Cancel or Escape answer nothing, and either closes the
/// prompt (`Some`); with neither pressed it stays open (`None`).
pub(crate) fn block_sites_answer(
    confirmed: bool,
    cancelled: bool,
    sites: &[String],
) -> Option<Vec<Command>> {
    if confirmed {
        Some(vec![Command::BlockSites(sites.to_vec())])
    } else if cancelled {
        Some(Vec::new())
    } else {
        None
    }
}

/// The title and the question of the block-sites prompt: one site is
/// named in the question, several are listed under it.
pub(crate) fn block_sites_prompt_text(sites: &[String]) -> (String, String) {
    const WARNING: &str =
        "Are you sure? Blocked sites can only be unblocked from profile settings.";
    match sites {
        [site] => (
            "Block site".to_string(),
            format!("Block {site} on this profile? {WARNING}"),
        ),
        _ => (
            "Block sites".to_string(),
            format!(
                "Block these sites on this profile?\n{}\n{WARNING}",
                sites
                    .iter()
                    .map(|s| format!("    {s}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        ),
    }
}

/// "Block <site> on this profile?": the warning that only the settings
/// window unblocks, and Block or Cancel.
fn draw_block_sites_prompt(ui: &mut Ui, state: &mut ChromeState) -> Vec<Command> {
    let Some(prompt) = state.prompt.as_ref() else {
        return Vec::new();
    };
    let Prompt::BlockSites { sites } = &prompt.prompt else {
        return Vec::new();
    };
    let (title, question) = block_sites_prompt_text(sites);
    let mut confirmed = false;
    let mut cancelled = false;
    egui::Window::new(title)
        .id(Id::new("chrome-prompt"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
        .show(ui.ctx(), |ui| {
            ui.set_min_width(360.0);
            ui.set_max_width(420.0);
            ui.label(question);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let danger = ui.visuals().error_fg_color;
                if ui
                    .add(Button::new(RichText::new("Block").color(danger)))
                    .clicked()
                {
                    confirmed = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                cancelled = true;
            }
        });
    match block_sites_answer(confirmed, cancelled, sites) {
        Some(commands) => {
            state.prompt = None;
            commands
        }
        None => Vec::new(),
    }
}
```

(`sites` borrows `state.prompt`; the borrow ends once `block_sites_answer` has returned its owned result, so the arm may clear the prompt.)

In `draw_prompt`'s dispatch, after the `Some(Prompt::QuitWithDownloads { .. }) => return draw_quit_prompt(ui, state),` line add:

```rust
        Some(Prompt::BlockSites { .. }) => return draw_block_sites_prompt(ui, state),
```

(The three exhaustive lists already carry `Prompt::BlockSites { .. }` from Phase 2, Task 7; leave them.)

**Testing** (`chrome.rs` tests):

- `the_block_prompt_names_one_site_and_lists_several`: `block_sites_prompt_text(&["youtube.com".to_string()])` is `("Block site", "Block youtube.com on this profile? Are you sure? Blocked sites can only be unblocked from profile settings.")`; with two sites the title is `"Block sites"`, the body starts with `"Block these sites on this profile?"`, contains both sites each on its own line, and ends with the same warning sentence.
- `the_block_prompt_answers_block_sites_on_block_and_nothing_on_cancel` (AC2.3, the chrome's half): with `sites = vec!["a.example".to_string(), "b.example".to_string()]`, `block_sites_answer(true, false, &sites) == Some(vec![Command::BlockSites(sites.clone())])`; `block_sites_answer(false, true, &sites) == Some(Vec::new())` (Cancel and Escape both set `cancelled`); `block_sites_answer(false, false, &sites) == None`; `block_sites_answer(true, true, &sites)` is the Block answer (a click wins over the key).

The drawing itself (the buttons, Escape) is seen in the debug build (Task 5); the model half of AC2.3 was tested in Phase 2.

**Verification:**
Run: `cargo test -p osmena-chrome`
Expected: pass.

**Commit (Tasks 1-2):**

```bash
cargo fmt --all
git add crates/osmena-chrome/src/menus.rs crates/osmena-chrome/src/chrome.rs crates/osmena-shell/src/window.rs
git commit -m "feat: block a site from the tab menu, behind a confirmation" -m "The tab menu ends with \"Block on this profile\" (or \"Block {n} sites on this profile\"), enabled only when a target has a site not yet blocked; the prompt names the site, warns that only the settings window unblocks, and answers BlockSites on Block, nothing on Cancel or Escape." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_2 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_SUBCOMPONENT_B (tasks 3-4) -->
<!-- START_TASK_3 -->
### Task 3: The block page

**Verifies:** blocked-sites.AC3.8 (the words and the sizing rule, by test; the look, by eye), blocked-sites.AC3.9 (the line is `Tab::blocked.line`, model state; the test draws the same page twice)

**Files:**
- Modify: `crates/osmena-chrome/src/chrome.rs` (`strip_background` at `:216-243` loses its mesh loop to a new `gradient_mesh`; new `draw_blocked_page` and `fit_size` after `draw_crash_panel`; imports)
- Test: `crates/osmena-chrome/src/chrome.rs`

**Implementation:**

Add `BlockedPage` to the `osmena_core::{...}` import at `chrome.rs:17-22`, and `FontId` to the `egui::{...}` import at `:11-14`.

Factor the gradient out of `strip_background` so the block page draws the same smooth curve. Replace its first half (from `const BANDS: u32 = 8;` through the closing `}` of the `for band in 0..BANDS` loop) with `let mesh = gradient_mesh(rect, palette, 8);`, keep the rest of the function as it is, and add just above it:

```rust
/// `rect` filled with the strip's colour curve top to bottom: `bands`
/// quads of vertex colours, which egui interpolates, so the quadratic
/// ease keeps its shape instead of flattening into one ramp.
fn gradient_mesh(rect: Rect, palette: &Palette, bands: u32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    for band in 0..bands {
        let t0 = band as f32 / bands as f32;
        let t1 = (band + 1) as f32 / bands as f32;
        let y0 = rect.top() + rect.height() * t0;
        let y1 = rect.top() + rect.height() * t1;
        let (c0, c1) = (palette.strip_at(t0), palette.strip_at(t1));
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(rect.left(), y0), c0);
        mesh.colored_vertex(Pos2::new(rect.right(), y0), c0);
        mesh.colored_vertex(Pos2::new(rect.right(), y1), c1);
        mesh.colored_vertex(Pos2::new(rect.left(), y1), c1);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}
```

(The loop body is the existing one moved verbatim; `strip_background`'s doc comment about the bands still holds. Run the chrome tests after this step alone: nothing visible changed.)

Add after `draw_crash_panel`:

```rust
/// The size a line drawn at `trial` points must shrink to so that its
/// width, `width_at_trial`, fits `max_width`; never below 18 points,
/// never larger than the trial.
pub(crate) fn fit_size(trial: f32, width_at_trial: f32, max_width: f32) -> f32 {
    if width_at_trial <= 0.0 || width_at_trial <= max_width {
        trial
    } else {
        (trial * max_width / width_at_trial).max(18.0)
    }
}

/// Replace the page area with the block page: the profile's colour
/// easing into the toolbar's as the strip does, the tab's line large
/// in the middle, and the site named beneath with where to unblock
/// it. Nothing on it is a control; the page behind is `about:blank`.
pub fn draw_blocked_page(ui: &mut Ui, rect: Rect, color: ChromeColor, page: &BlockedPage) {
    let palette = Palette::new(color, ui.visuals().dark_mode);
    let painter = ui.painter();
    // The strip's curve, over the whole page: more bands than the
    // strip's eight, since this is hundreds of pixels tall.
    painter.add(egui::Shape::mesh(gradient_mesh(rect, &palette, 24)));
    let ink = palette.ink_on(palette.strip_at(0.5));
    let muted = ink.gamma_multiply(0.72);
    let medium = FontFamily::Name(theme::MEDIUM.into());

    // The line, as large as the width allows.
    const TRIAL: f32 = 64.0;
    let text = page.text().to_string();
    let trial = painter.layout_no_wrap(text.clone(), FontId::new(TRIAL, medium.clone()), ink);
    let size = fit_size(TRIAL, trial.size().x, (rect.width() - 48.0).max(80.0));
    let line = painter.layout_no_wrap(text, FontId::new(size, medium), ink);
    let line_size = line.size();
    let line_pos = Pos2::new(
        rect.center().x - line_size.x / 2.0,
        rect.center().y - line_size.y / 2.0 - 20.0,
    );
    painter.galley(line_pos, line, ink);

    // Beneath: the site, and where to undo this.
    let site = painter.layout_no_wrap(
        format!("{} is blocked on this profile", page.site),
        FontId::new(15.0, FontFamily::Proportional),
        muted,
    );
    let site_pos = Pos2::new(
        rect.center().x - site.size().x / 2.0,
        line_pos.y + line_size.y + 18.0,
    );
    let site_height = site.size().y;
    painter.galley(site_pos, site, muted);
    let hint = painter.layout_no_wrap(
        "Unblock it under Settings \u{203a} Blocked sites".to_string(),
        FontId::new(12.0, FontFamily::Proportional),
        muted,
    );
    let hint_pos = Pos2::new(
        rect.center().x - hint.size().x / 2.0,
        site_pos.y + site_height + 6.0,
    );
    painter.galley(hint_pos, hint, muted);
}
```

(`Pos2` is already imported; if `Palette::ink_on` or `strip_at` changed visibility since the verification, they are `pub` at `chrome.rs:191` and `:204`.)

**Testing** (`chrome.rs` tests):

- `a_block_line_shrinks_to_its_width_and_no_further`: `fit_size(64.0, 300.0, 400.0) == 64.0`; `fit_size(64.0, 800.0, 400.0) == 32.0`; `fit_size(64.0, 8000.0, 100.0) == 18.0`; `fit_size(64.0, 0.0, 100.0) == 64.0`.
- `the_block_page_draws_narrow_and_wide_without_changing_its_line`: `let ctx = with_fonts();` then for each of `Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 200.0))` and `Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 900.0))`, for each `line` in `0..3u8`: `let page = BlockedPage { site: "youtube.com".to_string(), line };` and twice `let _ = ctx.run_ui(Default::default(), |ui| draw_blocked_page(ui, rect, ChromeColor::default(), &page));`, then `assert_eq!(page.line, line)` (the page is borrowed immutably, so this is the compiler's guarantee stated as a test). The test passes if nothing panics.

**Verification:**
Run: `cargo test -p osmena-chrome`
Expected: pass.
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: Paint the block page where the page would be

**Verifies:** blocked-sites.AC3.10 (input stays in the chrome: `content_is_egui`), blocked-sites.AC3.8 (the address bar shows `Tab::url`, untouched here)

**Files:**
- Modify: `crates/osmena-shell/src/window.rs:872` (read the state), `:930-1008` (the paint match), `:1011` (`content_is_egui`)

**Implementation:**

After `let crashed = model.active_tab().crashed.clone();` (line 872) add

```rust
        let blocked = model.active_tab().blocked.clone();
```

Change the paint match from `match &crashed { Some(reason) => {...} None => {...} }` to a match on both, keeping the existing bodies:

```rust
            match (&crashed, &blocked) {
                (Some(reason), _) => {
                    if chrome::draw_crash_panel(ui, rect, reason) {
                        commands.push(Command::Reload(active));
                    }
                }
                // The site is blocked: the engine shows a blank page
                // behind, the chrome says why. Nothing on it is a
                // control.
                (None, Some(page)) => {
                    chrome::draw_blocked_page(ui, rect, chrome_color, page);
                }
                (None, None) => {
                    // ... the existing paint_page body, unchanged ...
                }
            }
```

(`chrome_color` is the `browser.settings.chrome_color` copy from line 874 and is `Copy`; it is already captured by the closure for `ChromeContext`.)

Change `self.content_is_egui = crashed.is_some();` to `self.content_is_egui = crashed.is_some() || blocked.is_some();`.

**Verification:**

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

Expected: clean and green.

**Commit (Tasks 3-4):**

```bash
cargo fmt --all
git add crates/osmena-chrome/src/chrome.rs crates/osmena-shell/src/window.rs
git commit -m "feat: the block page, painted by the chrome over the blank page" -m "One of three lines, in Inter Medium sized to the width, on the profile colour's gradient, with the site and where to unblock it beneath; drawn where the page would be, like the crash panel, with pointer input kept in the chrome. The line is the tab's state, so it holds between frames and only Reload changes it." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_4 -->
<!-- END_SUBCOMPONENT_B -->

<!-- START_TASK_5 -->
### Task 5: See it in a debug build

**Verifies:** blocked-sites.AC2.3, blocked-sites.AC2.7, blocked-sites.AC3.8, blocked-sites.AC3.9, blocked-sites.AC3.10 by eye (recorded for the handoff in Phase 5; the automated tests above cover what can be asserted).

**Files:** none changed; nothing committed in a browser.

**Step 1: Run Nautilus against the checkout**

In PowerShell (the patch table is active; `CEF_PATH` set):

```
cd C:\Users\tomis\vibes\browser-project\nautilus
$env:RUST_LOG = "info,nautilus_engine=debug"
cargo run -- --profile blocked-probe https://example.com/
```

(`info,` keeps osmena_shell's own lines, such as `profile … at …` and the `--exit-after` report; a bare `nautilus_engine=debug` would drop them.)

**Step 2: Walk through it**

1. Right-click the tab: the menu ends with a separator and "Block on this profile", enabled. Press Ctrl+N: a new window opens on the new-tab URL, `about:blank` by default; right-click its tab: the entry is disabled. Close that window.
2. Pick it on the example.com tab: a dialog titled "Block site" reads "Block example.com on this profile? Are you sure? Blocked sites can only be unblocked from profile settings." with a red "Block" and "Cancel". Press Escape: nothing changes; open it again and click Cancel: nothing changes.
3. Open it again and click Block: the page area turns into the profile colour's gradient with one of the three lines large in the middle, "example.com is blocked on this profile" beneath and the settings hint under that; the address bar still reads `https://example.com/`; the tab's title in the strip is the address too, with no favicon (`Tab::block` cleared them, and `apply` ignores the blank page's title and icon). Clicking anywhere on the page does nothing. Keys go to the chrome (Ctrl+L focuses the address bar).
4. Reload (F5 or the button): the line changes; again: changes again. Resize the window narrow: the line shrinks to fit on one row.
5. Type `example.com/other` in the address bar and press Enter: the block page shows at once; the log shows exactly one `load started` for that tab after the block, which is the blank page the block page sits over (Phase 6 makes that line read `load started at about:blank`), and with `NAUTILUS_CEF_LOG=verbose` no request naming `example.com`. Open a second tab on another site, navigate it to example.com: the block page; Back: the other site returns and the page clears. (Back on the first tab is not telling yet: until Phase 6 Nautilus uses the default `replace_with_blank`, which pushes a blank entry instead of replacing one; Phase 6's Task 3 walks Back after a tab-menu block with the real one.)
6. Open the profile directory (the log's `profile "blocked-probe" at …` line names it): `blocked.toml` holds `sites = ["example.com"]`.
7. Quit. Delete the probe profile directory. `Remove-Item Env:RUST_LOG`. Restore Nautilus's lock: `git checkout -- Cargo.lock`.

Expected: every step as described. Record what was seen, and anything not reached, for Phase 5's handoff paragraph.
<!-- END_TASK_5 -->

---

## Phase 4 done when

- The gate is clean and green.
- The menu tests pass with the new entry (label, enabled state, command, disabled for an already-blocked or host-less target); the prompt's words and its answers (Block, Cancel, neither) are tested; the block page draws at a narrow and a wide rect in a font-loaded egui context without panicking and the sizing rule is tested; the strip's gradient is unchanged by the shared `gradient_mesh`.
- The debug walk-through in Task 5 was done and its outcome is written down for the handoff.
