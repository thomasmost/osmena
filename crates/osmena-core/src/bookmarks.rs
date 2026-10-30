//! Bookmarks, kept in named groups and stored as TOML in the profile
//! directory (`bookmarks.toml`). The star in the toolbar (or `Ctrl+D`)
//! puts a page in a group, "Bookmark all selected" makes a group of a
//! selection of tabs, the group called [`Bookmarks::BAR`] is what the
//! bookmarks bar shows, and the menus open a group's pages again, one
//! at a time or all at once.

use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::settings::write_atomically;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub url: Url,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkGroup {
    pub name: String,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bookmarks {
    pub groups: Vec<BookmarkGroup>,
}

#[derive(Debug)]
pub enum BookmarksError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for BookmarksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookmarksError::Io(e) => write!(f, "bookmarks file: {e}"),
            BookmarksError::Parse(e) => write!(f, "bookmarks file is not valid TOML: {e}"),
            BookmarksError::Serialize(e) => write!(f, "could not serialize bookmarks: {e}"),
        }
    }
}

impl std::error::Error for BookmarksError {}

impl Bookmarks {
    /// The group the bookmarks bar shows, and the one the star puts a
    /// page in until the user picks another.
    pub const BAR: &'static str = "Bookmarks bar";

    pub fn group(&self, name: &str) -> Option<&BookmarkGroup> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// The group names, in the order the groups were made.
    pub fn group_names(&self) -> Vec<String> {
        self.groups.iter().map(|g| g.name.clone()).collect()
    }

    /// Add `bookmarks` to the group called `name`, making the group if
    /// there is none. A page already in the group (same URL) is not added
    /// again. Returns how many were added.
    pub fn add(&mut self, name: &str, bookmarks: Vec<Bookmark>) -> usize {
        let group = match self.groups.iter().position(|g| g.name == name) {
            Some(i) => &mut self.groups[i],
            None => {
                self.groups.push(BookmarkGroup {
                    name: name.to_string(),
                    bookmarks: Vec::new(),
                });
                self.groups.last_mut().expect("just pushed")
            }
        };
        let mut added = 0;
        for bookmark in bookmarks {
            if group.bookmarks.iter().any(|b| b.url == bookmark.url) {
                continue;
            }
            group.bookmarks.push(bookmark);
            added += 1;
        }
        added
    }

    /// Whether a page at `url` can be bookmarked at all: a blank page
    /// cannot.
    pub fn bookmarkable(url: &Url) -> bool {
        !(url.scheme() == "about" && url.path() == "blank")
    }

    /// The key a site's favicon is kept under: the host, in lower case.
    /// `None` for an address without one.
    pub fn site_key(url: &Url) -> Option<String> {
        url.host_str()
            .filter(|h| !h.is_empty())
            .map(str::to_ascii_lowercase)
    }

    /// Whether `url` is bookmarked, in any group.
    pub fn contains(&self, url: &Url) -> bool {
        self.find(url).is_some()
    }

    /// The group holding `url` and the bookmark itself, if any (the
    /// first, when it is in several).
    pub fn find(&self, url: &Url) -> Option<(&str, &Bookmark)> {
        self.groups.iter().find_map(|g| {
            g.bookmarks
                .iter()
                .find(|b| b.url == *url)
                .map(|b| (g.name.as_str(), b))
        })
    }

    /// The group the bookmarks bar shows, if it exists.
    pub fn bar(&self) -> Option<&BookmarkGroup> {
        self.group(Self::BAR)
    }

    /// Every bookmark, group by group.
    pub fn all(&self) -> impl Iterator<Item = &Bookmark> {
        self.groups.iter().flat_map(|g| g.bookmarks.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.all().next().is_none()
    }

    /// Forget `url` wherever it is bookmarked. The group stays, even
    /// empty. Returns whether anything was removed.
    pub fn remove(&mut self, url: &Url) -> bool {
        let mut removed = false;
        for group in &mut self.groups {
            let before = group.bookmarks.len();
            group.bookmarks.retain(|b| b.url != *url);
            removed |= group.bookmarks.len() != before;
        }
        removed
    }

    /// Give the bookmark of `url` the title `title` and put it in `group`
    /// (made if new): where it was when that is the same group, at the
    /// end of `group` otherwise. A bookmark that does not exist is added.
    /// An empty group name means the bar.
    pub fn update(&mut self, url: &Url, title: &str, group: &str) {
        let group = match group.trim() {
            "" => Self::BAR,
            name => name,
        };
        let title = title.trim().to_string();
        let position = self.groups.iter().position(|g| g.name == group);
        if let Some(i) = position
            && let Some(b) = self.groups[i].bookmarks.iter_mut().find(|b| b.url == *url)
        {
            b.title = title;
            // It may also be in other groups from before; one place is
            // what the star shows, so the others let go.
            for (j, g) in self.groups.iter_mut().enumerate() {
                if j != i {
                    g.bookmarks.retain(|b| b.url != *url);
                }
            }
            return;
        }
        self.remove(url);
        self.add(
            group,
            vec![Bookmark {
                title,
                url: url.clone(),
            }],
        );
    }

    /// Rename the group `from` to `to`; a group already called `to`
    /// takes `from`'s bookmarks (those it lacks). Returns whether a
    /// group changed.
    pub fn rename_group(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim();
        if to.is_empty() || to == from {
            return false;
        }
        let Some(i) = self.groups.iter().position(|g| g.name == from) else {
            return false;
        };
        if self.groups.iter().any(|g| g.name == to) {
            let moved = std::mem::take(&mut self.groups[i].bookmarks);
            self.groups.remove(i);
            self.add(to, moved);
        } else {
            self.groups[i].name = to.to_string();
        }
        true
    }

    /// Delete the group `name` and every bookmark in it.
    pub fn delete_group(&mut self, name: &str) -> bool {
        let before = self.groups.len();
        self.groups.retain(|g| g.name != name);
        self.groups.len() != before
    }

    /// Move the bookmark at `from` in `group` so it sits at `to` (an
    /// index into the group as it is before the move; `to == len` puts
    /// it last). Returns whether the order changed.
    pub fn move_within(&mut self, group: &str, from: usize, to: usize) -> bool {
        let Some(g) = self.groups.iter_mut().find(|g| g.name == group) else {
            return false;
        };
        let len = g.bookmarks.len();
        if from >= len || to > len || from == to || from + 1 == to {
            return false;
        }
        let bookmark = g.bookmarks.remove(from);
        let at = if to > from { to - 1 } else { to };
        g.bookmarks.insert(at, bookmark);
        true
    }

    /// Take every group of `other` into this one (the pages a group
    /// already has are skipped). Returns how many bookmarks were added.
    pub fn merge(&mut self, other: Bookmarks) -> usize {
        other
            .groups
            .into_iter()
            .map(|g| self.add(&g.name, g.bookmarks))
            .sum()
    }

    pub fn parse(text: &str) -> Result<Bookmarks, BookmarksError> {
        toml::from_str(text).map_err(BookmarksError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, BookmarksError> {
        toml::to_string_pretty(self).map_err(BookmarksError::Serialize)
    }

    /// Read the file; a missing file is no bookmarks.
    pub fn load(path: &Path) -> Result<Bookmarks, BookmarksError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Bookmarks::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Bookmarks::default()),
            Err(e) => Err(BookmarksError::Io(e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), BookmarksError> {
        let text = self.to_toml()?;
        write_atomically(path, &text).map_err(BookmarksError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bookmark(title: &str, url: &str) -> Bookmark {
        Bookmark {
            title: title.to_string(),
            url: Url::parse(url).unwrap(),
        }
    }

    #[test]
    fn adding_makes_the_group_and_skips_pages_already_in_it() {
        let mut b = Bookmarks::default();
        assert_eq!(
            b.add(
                "Work",
                vec![
                    bookmark("A", "https://a.example/"),
                    bookmark("B", "https://b.example/")
                ]
            ),
            2
        );
        assert_eq!(
            b.add(
                "Work",
                vec![
                    bookmark("A again", "https://a.example/"),
                    bookmark("C", "https://c.example/")
                ]
            ),
            1
        );
        assert_eq!(b.group_names(), vec!["Work"]);
        let titles: Vec<&str> = b
            .group("Work")
            .unwrap()
            .bookmarks
            .iter()
            .map(|x| x.title.as_str())
            .collect();
        assert_eq!(titles, vec!["A", "B", "C"]);
        assert!(b.group("Play").is_none());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut b = Bookmarks::default();
        b.add("Work", vec![bookmark("A", "https://a.example/")]);
        b.add("Play", vec![bookmark("B", "https://b.example/?x=1")]);
        let text = b.to_toml().unwrap();
        assert!(text.contains("[[groups]]"), "{text}");
        assert!(text.contains("name = \"Work\""), "{text}");
        assert_eq!(Bookmarks::parse(&text).unwrap(), b);
        assert_eq!(Bookmarks::parse("").unwrap(), Bookmarks::default());
    }

    #[test]
    fn find_remove_and_update() {
        let mut b = Bookmarks::default();
        let a = Url::parse("https://a.example/").unwrap();
        assert!(!b.contains(&a));
        b.update(&a, "A", "");
        assert_eq!(
            b.find(&a).map(|(g, bm)| (g, bm.title.as_str())),
            Some((Bookmarks::BAR, "A"))
        );
        assert_eq!(b.bar().unwrap().bookmarks.len(), 1);
        // A retitle in place keeps the position; a new group moves it.
        b.add(Bookmarks::BAR, vec![bookmark("Z", "https://z.example/")]);
        b.update(&a, "A2", Bookmarks::BAR);
        assert_eq!(b.bar().unwrap().bookmarks[0].title, "A2");
        b.update(&a, "A3", "Work");
        assert!(b.bar().unwrap().bookmarks.iter().all(|x| x.url != a));
        assert_eq!(b.find(&a).unwrap().0, "Work");
        assert!(b.remove(&a));
        assert!(!b.remove(&a));
        assert!(!b.contains(&a));
        // The emptied group stays.
        assert_eq!(b.group_names(), vec![Bookmarks::BAR, "Work"]);
        assert!(!b.is_empty());
    }

    #[test]
    fn groups_rename_merge_and_delete() {
        let mut b = Bookmarks::default();
        b.add("Work", vec![bookmark("A", "https://a.example/")]);
        b.add("Play", vec![bookmark("B", "https://b.example/")]);
        assert!(!b.rename_group("Work", "  "));
        assert!(!b.rename_group("Work", "Work"));
        assert!(!b.rename_group("Nope", "X"));
        assert!(b.rename_group("Work", "Office"));
        assert_eq!(b.group_names(), vec!["Office", "Play"]);
        // Renaming onto an existing group pours it in.
        assert!(b.rename_group("Office", "Play"));
        assert_eq!(b.group_names(), vec!["Play"]);
        assert_eq!(b.group("Play").unwrap().bookmarks.len(), 2);
        assert!(b.delete_group("Play"));
        assert!(!b.delete_group("Play"));
        assert!(b.is_empty());
    }

    #[test]
    fn moving_within_a_group() {
        let mut b = Bookmarks::default();
        b.add(
            "G",
            vec![
                bookmark("A", "https://a.example/"),
                bookmark("B", "https://b.example/"),
                bookmark("C", "https://c.example/"),
            ],
        );
        let titles = |b: &Bookmarks| -> Vec<String> {
            b.group("G")
                .unwrap()
                .bookmarks
                .iter()
                .map(|x| x.title.clone())
                .collect()
        };
        assert!(b.move_within("G", 0, 3));
        assert_eq!(titles(&b), vec!["B", "C", "A"]);
        assert!(b.move_within("G", 2, 0));
        assert_eq!(titles(&b), vec!["A", "B", "C"]);
        // Dropping where it already is changes nothing.
        assert!(!b.move_within("G", 1, 1));
        assert!(!b.move_within("G", 1, 2));
        assert!(!b.move_within("G", 5, 0));
        assert!(!b.move_within("Nope", 0, 1));
        assert!(b.move_within("G", 0, 2));
        assert_eq!(titles(&b), vec!["B", "A", "C"]);
    }

    #[test]
    fn merging_adds_what_is_missing() {
        let mut b = Bookmarks::default();
        b.add("Work", vec![bookmark("A", "https://a.example/")]);
        let mut other = Bookmarks::default();
        other.add(
            "Work",
            vec![
                bookmark("A", "https://a.example/"),
                bookmark("B", "https://b.example/"),
            ],
        );
        other.add("Play", vec![bookmark("C", "https://c.example/")]);
        assert_eq!(b.merge(other), 2);
        assert_eq!(b.group_names(), vec!["Work", "Play"]);
    }

    #[test]
    fn load_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bookmarks.toml");
        assert_eq!(Bookmarks::load(&path).unwrap(), Bookmarks::default());
        let mut b = Bookmarks::default();
        b.add("Work", vec![bookmark("A", "https://a.example/")]);
        b.save(&path).unwrap();
        assert_eq!(Bookmarks::load(&path).unwrap(), b);
        std::fs::write(&path, "groups = 3").unwrap();
        assert!(matches!(
            Bookmarks::load(&path),
            Err(BookmarksError::Parse(_))
        ));
    }
}
