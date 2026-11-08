# Blocked Sites Implementation Plan

**Goal:** Per-profile blocked sites, keyed by registrable domain, that both browsers refuse to show: the list, the model, the chrome, the settings section, and a before-navigate hook in each engine.

**Architecture:** `osmena-core` gains a `blocked` module (the list, the reduction of a URL to its registrable domain through the `psl` crate, the TOML file, the block page's state) and `Tab::blocked`/`Browser::blocked`; `dispatch` guards every navigation the user starts and `apply` folds the engine's `NavigationBlocked` into the same state. The engine contract grows two defaulted methods; the chrome paints the block page with egui where the page would be; each engine cancels blocked navigations in its own hook.

**Tech Stack:** Rust 1.97.1 (edition 2024, `rust-version` 1.88), `url` 2.5, `toml` 1.1, `serde` 1, new dependency `psl` 2 (compiled-in public suffix list, no network, MIT OR Apache-2.0), egui 0.34.3 (chrome), cef 154.5.0 (Nautilus), servo 0.7.0 fork (Paladin).

**Scope:** 7 phases from the design `docs/design-plans/2026-11-07-blocked-sites.md`. This file is Phase 1 of 7.

**Codebase verified:** 2026-11-07 (osmena `1bdea01` on branch `blocked-sites`; nautilus `1df5bc8`; paladin `cc9a89f`).

---

## Acceptance Criteria Coverage

This phase implements and tests:

### blocked-sites.AC1: The list and the matching
- **blocked-sites.AC1.1 Success:** `www.`, `m.` and deeper subdomains of a site reduce to one entry (`youtube.com`).
- **blocked-sites.AC1.2 Success:** Multi-label public suffixes hold: `bbc.co.uk` stays `bbc.co.uk`, never `co.uk`.
- **blocked-sites.AC1.3 Edge:** IPv4, IPv6 and `localhost` hosts are kept whole.
- **blocked-sites.AC1.4 Failure:** `about:`, `data:`, `view-source:`, `file:` and other host-less URLs reduce to nothing and are never blockable.
- **blocked-sites.AC1.5 Success:** `blocked.toml` round-trips; a missing file is an empty list; an unreadable file logs a warning, starts empty, and the browser still starts.

(AC1.5's "logs a warning and the browser still starts" is the shell's side, finished in Phase 3; this phase delivers the `load` behaviour it relies on: a missing file is `Ok(empty)`, a bad file is `Err`.)

---

## Context for the engineer

All paths in this phase are under `C:\Users\tomis\vibes\browser-project\osmena\`. Work on branch `blocked-sites`.

**How this repository verifies work** (from `CLAUDE.md:44-46`, CI runs the same):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

Run `cargo fmt --all` before every commit. Commit messages are Conventional Commits with a lower-case, sentence-like subject (`feat: ...`, `test: ...`, `chore: ...`), a short prose body saying why, and the attribution trailer the executing session is given (`Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` when this plan was written; the commit commands below spell that one out, so replace it if the session's differs).

**Testing conventions:** unit tests live inline in `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of each file. Test names are long snake_case sentences without a `test_` prefix (`levels_are_per_host_and_the_default_is_no_entry`). Plain `assert_eq!`/`assert!`. File round-trips use `tempfile::tempdir()` (already a dev-dependency of `osmena-core`). The module `crates/osmena-core/src/zoom.rs` is the template this phase copies: a per-site table in its own TOML file, with a hand-written three-variant error enum (no `thiserror` in this workspace), `parse`, `to_toml`, `load` (a missing file is the default) and `save` through `crate::settings::write_atomically`.

**Facts about `psl` 2.1.248 established by a probe on this machine (Rust 1.97.1):** `psl::domain(name: &[u8]) -> Option<psl::Domain>`; `Domain::as_bytes()`; `psl::suffix(name) -> Option<psl::Suffix>`; `Suffix::is_known() -> bool`. It does **not** lowercase (`WWW.Example.COM` is mis-parsed), so feed it lowercase. A bare known suffix (`co.uk`, `com`, `github.io`) gives `domain() == None` with `suffix().is_known() == true`. A single unknown label (`localhost`, `intranet`) gives `domain() == None` with an unknown suffix. An unknown TLD (`host.internal`) gives `domain() == Some("host.internal")`. Private suffixes work (`user.github.io` stays `user.github.io`). IP literals are garbage to it (`127.0.0.1` becomes `0.1`), so IPs are kept whole through `url::Host` before `psl` is asked. A fresh debug build with `psl` added took about 10 s. `url::Url::host()` already lowercases `Host::Domain` and returns `None` for `about:`, `data:`, `view-source:` and `file:///C:/x`.

---

<!-- START_TASK_1 -->
### Task 1: The `psl` dependency and an `about_blank()` helper

**Verifies:** None (infrastructure).

**Files:**
- Modify: `crates/osmena-core/Cargo.toml:10-15` (the `[dependencies]` table)
- Modify: `crates/osmena-core/src/model.rs:77-83` (beside `site_key`)
- Modify: `crates/osmena-core/src/model.rs:508` and `crates/osmena-core/src/event.rs:337` (the two inline `about:blank` parses)
- Modify: `crates/osmena-core/src/lib.rs:65-68` (the `pub use model::{...}` line)

**Step 1: Add the dependency**

In `crates/osmena-core/Cargo.toml` the `[dependencies]` table currently reads:

```toml
[dependencies]
directories = "6.0.0"
keyboard-types = { workspace = true }
serde = { workspace = true }
toml = "1.1"
url = { workspace = true }
```

Add `psl` between `keyboard-types` and `serde` (alphabetical):

```toml
[dependencies]
directories = "6.0.0"
keyboard-types = { workspace = true }
# The public suffix list, compiled in: what tells `bbc.co.uk` apart from
# `co.uk` when a URL is reduced to the site it belongs to (`blocked.rs`).
psl = "2"
serde = { workspace = true }
toml = "1.1"
url = { workspace = true }
```

**Step 2: Add `about_blank()` to `model.rs`**

Directly after `site_key` (which ends at `crates/osmena-core/src/model.rs:83`) add:

```rust
/// The empty page every engine shows without a request.
pub fn about_blank() -> Url {
    Url::parse("about:blank").expect("about:blank parses")
}
```

Then replace the two existing inline parses with it:
- `crates/osmena-core/src/model.rs:508` (inside `Browser::new_tab_url`): the expression `Url::parse("about:blank").expect("about:blank parses")` becomes `about_blank()`.
- `crates/osmena-core/src/event.rs:337` (inside the `NewTabRequested` arm): the same expression becomes `crate::model::about_blank()`.

**Step 3: Re-export it**

In `crates/osmena-core/src/lib.rs` the model re-export reads:

```rust
pub use model::{
    Browser, Favicon, FindState, Folder, FolderId, LoadState, RgbaImage, StripItem, Tab, TabId,
    Window, WindowId, site_key,
};
```

Change it to:

```rust
pub use model::{
    Browser, Favicon, FindState, Folder, FolderId, LoadState, RgbaImage, StripItem, Tab, TabId,
    Window, WindowId, about_blank, site_key,
};
```

**Step 4: Verify operationally**

Run: `cargo build -p osmena-core`
Expected: builds; `Cargo.lock` gains `psl` and `psl-types`.

Run: `cargo test -p osmena-core`
Expected: all existing tests pass (nothing changed behaviour).

**Step 5: Commit**

```bash
cargo fmt --all
git add Cargo.lock crates/osmena-core/Cargo.toml crates/osmena-core/src/model.rs crates/osmena-core/src/event.rs crates/osmena-core/src/lib.rs
git commit -m "chore: take the psl crate into osmena-core, and name about:blank once" -m "Blocked sites are keyed by registrable domain, which the public suffix list decides; psl compiles it in with no network. about_blank() replaces the two inline parses so the block page can use it too." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_1 -->

<!-- START_SUBCOMPONENT_A (tasks 2-4) -->
<!-- START_TASK_2 -->
### Task 2: `BlockedSites`: the set, the reduction, and the match

**Verifies:** blocked-sites.AC1.1, blocked-sites.AC1.2, blocked-sites.AC1.3, blocked-sites.AC1.4

**Files:**
- Create: `crates/osmena-core/src/blocked.rs`
- Modify: `crates/osmena-core/src/lib.rs:12-30` (module list) and the re-export block after it
- Test: `crates/osmena-core/src/blocked.rs` (unit, inline `mod tests`)

**Implementation:**

Create `crates/osmena-core/src/blocked.rs` with this content (the file section and the block-page section are added in Tasks 3 and 4):

```rust
//! Blocked sites: the websites a profile refuses to show (`blocked.toml`
//! in the profile). A site is a registrable domain (`youtube.com`, never
//! `www.youtube.com`), which the public suffix list decides, so an
//! entry covers every subdomain. A tab on one shows the block page
//! (`BlockedPage`) over `about:blank`; nothing is requested from the
//! site.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

/// The sites a profile blocks, each a registrable domain in lower case.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockedSites {
    pub sites: BTreeSet<String>,
}

impl BlockedSites {
    /// The site `url` belongs to: its registrable domain (`youtube.com`
    /// for `m.youtube.com`, `bbc.co.uk` for `news.bbc.co.uk`). An IP
    /// address or a single-label host (`localhost`) is kept whole. `None`
    /// for anything that is not a web page (`about:`, `data:`,
    /// `view-source:`, `file:`, an engine's own `chrome:` or
    /// `devtools:` pages, an extension's) and for a bare public suffix
    /// (`co.uk`), none of which is a site.
    pub fn site_for(url: &Url) -> Option<String> {
        // Only web pages have sites: `url` gives `file://server/x`,
        // `chrome://extensions/` and `devtools://devtools/x` a host too,
        // and those must never be blockable.
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        match url.host()? {
            Host::Ipv4(_) | Host::Ipv6(_) => url.host_str().map(str::to_owned),
            Host::Domain(domain) => {
                // `Url` lowercases domains already; `psl` does not, so
                // make sure, and drop a trailing dot it would keep.
                let domain = domain.trim_end_matches('.').to_ascii_lowercase();
                if domain.is_empty() {
                    return None;
                }
                match psl::domain(domain.as_bytes()) {
                    Some(site) => std::str::from_utf8(site.as_bytes())
                        .ok()
                        .map(str::to_owned),
                    // No registrable domain: a public suffix on its own
                    // (`co.uk`) is no site, a single label the list does
                    // not know (`localhost`, `intranet`) is one, whole.
                    None if psl::suffix(domain.as_bytes()).is_some_and(|s| s.is_known()) => None,
                    None => Some(domain),
                }
            }
        }
    }

    /// The blocked site `url` falls under, if any.
    pub fn blocks(&self, url: &Url) -> Option<&str> {
        let site = Self::site_for(url)?;
        self.sites.get(&site).map(String::as_str)
    }

    /// Whether `url` can be blocked at all: it has a site.
    pub fn is_blockable(url: &Url) -> bool {
        Self::site_for(url).is_some()
    }

    /// Block `site`. Returns whether the list changed.
    pub fn add(&mut self, site: &str) -> bool {
        self.sites.insert(site.to_string())
    }

    /// Unblock `site`. Returns whether the list changed.
    pub fn remove(&mut self, site: &str) -> bool {
        self.sites.remove(site)
    }

    /// The distinct sites among `urls` that are not blocked yet, in the
    /// order first met: what a "Block" entry or prompt would add.
    pub fn sites_to_block<'a>(&self, urls: impl IntoIterator<Item = &'a Url>) -> Vec<String> {
        let mut sites: Vec<String> = Vec::new();
        for url in urls {
            if let Some(site) = Self::site_for(url)
                && !self.sites.contains(&site)
                && !sites.contains(&site)
            {
                sites.push(site);
            }
        }
        sites
    }
}
```

Register the module in `crates/osmena-core/src/lib.rs`. The module list is alphabetical; add `pub mod blocked;` as the first line of the list, before `pub mod bookmarks;`. Add the re-export as the first `pub use` line, before `pub use bookmarks::{...}`:

```rust
pub use blocked::BlockedSites;
```

(Tasks 3 and 4 extend this line.)

**Testing:**

Add at the bottom of `blocked.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn site(s: &str) -> Option<String> {
        BlockedSites::site_for(&Url::parse(s).unwrap())
    }

    #[test]
    fn subdomains_reduce_to_the_registrable_domain() {
        // AC1.1
        assert_eq!(site("https://www.youtube.com/watch").as_deref(), Some("youtube.com"));
        assert_eq!(site("https://m.youtube.com/").as_deref(), Some("youtube.com"));
        assert_eq!(site("https://a.b.c.youtube.com/").as_deref(), Some("youtube.com"));
        assert_eq!(site("https://youtube.com/").as_deref(), Some("youtube.com"));
        // Case and a trailing dot make no difference.
        assert_eq!(site("https://WWW.YouTube.COM/").as_deref(), Some("youtube.com"));
        assert_eq!(site("https://www.youtube.com./").as_deref(), Some("youtube.com"));
        // Punycode passes through unchanged.
        assert_eq!(
            site("https://www.xn--85x722f.xn--55qx5d.cn/").as_deref(),
            Some("xn--85x722f.xn--55qx5d.cn")
        );
    }

    #[test]
    fn multi_label_public_suffixes_hold() {
        // AC1.2
        assert_eq!(site("https://news.bbc.co.uk/").as_deref(), Some("bbc.co.uk"));
        assert_eq!(site("https://bbc.co.uk/").as_deref(), Some("bbc.co.uk"));
        assert_eq!(site("https://co.uk/"), None);
        assert_eq!(site("https://com/"), None);
        // Private suffixes count too.
        assert_eq!(site("https://user.github.io/x").as_deref(), Some("user.github.io"));
        assert_eq!(site("https://github.io/"), None);
    }

    #[test]
    fn addresses_and_single_labels_are_kept_whole() {
        // AC1.3
        assert_eq!(site("http://127.0.0.1:8080/x").as_deref(), Some("127.0.0.1"));
        assert_eq!(site("http://[::1]/").as_deref(), Some("[::1]"));
        assert_eq!(site("http://localhost/").as_deref(), Some("localhost"));
        assert_eq!(site("http://intranet/").as_deref(), Some("intranet"));
        // An unknown top-level domain still gives the last two labels.
        assert_eq!(site("http://host.internal/").as_deref(), Some("host.internal"));
    }

    #[test]
    fn host_less_urls_are_no_site() {
        // AC1.4: no host, or a host that is not a web page's (a UNC
        // path, an engine's own pages, an extension's pages).
        for u in [
            "about:blank",
            "data:text/plain,hi",
            "view-source:https://a.example/",
            "file:///C:/x.html",
            "file://server/share/x.html",
            "javascript:void(0)",
            "chrome://extensions/",
            "devtools://devtools/bundled/inspector.html",
            "chrome-extension://abcdefghijklmnop/popup.html",
        ] {
            assert_eq!(site(u), None, "{u}");
            assert!(!BlockedSites::is_blockable(&Url::parse(u).unwrap()), "{u}");
        }
    }

    #[test]
    fn a_blocked_site_covers_its_subdomains_and_nothing_else() {
        let mut b = BlockedSites::default();
        assert!(b.add("youtube.com"));
        assert!(!b.add("youtube.com"));
        let yt = Url::parse("https://m.youtube.com/watch").unwrap();
        let other = Url::parse("https://notyoutube.com/").unwrap();
        assert_eq!(b.blocks(&yt), Some("youtube.com"));
        assert_eq!(b.blocks(&other), None);
        assert_eq!(b.blocks(&Url::parse("about:blank").unwrap()), None);
        assert!(b.remove("youtube.com"));
        assert!(!b.remove("youtube.com"));
        assert_eq!(b.blocks(&yt), None);
    }

    #[test]
    fn the_sites_to_block_are_distinct_and_not_yet_blocked() {
        let mut b = BlockedSites::default();
        b.add("a.example");
        let urls = [
            Url::parse("https://www.a.example/").unwrap(),
            Url::parse("https://b.example/1").unwrap(),
            Url::parse("https://m.b.example/2").unwrap(),
            Url::parse("about:blank").unwrap(),
            Url::parse("https://c.example/").unwrap(),
        ];
        assert_eq!(b.sites_to_block(&urls), vec!["b.example", "c.example"]);
        assert!(b.sites_to_block(&urls[..1]).is_empty());
    }
}
```

**Verification:**
Run: `cargo test -p osmena-core blocked`
Expected: the six tests pass.

**Commit:** after Task 4 (one commit for the module).
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: `blocked.toml`: parse, serialise, load, save; `Profile::blocked_path`

**Verifies:** blocked-sites.AC1.5 (the file side)

**Files:**
- Modify: `crates/osmena-core/src/blocked.rs` (add the error enum and the file methods)
- Modify: `crates/osmena-core/src/profile.rs:605-608` (after `zoom_path`)
- Modify: `crates/osmena-core/src/lib.rs` (re-export `BlockedError`)
- Test: `crates/osmena-core/src/blocked.rs`, `crates/osmena-core/src/profile.rs`

**Implementation:**

In `blocked.rs`, extend the imports to:

```rust
use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::settings::write_atomically;
```

Add, between the struct and its `impl` block, the error type (the same shape as `ZoomError` in `zoom.rs:68-85`):

```rust
#[derive(Debug)]
pub enum BlockedError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for BlockedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockedError::Io(e) => write!(f, "cannot read or write the blocked sites: {e}"),
            BlockedError::Parse(e) => write!(f, "the blocked sites file is not valid: {e}"),
            BlockedError::Serialize(e) => write!(f, "cannot write the blocked sites: {e}"),
        }
    }
}

impl std::error::Error for BlockedError {}
```

Add to `impl BlockedSites`, after `sites_to_block`:

```rust
    pub fn parse(text: &str) -> Result<BlockedSites, BlockedError> {
        toml::from_str(text).map_err(BlockedError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, BlockedError> {
        toml::to_string_pretty(self).map_err(BlockedError::Serialize)
    }

    /// Read the file; a missing file is no blocked sites.
    pub fn load(path: &Path) -> Result<BlockedSites, BlockedError> {
        match std::fs::read_to_string(path) {
            Ok(text) => BlockedSites::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(BlockedSites::default()),
            Err(e) => Err(BlockedError::Io(e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), BlockedError> {
        let text = self.to_toml()?;
        write_atomically(path, &text).map_err(BlockedError::Io)
    }
```

In `crates/osmena-core/src/profile.rs`, directly after `zoom_path` (lines 605-608), add:

```rust
    /// The sites this profile refuses to show (`crate::blocked`).
    pub fn blocked_path(&self) -> PathBuf {
        self.dir.join("blocked.toml")
    }
```

In `lib.rs` the re-export becomes `pub use blocked::{BlockedError, BlockedSites};`.

**Testing:**

Add to `mod tests` in `blocked.rs`:

```rust
    #[test]
    fn round_trips_through_toml_and_files() {
        // AC1.5
        let mut b = BlockedSites::default();
        b.add("youtube.com");
        b.add("bbc.co.uk");
        let text = b.to_toml().unwrap();
        assert!(text.contains("sites = ["), "{text}");
        assert!(text.contains("\"bbc.co.uk\""), "{text}");
        assert_eq!(BlockedSites::parse(&text).unwrap(), b);
        assert_eq!(BlockedSites::parse("").unwrap(), BlockedSites::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked.toml");
        assert_eq!(BlockedSites::load(&path).unwrap(), BlockedSites::default());
        b.save(&path).unwrap();
        assert_eq!(BlockedSites::load(&path).unwrap(), b);
        std::fs::write(&path, "sites = 3").unwrap();
        assert!(matches!(BlockedSites::load(&path), Err(BlockedError::Parse(_))));
    }
```

Add to the existing `mod tests` in `crates/osmena-core/src/profile.rs` (it starts near line 867 and already imports `super::*` and `PathBuf`):

```rust
    #[test]
    fn the_blocked_sites_file_sits_in_the_profile() {
        let p = Profile {
            name: "x".to_string(),
            dir: PathBuf::from("/p"),
            engine_dir: PathBuf::from("/p/engine"),
            guest: false,
        };
        assert_eq!(p.blocked_path(), PathBuf::from("/p").join("blocked.toml"));
    }
```

**Verification:**
Run: `cargo test -p osmena-core blocked`
Expected: all pass, including `round_trips_through_toml_and_files` and `the_blocked_sites_file_sits_in_the_profile`.
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: The block page's state: `BLOCKED_LINES`, `pick_line`, `BlockedPage`, `line_seed`

**Verifies:** None of AC1 directly (used by AC3.7, AC3.8 and AC3.9 in later phases); the design's Phase 1 "Done when" asks for `pick_line` over fixed seeds landing on every line.

**Files:**
- Modify: `crates/osmena-core/src/blocked.rs`
- Modify: `crates/osmena-core/src/lib.rs` (re-exports)
- Test: `crates/osmena-core/src/blocked.rs`

**Implementation:**

Add to `blocked.rs`, after the `impl BlockedSites` block:

```rust
/// What the block page says, one line at a time, picked at random.
pub const BLOCKED_LINES: [&str; 3] = [
    "Nice Try ;)",
    "Shouldn't you be doing something else?",
    "You don't want you to be going there",
];

/// Which of [`BLOCKED_LINES`] a `seed` lands on.
pub fn pick_line(seed: u64) -> u8 {
    (seed % BLOCKED_LINES.len() as u64) as u8
}

/// A seed for [`pick_line`]: the clock, at nanosecond grain, so two rolls
/// within one second still differ. `dispatch` and `apply` call it; tests
/// pass seeds of their own.
pub fn line_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// The block page a tab shows: the site it is blocked on and the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockedPage {
    pub site: String,
    /// An index into [`BLOCKED_LINES`].
    pub line: u8,
}

impl BlockedPage {
    pub fn new(site: impl Into<String>, seed: u64) -> BlockedPage {
        BlockedPage {
            site: site.into(),
            line: pick_line(seed),
        }
    }

    /// Another line than the one showing (a reload re-rolls).
    pub fn reroll(&mut self, seed: u64) {
        let n = BLOCKED_LINES.len() as u64;
        let step = 1 + (seed % (n - 1));
        self.line = ((u64::from(self.line) + step) % n) as u8;
    }

    pub fn text(&self) -> &'static str {
        BLOCKED_LINES[usize::from(self.line) % BLOCKED_LINES.len()]
    }
}
```

In `lib.rs` the re-export becomes:

```rust
pub use blocked::{BLOCKED_LINES, BlockedError, BlockedPage, BlockedSites, line_seed, pick_line};
```

**Testing:**

Add to `mod tests` in `blocked.rs`:

```rust
    #[test]
    fn seeds_land_on_every_line_and_a_reroll_always_moves() {
        let lines: std::collections::BTreeSet<u8> = (0..9u64).map(pick_line).collect();
        assert_eq!(lines, (0..3u8).collect());
        for seed in 0..12u64 {
            let mut page = BlockedPage::new("a.example", seed);
            let before = page.line;
            assert_eq!(page.text(), BLOCKED_LINES[usize::from(before)]);
            for reseed in 0..12u64 {
                let mut p = page.clone();
                p.reroll(reseed);
                assert_ne!(p.line, before, "seed {seed} reseed {reseed}");
                assert!(usize::from(p.line) < BLOCKED_LINES.len());
            }
            page.reroll(seed);
            assert_ne!(page.line, before);
        }
        assert_eq!(BlockedPage::new("x", 4).site, "x");
    }
```

**Verification:**
Run: `cargo test -p osmena-core blocked`
Expected: all eight `blocked` tests pass.

**Commit (Tasks 2-4):**

```bash
cargo fmt --all
git add crates/osmena-core/src/blocked.rs crates/osmena-core/src/lib.rs crates/osmena-core/src/profile.rs
git commit -m "feat: the blocked sites list, keyed by registrable domain, and its file" -m "BlockedSites reduces a URL to its site through the public suffix list (subdomains fold in, co.uk stays no site, IPs and localhost are kept whole, host-less schemes are never blockable), keeps the set in blocked.toml beside zoom.toml, and names the block page's three lines with a seeded pick and a re-roll that always moves." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_4 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_5 -->
### Task 5: `Tab::blocked` and `Browser::blocked`

**Verifies:** None (model fields; exercised by Phase 2's tests). Confirms the design's requirement that `Tab::blocked` stays out of the session file: `Tab` is not a serde type, the session is a separate projection (`session::Session::capture` at `crates/osmena-core/src/session.rs:92-125` copies only `url`, `title` and the folder name), so no annotation is needed.

**Files:**
- Modify: `crates/osmena-core/src/model.rs:85-132` (`Tab` and `Tab::new`) and `:345-418` (`Browser` and `Browser::new`)

**Implementation:**

In `model.rs`, add the import at the top beside the other `crate::` imports:

```rust
use crate::blocked::{BlockedPage, BlockedSites};
```

In `pub struct Tab`, after the `warming: bool` field, add:

```rust
    /// The block page this tab shows instead of its page: its site is
    /// blocked on this profile. `url` keeps the attempted address, so the
    /// address bar says where the user tried to go; the engine shows
    /// `about:blank` behind the chrome's page.
    pub blocked: Option<BlockedPage>,
```

In `Tab::new`, after `warming: false,` add `blocked: None,`.

After `Tab::new` (still inside `impl Tab`) add the one way a tab is put on the block page, used by Phase 2 everywhere a tab is blocked:

```rust
    /// Show the block page `page` instead of the page: the title and
    /// favicon of the page before go with it (the strip and the address
    /// bar show the attempted address), the URL stays.
    pub fn block(&mut self, page: BlockedPage) {
        self.blocked = Some(page);
        self.title.clear();
        self.favicon = None;
    }
```

In `pub struct Browser`, after the `zoom: SiteZoom` field, add:

```rust
    /// The sites this profile refuses to show (`blocked.toml`).
    pub blocked: BlockedSites,
```

In `Browser::new`, after `zoom: SiteZoom::default(),` add `blocked: BlockedSites::default(),`.

**Verification:**

Run the full gate:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --features osmena-engine/glutin -- -D warnings
cargo test --workspace --features osmena-engine/glutin
```

Expected: all clean and green. Any struct literal of `Tab` elsewhere (for example in tests) must gain `blocked: None`; the compiler lists them, and `grep -rn "warming: false" crates` finds every `Tab { ... }` literal.

**Commit:**

```bash
cargo fmt --all
git add -A crates/osmena-core/src
git commit -m "feat: a tab knows the block page it shows and the browser its blocked sites" -m "Tab::blocked and Browser::blocked, both defaulted, and Tab::block, which drops the title and favicon of the page before so the strip shows the attempted address; the session file does not record a tab's block (it is a projection of url and title), so a restored tab is re-checked when shown." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
<!-- END_TASK_5 -->

---

## Phase 1 done when

- `cargo test --workspace --features osmena-engine/glutin`, clippy with `-D warnings` and `cargo fmt --all --check` are clean.
- The eight tests in `blocked.rs` pass: the reduction (www and deep subdomains, `co.uk`, IPv4, IPv6, `localhost`, punycode, no-host schemes), add and remove reporting change, the TOML round trip including a missing file, and `pick_line` over fixed seeds landing on every line.
- No CHANGELOG entry yet: nothing a browser notices until Phase 3 (the entry is written there).
