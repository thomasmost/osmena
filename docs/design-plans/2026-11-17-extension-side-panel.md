# Extension Side Panel Design

## Summary

This design adds a docked side panel to osmena, the shared browser shell,
and makes Nautilus (the Chromium/CEF browser) use it to run Chrome
extensions such as Claude and Raindrop.io. The panel is a fixed-width
region on the right of the page area. It shows a page that the engine
renders, as in `chrome.sidePanel`. In osmena it is a generic capability: a
new `ExtensionSupport::side_panels` flag, defaulted trait methods, and a
second painted rectangle with its own pointer and keyboard routing.
Paladin does not set the flag, so it only picks up a bumped pin.

Nautilus's tabs are windowless CEF browsers that sit in no Chromium
window. Chromium's extension APIs that walk windows or tab strips
(`tabs.query`, `windows`, `tabGroups`, `sidePanel`) therefore cannot see
them, and some calls crash. The design replaces those APIs inside each
extension context with a bridge script. The bridge answers read calls from
a snapshot of osmena's model, which the engine receives after every change
(`strip_changed`). Calls that change the browser become an `EngineEvent`
(`ExtensionRequest`). The pure `apply` function folds that event into the
model and returns a reply that resolves the extension's promise. Calls
addressed by tab id (`scripting`, `debugger`, `webNavigation`) stay with
Chromium, joined to Nautilus tabs through a tab-id table. The bridge fails
closed, so an unknown method rejects instead of reaching the crashing
Chromium path. Tab groups map onto osmena's tab folders, and extension
shortcuts are resolved after the keymap, so the keymap wins. Work runs in
eight phases: a Nautilus spike, the osmena model, the engine contract and
shell, the chrome, the tabs bridge, the panel, groups and commands, then
hand acceptance on macOS and Windows.

## Definition of Done

1. **A generic side panel in osmena.** A fixed-width panel docked on the
   right of the page area, hosting a page the engine renders, with both
   window-level and tab-level panels in the sense of `chrome.sidePanel`.
   An engine that doesn't offer it needs no changes, so Paladin is
   untouched.
2. **Nautilus implements it on macOS and Windows together.**
   - Clicking a toolbar action that has no popup reaches the extension.
   - `chrome.sidePanel` opens and closes the docked panel.
   - Nautilus tabs are real tabs to the extension APIs (`tabs`,
     `tabGroups`, `windows`, `scripting`, `debugger`, `webNavigation`),
     and extensions see every tab in the window, as in Chrome.
   - A tab group an extension creates is a Nautilus tab folder: it takes
     the group's title and follows renames, falls back to
     "<Extension name> Session" when untitled, tracks membership, and
     starts expanded.
3. **Extension keyboard commands work generically.** Any extension's
   declared shortcut fires unless it collides with a Nautilus binding, in
   which case Nautilus wins.
4. **Two acceptance tests pass by hand on both platforms.**
   - Claude (`fcoeoabgfenejglbffodgkkbkcdhcgfn`): opens from the toolbar
     and with Cmd+E; chat works; it owns a folder; it reads and drives the
     active tab and the tabs in its folder (click, type, navigate,
     screenshot).
   - Raindrop.io: its side panel opens and renders.
5. **Out of scope:**
   - resizing the panel or docking it on the left;
   - native-messaging pairing with the desktop app or Claude Code;
   - extensions in Paladin;
   - restoring an extension's session across a relaunch (the folder
     survives as an ordinary folder and the panel starts closed).

## Acceptance Criteria

### side-panel.AC1: The panel in the window

- **side-panel.AC1.1 Success:** With a panel shown, the page area is
  narrower by the panel's fixed width and the panel occupies the right
  edge under the toolbar, with a header naming the extension and a close
  control.
- **side-panel.AC1.2 Success:** Pointer input over the panel reaches the
  panel's page with coordinates from the panel's own top left; input over
  the page reaches the tab as before.
- **side-panel.AC1.3 Success:** A click in the panel gives it the
  keyboard; a click in the page gives it back. Keys that are bound in the
  keymap act on the browser whichever has the keyboard.
- **side-panel.AC1.4 Success:** A panel opened for one tab shows only
  while that tab is active; a panel opened for the window shows on every
  tab that has none of its own.
- **side-panel.AC1.5 Success:** The close control and
  `Command::CloseSidePanel` close the shown panel and the page takes the
  full width again.
- **side-panel.AC1.6 Failure:** An engine with
  `ExtensionSupport::side_panels` false never has a panel drawn, and a
  browser implementing none of the new trait methods still builds.
- **side-panel.AC1.7 Edge:** In full screen no panel is drawn; leaving
  full screen brings it back.

### side-panel.AC2: Actions and shortcuts

- **side-panel.AC2.1 Success:** A pinned extension whose action has no
  popup shows a toolbar button; clicking it runs the extension's
  `chrome.action.onClicked` listeners with the active tab.
- **side-panel.AC2.2 Success:** An extension's declared shortcut that no
  keymap chord uses runs its `chrome.commands.onCommand` listeners.
- **side-panel.AC2.3 Failure:** A declared shortcut equal to a chord in
  the keymap runs the keymap's command and not the extension's.
- **side-panel.AC2.4 Edge:** A click or shortcut for an extension whose
  service worker is stopped starts it and is delivered once.

### side-panel.AC3: Tabs as extensions see them

- **side-panel.AC3.1 Success:** `chrome.tabs.query({})` lists every tab of
  every window in strip order, with `active`, `index`, `windowId` and
  `groupId` matching the model; `{active: true, currentWindow: true}`
  returns the active tab of the window whose panel or action asked.
- **side-panel.AC3.2 Success:** `chrome.tabs.onCreated`, `onUpdated`,
  `onActivated`, `onMoved` and `onRemoved` fire for the model's changes.
- **side-panel.AC3.3 Success:** `chrome.tabs.create`, `remove` and
  `update({active: true})` open, close and activate tabs in the strip, and
  `create` resolves with the id of the tab that exists afterwards.
- **side-panel.AC3.4 Success:** `scripting.executeScript`,
  `tabs.sendMessage`, `tabs.update({url})`, `webNavigation` events and
  `chrome.debugger` (attach, `Runtime.evaluate`, `Page.captureScreenshot`,
  `Input.dispatchMouseEvent`, `Input.dispatchKeyEvent`) work on a tab by
  the id `tabs.query` returned.
- **side-panel.AC3.5 Failure:** `tabs.group`, `tabs.discard`, `tabs.create`
  and every other bridged call never reach Chromium's own implementation:
  the browser does not crash and no Chromium window appears.
- **side-panel.AC3.6 Edge:** A deferred (restored, not yet loaded) tab is
  listed with its URL and title and gets an id when it is first shown.

### side-panel.AC4: Tab groups are folders

- **side-panel.AC4.1 Success:** `chrome.tabs.group({tabIds})` creates a
  folder holding those tabs, expanded, named "<Extension name> Session";
  `chrome.tabGroups.update(id, {title})` renames it and later renames
  follow.
- **side-panel.AC4.2 Success:** `tabGroups.update(id, {collapsed})`
  collapses and expands the folder; `tabs.ungroup` takes tabs out;
  `tabs.group({tabIds, groupId})` adds to an existing folder.
- **side-panel.AC4.3 Success:** A folder the user made is a tab group to
  extensions (`tabGroups.query`, `tab.groupId`), and the user's renames
  and moves fire `tabGroups.onUpdated` and `tabs.onUpdated`.
- **side-panel.AC4.4 Edge:** A folder whose last tab goes is removed and
  `tabGroups.onRemoved` fires.

### side-panel.AC5: chrome.sidePanel

- **side-panel.AC5.1 Success:** `sidePanel.setOptions({tabId, path,
  enabled: true})` then `open({tabId})` shows that page in the panel for
  that tab; `open({windowId})` shows the extension's default panel for the
  window.
- **side-panel.AC5.2 Success:** `runtime.getContexts({contextTypes:
  ['SIDE_PANEL']})` reports the open panel; `window.close()` in the panel
  and `setOptions({enabled: false})` close it.
- **side-panel.AC5.3 Failure:** `open` for a tab or window that does not
  exist rejects with Chromium's message for that case.

### side-panel.AC6: The two extensions, by hand, on macOS and Windows

- **side-panel.AC6.1 Success:** Claude opens from its toolbar button and
  from Cmd+E (Ctrl+E on Windows); a chat message gets an answer.
- **side-panel.AC6.2 Success:** Asked to act on a page, Claude makes a
  folder that takes its conversation's title, reads the active tab,
  clicks, types, navigates and takes a screenshot there and in another tab
  of the folder.
- **side-panel.AC6.3 Success:** Raindrop.io's panel opens from its command
  and renders.
- **side-panel.AC6.4 Success:** Paladin builds and runs against the same
  osmena revision with no panel and no new behaviour.

## Glossary

- **osmena**: The shared browser shell library (model, engine contract,
  egui chrome, winit application) that Paladin and Nautilus depend on by
  git revision.
- **Nautilus**: The browser built on Chromium through CEF. It is where the
  extension work is implemented.
- **Paladin**: The browser built on Servo. It implements none of the new
  methods and is only rebuilt against the new revision.
- **CEF (Chromium Embedded Framework)**: The embedding layer Nautilus uses
  to run Chromium.
- **`Engine` / `TabHost`**: The contract an engine crate implements for
  osmena. `Engine` is browser-wide and `TabHost` is per window. New
  methods are defaulted to do nothing and gated on capability flags.
- **`ExtensionSupport` / `side_panels`**: The capability flags an engine
  reports. When `side_panels` is false, no panel is drawn.
- **`Command`**: A user action emitted by the chrome or keymap, such as
  `CloseSidePanel` or `ExtensionCommand`. The pure `dispatch` function
  applies it to the model.
- **`EngineAction`**: What `dispatch` and `apply` return for the shell to
  forward to the engine, such as `CloseView` or `ExtensionReply`.
- **`EngineEvent` / `apply`**: A report from the engine, such as a page
  asking to open a tab. The pure `apply` function folds it into the model.
- **`ExtensionRequest` / `ExtensionReply`**: A request an extension makes
  of the browser (open tab, group tabs, open side panel) and the answer. A
  token pairs them so the extension's promise can resolve.
- **`SidePanel`**: A model record of an open panel: the extension, the
  page address and the engine view that renders it. It lives on
  `Window::panel` or `Tab::panel`.
- **Window-level and tab-level panel**: The two scopes of
  `chrome.sidePanel`. A window panel shows on every tab that has no panel
  of its own. A tab panel shows only while that tab is active.
- **View**: Something the engine renders under a `TabId`. A tab is a view
  in the strip; the panel's view is in no strip, so the existing paint,
  resize and input methods carry it.
- **`WindowStrip` / `strip_changed`**: A read-only snapshot of each
  window's tabs, folders and shown panel, given to the engine after every
  model change. The engine diffs it to fire `chrome.tabs.on*` events.
- **Windowless (Alloy-style) browser**: How Nautilus renders tabs and the
  panel: off-screen, with no native window, the shell painting the pixels.
  Chromium's window-based tab APIs cannot find these, which is the core
  problem.
- **Service worker**: An extension's background script. Chromium stops it
  when idle; a click or event must wake it and be delivered exactly once.
- **Bridge**: A script injected into each extension context that replaces
  whole `chrome.*` namespaces and routes calls to the engine. It fails
  closed.
- **Loader**: The file the manifest rewrite sets as
  `background.service_worker`. It loads the bridge before the extension's
  original entry file.
- **Relay page**: A page in a hidden browser that carries messages between
  the engine and the bridge in the service worker. It also wakes a stopped
  worker.
- **Tab-id table**: The mapping between a Nautilus tab and Chromium's tab
  id, which is a session id that changes on every launch. It is built by
  joining each tab's DevTools target id with `chrome.debugger.getTargets`,
  called from a helper extension.
- **Popup-less action**: A toolbar action with no popup. Clicking it runs
  the extension's `action.onClicked` listeners with the active tab.
- **`activeTab`**: A permission that a user gesture normally grants. A
  click delivered through the bridge carries no gesture, so an extension
  that relies on `activeTab` can open its panel but not read the page.
- **Keymap**: osmena's table of shortcut chords. A chord in the keymap
  beats an extension's declared shortcut.
- **Tab folder**: osmena's contiguous run of tabs with a name and an
  expanded state. A Chrome tab group maps onto a folder.
- **External message pump**: How CEF asks the application to do its work.
  Extension calls stalled for seconds when CEF stopped asking; Phase 1
  finds the cause.
- **`StrayClient`**: The Nautilus client that adopts a browser Chromium
  opens on its own, turning its URL into a Nautilus tab.
- **`electron-chrome-extensions`**: An existing project whose approach the
  bridge follows: override the `chrome.*` namespace in each context, route
  to the host, wake the worker to deliver events.

## Architecture

Chromium keeps serving every extension API that addresses a tab by id.
Nautilus's tabs are windowless Alloy-style CEF browsers; CEF resolves
them for `tabs.get`, `scripting`, `tabs.sendMessage`, `webNavigation` and
`chrome.debugger`, but they sit in no Chromium window, so every API that
walks windows or tab strips omits them, errors, or crashes. A bridge
script in each extension context replaces exactly those APIs and answers
them from osmena's model, which the engine receives as a snapshot after
every change. What an extension asks the browser to do becomes an
`EngineEvent` that `apply` folds in.

The panel is one more windowless browser. To osmena it is a view with a
`TabId` that is in no strip, so the existing paint, resize and input
methods of `TabHost` carry it; the shell gains a second painted rectangle
and a second place for the keyboard to be.

### Contracts

`osmena-core`, the model:

```rust
pub struct Extension {
    // existing fields …
    /// Set for any toolbar action, with or without a popup.
    pub action_title: Option<String>,
    pub side_panel: bool,
    pub commands: Vec<ExtensionCommand>,
}

pub struct ExtensionCommand {
    pub name: String,
    pub description: String,
    pub chord: Option<Chord>,   // the manifest's suggestion for this platform
}

/// A panel an extension opened. `view` is the engine view that renders it.
pub struct SidePanel {
    pub extension: String,
    pub page: String,           // the engine's address for the page
    pub view: TabId,
}

pub struct Window { /* … */ pub panel: Option<SidePanel> }
pub struct Tab    { /* … */ pub panel: Option<SidePanel> }

impl Window {
    /// The active tab's panel, else the window's.
    pub fn shown_panel(&self) -> Option<&SidePanel>;
}

pub struct ExtensionSupport { /* … */ pub side_panels: bool }
```

`osmena-core`, what crosses the boundary:

```rust
pub enum Command {
    // …
    CloseSidePanel { window: WindowId },
    ExtensionCommand { window: WindowId, extension: String, name: String },
}

pub enum EngineAction {
    // …
    ExtensionCommand { window: WindowId, extension: String, name: String },
    ExtensionReply { token: u64, reply: ExtensionReply },
    CloseView(TabId),           // a panel's view, no longer shown anywhere
}

pub enum EngineEvent {
    // …
    ExtensionRequest { token: u64, extension: String, request: ExtensionRequest },
}

pub enum ExtensionRequest {
    OpenTab { window: Option<WindowId>, url: Url, active: bool },
    CloseTabs(Vec<TabId>),
    ActivateTab(TabId),
    MoveTab { tab: TabId, index: usize },
    GroupTabs { tabs: Vec<TabId>, folder: Option<FolderId> },
    UngroupTabs(Vec<TabId>),
    UpdateFolder { folder: FolderId, name: Option<String>, expanded: Option<bool> },
    OpenWindow { url: Option<Url> },
    OpenSidePanel { window: WindowId, tab: Option<TabId>, page: String, view: TabId },
    CloseSidePanel { window: WindowId, tab: Option<TabId> },
}

pub enum ExtensionReply {
    Done,
    Tab(TabId),
    Folder(FolderId),
    Window(WindowId),
    Refused(String),
}
```

`osmena-engine`, all defaulted to nothing and gated on
`ExtensionSupport::side_panels`:

```rust
/// One window's strip as extensions need it.
pub struct WindowStrip<'a> {
    pub window: WindowId,
    pub focused: bool,
    pub tabs: &'a [StripTab<'a>],       // strip order
    pub folders: &'a [Folder],
    pub shown_panel: Option<&'a SidePanel>,
}
pub struct StripTab<'a> {
    pub id: TabId, pub url: &'a Url, pub title: &'a str,
    pub active: bool, pub folder: Option<FolderId>,
    pub load: LoadState, pub deferred: bool,
}

trait Engine {
    fn strip_changed(&self, _windows: &[WindowStrip<'_>]) {}
    fn extension_reply(&self, _token: u64, _reply: ExtensionReply) {}
}

trait TabHost {
    fn extension_command(&self, _tab: TabId, _extension: &str, _name: &str) {}
    fn close_view(&self, _view: TabId) {}
}
```

A panel's view answers the existing `resize_content`, `paint_page`,
`input`, `set_focused` and `show`/`hide` under its `TabId`. The engine
allocates the id (as it does for a tab a page opens) and reports it in
`OpenSidePanel`.

The bridge's wire format between an extension context and the engine is
Nautilus's own and is settled in Phase 1; nothing outside
`nautilus-engine` sees it.

### Data flow

A click on a popup-less action: chrome → `Command::OpenExtensionAction` →
`dispatch` → `EngineAction::OpenExtensionAction` →
`TabHost::open_extension_action`. The host opens the popup if the action
has one; otherwise it sends the click through the extension's relay page
to the bridge in the service worker, which runs the `onClicked`
listeners with the active tab's object.

An extension call that the bridge owns (`tabs.query`, `tabGroups.get`,
`windows.getAll`, `sidePanel.getOptions`): the bridge asks the engine
through the relay, and the engine answers from the last `strip_changed`
snapshot and its own tab-id table. Nothing reaches osmena.

An extension call that changes the browser (`tabs.create`, `tabs.group`,
`sidePanel.open`): bridge → engine → `EngineEvent::ExtensionRequest` →
`apply` changes the model and returns the ordinary actions (`CreateTab`,
…) plus `ExtensionReply`; the shell forwards the reply to
`Engine::extension_reply`, which resolves the extension's promise. The
`strip_changed` that follows makes the engine fire the matching
`chrome.*` events.

A model change from anywhere (the user closes a tab, renames a folder):
the shell calls `Engine::strip_changed` after `dispatch` or `apply`
returns; the engine diffs against the previous snapshot and sends
`tabs.on*` and `tabGroups.on*` events to every bridged extension that
listens.

A shortcut: `route_key` resolves the keymap first; a chord the keymap
does not hold is looked up among enabled extensions' commands and becomes
`Command::ExtensionCommand`.

### Module map

- `osmena/crates/osmena-core/src/extension.rs`: `ExtensionCommand`,
  `SidePanel`, `ExtensionRequest`, `ExtensionReply`.
- `osmena/crates/osmena-core/src/model.rs`: `Window::panel`, `Tab::panel`,
  `shown_panel`.
- `osmena/crates/osmena-core/src/command.rs`, `event.rs`: the new
  commands and actions, `apply` for `ExtensionRequest`.
- `osmena/crates/osmena-engine/src/lib.rs`: `WindowStrip`, the four
  defaulted methods.
- `osmena/crates/osmena-chrome/src/chrome.rs`, `routing.rs`, `input.rs`:
  the panel region and header, the page and panel rectangles, the pointer
  and key targets.
- `osmena/crates/osmena-shell/src/window.rs`, `app.rs`: painting and
  sizing the panel's view, the keyboard's place, `strip_changed`, replies.
- `nautilus/crates/nautilus-engine/src/bridge/` (new): the bridge script
  and its loader, the relay page and its client, the tab-id table, the
  request router, the event differ.
- `nautilus/crates/nautilus-engine/src/panel.rs` (new): the panel's
  browser.
- `nautilus/crates/nautilus-engine/src/extensions.rs`, `crx.rs`,
  `store.rs`, `host.rs`, `delegate.rs`: manifest reading (side panel,
  commands, the worker entry), the rewrite at install and load, the
  render-process injection, the action click, the helper extension.

## Existing Patterns

Followed:

- **Extensions are the engine's; the model only lists them** (osmena
  `CLAUDE.md`). The panel's page, the bridge and Chromium's ids stay in
  `nautilus-engine`; the model holds which panel is open where.
- **Capability flags on `EngineInfo`, defaulted trait methods.** Every
  newer `Engine` and `TabHost` method is defaulted and gated on a flag
  (`find`, `downloads`, `extensions`); `side_panels` joins
  `ExtensionSupport` the same way, so Paladin changes nothing.
- **What a page asks for is an `EngineEvent`; `apply` decides.** New tabs
  and windows from a page already arrive this way;
  `ExtensionRequest` is the same shape with a token for the answer.
- **Per-window UI state in the model, drawn by the chrome**: `Window::find`
  (`FindState`) is the precedent for `Window::panel`. Like the find bar it
  is not persisted.
- **The crash panel and block page** show how the chrome and
  `BrowserWindow::frame` share the content rectangle; the panel region is
  carved from the same rectangle before the page is sized.
- **`store.rs`**: binding a function in an extension-related V8 context
  from the render-process handler and answering through a process
  message. The bridge in extension pages and the relay page use it.
- **`extensions.rs` rewrites the unpacked manifest** (the publisher's
  `key`) at install. Pointing `background.service_worker` at a loader is
  the same step.
- **Tabs created by something other than the user get a `TabId` from the
  engine** and are reported to the model; the panel's view does the same.

New:

- **A second engine surface in one window.** Until now a window paints
  and routes input to exactly one view, the active tab; the chrome and
  page are divided by one y coordinate (`content_top_px`). The shell
  learns a rectangle per view. The page still starts at x = 0, which is
  why the panel docks on the right.
- **The engine sees the strip.** No engine has needed tab order, the
  active tab or folders before. `strip_changed` hands a read-only
  snapshot; the engine never changes the model except by events.
- **Replacing `chrome.*` in extension contexts.** Nothing in Nautilus
  does this today. The shape follows `electron-chrome-extensions`
  (override the namespace in each context, route to the host, wake the
  worker to deliver events), which covers `tabs`, `windows` and `action`
  but not `sidePanel`, `tabGroups` or `debugger`.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: The spike in Nautilus

**Goal:** Prove the four things the rest depends on, in `nautilus` alone
and against the pinned osmena, and fix the message-pump stall.

**Components:**
- `nautilus/crates/nautilus-engine/src/bridge/` — a minimal loader and
  bridge that wrap an unpacked extension's service worker entry, a relay
  page hosted in a hidden browser, and the channel between relay and
  engine; enough to deliver one `action.onClicked` to a stopped worker
  and to answer one bridged call (`tabs.query`).
- The tab-id table: each tab's DevTools target id
  (`Target.getTargetInfo` on its own browser) joined with Chromium's tab
  id, through a helper extension shipped in the app's resources that may
  call `chrome.debugger.getTargets`. If a route without a helper
  extension turns up, take it.
- `nautilus/crates/nautilus-engine/src/engine.rs` — the root cause of
  extension calls stalling 5 to 100 s when CEF stops asking the external
  message pump for work, and its fix.
- `nautilus/crates/nautilus-engine/src/extensions.rs`, `host.rs` — the
  manifest's action without popup reported with an `action_title`; the
  click reaching the relay.
- `nautilus/docs/upstream-issues.md` — the crashes and limits found.

**Dependencies:** None.

**Done when:** With a test extension loaded unpacked, a scripted run
shows (a) the bridge present in the worker after a browser restart and
after the worker was stopped, (b) an action click delivered to a stopped
worker exactly once, (c) `tabs.query({})` answered by the engine with
ids that `scripting.executeScript` and `debugger.attach` then accept,
(d) no API call taking over a second with the window unfocused. Unit
tests cover the manifest rewrite and the table's join. Covers
side-panel.AC2.4 in part.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: The model, the requests and the commands

**Goal:** osmena's model knows panels, extension commands and extension
requests.

**Components:**
- `osmena/crates/osmena-core/src/extension.rs` — `ExtensionCommand`,
  `SidePanel`, `ExtensionRequest`, `ExtensionReply`; `side_panel` and
  `commands` on `Extension`; `has_action` true for any action.
- `osmena/crates/osmena-core/src/model.rs` — `Window::panel`,
  `Tab::panel`, `shown_panel`; panels dropped with their tab or window.
- `osmena/crates/osmena-core/src/command.rs` — `CloseSidePanel`,
  `ExtensionCommand`, `EngineAction::{ExtensionCommand, ExtensionReply,
  CloseView}`; `OpenExtensionAction` no longer needs a popup.
- `osmena/crates/osmena-core/src/event.rs` — `apply` for every
  `ExtensionRequest` variant, reusing the tab, folder and window
  operations the commands use, with the fallback folder name.
- `osmena/crates/osmena-core/src/brand.rs` — `ExtensionSupport::side_panels`.

**Dependencies:** None (independent of Phase 1).

**Done when:** `cargo test` passes with new tests for each request's
effect and reply, panel precedence (tab over window), panels closed with
their tab, the folder fallback name and rename, a refused request for an
unknown tab or folder, and the blocked-sites guard applying to
`OpenTab`. Covers side-panel.AC1.4, AC1.5, AC4.1, AC4.2, AC4.4 at the
model level.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: The engine contract and the shell

**Goal:** The shell paints, sizes and routes input to a second view, and
keeps the engine told of the strip.

**Components:**
- `osmena/crates/osmena-engine/src/lib.rs` — `WindowStrip`, `StripTab`,
  `Engine::{strip_changed, extension_reply}`,
  `TabHost::{extension_command, close_view}`.
- `osmena/crates/osmena-chrome/src/routing.rs`, `input.rs` — the page and
  panel rectangles from one content rectangle; `route_pointer` and
  `PointerCapture` with a view target; `route_key` with the keyboard's
  place (page or panel) and extension commands after the keymap.
- `osmena/crates/osmena-shell/src/window.rs` — `resize_content`,
  `paint_page` and textures for the panel's view; focus moving between
  the two; held keys and buttons released as for tabs.
- `osmena/crates/osmena-shell/src/app.rs` — `strip_changed` after
  `dispatch` and `apply`, the new actions forwarded.
- `osmena/README.md` — the engine contract.

**Dependencies:** Phase 2.

**Done when:** `cargo test` and clippy pass with tests for the two
rectangles, pointer routing across the boundary, capture across it, key
routing with the panel focused, and extension chords losing to the
keymap; both browsers build against the checkout with none of the new
methods implemented. Covers side-panel.AC1.2, AC1.3, AC1.6, AC2.3.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: The chrome

**Goal:** The panel region and its controls are drawn.

**Components:**
- `osmena/crates/osmena-chrome/src/chrome.rs` — the right-hand region of
  fixed width with its header (extension icon and name, close), drawn
  when the window shows a panel and not in full screen; action buttons
  for popup-less actions.
- `osmena/crates/osmena-chrome/src/menus.rs` — the extensions menu
  listing popup-less actions; the macOS menu bar unchanged otherwise.
- `osmena/crates/osmena-chrome/src/theme.rs` — the panel's width and
  header through `Palette`.
- `osmena/crates/osmena-shell/src/settings_window.rs` — each extension's
  shortcuts listed, with the ones the keymap overrides marked.
- `osmena/CHANGELOG.md`.

**Dependencies:** Phase 3.

**Done when:** Tests cover the region's rectangle at several window
sizes, its absence in full screen and without `side_panels`, and the
menu's entries; a fake engine view shows in a debug browser's panel
region. Covers side-panel.AC1.1, AC1.5, AC1.7.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: The bridge for tabs and windows

**Goal:** Extensions see and change Nautilus's tabs and windows.

**Components:**
- `nautilus/crates/nautilus-engine/src/bridge/` — the full bridge for
  `chrome.tabs` (query, create, remove, move, duplicate, discard,
  captureVisibleTab, the events, and the strip fields on every tab object
  Chromium returns) and `chrome.windows`; the router from bridge calls to
  the snapshot or to `ExtensionRequest`; the differ from snapshots to
  events; injection into extension pages from the render-process handler.
- `nautilus/crates/nautilus-engine/src/engine.rs` — `strip_changed`,
  `extension_reply`, osmena's `rev` bumped.
- `nautilus/crates/nautilus-engine/src/extensions.rs`, `crx.rs` — the
  rewrite applied at install, at load, and once to extensions already
  unpacked.

**Dependencies:** Phases 1 and 3.

**Done when:** Unit tests cover the router and the differ; a scripted run
with the test extension shows side-panel.AC3.1 to AC3.6, including that
`tabs.group`, `tabs.discard` and `tabs.create` cause no crash and no
Chromium window.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: The side panel in Nautilus

**Goal:** `chrome.sidePanel` shows an extension's page in the panel.

**Components:**
- `nautilus/crates/nautilus-engine/src/panel.rs` — a windowless browser
  per open panel, registered with the host as a view under its own
  `TabId`, reusing the tab's render handler, `frame.rs` and `input.rs`.
- `nautilus/crates/nautilus-engine/src/bridge/` — `chrome.sidePanel`
  (setOptions, getOptions, setPanelBehavior, open, the default path from
  the manifest) and `runtime.getContexts` reporting the panel.
- `nautilus/crates/nautilus-engine/src/host.rs` — `close_view`; the
  action click opening the panel when `setPanelBehavior` asked for it.
- `nautilus/crates/nautilus-engine/src/engine.rs` —
  `INFO.extensions.side_panels`.

**Dependencies:** Phases 4 and 5.

**Done when:** A scripted run with the test extension shows
side-panel.AC5.1 to AC5.3 and AC1.1 to AC1.4 in the real window on
macOS; the chrome-extensions-samples `cookbook.sidepanel-global` and
`cookbook.sidepanel-site-specific` open their panels.
<!-- END_PHASE_6 -->

<!-- START_PHASE_7 -->
### Phase 7: Groups, commands, and what Claude needs

**Goal:** Tab groups are folders, shortcuts fire, and a tab being driven
answers promptly.

**Components:**
- `nautilus/crates/nautilus-engine/src/bridge/` — `chrome.tabGroups`,
  `tabs.group` and `tabs.ungroup`, group events; `chrome.commands`
  (getAll from the manifest and the keymap's verdict, `onCommand`).
- `nautilus/crates/nautilus-engine/src/host.rs` — `extension_command`.
- `nautilus/crates/nautilus-engine/src/extensions.rs` — `commands` read
  from the manifest with the platform's suggested key.
- `nautilus/crates/nautilus-engine/src/host.rs`, `delegate.rs` — a tab
  with a debugger attached kept painting while hidden, so a screenshot of
  a tab in the folder that is not the active one does not wait.

**Dependencies:** Phase 6.

**Done when:** A scripted run shows side-panel.AC4.1 to AC4.4, AC2.1 and
AC2.2, and a screenshot of a hidden tab returning in under a second;
unit tests cover the group mapping and the manifest's commands.
<!-- END_PHASE_7 -->

<!-- START_PHASE_8 -->
### Phase 8: Acceptance and the record

**Goal:** Both extensions work by hand on both platforms and the
repositories say so.

**Components:**
- The manual checklist in `nautilus/README.md` — a side panel item
  (Claude, Raindrop.io) and the known limitations.
- `nautilus/CHANGELOG.md`, `nautilus/CLAUDE.md` and `AGENTS.md` — the
  bridge's conventions and a handoff of what was and was not verified.
- `osmena/CLAUDE.md` and `AGENTS.md`, `osmena/README.md` — the side panel
  convention and the contract.
- `paladin/Cargo.toml` and `paladin/CHANGELOG.md` — the pin bump.
- Fixes for what the hand run finds, each with a test where one fits.

**Dependencies:** Phase 7.

**Done when:** side-panel.AC6.1 to AC6.4 are checked by hand on macOS and
on Windows and recorded in the handoff, with anything not verified named;
CI is green in all three repositories.
<!-- END_PHASE_8 -->

## Additional Considerations

**Evidence this design rests on (2026-11-17).** A probe build of
Nautilus on macOS (CEF 154.0.34) with a test extension established, over
ten runs: `tabs.get`, `scripting.executeScript`, `tabs.sendMessage`,
`tabs.update({url})`, `tabs.reload`, `webNavigation` events and
`getAllFrames`, and `chrome.debugger` (`attach` by tab id and by target
id, `Runtime.evaluate`, `Page.captureScreenshot` returning a real image,
`Input.dispatchMouseEvent` seen by the page as trusted) all work on a
Nautilus tab. A tab's extension id is Chromium's session id, different
on every launch, not `CefBrowser::GetIdentifier()`; `tabs.query`,
`windows.*` and `tabGroups.query` see only the hidden management
browser; no `chrome.tabs` event fires for a Nautilus tab. `tabs.group`
and `tabs.discard` on a Nautilus tab crash the browser process;
`tabs.create` opens a Chromium window for about 100 ms before
`StrayClient` adopts its URL, and the id it returns is dead.
`sidePanel.setOptions` stores options; `sidePanel.open` rejects a
Nautilus tab id and showed nothing for the management window.
`chrome.action.onClicked.dispatch` and `chrome.commands.onCommand.dispatch`
called inside the worker run the extension's listeners. Reading CEF at
branch 8037 (commit 14c5a089) agrees: one fallback in
`ExtensionTabUtil::GetTabById` finds Alloy-style browsers and nothing
else knows them; CEF's maintainer has said general tab API support for
Alloy style is not planned. Chrome-style browsers cannot be windowless
and cannot have a native parent on macOS, which is why the tabs stay as
they are.

**Not yet proven, and where it is settled.** The relay page waking a
stopped worker, the wrapped entry surviving worker restarts, and the
tab-id join are Phase 1. Whether the pump stall happens with a focused
window is unknown; Phase 1 finds the cause either way. Nothing here has
run on Windows; Phase 8 is the first time it must.

**Accepted limits.**
- A delivered click carries no user gesture, so no `activeTab` grant
  comes with it. Extensions with host permissions are unaffected; one
  that relies on `activeTab` can open its panel but not read the page.
- Extensions installed before this change are rewritten on disk once.
  The rewrite keeps the original entry file and only adds the loader and
  bridge, so removing them restores the extension.
- `tabGroups` colours are accepted and ignored; folders have none.
- The panel has no IME, as pages have none (osmena open item 3).
- A panel is not restored after a relaunch; its folder is an ordinary
  folder.
- Extension pages opened as tabs and action popups get the bridge too,
  so a popup's `tabs.query({active: true})` starts returning the real
  active tab. That removes a limitation the README records today.

**Error handling.** A bridged call for a tab, window or folder the model
does not have rejects with Chromium's own message for that case
("No tab with id: N."), so extensions' existing error paths apply. A
request `apply` refuses returns `ExtensionReply::Refused` with the same
kind of message. If the relay page or the worker is gone the engine
recreates it and retries once; a second failure is logged and the click
or event is dropped.

**The bridge must fail closed.** Because two unbridged calls crash the
browser, the bridge replaces whole namespaces (`tabs`, `windows`,
`tabGroups`, `sidePanel`) and forwards the id-addressed methods to
Chromium by name; a method it does not know rejects as unsupported
rather than falling through.
