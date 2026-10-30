//! The session: the windows and their tabs as they stand, written by the
//! shell a moment after they change and at exit (`session.toml` in the
//! profile), and read back at the next start when the setting says so,
//! after an update's restart, or on request after a crash. Restored
//! tabs other than each window's active one are deferred: they load
//! when first shown (`Tab::deferred`).

use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::model::{Browser, WindowId};
use crate::settings::write_atomically;

/// A window's place on screen, in physical pixels: the outer position
/// and the inner size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTab {
    pub url: Url,
    #[serde(default)]
    pub title: String,
    /// The folder's name, when the tab is in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFolder {
    pub name: String,
    #[serde(default)]
    pub expanded: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionWindow {
    /// The index of the active tab in `tabs`.
    pub active: usize,
    pub geometry: Option<WindowGeometry>,
    #[serde(rename = "folder")]
    pub folders: Vec<SessionFolder>,
    #[serde(rename = "tab")]
    pub tabs: Vec<SessionTab>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The URLs of tabs closed during the run, oldest first, so "Reopen
    /// closed tab" works across a restart.
    pub closed_tabs: Vec<Url>,
    #[serde(rename = "window")]
    pub windows: Vec<SessionWindow>,
}

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "cannot read or write the session: {e}"),
            SessionError::Parse(e) => write!(f, "the session file is not valid: {e}"),
            SessionError::Serialize(e) => write!(f, "cannot write the session: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl Session {
    /// The browser as it stands: every window with its tabs in strip
    /// order, its folders, its active tab and, through `geometry`, its
    /// place on screen.
    pub fn capture(
        browser: &Browser,
        geometry: &dyn Fn(WindowId) -> Option<WindowGeometry>,
    ) -> Session {
        let windows = browser
            .windows
            .iter()
            .map(|w| SessionWindow {
                active: w.active_index(),
                geometry: geometry(w.id),
                folders: w
                    .folders
                    .iter()
                    .map(|f| SessionFolder {
                        name: f.name.clone(),
                        expanded: f.expanded,
                    })
                    .collect(),
                tabs: w
                    .tabs
                    .iter()
                    .map(|t| SessionTab {
                        url: t.url.clone(),
                        title: t.title.clone(),
                        folder: t.folder.and_then(|id| w.folder(id)).map(|f| f.name.clone()),
                    })
                    .collect(),
            })
            .collect();
        Session {
            closed_tabs: browser.closed_tabs.clone(),
            windows,
        }
    }

    /// Nothing to restore: no window with a tab.
    pub fn is_empty(&self) -> bool {
        self.windows.iter().all(|w| w.tabs.is_empty())
    }

    /// How many tabs the session holds, over every window.
    pub fn tab_count(&self) -> usize {
        self.windows.iter().map(|w| w.tabs.len()).sum()
    }

    pub fn parse(text: &str) -> Result<Session, SessionError> {
        toml::from_str(text).map_err(SessionError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, SessionError> {
        toml::to_string_pretty(self).map_err(SessionError::Serialize)
    }

    /// Read the file; a missing file is no session.
    pub fn load(path: &Path) -> Result<Session, SessionError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Session::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Session::default()),
            Err(e) => Err(SessionError::Io(e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), SessionError> {
        let text = self.to_toml()?;
        write_atomically(path, &text).map_err(SessionError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            closed_tabs: vec![Url::parse("https://gone.example/").unwrap()],
            windows: vec![
                SessionWindow {
                    active: 1,
                    geometry: Some(WindowGeometry {
                        x: 120,
                        y: 80,
                        width: 1440,
                        height: 900,
                        maximized: false,
                    }),
                    folders: vec![SessionFolder {
                        name: "Reading".to_string(),
                        expanded: true,
                    }],
                    tabs: vec![
                        SessionTab {
                            url: Url::parse("https://a.example/").unwrap(),
                            title: "A".to_string(),
                            folder: None,
                        },
                        SessionTab {
                            url: Url::parse("https://b.example/").unwrap(),
                            title: String::new(),
                            folder: Some("Reading".to_string()),
                        },
                    ],
                },
                SessionWindow {
                    active: 0,
                    geometry: None,
                    folders: Vec::new(),
                    tabs: vec![SessionTab {
                        url: Url::parse("https://c.example/").unwrap(),
                        title: "C".to_string(),
                        folder: None,
                    }],
                },
            ],
        }
    }

    #[test]
    fn round_trips_through_toml_and_files() {
        let s = session();
        let text = s.to_toml().unwrap();
        assert!(text.contains("[[window]]"), "{text}");
        assert!(text.contains("[window.geometry]"), "{text}");
        assert!(text.contains("[[window.tab]]"), "{text}");
        assert!(text.contains("folder = \"Reading\""), "{text}");
        assert_eq!(Session::parse(&text).unwrap(), s);
        assert_eq!(Session::parse("").unwrap(), Session::default());
        assert!(Session::default().is_empty());
        assert!(!s.is_empty());
        assert_eq!(s.tab_count(), 3);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.toml");
        assert_eq!(Session::load(&path).unwrap(), Session::default());
        s.save(&path).unwrap();
        assert_eq!(Session::load(&path).unwrap(), s);
        std::fs::write(&path, "window = 1").unwrap();
        assert!(matches!(Session::load(&path), Err(SessionError::Parse(_))));
    }
}
