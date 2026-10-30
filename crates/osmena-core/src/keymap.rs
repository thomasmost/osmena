//! Keyboard shortcuts: a key chord maps to a command kind. The defaults
//! follow Chromium's conventions on each platform; the user's changes are
//! stored as `keymap.toml` in the profile and edited in the settings
//! window, slot by slot, the way a game's controls screen does it.

use std::fmt;
use std::io;
use std::path::Path;
use std::str::FromStr;

use keyboard_types::{Code, Modifiers};
use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::model::{TabId, WindowId};
use crate::settings::write_atomically;

/// A physical key plus modifiers. Chords use the physical key code, not
/// the produced character, so `Cmd+Shift+]` is the same chord on every
/// keyboard layout and does not depend on what `Shift+]` types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct KeyChord {
    pub modifiers: Modifiers,
    pub code: Code,
}

impl KeyChord {
    /// Only these modifiers take part in matching; lock keys and the like
    /// are ignored.
    pub const RELEVANT_MODIFIERS: Modifiers = Modifiers::CONTROL
        .union(Modifiers::SHIFT)
        .union(Modifiers::ALT)
        .union(Modifiers::META);

    pub fn new(modifiers: Modifiers, code: Code) -> KeyChord {
        KeyChord {
            modifiers: modifiers & Self::RELEVANT_MODIFIERS,
            code,
        }
    }
}

/// The stored form: modifiers in a fixed order and the W3C key code,
/// joined with `+`, like `Ctrl+Shift+KeyT` or `F5`. `Meta` is the
/// Command key on macOS and the Windows key elsewhere.
impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (flag, name) in [
            (Modifiers::CONTROL, "Ctrl"),
            (Modifiers::ALT, "Alt"),
            (Modifiers::SHIFT, "Shift"),
            (Modifiers::META, "Meta"),
        ] {
            if self.modifiers.contains(flag) {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.code)
    }
}

/// The text of a chord that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChordParseError(pub String);

impl fmt::Display for ChordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a key chord: {:?}", self.0)
    }
}

impl std::error::Error for ChordParseError {}

impl FromStr for KeyChord {
    type Err = ChordParseError;

    /// Accepts the [`Display`](fmt::Display) form, with the modifiers in
    /// any order and under their platform names too (`Cmd`, `Win`,
    /// `Option`, `Control`), case-insensitively.
    fn from_str(text: &str) -> Result<KeyChord, ChordParseError> {
        let err = || ChordParseError(text.to_string());
        let mut parts = text.split('+').map(str::trim).peekable();
        let mut modifiers = Modifiers::empty();
        let mut code = None;
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                code = Some(Code::from_str(part).map_err(|_| err())?);
            } else {
                modifiers |= match part.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => Modifiers::CONTROL,
                    "shift" => Modifiers::SHIFT,
                    "alt" | "option" => Modifiers::ALT,
                    "meta" | "cmd" | "command" | "win" | "super" => Modifiers::META,
                    _ => return Err(err()),
                };
            }
        }
        Ok(KeyChord::new(modifiers, code.ok_or_else(err)?))
    }
}

impl From<KeyChord> for String {
    fn from(chord: KeyChord) -> String {
        chord.to_string()
    }
}

impl TryFrom<String> for KeyChord {
    type Error = ChordParseError;

    fn try_from(text: String) -> Result<KeyChord, ChordParseError> {
        text.parse()
    }
}

/// A [`Command`] without its window or tab argument. The keymap resolves a
/// chord to one of these; the app fills in the focused window and its
/// active tab with [`CommandKind::into_command`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum CommandKind {
    NewTab,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PreviousTab,
    /// Switch to the tab at this position in the strip, 1 to 8.
    TabAt(u8),
    LastTab,
    Back,
    Forward,
    Reload,
    Stop,
    Home,
    FocusAddressBar,
    Find,
    FindNext,
    FindPrevious,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    BookmarkPage,
    ToggleBookmarksBar,
    NewWindow,
    CloseWindow,
    ToggleFullscreen,
    OpenSettings,
    Quit,
}

impl CommandKind {
    /// Every kind, in the order the settings window lists them. Used to
    /// check that the defaults bind each one and to write the keymap file.
    pub const ALL: [CommandKind; 33] = [
        CommandKind::NewTab,
        CommandKind::CloseTab,
        CommandKind::ReopenClosedTab,
        CommandKind::NextTab,
        CommandKind::PreviousTab,
        CommandKind::TabAt(1),
        CommandKind::TabAt(2),
        CommandKind::TabAt(3),
        CommandKind::TabAt(4),
        CommandKind::TabAt(5),
        CommandKind::TabAt(6),
        CommandKind::TabAt(7),
        CommandKind::TabAt(8),
        CommandKind::LastTab,
        CommandKind::Back,
        CommandKind::Forward,
        CommandKind::Reload,
        CommandKind::Stop,
        CommandKind::Home,
        CommandKind::FocusAddressBar,
        CommandKind::Find,
        CommandKind::FindNext,
        CommandKind::FindPrevious,
        CommandKind::ZoomIn,
        CommandKind::ZoomOut,
        CommandKind::ZoomReset,
        CommandKind::BookmarkPage,
        CommandKind::ToggleBookmarksBar,
        CommandKind::NewWindow,
        CommandKind::CloseWindow,
        CommandKind::ToggleFullscreen,
        CommandKind::OpenSettings,
        CommandKind::Quit,
    ];

    /// The stable name used in `keymap.toml`.
    pub fn name(self) -> String {
        match self {
            CommandKind::NewTab => "new-tab".to_string(),
            CommandKind::CloseTab => "close-tab".to_string(),
            CommandKind::ReopenClosedTab => "reopen-closed-tab".to_string(),
            CommandKind::NextTab => "next-tab".to_string(),
            CommandKind::PreviousTab => "previous-tab".to_string(),
            CommandKind::TabAt(n) => format!("tab-{n}"),
            CommandKind::LastTab => "last-tab".to_string(),
            CommandKind::Back => "back".to_string(),
            CommandKind::Forward => "forward".to_string(),
            CommandKind::Reload => "reload".to_string(),
            CommandKind::Stop => "stop".to_string(),
            CommandKind::Home => "home".to_string(),
            CommandKind::FocusAddressBar => "focus-address-bar".to_string(),
            CommandKind::Find => "find".to_string(),
            CommandKind::FindNext => "find-next".to_string(),
            CommandKind::FindPrevious => "find-previous".to_string(),
            CommandKind::ZoomIn => "zoom-in".to_string(),
            CommandKind::ZoomOut => "zoom-out".to_string(),
            CommandKind::ZoomReset => "zoom-reset".to_string(),
            CommandKind::BookmarkPage => "bookmark-page".to_string(),
            CommandKind::ToggleBookmarksBar => "toggle-bookmarks-bar".to_string(),
            CommandKind::NewWindow => "new-window".to_string(),
            CommandKind::CloseWindow => "close-window".to_string(),
            CommandKind::ToggleFullscreen => "toggle-fullscreen".to_string(),
            CommandKind::OpenSettings => "open-settings".to_string(),
            CommandKind::Quit => "quit".to_string(),
        }
    }

    /// The kind with this [`name`](CommandKind::name).
    pub fn from_name(name: &str) -> Option<CommandKind> {
        CommandKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The label the settings window and menus show.
    pub fn label(self) -> String {
        match self {
            CommandKind::NewTab => "New tab".to_string(),
            CommandKind::CloseTab => "Close tab".to_string(),
            CommandKind::ReopenClosedTab => "Reopen closed tab".to_string(),
            CommandKind::NextTab => "Next tab".to_string(),
            CommandKind::PreviousTab => "Previous tab".to_string(),
            CommandKind::TabAt(n) => format!("Switch to tab {n}"),
            CommandKind::LastTab => "Switch to last tab".to_string(),
            CommandKind::Back => "Back".to_string(),
            CommandKind::Forward => "Forward".to_string(),
            CommandKind::Reload => "Reload".to_string(),
            CommandKind::Stop => "Stop loading".to_string(),
            CommandKind::Home => "Open homepage".to_string(),
            CommandKind::FocusAddressBar => "Focus address bar".to_string(),
            CommandKind::Find => "Find in page".to_string(),
            CommandKind::FindNext => "Find next".to_string(),
            CommandKind::FindPrevious => "Find previous".to_string(),
            CommandKind::ZoomIn => "Zoom in".to_string(),
            CommandKind::ZoomOut => "Zoom out".to_string(),
            CommandKind::ZoomReset => "Actual size".to_string(),
            CommandKind::BookmarkPage => "Bookmark this page".to_string(),
            CommandKind::ToggleBookmarksBar => "Show bookmarks bar".to_string(),
            CommandKind::NewWindow => "New window".to_string(),
            CommandKind::CloseWindow => "Close window".to_string(),
            CommandKind::ToggleFullscreen => "Full screen".to_string(),
            CommandKind::OpenSettings => "Settings".to_string(),
            CommandKind::Quit => "Quit".to_string(),
        }
    }

    pub fn into_command(self, window: WindowId, tab: TabId) -> Command {
        match self {
            CommandKind::NewTab => Command::NewTab { window, url: None },
            CommandKind::CloseTab => Command::CloseTab(tab),
            CommandKind::ReopenClosedTab => Command::ReopenClosedTab(window),
            CommandKind::NextTab => Command::NextTab(window),
            CommandKind::PreviousTab => Command::PreviousTab(window),
            CommandKind::TabAt(n) => Command::ActivateTabAt {
                window,
                index: usize::from(n.max(1)) - 1,
            },
            CommandKind::LastTab => Command::ActivateLastTab(window),
            CommandKind::Back => Command::Back(tab),
            CommandKind::Forward => Command::Forward(tab),
            CommandKind::Reload => Command::Reload(tab),
            CommandKind::Stop => Command::Stop(tab),
            CommandKind::Home => Command::Home(tab),
            CommandKind::FocusAddressBar => Command::FocusAddressBar(window),
            CommandKind::Find => Command::OpenFind(window),
            CommandKind::FindNext => Command::FindNext(window),
            CommandKind::FindPrevious => Command::FindPrevious(window),
            CommandKind::ZoomIn => Command::ZoomIn(tab),
            CommandKind::ZoomOut => Command::ZoomOut(tab),
            CommandKind::ZoomReset => Command::ZoomReset(tab),
            CommandKind::BookmarkPage => Command::ToggleBookmark(tab),
            CommandKind::ToggleBookmarksBar => Command::ToggleBookmarksBar,
            CommandKind::NewWindow => Command::NewWindow,
            CommandKind::CloseWindow => Command::CloseWindow(window),
            CommandKind::ToggleFullscreen => Command::ToggleFullscreen(window),
            CommandKind::OpenSettings => Command::OpenSettings,
            CommandKind::Quit => Command::Quit,
        }
    }
}

impl From<CommandKind> for String {
    fn from(kind: CommandKind) -> String {
        kind.name()
    }
}

impl TryFrom<String> for CommandKind {
    type Error = String;

    fn try_from(name: String) -> Result<CommandKind, String> {
        CommandKind::from_name(&name).ok_or_else(|| format!("unknown action {name:?}"))
    }
}

/// Which platform's conventions the default bindings follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Windows
        }
    }

    /// The "primary" shortcut modifier: Command on macOS, Control elsewhere.
    pub fn primary(self) -> Modifiers {
        match self {
            Platform::MacOs => Modifiers::META,
            Platform::Windows => Modifiers::CONTROL,
        }
    }
}

/// Ordered chord-to-command bindings. Several chords may map to one kind
/// (the first is its primary shortcut, shown in menus); one chord maps to
/// at most one kind. Two keymaps are equal when every kind has the same
/// chords in the same order, whatever order the kinds were bound in.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<(KeyChord, CommandKind)>,
}

impl PartialEq for Keymap {
    fn eq(&self, other: &Keymap) -> bool {
        self.bindings.len() == other.bindings.len()
            && CommandKind::ALL
                .into_iter()
                .all(|kind| self.chords_for(kind) == other.chords_for(kind))
    }
}

impl Keymap {
    /// The built-in bindings for a platform: Chromium's, as far as the
    /// commands exist here. Each kind's first binding is its primary one.
    pub fn defaults(platform: Platform) -> Keymap {
        let primary = platform.primary();
        let shift = primary | Modifiers::SHIFT;
        let mut keymap = Keymap::default();
        let mut bind = |modifiers: Modifiers, code: Code, kind: CommandKind| {
            keymap.bind(KeyChord::new(modifiers, code), kind);
        };
        bind(primary, Code::KeyT, CommandKind::NewTab);
        bind(primary, Code::KeyW, CommandKind::CloseTab);
        bind(shift, Code::KeyT, CommandKind::ReopenClosedTab);
        bind(Modifiers::CONTROL, Code::Tab, CommandKind::NextTab);
        bind(
            Modifiers::CONTROL | Modifiers::SHIFT,
            Code::Tab,
            CommandKind::PreviousTab,
        );
        for (n, code) in [
            Code::Digit1,
            Code::Digit2,
            Code::Digit3,
            Code::Digit4,
            Code::Digit5,
            Code::Digit6,
            Code::Digit7,
            Code::Digit8,
        ]
        .into_iter()
        .enumerate()
        {
            bind(primary, code, CommandKind::TabAt(n as u8 + 1));
        }
        bind(primary, Code::Digit9, CommandKind::LastTab);
        bind(primary, Code::KeyR, CommandKind::Reload);
        // Not bare Escape: pages need it to close menus and dialogs.
        bind(primary, Code::Period, CommandKind::Stop);
        bind(primary, Code::KeyL, CommandKind::FocusAddressBar);
        bind(primary, Code::KeyF, CommandKind::Find);
        bind(primary, Code::KeyG, CommandKind::FindNext);
        bind(shift, Code::KeyG, CommandKind::FindPrevious);
        bind(primary, Code::Equal, CommandKind::ZoomIn);
        bind(primary, Code::NumpadAdd, CommandKind::ZoomIn);
        bind(primary, Code::Minus, CommandKind::ZoomOut);
        bind(primary, Code::NumpadSubtract, CommandKind::ZoomOut);
        bind(primary, Code::Digit0, CommandKind::ZoomReset);
        bind(primary, Code::Numpad0, CommandKind::ZoomReset);
        bind(primary, Code::KeyD, CommandKind::BookmarkPage);
        bind(shift, Code::KeyB, CommandKind::ToggleBookmarksBar);
        bind(primary, Code::KeyN, CommandKind::NewWindow);
        bind(shift, Code::KeyW, CommandKind::CloseWindow);
        bind(primary, Code::Comma, CommandKind::OpenSettings);
        bind(primary, Code::KeyQ, CommandKind::Quit);
        match platform {
            Platform::MacOs => {
                bind(shift, Code::BracketRight, CommandKind::NextTab);
                bind(shift, Code::BracketLeft, CommandKind::PreviousTab);
                // Not Cmd+Arrow: those move the caret to line start and
                // end in text fields.
                bind(primary, Code::BracketLeft, CommandKind::Back);
                bind(primary, Code::BracketRight, CommandKind::Forward);
                bind(shift, Code::KeyH, CommandKind::Home);
                bind(
                    Modifiers::CONTROL | Modifiers::META,
                    Code::KeyF,
                    CommandKind::ToggleFullscreen,
                );
            }
            Platform::Windows => {
                bind(primary, Code::F4, CommandKind::CloseTab);
                bind(primary, Code::PageDown, CommandKind::NextTab);
                bind(primary, Code::PageUp, CommandKind::PreviousTab);
                bind(Modifiers::ALT, Code::ArrowLeft, CommandKind::Back);
                bind(Modifiers::ALT, Code::ArrowRight, CommandKind::Forward);
                bind(Modifiers::empty(), Code::F5, CommandKind::Reload);
                bind(Modifiers::ALT, Code::Home, CommandKind::Home);
                bind(Modifiers::ALT, Code::KeyD, CommandKind::FocusAddressBar);
                bind(Modifiers::empty(), Code::F3, CommandKind::FindNext);
                bind(Modifiers::SHIFT, Code::F3, CommandKind::FindPrevious);
                bind(Modifiers::empty(), Code::F11, CommandKind::ToggleFullscreen);
            }
        }
        keymap
    }

    /// Bind `chord` to `kind`, replacing any previous binding of that chord.
    /// The chord becomes the last of `kind`'s bindings.
    pub fn bind(&mut self, chord: KeyChord, kind: CommandKind) {
        // Normalise so a chord built with lock keys or other irrelevant
        // modifiers is stored in the same form `resolve` matches on.
        let chord = KeyChord::new(chord.modifiers, chord.code);
        self.bindings.retain(|(c, _)| *c != chord);
        self.bindings.push((chord, kind));
    }

    /// Remove `chord`'s binding, returning the kind it fired.
    pub fn unbind(&mut self, chord: KeyChord) -> Option<CommandKind> {
        let chord = KeyChord::new(chord.modifiers, chord.code);
        let kind = self.resolve(chord)?;
        self.bindings.retain(|(c, _)| *c != chord);
        Some(kind)
    }

    pub fn resolve(&self, chord: KeyChord) -> Option<CommandKind> {
        let chord = KeyChord::new(chord.modifiers, chord.code);
        self.bindings
            .iter()
            .find(|(c, _)| *c == chord)
            .map(|(_, kind)| *kind)
    }

    /// The primary chord of `kind`, for showing hints in menus.
    pub fn chord_for(&self, kind: CommandKind) -> Option<KeyChord> {
        self.chords_for(kind).into_iter().next()
    }

    /// Every chord bound to `kind`, primary first.
    pub fn chords_for(&self, kind: CommandKind) -> Vec<KeyChord> {
        self.bindings
            .iter()
            .filter(|(_, k)| *k == kind)
            .map(|(c, _)| *c)
            .collect()
    }

    /// Make `chords` the bindings of `kind`, in that order, in place of
    /// whatever it had. A chord another kind held moves to `kind`; those
    /// displaced bindings are returned so the UI can say what was unbound.
    pub fn set_chords(
        &mut self,
        kind: CommandKind,
        chords: &[KeyChord],
    ) -> Vec<(KeyChord, CommandKind)> {
        let chords: Vec<KeyChord> = chords
            .iter()
            .map(|c| KeyChord::new(c.modifiers, c.code))
            .collect();
        let displaced: Vec<(KeyChord, CommandKind)> = self
            .bindings
            .iter()
            .filter(|(c, k)| *k != kind && chords.contains(c))
            .copied()
            .collect();
        self.bindings
            .retain(|(c, k)| *k != kind && !chords.contains(c));
        for chord in chords {
            if !self.bindings.iter().any(|(c, _)| *c == chord) {
                self.bindings.push((chord, kind));
            }
        }
        displaced
    }

    /// Set slot `slot` (0 is primary) of `kind` to `chord`, or clear it
    /// with `None`. A slot past the end appends; clearing a slot past the
    /// end does nothing. Returns the binding of another kind the chord was
    /// taken from, if any.
    pub fn set_slot(
        &mut self,
        kind: CommandKind,
        slot: usize,
        chord: Option<KeyChord>,
    ) -> Option<(KeyChord, CommandKind)> {
        let mut chords = self.chords_for(kind);
        match chord {
            Some(chord) => {
                let chord = KeyChord::new(chord.modifiers, chord.code);
                match chords.iter().position(|c| *c == chord) {
                    // Already in this slot: nothing to do.
                    Some(at) if at == slot => {}
                    // In another slot of this kind: the two slots swap.
                    Some(at) if slot < chords.len() => chords.swap(at, slot),
                    Some(at) => {
                        chords.remove(at);
                        chords.push(chord);
                    }
                    None if slot < chords.len() => chords[slot] = chord,
                    None => chords.push(chord),
                }
            }
            None => {
                if slot < chords.len() {
                    chords.remove(slot);
                }
            }
        }
        self.set_chords(kind, &chords).into_iter().next()
    }

    pub fn bindings(&self) -> impl Iterator<Item = (KeyChord, CommandKind)> + '_ {
        self.bindings.iter().copied()
    }

    /// The keymap as `keymap.toml`: every action with its chords, in
    /// [`CommandKind::ALL`] order.
    pub fn to_toml(&self) -> String {
        let mut text = String::from(
            "# Keyboard shortcuts. Each action lists the chords that fire it, primary\n\
             # first; an empty list unbinds the action, and an action left out keeps\n\
             # its default. A chord is modifiers (Ctrl, Alt, Shift, Meta) and a key\n\
             # code (KeyT, Digit1, F5, BracketRight, ...) joined with \"+\". Meta is\n\
             # Command on macOS and the Windows key elsewhere.\n\
             \n\
             [bindings]\n",
        );
        for kind in CommandKind::ALL {
            let chords: Vec<String> = self
                .chords_for(kind)
                .iter()
                .map(|c| format!("{c:?}", c = c.to_string()))
                .collect();
            text.push_str(&format!("{} = [{}]\n", kind.name(), chords.join(", ")));
        }
        text
    }

    /// The keymap described by `text`, laid over `platform`'s defaults:
    /// an action named in the file gets exactly the chords listed there,
    /// the others keep their defaults. Unknown actions are skipped; a
    /// chord that does not parse is an error.
    pub fn parse(text: &str, platform: Platform) -> Result<Keymap, KeymapError> {
        let file: KeymapFile = toml::from_str(text).map_err(KeymapError::Parse)?;
        let mut keymap = Keymap::defaults(platform);
        // Clear every listed action first, so a chord that one action
        // keeps by default and another action takes in the file is not
        // bound back by the first action's defaults.
        let mut listed = Vec::new();
        for (name, chords) in file.entries() {
            let Some(kind) = CommandKind::from_name(&name) else {
                continue;
            };
            let chords = chords
                .iter()
                .map(|c| c.parse::<KeyChord>())
                .collect::<Result<Vec<KeyChord>, ChordParseError>>()
                .map_err(KeymapError::Chord)?;
            keymap.set_chords(kind, &[]);
            listed.push((kind, chords));
        }
        for (kind, chords) in listed {
            keymap.set_chords(kind, &chords);
        }
        Ok(keymap)
    }

    /// Load from `path`. A missing file yields the platform's defaults; a
    /// malformed file is an error so a typo never silently resets the
    /// user's shortcuts.
    pub fn load(path: &Path, platform: Platform) -> Result<Keymap, KeymapError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Keymap::parse(&text, platform),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Keymap::defaults(platform)),
            Err(e) => Err(KeymapError::Io(e)),
        }
    }

    /// Write to `path` atomically, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), KeymapError> {
        write_atomically(path, &self.to_toml()).map_err(KeymapError::Io)
    }
}

/// The shape of `keymap.toml`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct KeymapFile {
    bindings: toml::Table,
}

impl KeymapFile {
    /// The actions in file order with their chord strings. A value that is
    /// not an array of strings is treated as a chord that does not parse.
    fn entries(&self) -> Vec<(String, Vec<String>)> {
        self.bindings
            .iter()
            .map(|(name, value)| {
                let chords = match value.as_array() {
                    Some(items) => items
                        .iter()
                        .map(|v| v.as_str().unwrap_or("?").to_string())
                        .collect(),
                    None => vec!["?".to_string()],
                };
                (name.clone(), chords)
            })
            .collect()
    }
}

#[derive(Debug)]
pub enum KeymapError {
    Io(io::Error),
    Parse(toml::de::Error),
    Chord(ChordParseError),
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeymapError::Io(e) => write!(f, "keymap file: {e}"),
            KeymapError::Parse(e) => write!(f, "keymap file is not valid TOML: {e}"),
            KeymapError::Chord(e) => write!(f, "keymap file: {e}"),
        }
    }
}

impl std::error::Error for KeymapError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_defaults_use_command_key() {
        let k = Keymap::defaults(Platform::MacOs);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::KeyT)),
            Some(CommandKind::NewTab)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::CONTROL, Code::KeyT)),
            None
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::BracketLeft)),
            Some(CommandKind::Back)
        );
        assert_eq!(
            k.resolve(KeyChord::new(
                Modifiers::META | Modifiers::SHIFT,
                Code::BracketLeft
            )),
            Some(CommandKind::PreviousTab)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::Period)),
            Some(CommandKind::Stop)
        );
        assert_eq!(
            k.resolve(KeyChord::new(
                Modifiers::CONTROL | Modifiers::META,
                Code::KeyF
            )),
            Some(CommandKind::ToggleFullscreen)
        );
    }

    #[test]
    fn keys_pages_need_are_left_unbound() {
        for platform in [Platform::MacOs, Platform::Windows] {
            let k = Keymap::defaults(platform);
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::empty(), Code::Escape)),
                None
            );
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::META, Code::ArrowLeft)),
                None
            );
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::META, Code::ArrowRight)),
                None
            );
        }
    }

    #[test]
    fn windows_defaults_follow_chromium() {
        let k = Keymap::defaults(Platform::Windows);
        let ctrl = |code| KeyChord::new(Modifiers::CONTROL, code);
        assert_eq!(k.resolve(ctrl(Code::KeyT)), Some(CommandKind::NewTab));
        assert_eq!(k.resolve(KeyChord::new(Modifiers::META, Code::KeyT)), None);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::ALT, Code::ArrowLeft)),
            Some(CommandKind::Back)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::empty(), Code::F5)),
            Some(CommandKind::Reload)
        );
        assert_eq!(k.resolve(ctrl(Code::F4)), Some(CommandKind::CloseTab));
        assert_eq!(
            k.resolve(KeyChord::new(
                Modifiers::CONTROL | Modifiers::SHIFT,
                Code::KeyT
            )),
            Some(CommandKind::ReopenClosedTab)
        );
        assert_eq!(k.resolve(ctrl(Code::PageDown)), Some(CommandKind::NextTab));
        assert_eq!(
            k.resolve(ctrl(Code::PageUp)),
            Some(CommandKind::PreviousTab)
        );
        assert_eq!(k.resolve(ctrl(Code::Digit1)), Some(CommandKind::TabAt(1)));
        assert_eq!(k.resolve(ctrl(Code::Digit8)), Some(CommandKind::TabAt(8)));
        assert_eq!(k.resolve(ctrl(Code::Digit9)), Some(CommandKind::LastTab));
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::ALT, Code::Home)),
            Some(CommandKind::Home)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::ALT, Code::KeyD)),
            Some(CommandKind::FocusAddressBar)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::empty(), Code::F11)),
            Some(CommandKind::ToggleFullscreen)
        );
        // The primary chord is the one menus show.
        assert_eq!(k.chord_for(CommandKind::Reload), Some(ctrl(Code::KeyR)));
        assert_eq!(k.chord_for(CommandKind::CloseTab), Some(ctrl(Code::KeyW)));
    }

    #[test]
    fn every_command_kind_has_a_default_chord_on_both_platforms() {
        for platform in [Platform::MacOs, Platform::Windows] {
            let k = Keymap::defaults(platform);
            for kind in CommandKind::ALL {
                assert!(
                    k.chord_for(kind).is_some(),
                    "{kind:?} has no default on {platform:?}"
                );
                assert!(
                    k.chords_for(kind).len() <= 2,
                    "{kind:?} has more chords than the editor shows on {platform:?}"
                );
            }
        }
    }

    #[test]
    fn irrelevant_modifiers_are_ignored_when_matching() {
        let k = Keymap::defaults(Platform::MacOs);
        let with_caps_lock = KeyChord {
            modifiers: Modifiers::META | Modifiers::CAPS_LOCK,
            code: Code::KeyT,
        };
        assert_eq!(k.resolve(with_caps_lock), Some(CommandKind::NewTab));
    }

    #[test]
    fn bind_replaces_an_existing_chord() {
        let mut k = Keymap::defaults(Platform::MacOs);
        let chord = KeyChord::new(Modifiers::META, Code::KeyT);
        k.bind(chord, CommandKind::Reload);
        assert_eq!(k.resolve(chord), Some(CommandKind::Reload));
        assert_eq!(k.bindings().filter(|(c, _)| *c == chord).count(), 1);
    }

    #[test]
    fn bind_normalises_irrelevant_modifiers_so_the_chord_resolves() {
        let mut k = Keymap::default();
        let noisy = KeyChord {
            modifiers: Modifiers::META | Modifiers::CAPS_LOCK,
            code: Code::KeyT,
        };
        k.bind(noisy, CommandKind::NewTab);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::KeyT)),
            Some(CommandKind::NewTab)
        );
        assert_eq!(k.resolve(noisy), Some(CommandKind::NewTab));
        assert_eq!(
            k.bindings().next(),
            Some((
                KeyChord::new(Modifiers::META, Code::KeyT),
                CommandKind::NewTab
            ))
        );
    }

    #[test]
    fn bind_replaces_existing_chord_given_irrelevant_modifiers() {
        let mut k = Keymap::defaults(Platform::MacOs);
        let noisy = KeyChord {
            modifiers: Modifiers::META | Modifiers::CAPS_LOCK,
            code: Code::KeyT,
        };
        k.bind(noisy, CommandKind::Reload);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::KeyT)),
            Some(CommandKind::Reload)
        );
        assert_eq!(
            k.bindings()
                .filter(|(c, _)| c.code == Code::KeyT && c.modifiers == Modifiers::META)
                .count(),
            1
        );
    }

    #[test]
    fn unbind_removes_the_chord_and_reports_its_kind() {
        let mut k = Keymap::defaults(Platform::Windows);
        let chord = KeyChord::new(Modifiers::empty(), Code::F5);
        assert_eq!(k.unbind(chord), Some(CommandKind::Reload));
        assert_eq!(k.resolve(chord), None);
        assert_eq!(k.unbind(chord), None);
        // The primary chord is untouched.
        assert_eq!(
            k.chord_for(CommandKind::Reload),
            Some(KeyChord::new(Modifiers::CONTROL, Code::KeyR))
        );
    }

    #[test]
    fn into_command_fills_in_window_and_tab() {
        let w = WindowId(3);
        let t = TabId(9);
        assert_eq!(
            CommandKind::NewTab.into_command(w, t),
            Command::NewTab {
                window: w,
                url: None
            }
        );
        assert_eq!(
            CommandKind::CloseTab.into_command(w, t),
            Command::CloseTab(t)
        );
        assert_eq!(CommandKind::NextTab.into_command(w, t), Command::NextTab(w));
        assert_eq!(CommandKind::Quit.into_command(w, t), Command::Quit);
        assert_eq!(
            CommandKind::TabAt(3).into_command(w, t),
            Command::ActivateTabAt {
                window: w,
                index: 2
            }
        );
        assert_eq!(
            CommandKind::LastTab.into_command(w, t),
            Command::ActivateLastTab(w)
        );
        assert_eq!(
            CommandKind::ReopenClosedTab.into_command(w, t),
            Command::ReopenClosedTab(w)
        );
        assert_eq!(CommandKind::Home.into_command(w, t), Command::Home(t));
        assert_eq!(
            CommandKind::ToggleFullscreen.into_command(w, t),
            Command::ToggleFullscreen(w)
        );
    }

    #[test]
    fn names_are_unique_and_round_trip() {
        let mut seen = std::collections::HashSet::new();
        for kind in CommandKind::ALL {
            let name = kind.name();
            assert!(seen.insert(name.clone()), "duplicate name {name}");
            assert_eq!(CommandKind::from_name(&name), Some(kind));
            assert!(!kind.label().is_empty());
        }
        assert_eq!(CommandKind::from_name("tab-3"), Some(CommandKind::TabAt(3)));
        assert_eq!(CommandKind::from_name("tab-9"), None);
        assert_eq!(CommandKind::from_name("fly"), None);
    }

    #[test]
    fn chords_display_and_parse() {
        let chord = KeyChord::new(Modifiers::CONTROL | Modifiers::SHIFT, Code::BracketRight);
        assert_eq!(chord.to_string(), "Ctrl+Shift+BracketRight");
        assert_eq!("Ctrl+Shift+BracketRight".parse::<KeyChord>(), Ok(chord));
        assert_eq!("shift + control + BracketRight".parse(), Ok(chord));
        assert_eq!(
            "F5".parse::<KeyChord>(),
            Ok(KeyChord::new(Modifiers::empty(), Code::F5))
        );
        assert_eq!(
            "Cmd+KeyT".parse::<KeyChord>(),
            Ok(KeyChord::new(Modifiers::META, Code::KeyT))
        );
        assert_eq!(
            "Win+Option+KeyT".parse::<KeyChord>(),
            Ok(KeyChord::new(Modifiers::META | Modifiers::ALT, Code::KeyT))
        );
        assert_eq!(
            KeyChord::new(
                Modifiers::META | Modifiers::ALT | Modifiers::CONTROL | Modifiers::SHIFT,
                Code::KeyA
            )
            .to_string(),
            "Ctrl+Alt+Shift+Meta+KeyA"
        );
        for bad in ["", "Ctrl+", "Ctrl+T", "Hyper+KeyT", "Ctrl+Shift", "+"] {
            assert!(bad.parse::<KeyChord>().is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn set_chords_replaces_a_kinds_bindings_and_reports_what_it_took() {
        let mut k = Keymap::defaults(Platform::Windows);
        let ctrl_t = KeyChord::new(Modifiers::CONTROL, Code::KeyT);
        let f9 = KeyChord::new(Modifiers::empty(), Code::F9);
        let displaced = k.set_chords(CommandKind::Reload, &[f9, ctrl_t]);
        assert_eq!(displaced, vec![(ctrl_t, CommandKind::NewTab)]);
        assert_eq!(k.chords_for(CommandKind::Reload), vec![f9, ctrl_t]);
        assert_eq!(k.chords_for(CommandKind::NewTab), vec![]);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::empty(), Code::F5)),
            None,
            "F5 was Reload's and is gone"
        );
        assert_eq!(k.set_chords(CommandKind::Reload, &[]), vec![]);
        assert_eq!(k.chord_for(CommandKind::Reload), None);
    }

    #[test]
    fn set_slot_edits_one_slot_and_keeps_the_other() {
        let mut k = Keymap::defaults(Platform::Windows);
        let ctrl_r = KeyChord::new(Modifiers::CONTROL, Code::KeyR);
        let f5 = KeyChord::new(Modifiers::empty(), Code::F5);
        let f9 = KeyChord::new(Modifiers::empty(), Code::F9);
        assert_eq!(k.chords_for(CommandKind::Reload), vec![ctrl_r, f5]);

        assert_eq!(k.set_slot(CommandKind::Reload, 1, Some(f9)), None);
        assert_eq!(k.chords_for(CommandKind::Reload), vec![ctrl_r, f9]);
        assert_eq!(k.resolve(f5), None);

        // Taking another kind's chord reports it.
        let ctrl_t = KeyChord::new(Modifiers::CONTROL, Code::KeyT);
        assert_eq!(
            k.set_slot(CommandKind::Reload, 0, Some(ctrl_t)),
            Some((ctrl_t, CommandKind::NewTab))
        );
        assert_eq!(k.chords_for(CommandKind::Reload), vec![ctrl_t, f9]);
        assert_eq!(k.chords_for(CommandKind::NewTab), vec![]);

        // Clearing the primary promotes the secondary; clearing an empty
        // slot does nothing.
        assert_eq!(k.set_slot(CommandKind::Reload, 0, None), None);
        assert_eq!(k.chords_for(CommandKind::Reload), vec![f9]);
        assert_eq!(k.set_slot(CommandKind::Reload, 1, None), None);
        assert_eq!(k.chords_for(CommandKind::Reload), vec![f9]);

        // A slot past the end appends; the same chord twice in one kind
        // collapses to one.
        assert_eq!(k.set_slot(CommandKind::Reload, 5, Some(ctrl_r)), None);
        assert_eq!(k.chords_for(CommandKind::Reload), vec![f9, ctrl_r]);
        assert_eq!(k.set_slot(CommandKind::Reload, 0, Some(ctrl_r)), None);
        assert_eq!(k.chords_for(CommandKind::Reload), vec![ctrl_r, f9]);
    }

    #[test]
    fn toml_round_trips_and_lists_every_action() {
        let mut k = Keymap::defaults(Platform::Windows);
        k.set_chords(CommandKind::Stop, &[]);
        k.set_slot(
            CommandKind::Home,
            0,
            Some(KeyChord::new(
                Modifiers::CONTROL | Modifiers::ALT,
                Code::KeyH,
            )),
        );
        let text = k.to_toml();
        assert!(text.contains("[bindings]\n"), "{text}");
        assert!(text.contains("new-tab = [\"Ctrl+KeyT\"]\n"), "{text}");
        assert!(
            text.contains("reload = [\"Ctrl+KeyR\", \"F5\"]\n"),
            "{text}"
        );
        assert!(text.contains("stop = []\n"), "{text}");
        assert!(text.contains("home = [\"Ctrl+Alt+KeyH\"]\n"), "{text}");
        for kind in CommandKind::ALL {
            assert!(text.contains(&format!("\n{} = [", kind.name())), "{kind:?}");
        }
        assert_eq!(Keymap::parse(&text, Platform::Windows).unwrap(), k);
    }

    #[test]
    fn a_partial_file_overlays_the_defaults() {
        let text = "[bindings]\nreload = [\"F9\"]\nstop = []\n";
        let k = Keymap::parse(text, Platform::Windows).unwrap();
        assert_eq!(
            k.chords_for(CommandKind::Reload),
            vec![KeyChord::new(Modifiers::empty(), Code::F9)]
        );
        assert_eq!(k.chords_for(CommandKind::Stop), vec![]);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::CONTROL, Code::KeyT)),
            Some(CommandKind::NewTab),
            "unlisted actions keep their defaults"
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::empty(), Code::F5)),
            None,
            "a listed action loses its default chords"
        );
    }

    #[test]
    fn a_file_may_move_a_default_chord_between_actions() {
        // Ctrl+T is New tab's by default; the file gives it to Reload and
        // New tab something else. Neither order of the two lines matters.
        for text in [
            "[bindings]\nreload = [\"Ctrl+KeyT\"]\nnew-tab = [\"Ctrl+KeyY\"]\n",
            "[bindings]\nnew-tab = [\"Ctrl+KeyY\"]\nreload = [\"Ctrl+KeyT\"]\n",
        ] {
            let k = Keymap::parse(text, Platform::Windows).unwrap();
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::CONTROL, Code::KeyT)),
                Some(CommandKind::Reload),
                "{text}"
            );
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::CONTROL, Code::KeyY)),
                Some(CommandKind::NewTab),
                "{text}"
            );
        }
    }

    #[test]
    fn empty_and_unknown_actions_are_tolerated_but_bad_chords_are_errors() {
        assert_eq!(
            Keymap::parse("", Platform::MacOs).unwrap(),
            Keymap::defaults(Platform::MacOs)
        );
        let k = Keymap::parse("[bindings]\nfly = [\"F1\"]\n", Platform::MacOs).unwrap();
        assert_eq!(k, Keymap::defaults(Platform::MacOs));
        assert!(matches!(
            Keymap::parse("[bindings]\nreload = [\"Ctrl+T\"]\n", Platform::MacOs),
            Err(KeymapError::Chord(_))
        ));
        assert!(matches!(
            Keymap::parse("[bindings]\nreload = \"F5\"\n", Platform::MacOs),
            Err(KeymapError::Chord(_))
        ));
        assert!(matches!(
            Keymap::parse("bindings = ", Platform::MacOs),
            Err(KeymapError::Parse(_))
        ));
    }

    #[test]
    fn load_of_missing_file_is_the_defaults_and_save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("keymap.toml");
        assert_eq!(
            Keymap::load(&path, Platform::Windows).unwrap(),
            Keymap::defaults(Platform::Windows)
        );
        let mut k = Keymap::defaults(Platform::Windows);
        k.set_slot(
            CommandKind::NewTab,
            0,
            Some(KeyChord::new(Modifiers::CONTROL, Code::KeyY)),
        );
        k.save(&path).unwrap();
        assert_eq!(Keymap::load(&path, Platform::Windows).unwrap(), k);
    }
}
