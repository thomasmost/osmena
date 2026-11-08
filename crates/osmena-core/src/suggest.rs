//! What the address bar and the new-tab launcher offer for what the
//! user types: the history's matches and the bookmarks', ranked
//! together. A page that is both is one row, a bookmark. The launcher
//! also fills the field in ahead of the typist ([`completion`]): the
//! most visited page whose address starts with what was typed, until
//! the first space makes the text a search.

use std::cmp::Reverse;

use url::Url;

use crate::bookmarks::Bookmarks;
use crate::history::{History, MAX_MATCHES};

/// Where a suggestion came from; a bookmark is drawn with a star.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestionSource {
    History,
    Bookmark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub url: Url,
    /// The bookmark's title for a bookmark, the page's last title
    /// otherwise; may be empty.
    pub title: String,
    pub source: SuggestionSource,
}

impl Suggestion {
    /// The title, or the address for an untitled page.
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            self.url.as_str()
        } else {
            &self.title
        }
    }
}

/// The suggestions for `query`, best first, at most [`MAX_MATCHES`].
/// Every word of the query must appear in the address or the title. A
/// page whose address starts with the query (with or without its scheme
/// and `www.`) comes before one that merely contains it; within a tier
/// bookmarks come before history, then the more visited and the more
/// recent.
pub fn suggestions(history: &History, bookmarks: &Bookmarks, query: &str) -> Vec<Suggestion> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut scored: Vec<(u64, Suggestion)> = Vec::new();
    for entry in history.entries() {
        let bookmark = bookmarks.find(&entry.url).map(|(_, b)| b);
        let title = bookmark
            .filter(|b| !b.title.is_empty())
            .map(|b| b.title.as_str())
            .unwrap_or(&entry.title);
        let Some(score) = score(
            &entry.url,
            title,
            &words,
            &query,
            bookmark.is_some(),
            entry.visits,
            entry.last_visit,
        ) else {
            continue;
        };
        scored.push((
            score,
            Suggestion {
                url: entry.url.clone(),
                title: title.to_string(),
                source: if bookmark.is_some() {
                    SuggestionSource::Bookmark
                } else {
                    SuggestionSource::History
                },
            },
        ));
    }
    for bookmark in bookmarks.all() {
        if History::remembers(&bookmark.url)
            && history.entries().iter().any(|e| e.url == bookmark.url)
        {
            continue;
        }
        if scored.iter().any(|(_, s)| s.url == bookmark.url) {
            continue;
        }
        let Some(score) = score(&bookmark.url, &bookmark.title, &words, &query, true, 0, 0) else {
            continue;
        };
        scored.push((
            score,
            Suggestion {
                url: bookmark.url.clone(),
                title: bookmark.title.clone(),
                source: SuggestionSource::Bookmark,
            },
        ));
    }
    scored.sort_by(|(a, sa), (b, sb)| b.cmp(a).then_with(|| sa.url.as_str().cmp(sb.url.as_str())));
    scored
        .into_iter()
        .map(|(_, s)| s)
        .take(MAX_MATCHES)
        .collect()
}

/// The rank of a page for `words` (all of which must appear in the
/// address or title), or `None` when it does not match.
fn score(
    url: &Url,
    title: &str,
    words: &[&str],
    query: &str,
    bookmarked: bool,
    visits: u32,
    last_visit: u64,
) -> Option<u64> {
    let address = url.as_str().to_lowercase();
    let title = title.to_lowercase();
    if !words
        .iter()
        .all(|w| address.contains(w) || title.contains(w))
    {
        return None;
    }
    let prefix = address.starts_with(query) || bare(&address).starts_with(query);
    Some(
        (u64::from(prefix) << 63)
            | (u64::from(bookmarked) << 62)
            | (u64::from(visits.min((1 << 20) - 1)) << 40)
            | (last_visit & ((1 << 40) - 1)),
    )
}

/// `url` without its scheme and a leading `www.`.
fn bare(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    rest.strip_prefix("www.").unwrap_or(rest)
}

/// The page the launcher's field fills in ahead of the typist, and the
/// text the field shows for it: what was typed, as the user spelt it,
/// with the rest of the page's address after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub suggestion: Suggestion,
    pub text: String,
}

/// The page to complete `typed` to, before the first space: of the
/// pages whose address starts with `typed` (ignoring case), the most
/// visited, then the one with the shortest address (so a site's front
/// page beats a page deep inside it), then the most recent. A bookmark
/// never visited counts as a page with no visits. The address is
/// matched without its scheme and `www.`, as it is typed, or with
/// `www.` when the user typed that, or in full once the user has typed
/// the scheme and its colon (so `h` never completes to `https://…`).
/// A site's front page is shown without its trailing slash. `None`
/// when nothing matches, when `typed` is empty, or once it holds a
/// space: a phrase is a search.
pub fn completion(history: &History, bookmarks: &Bookmarks, typed: &str) -> Option<Completion> {
    if typed.is_empty() || typed.chars().any(char::is_whitespace) {
        return None;
    }
    let query = typed.to_lowercase();
    let mut best: Option<(CompletionRank, Completion)> = None;
    let mut consider = |url: &Url, title: &str, source, visits: u32, last_visit: u64| {
        let Some(form) = completing_form(url, typed, &query) else {
            return;
        };
        let rank = (
            visits,
            Reverse(form.chars().count()),
            last_visit,
            Reverse(url.to_string()),
        );
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            let rest: String = form.chars().skip(typed.chars().count()).collect();
            best = Some((
                rank,
                Completion {
                    suggestion: Suggestion {
                        url: url.clone(),
                        title: title.to_string(),
                        source,
                    },
                    text: format!("{typed}{rest}"),
                },
            ));
        }
    };
    for entry in history.entries() {
        let bookmark = bookmarks.find(&entry.url).map(|(_, b)| b);
        let title = bookmark
            .filter(|b| !b.title.is_empty())
            .map(|b| b.title.as_str())
            .unwrap_or(&entry.title);
        let source = if bookmark.is_some() {
            SuggestionSource::Bookmark
        } else {
            SuggestionSource::History
        };
        consider(&entry.url, title, source, entry.visits, entry.last_visit);
    }
    for bookmark in bookmarks.all() {
        if history.entries().iter().any(|e| e.url == bookmark.url) {
            continue;
        }
        consider(
            &bookmark.url,
            &bookmark.title,
            SuggestionSource::Bookmark,
            0,
            0,
        );
    }
    best.map(|(_, c)| c)
}

/// How a candidate ranks: visits, then the shorter address, then the
/// later visit, then the address itself so the pick is settled.
type CompletionRank = (u32, Reverse<usize>, u64, Reverse<String>);

/// The spelling of `url` that starts with `typed` (`query` is its
/// lower case): the address without its scheme and `www.`, without its
/// scheme only, or in full when the user has typed the scheme and its
/// colon. A front page loses its trailing slash.
fn completing_form(url: &Url, typed: &str, query: &str) -> Option<String> {
    let full = url.as_str();
    let without_scheme = full.split_once("://").map(|(_, rest)| rest).unwrap_or(full);
    let forms = [
        (bare(full), true),
        (without_scheme, true),
        (full, typed.starts_with(&format!("{}:", url.scheme()))),
    ];
    let form = forms
        .into_iter()
        .filter(|(_, allowed)| *allowed)
        .map(|(form, _)| form)
        .find(|form| form.to_lowercase().starts_with(query))?;
    // A front page loses its slash, unless the user typed it.
    let front_page = url.path() == "/" && url.query().is_none() && url.fragment().is_none();
    let form = match form.strip_suffix('/') {
        Some(shorter) if front_page && shorter.to_lowercase().starts_with(query) => shorter,
        _ => form,
    };
    Some(form.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmarks::Bookmark;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn urls(suggestions: &[Suggestion]) -> Vec<&str> {
        suggestions.iter().map(|s| s.url.as_str()).collect()
    }

    #[test]
    fn bookmarks_rank_above_history_within_a_tier_and_merge_with_it() {
        let mut history = History::default();
        history.record(&url("https://google.com/"), "Google", 100);
        history.record(&url("https://google.com/"), "Google", 200);
        history.record(&url("https://gitlab.com/"), "GitLab", 150);
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(
            Bookmarks::BAR,
            vec![
                Bookmark {
                    title: "GitHub".to_string(),
                    url: url("https://github.com/"),
                },
                Bookmark {
                    title: "My GitLab".to_string(),
                    url: url("https://gitlab.com/"),
                },
            ],
        );
        let found = suggestions(&history, &bookmarks, "g");
        assert_eq!(
            urls(&found),
            vec![
                "https://gitlab.com/",
                "https://github.com/",
                "https://google.com/"
            ]
        );
        // The visited bookmark is one row, with the bookmark's title.
        assert_eq!(found[0].source, SuggestionSource::Bookmark);
        assert_eq!(found[0].title, "My GitLab");
        assert_eq!(found[1].source, SuggestionSource::Bookmark);
        assert_eq!(found[2].source, SuggestionSource::History);
        assert_eq!(found[2].display_title(), "Google");
    }

    #[test]
    fn a_title_word_finds_a_bookmark_never_visited_and_prefixes_come_first() {
        let mut history = History::default();
        history.record(&url("https://docs.rs/egui"), "egui - Rust", 10);
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(
            "Work",
            vec![Bookmark {
                title: "The egui book".to_string(),
                url: url("https://example.org/book"),
            }],
        );
        let found = suggestions(&history, &bookmarks, "egui book");
        assert_eq!(urls(&found), vec!["https://example.org/book"]);
        let found = suggestions(&history, &bookmarks, "docs.rs");
        // A prefix match on the address beats a bookmark that only
        // contains the words.
        assert_eq!(urls(&found), vec!["https://docs.rs/egui"]);
        assert!(suggestions(&history, &bookmarks, "   ").is_empty());
        assert!(suggestions(&history, &bookmarks, "zzz").is_empty());
    }

    fn completes_to(
        history: &History,
        bookmarks: &Bookmarks,
        typed: &str,
    ) -> Option<(String, String)> {
        completion(history, bookmarks, typed).map(|c| (c.text, c.suggestion.url.to_string()))
    }

    #[test]
    fn the_completion_is_the_most_visited_prefix_match_spelt_as_typed() {
        let mut history = History::default();
        for t in 0..3 {
            history.record(&url("https://github.com/"), "GitHub", 100 + t);
        }
        for t in 0..5 {
            history.record(&url("https://gitlab.com/explore"), "Explore", 200 + t);
        }
        history.record(&url("https://www.google.com/"), "Google", 900);
        let bookmarks = Bookmarks::default();
        // More visits win, whatever the recency; the user's case stays
        // and the rest of the address follows it.
        assert_eq!(
            completes_to(&history, &bookmarks, "Gi"),
            Some((
                "Gitlab.com/explore".into(),
                "https://gitlab.com/explore".into()
            ))
        );
        assert_eq!(
            completes_to(&history, &bookmarks, "gith"),
            Some(("github.com".into(), "https://github.com/".into()))
        );
        // `www.` is skipped as it is typed, and matched when it is.
        assert_eq!(
            completes_to(&history, &bookmarks, "goo").map(|c| c.0),
            Some("google.com".into())
        );
        assert_eq!(
            completes_to(&history, &bookmarks, "www.goo").map(|c| c.0),
            Some("www.google.com".into())
        );
        // The typed slash of a front page stays.
        assert_eq!(
            completes_to(&history, &bookmarks, "github.com/").map(|c| c.0),
            Some("github.com/".into())
        );
        // The scheme completes only once its colon is typed.
        assert_eq!(completes_to(&history, &bookmarks, "h"), None);
        assert_eq!(
            completes_to(&history, &bookmarks, "https:").map(|c| c.0),
            Some("https://gitlab.com/explore".into())
        );
        // A phrase is a search, and nothing completes nothing.
        assert_eq!(completes_to(&history, &bookmarks, "git hub"), None);
        assert_eq!(completes_to(&history, &bookmarks, ""), None);
        assert_eq!(completes_to(&history, &bookmarks, "zzz"), None);
        // The address must start with the typing; a word inside it or
        // in the title is not enough.
        assert_eq!(completes_to(&history, &bookmarks, "explore"), None);
    }

    #[test]
    fn at_equal_visits_the_front_page_wins_and_a_bookmark_is_the_last_resort() {
        let mut history = History::default();
        history.record(&url("https://docs.rs/egui/latest"), "egui", 50);
        history.record(&url("https://docs.rs/"), "Docs.rs", 10);
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(
            Bookmarks::BAR,
            vec![Bookmark {
                title: "Rust docs".to_string(),
                url: url("https://doc.rust-lang.org/std/"),
            }],
        );
        let found = completion(&history, &bookmarks, "doc").unwrap();
        assert_eq!(found.text, "docs.rs");
        assert_eq!(found.suggestion.display_title(), "Docs.rs");
        assert_eq!(found.suggestion.source, SuggestionSource::History);
        // The never-visited bookmark completes only what nothing in
        // the history does, and carries its star.
        let found = completion(&history, &bookmarks, "doc.r").unwrap();
        assert_eq!(found.text, "doc.rust-lang.org/std/");
        assert_eq!(found.suggestion.source, SuggestionSource::Bookmark);
        assert_eq!(found.suggestion.title, "Rust docs");
    }

    #[test]
    fn at_most_max_matches() {
        let mut bookmarks = Bookmarks::default();
        for i in 0..20 {
            bookmarks.add(
                "G",
                vec![Bookmark {
                    title: format!("Page {i}"),
                    url: url(&format!("https://p{i}.example/")),
                }],
            );
        }
        assert_eq!(
            suggestions(&History::default(), &bookmarks, "example").len(),
            MAX_MATCHES
        );
    }
}
