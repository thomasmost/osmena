//! What the address bar and the new-tab launcher offer for what the
//! user types: the history's matches and the bookmarks', ranked
//! together. A page that is both is one row, a bookmark.

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
