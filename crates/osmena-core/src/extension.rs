//! Browser extensions as the engine reports them, and the ways one can
//! be added. The model holds the list; the engine owns the extensions
//! themselves (installing, enabling, running them) and reports every
//! change with [`EngineEvent::ExtensionsChanged`].
//!
//! [`EngineEvent::ExtensionsChanged`]: crate::EngineEvent::ExtensionsChanged

use std::path::PathBuf;

use crate::model::Favicon;

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
