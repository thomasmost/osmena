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
use url::Url;

use crate::keymap::{CommandKind, KeyChord, Keymap};
use crate::model::{Favicon, FolderId, TabId, WindowId};
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
    /// The title of its toolbar action. Set for any toolbar action, with
    /// or without a popup: the engine opens the popup when there is one
    /// and tells the extension of the click otherwise.
    pub action_title: Option<String>,
    /// Whether it declares a side panel.
    pub side_panel: bool,
    /// The keyboard commands it declares, in its own order.
    pub commands: Vec<ExtensionCommand>,
}

impl Extension {
    /// Whether clicking the extension's toolbar button does anything.
    pub fn has_action(&self) -> bool {
        self.enabled && self.action_title.is_some()
    }

    /// Whether the extension is enabled and declares a command called
    /// `name`.
    pub fn has_command(&self, name: &str) -> bool {
        self.enabled && self.commands.iter().any(|c| c.name == name)
    }
}

/// A keyboard command an extension declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionCommand {
    /// The extension's own name for it, which it is told when the
    /// command runs.
    pub name: String,
    pub description: String,
    /// The chord the extension suggests for this platform, if any. A
    /// chord the keymap holds is the keymap's.
    pub chord: Option<KeyChord>,
}

/// What becomes of an extension command's suggested shortcut. The
/// keymap is resolved before any extension's chord, so a chord the
/// keymap holds never reaches the extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutVerdict {
    /// The manifest suggests no key for this platform.
    Unassigned,
    /// No keymap chord uses it. Another enabled command may still declare
    /// the same chord, and then the key runs that one instead.
    Free(KeyChord),
    /// The keymap holds the chord: the key runs `by`, not the
    /// extension's command.
    Overridden { chord: KeyChord, by: CommandKind },
}

/// The verdict on `command`'s shortcut against `keymap` alone: the first
/// lookup the key router makes. Which extension command holds a chord the
/// keymap leaves free is not decided here; the settings window decides it
/// where it draws the list.
pub fn shortcut_verdict(keymap: &Keymap, command: &ExtensionCommand) -> ShortcutVerdict {
    match command.chord {
        None => ShortcutVerdict::Unassigned,
        Some(chord) => match keymap.resolve(chord) {
            Some(by) => ShortcutVerdict::Overridden { chord, by },
            None => ShortcutVerdict::Free(chord),
        },
    }
}

/// A side panel an extension opened: a page of the extension docked
/// beside the tab's page. `view` is the engine view that renders it, an
/// id of the tabs' kind that is in no strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidePanel {
    /// The id of the extension it belongs to.
    pub extension: String,
    /// The engine's address for the page it shows.
    pub page: String,
    pub view: TabId,
}

/// Something an extension asks of the browser. It arrives as
/// [`EngineEvent::ExtensionRequest`] and is answered with one
/// [`ExtensionReply`].
///
/// [`EngineEvent::ExtensionRequest`]: crate::EngineEvent::ExtensionRequest
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionRequest {
    /// Open `url` in a new tab at the end of `window`'s strip (`None`:
    /// the focused window, else the first), `active` or behind the
    /// active tab. Answered with `Tab`.
    OpenTab {
        window: Option<WindowId>,
        url: Url,
        active: bool,
    },
    CloseTabs(Vec<TabId>),
    ActivateTab(TabId),
    /// Put `tab` at `index` of its window's strip (past the end: last).
    MoveTab {
        tab: TabId,
        index: usize,
    },
    /// Put the tabs in `folder`, or in a new one (`None`), which starts
    /// expanded and named after the extension. Answered with `Folder`.
    GroupTabs {
        tabs: Vec<TabId>,
        folder: Option<FolderId>,
    },
    /// Take the tabs out of their folders.
    UngroupTabs(Vec<TabId>),
    /// Rename `folder` (an empty name is the name a new one would get),
    /// expand or collapse it, or both.
    UpdateFolder {
        folder: FolderId,
        name: Option<String>,
        expanded: Option<bool>,
    },
    /// Open a window on `url`, or on the new-window page. Answered with
    /// `Window`.
    OpenWindow {
        url: Option<Url>,
    },
    /// Show the extension's `page` in a side panel: `tab`'s own, or
    /// `window`'s when `tab` is `None`.
    OpenSidePanel {
        window: WindowId,
        tab: Option<TabId>,
        page: String,
    },
    /// Close the extension's side panel of `tab`, or of `window`.
    CloseSidePanel {
        window: WindowId,
        tab: Option<TabId>,
    },
}

/// The answer to an [`ExtensionRequest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionReply {
    Done,
    /// The tab that was opened.
    Tab(TabId),
    /// The folder the tabs are in now.
    Folder(FolderId),
    /// The window that was opened.
    Window(WindowId),
    /// Nothing was changed, and why, in the words the extension's own
    /// API uses for the case ("No tab with id: 7.").
    Refused(String),
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
            side_panel: false,
            commands: Vec::new(),
        }
    }

    #[test]
    fn a_command_counts_only_while_its_extension_is_enabled_and_declares_it() {
        let mut e = extension("aaaa", "Notes");
        assert!(!e.has_command("open"));
        e.commands.push(ExtensionCommand {
            name: "open".to_string(),
            description: "Open the notes".to_string(),
            chord: "Ctrl+KeyE".parse().ok(),
        });
        assert!(e.commands[0].chord.is_some());
        assert!(e.has_command("open"));
        assert!(!e.has_command("close"));
        e.enabled = false;
        assert!(!e.has_command("open"));
        // An action is one with or without a popup: the title is enough.
        e.enabled = true;
        assert!(!e.has_action());
        e.action_title = Some("Notes".to_string());
        assert!(e.has_action());
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

    use crate::keymap::Platform;
    use keyboard_types::{Code, Modifiers};

    fn shortcut(chord: Option<KeyChord>) -> ExtensionCommand {
        ExtensionCommand {
            name: "toggle".to_string(),
            description: "Toggle the panel".to_string(),
            chord,
        }
    }

    #[test]
    fn a_shortcut_is_free_overridden_or_unassigned() {
        let mut keymap = Keymap::defaults(Platform::Windows);
        let ctrl_e = KeyChord::new(Modifiers::CONTROL, Code::KeyE);
        let ctrl_t = KeyChord::new(Modifiers::CONTROL, Code::KeyT);
        assert_eq!(
            shortcut_verdict(&keymap, &shortcut(None)),
            ShortcutVerdict::Unassigned
        );
        assert_eq!(
            shortcut_verdict(&keymap, &shortcut(Some(ctrl_e))),
            ShortcutVerdict::Free(ctrl_e)
        );
        assert_eq!(
            shortcut_verdict(&keymap, &shortcut(Some(ctrl_t))),
            ShortcutVerdict::Overridden {
                chord: ctrl_t,
                by: CommandKind::NewTab
            }
        );
        // The verdict follows the user's keymap, not the defaults.
        keymap.bind(ctrl_e, CommandKind::Find);
        assert_eq!(
            shortcut_verdict(&keymap, &shortcut(Some(ctrl_e))),
            ShortcutVerdict::Overridden {
                chord: ctrl_e,
                by: CommandKind::Find
            }
        );
        keymap.unbind(ctrl_t);
        assert_eq!(
            shortcut_verdict(&keymap, &shortcut(Some(ctrl_t))),
            ShortcutVerdict::Free(ctrl_t)
        );
    }
}
