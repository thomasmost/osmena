//! Browsing history: one entry per URL with its last title, how often it
//! was visited and when. The model records a visit when a page finishes
//! loading and updates the title when the page reports one; the chrome
//! asks for the entries matching what the user is typing, in the address
//! bar or the new-tab launcher. Saved as `history.toml` in the profile.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::settings::write_atomically;

/// How many URLs are remembered; the least recently visited go first.
pub const MAX_ENTRIES: usize = 5000;
/// How many matches [`History::matches`] returns.
pub const MAX_MATCHES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: Url,
    #[serde(default)]
    pub title: String,
    /// How many times the page finished loading.
    pub visits: u32,
    /// The last visit, in seconds since the Unix epoch.
    pub last_visit: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    #[serde(default, rename = "entry")]
    entries: Vec<HistoryEntry>,
}

#[derive(Debug)]
pub enum HistoryError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HistoryError::Io(e) => write!(f, "cannot read or write the history: {e}"),
            HistoryError::Parse(e) => write!(f, "the history file is not valid: {e}"),
            HistoryError::Serialize(e) => write!(f, "cannot write the history: {e}"),
        }
    }
}

impl std::error::Error for HistoryError {}

impl History {
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether a URL is worth remembering: pages with an address the user
    /// could type again. Blank pages, internal pages, and view-source
    /// views are not.
    pub fn remembers(url: &Url) -> bool {
        matches!(url.scheme(), "http" | "https" | "file")
    }

    /// A page at `url` finished loading at `now` with `title` (which may
    /// still be empty). Returns whether anything changed.
    pub fn record(&mut self, url: &Url, title: &str, now: u64) -> bool {
        if !History::remembers(url) {
            return false;
        }
        match self.entries.iter_mut().find(|e| e.url == *url) {
            Some(entry) => {
                entry.visits = entry.visits.saturating_add(1);
                entry.last_visit = now;
                if !title.is_empty() {
                    entry.title = title.to_string();
                }
            }
            None => {
                self.entries.push(HistoryEntry {
                    url: url.clone(),
                    title: title.to_string(),
                    visits: 1,
                    last_visit: now,
                });
                if self.entries.len() > MAX_ENTRIES {
                    let oldest = self
                        .entries
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, e)| e.last_visit)
                        .map(|(i, _)| i)
                        .expect("not empty");
                    self.entries.remove(oldest);
                }
            }
        }
        true
    }

    /// The page at `url` reported `title`. Returns whether anything
    /// changed: only a remembered page with a different title does.
    pub fn retitle(&mut self, url: &Url, title: &str) -> bool {
        if title.is_empty() {
            return false;
        }
        match self.entries.iter_mut().find(|e| e.url == *url) {
            Some(entry) if entry.title != title => {
                entry.title = title.to_string();
                true
            }
            _ => false,
        }
    }

    /// The entries that match `query`, best first, at most
    /// [`MAX_MATCHES`]. Every word of the query must appear in the URL or
    /// the title, case-insensitively; an entry whose address starts with
    /// the query (with or without its scheme and `www.`) comes before one
    /// that merely contains it, and among equals the more visited and
    /// the more recent come first.
    pub fn matches(&self, query: &str) -> Vec<&HistoryEntry> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut scored: Vec<(u64, &HistoryEntry)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let url = entry.url.as_str().to_lowercase();
                let title = entry.title.to_lowercase();
                if !words.iter().all(|w| url.contains(w) || title.contains(w)) {
                    return None;
                }
                let prefix = url.starts_with(&query) || bare(&url).starts_with(&query);
                let score = (u64::from(prefix) << 62)
                    | (u64::from(entry.visits.min(1 << 20)) << 40)
                    | (entry.last_visit & ((1 << 40) - 1));
                Some((score, entry))
            })
            .collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored
            .into_iter()
            .map(|(_, entry)| entry)
            .take(MAX_MATCHES)
            .collect()
    }

    pub fn parse(text: &str) -> Result<History, HistoryError> {
        toml::from_str(text).map_err(HistoryError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, HistoryError> {
        toml::to_string(self).map_err(HistoryError::Serialize)
    }

    /// Read the file; a missing file is an empty history.
    pub fn load(path: &Path) -> Result<History, HistoryError> {
        match std::fs::read_to_string(path) {
            Ok(text) => History::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(History::default()),
            Err(e) => Err(HistoryError::Io(e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), HistoryError> {
        let text = self.to_toml()?;
        write_atomically(path, &text).map_err(HistoryError::Io)
    }
}

/// `url` without its scheme and a leading `www.`: what a user types to
/// mean it.
fn bare(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.strip_prefix("www.").unwrap_or(rest)
}

/// Seconds since the Unix epoch, for a visit happening now.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_visit_is_recorded_once_per_url_and_counted() {
        let mut h = History::default();
        assert!(h.record(&url("https://a.test/"), "", 10));
        assert!(h.record(&url("https://a.test/"), "A", 20));
        assert!(h.record(&url("https://b.test/"), "B", 30));
        assert_eq!(h.entries().len(), 2);
        let a = &h.entries()[0];
        assert_eq!((a.visits, a.last_visit, a.title.as_str()), (2, 20, "A"));
        // A later load without a title keeps the title it had.
        h.record(&url("https://a.test/"), "", 40);
        assert_eq!(h.entries()[0].title, "A");
    }

    #[test]
    fn blank_and_internal_pages_are_not_remembered() {
        let mut h = History::default();
        assert!(!h.record(&url("about:blank"), "", 1));
        assert!(!h.record(&url("chrome://extensions"), "", 1));
        assert!(!h.record(&url("view-source:https://a.test/"), "", 1));
        assert!(h.is_empty());
    }

    #[test]
    fn a_title_report_updates_only_a_remembered_page() {
        let mut h = History::default();
        assert!(!h.retitle(&url("https://a.test/"), "A"));
        h.record(&url("https://a.test/"), "", 1);
        assert!(h.retitle(&url("https://a.test/"), "A"));
        assert!(!h.retitle(&url("https://a.test/"), "A"), "unchanged");
        assert!(!h.retitle(&url("https://a.test/"), ""), "empty is ignored");
        assert_eq!(h.entries()[0].title, "A");
    }

    #[test]
    fn the_least_recently_visited_entry_goes_when_the_history_is_full() {
        let mut h = History::default();
        for i in 0..MAX_ENTRIES {
            h.record(&url(&format!("https://{i}.test/")), "", 1000 + i as u64);
        }
        // Make the second entry the oldest visit, then overflow.
        h.record(&url("https://0.test/"), "", 99_999);
        h.record(&url("https://new.test/"), "", 99_999);
        assert_eq!(h.entries().len(), MAX_ENTRIES);
        assert!(
            h.entries()
                .iter()
                .all(|e| e.url.as_str() != "https://1.test/")
        );
        assert!(
            h.entries()
                .iter()
                .any(|e| e.url.as_str() == "https://0.test/")
        );
    }

    #[test]
    fn matches_need_every_word_and_rank_prefixes_visits_and_recency() {
        let mut h = History::default();
        h.record(&url("https://www.wikipedia.org/"), "Wikipedia", 100);
        h.record(
            &url("https://en.wikipedia.org/wiki/Rust"),
            "Rust - Wikipedia",
            200,
        );
        h.record(
            &url("https://en.wikipedia.org/wiki/Rust"),
            "Rust - Wikipedia",
            300,
        );
        h.record(
            &url("https://rust-lang.org/"),
            "Rust Programming Language",
            50,
        );
        h.record(&url("https://a.test/"), "Nothing here", 400);

        let urls =
            |q: &str| -> Vec<String> { h.matches(q).iter().map(|e| e.url.to_string()).collect() };
        // "wiki" is a prefix of wikipedia.org (without www.), so the front
        // page outranks the article even though the article has more
        // visits; both come before anything else.
        assert_eq!(
            urls("wiki"),
            [
                "https://www.wikipedia.org/",
                "https://en.wikipedia.org/wiki/Rust"
            ]
        );
        // Both words must match, in the URL or the title.
        assert_eq!(urls("rust wiki"), ["https://en.wikipedia.org/wiki/Rust"]);
        // "rust" is a prefix of rust-lang.org, so that page comes before
        // the more visited article that merely contains the word.
        assert_eq!(
            urls("rust"),
            [
                "https://rust-lang.org/",
                "https://en.wikipedia.org/wiki/Rust"
            ]
        );
        // Among plain matches, more visits win over recency.
        let mut h2 = h.clone();
        h2.record(&url("https://x.test/news"), "Zebra news", 900);
        h2.record(&url("https://y.test/facts"), "Zebra facts", 100);
        h2.record(&url("https://y.test/facts"), "Zebra facts", 101);
        let zebra: Vec<String> = h2
            .matches("zebra")
            .iter()
            .map(|e| e.url.to_string())
            .collect();
        assert_eq!(zebra, ["https://y.test/facts", "https://x.test/news"]);
        assert_eq!(urls("RUST PROGRAMMING"), ["https://rust-lang.org/"]);
        assert!(urls("").is_empty());
        assert!(urls("zzz").is_empty());
    }

    #[test]
    fn at_most_max_matches_come_back() {
        let mut h = History::default();
        for i in 0..20 {
            h.record(&url(&format!("https://site{i}.test/")), "", i);
        }
        assert_eq!(h.matches("site").len(), MAX_MATCHES);
    }

    #[test]
    fn history_round_trips_through_toml_and_a_missing_file_is_empty() {
        let mut h = History::default();
        h.record(&url("https://a.test/path?q=1"), "A \"quoted\" title", 1);
        h.record(&url("https://b.test/"), "", 2);
        let text = h.to_toml().unwrap();
        assert_eq!(History::parse(&text).unwrap(), h);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.toml");
        assert!(History::load(&path).unwrap().is_empty());
        h.save(&path).unwrap();
        assert_eq!(History::load(&path).unwrap(), h);
    }
}
