//! Blocked sites: the websites a profile refuses to show (`blocked.toml`
//! in the profile). A site is a registrable domain (`youtube.com`, never
//! `www.youtube.com`), which the public suffix list decides, so an
//! entry covers every subdomain. A tab on one shows the block page
//! (`BlockedPage`) over `about:blank`; nothing is requested from the
//! site.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::settings::write_atomically;

/// The sites a profile blocks, each a registrable domain in lower case.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockedSites {
    pub sites: BTreeSet<String>,
}

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
                    Some(site) => std::str::from_utf8(site.as_bytes()).ok().map(str::to_owned),
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
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn site(s: &str) -> Option<String> {
        BlockedSites::site_for(&Url::parse(s).unwrap())
    }

    #[test]
    fn subdomains_reduce_to_the_registrable_domain() {
        // AC1.1
        assert_eq!(
            site("https://www.youtube.com/watch").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(
            site("https://m.youtube.com/").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(
            site("https://a.b.c.youtube.com/").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(site("https://youtube.com/").as_deref(), Some("youtube.com"));
        // Case and a trailing dot make no difference.
        assert_eq!(
            site("https://WWW.YouTube.COM/").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(
            site("https://www.youtube.com./").as_deref(),
            Some("youtube.com")
        );
        // Punycode passes through unchanged.
        assert_eq!(
            site("https://www.xn--85x722f.xn--55qx5d.cn/").as_deref(),
            Some("xn--85x722f.xn--55qx5d.cn")
        );
    }

    #[test]
    fn multi_label_public_suffixes_hold() {
        // AC1.2
        assert_eq!(
            site("https://news.bbc.co.uk/").as_deref(),
            Some("bbc.co.uk")
        );
        assert_eq!(site("https://bbc.co.uk/").as_deref(), Some("bbc.co.uk"));
        assert_eq!(site("https://co.uk/"), None);
        assert_eq!(site("https://com/"), None);
        // Private suffixes count too.
        assert_eq!(
            site("https://user.github.io/x").as_deref(),
            Some("user.github.io")
        );
        assert_eq!(site("https://github.io/"), None);
    }

    #[test]
    fn addresses_and_single_labels_are_kept_whole() {
        // AC1.3
        assert_eq!(
            site("http://127.0.0.1:8080/x").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(site("http://[::1]/").as_deref(), Some("[::1]"));
        assert_eq!(site("http://localhost/").as_deref(), Some("localhost"));
        assert_eq!(site("http://intranet/").as_deref(), Some("intranet"));
        // An unknown top-level domain still gives the last two labels.
        assert_eq!(
            site("http://host.internal/").as_deref(),
            Some("host.internal")
        );
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
        assert!(matches!(
            BlockedSites::load(&path),
            Err(BlockedError::Parse(_))
        ));
    }

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
}
