//! Browser extensions as the engine reports them, and the ways one can
//! be added. The model holds the list; the engine owns the extensions
//! themselves (installing, enabling, running them) and reports every
//! change with [`EngineEvent::ExtensionsChanged`].
//!
//! [`EngineEvent::ExtensionsChanged`]: crate::EngineEvent::ExtensionsChanged

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Favicon;
use crate::settings::write_atomically;

/// One installed extension.
#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
    /// The engine's stable id for it.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// A disabled extension stays installed but does nothing, and its
    /// toolbar action is not shown.
    pub enabled: bool,
    /// Its icon, when it has one the engine could decode.
    pub icon: Option<Favicon>,
    /// The title of its toolbar action, when it has one with a popup the
    /// browser can open. An action without a popup cannot be triggered
    /// from the toolbar, so it reads as `None`.
    pub action_title: Option<String>,
}

impl Extension {
    /// Whether clicking the extension's toolbar button does anything.
    pub fn has_action(&self) -> bool {
        self.enabled && self.action_title.is_some()
    }
}

/// Where an extension to install comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionSource {
    /// An unpacked extension: the directory holding its manifest.
    Directory(PathBuf),
    /// Let the engine ask the user for the directory with a dialog.
    AskForDirectory,
    /// An extension of the engine's store, by its id in that store.
    Store { id: String },
}

/// The installed extensions as last reported, written to
/// `extensions.toml` in the profile so that another profile can copy
/// them ("copy extensions from the default profile") without the
/// engine of this one running: each is installed again by its id from
/// the engine's store. An unpacked extension's id names nothing in the
/// store, and the engine reports that install as failed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstalledExtensions {
    pub extensions: Vec<InstalledExtension>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledExtension {
    pub id: String,
    pub name: String,
}

#[derive(Debug)]
pub enum InstalledExtensionsError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for InstalledExtensionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstalledExtensionsError::Io(e) => write!(f, "extensions file: {e}"),
            InstalledExtensionsError::Parse(e) => {
                write!(f, "extensions file is not valid TOML: {e}")
            }
            InstalledExtensionsError::Serialize(e) => {
                write!(f, "could not serialize the extensions: {e}")
            }
        }
    }
}

impl std::error::Error for InstalledExtensionsError {}

impl InstalledExtensions {
    /// The snapshot of `extensions`, ids in their order.
    pub fn of(extensions: &[Extension]) -> InstalledExtensions {
        InstalledExtensions {
            extensions: extensions
                .iter()
                .map(|e| InstalledExtension {
                    id: e.id.clone(),
                    name: e.name.clone(),
                })
                .collect(),
        }
    }

    /// The ids listed here that `installed` lacks: what a profile
    /// copying from this one still has to install.
    pub fn missing_from<'a>(&'a self, installed: &[Extension]) -> Vec<&'a InstalledExtension> {
        self.extensions
            .iter()
            .filter(|e| !installed.iter().any(|i| i.id == e.id))
            .collect()
    }

    pub fn parse(text: &str) -> Result<InstalledExtensions, InstalledExtensionsError> {
        toml::from_str(text).map_err(InstalledExtensionsError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, InstalledExtensionsError> {
        toml::to_string_pretty(self).map_err(InstalledExtensionsError::Serialize)
    }

    /// Read the file; a missing file is no extensions.
    pub fn load(path: &Path) -> Result<InstalledExtensions, InstalledExtensionsError> {
        match std::fs::read_to_string(path) {
            Ok(text) => InstalledExtensions::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(InstalledExtensions::default()),
            Err(e) => Err(InstalledExtensionsError::Io(e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), InstalledExtensionsError> {
        let text = self.to_toml()?;
        write_atomically(path, &text).map_err(InstalledExtensionsError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extension(id: &str, name: &str) -> Extension {
        Extension {
            id: id.to_string(),
            name: name.to_string(),
            version: "1".to_string(),
            description: String::new(),
            enabled: true,
            icon: None,
            action_title: None,
        }
    }

    #[test]
    fn the_snapshot_round_trips_and_says_what_another_profile_lacks() {
        let a = extension("aaaa", "Blocker");
        let b = extension("bbbb", "Notes");
        let snapshot = InstalledExtensions::of(&[a.clone(), b.clone()]);
        let text = snapshot.to_toml().unwrap();
        assert!(text.contains("[[extensions]]"), "{text}");
        assert_eq!(InstalledExtensions::parse(&text).unwrap(), snapshot);
        let missing = snapshot.missing_from(&[b]);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].id, "aaaa");
        assert!(
            snapshot
                .missing_from(&[a.clone(), extension("bbbb", "x")])
                .is_empty()
        );
    }

    #[test]
    fn a_missing_file_is_no_extensions_and_save_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extensions.toml");
        assert_eq!(
            InstalledExtensions::load(&path).unwrap(),
            InstalledExtensions::default()
        );
        let snapshot = InstalledExtensions::of(&[extension("aaaa", "Blocker")]);
        snapshot.save(&path).unwrap();
        assert_eq!(InstalledExtensions::load(&path).unwrap(), snapshot);
    }
}
