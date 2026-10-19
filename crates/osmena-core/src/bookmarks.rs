//! Bookmarks, kept in named groups and stored as TOML in the profile
//! directory (`bookmarks.toml`). A group is what "Bookmark all selected"
//! makes of a selection of tabs; the window menu opens a group's pages
//! again, one at a time or all at once.

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
