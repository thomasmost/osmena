//! Per-user profile directories. All of a profile's state lives under one
//! directory so one process can run per profile.
//!
//! | OS      | Profile directory                                           |
//! |---------|-------------------------------------------------------------|
//! | macOS   | `~/Library/Application Support/<brand>/profiles/<name>/`    |
//! | Windows | `%APPDATA%\<brand>\profiles\<name>\`                        |
//!
//! where `<brand>` is the [`Brand::data_dir`] of the browser.
//!
//! A profile's directory name is its identity on the command line
//! (`--profile <name>`) and on disk; what the user sees is the display
//! name, avatar and colour in its `settings.toml` (see
//! [`ProfileSettings`]), which [`Profile::siblings`] reads for the other
//! profiles on the machine.
//!
//! [`ProfileSettings`]: crate::settings::ProfileSettings

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::brand::{Brand, EngineInfo};
use crate::settings::{ChromeColor, Settings};

pub const PROFILES_DIR_NAME: &str = "profiles";
pub const DEFAULT_PROFILE_NAME: &str = "default";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub dir: PathBuf,
    /// The engine's data directory inside `dir`: cookies, local storage,
    /// the HTTP cache, and whatever else the engine keeps. Its name comes
    /// from [`EngineInfo::data_dir`], so two engines never share one.
    pub engine_dir: PathBuf,
}

/// Another profile on this machine, as the menus list it: its directory
/// name (what `--profile` takes) and what its settings say it looks
/// like.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileEntry {
    /// The directory name.
    pub dir: String,
    /// The display name ([`ProfileSettings::display_name`]).
    ///
    /// [`ProfileSettings::display_name`]: crate::settings::ProfileSettings::display_name
    pub name: String,
    pub avatar: Avatar,
    pub color: ChromeColor,
}

/// A profile's picture: a letter (the display name's first) or one of a
/// few glyphs, drawn on the profile's colour. Stored by name
/// (`avatar = "whale"`, `avatar = "tropical-fish"`). The dialog offers
/// the glyphs in [`Avatar::PALETTES`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Avatar {
    /// The first letter of the display name.
    #[default]
    Initial,
    Cat,
    Panda,
    Whale,
    Octopus,
    Shell,
    Turtle,
    Penguin,
    Frog,
    Rabbit,
    Bear,
    Koala,
    Dragon,
    Rocket,
    Moon,
    Star,
    Cactus,
    Blossom,
    Coffee,
    Guitar,
    Anchor,
    Bolt,
    Ghost,
    Telescope,
    Sailboat,
    SpoutingWhale,
    Dolphin,
    Fish,
    TropicalFish,
    Blowfish,
    Shark,
    Seal,
    Otter,
    Squid,
    Jellyfish,
    Crab,
    Lobster,
    Shrimp,
    Oyster,
    Coral,
    Crocodile,
    Merperson,
    TearsOfJoy,
    RedHeart,
    RollingOnTheFloor,
    ThumbsUp,
    LoudlyCrying,
    FoldedHands,
    BlowingAKiss,
    SmilingWithHearts,
    HeartEyes,
    SmilingEyes,
    PartyPopper,
    Beaming,
    TwoHearts,
    Pleading,
    SweatSmile,
    Fire,
    Smiling,
    Facepalm,
    HeartSuit,
    Shrug,
    EyeRoll,
    Laughing,
    Hug,
    Wink,
}

/// A named group of avatars, as the profile dialog shows them: a heading
/// and a grid of glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvatarPalette {
    pub name: &'static str,
    pub avatars: &'static [Avatar],
}

impl Avatar {
    /// The dialog's palettes, one tab each, in order: the sea creatures,
    /// the 24 most used emoji (Unicode's frequency ranking, most used
    /// first), then the assorted set. [`Avatar::Initial`] belongs to
    /// none; the dialog offers it above the tabs. An avatar may appear in
    /// more than one palette.
    pub const PALETTES: [AvatarPalette; 3] = [
        AvatarPalette {
            name: "Sea creatures",
            avatars: &[
                Avatar::Whale,
                Avatar::SpoutingWhale,
                Avatar::Dolphin,
                Avatar::Shark,
                Avatar::Seal,
                Avatar::Otter,
                Avatar::Octopus,
                Avatar::Squid,
                Avatar::Jellyfish,
                Avatar::Crab,
                Avatar::Lobster,
                Avatar::Shrimp,
                Avatar::Fish,
                Avatar::TropicalFish,
                Avatar::Blowfish,
                Avatar::Turtle,
                Avatar::Penguin,
                Avatar::Crocodile,
                Avatar::Shell,
                Avatar::Oyster,
                Avatar::Coral,
                Avatar::Merperson,
            ],
        },
        AvatarPalette {
            name: "Emoji",
            avatars: &[
                Avatar::TearsOfJoy,
                Avatar::RedHeart,
                Avatar::RollingOnTheFloor,
                Avatar::ThumbsUp,
                Avatar::LoudlyCrying,
                Avatar::FoldedHands,
                Avatar::BlowingAKiss,
                Avatar::SmilingWithHearts,
                Avatar::HeartEyes,
                Avatar::SmilingEyes,
                Avatar::PartyPopper,
                Avatar::Beaming,
                Avatar::TwoHearts,
                Avatar::Pleading,
                Avatar::SweatSmile,
                Avatar::Fire,
                Avatar::Smiling,
                Avatar::Facepalm,
                Avatar::HeartSuit,
                Avatar::Shrug,
                Avatar::EyeRoll,
                Avatar::Laughing,
                Avatar::Hug,
                Avatar::Wink,
            ],
        },
        AvatarPalette {
            name: "Assorted",
            avatars: &[
                Avatar::Cat,
                Avatar::Panda,
                Avatar::Whale,
                Avatar::Octopus,
                Avatar::Shell,
                Avatar::Turtle,
                Avatar::Penguin,
                Avatar::Frog,
                Avatar::Rabbit,
                Avatar::Bear,
                Avatar::Koala,
                Avatar::Dragon,
                Avatar::Rocket,
                Avatar::Moon,
                Avatar::Star,
                Avatar::Cactus,
                Avatar::Blossom,
                Avatar::Coffee,
                Avatar::Guitar,
                Avatar::Anchor,
                Avatar::Bolt,
                Avatar::Ghost,
                Avatar::Telescope,
                Avatar::Sailboat,
            ],
        },
    ];

    /// Every avatar once: the initial, then every glyph.
    pub const ALL: [Avatar; 66] = [
        Avatar::Initial,
        Avatar::Cat,
        Avatar::Panda,
        Avatar::Whale,
        Avatar::Octopus,
        Avatar::Shell,
        Avatar::Turtle,
        Avatar::Penguin,
        Avatar::Frog,
        Avatar::Rabbit,
        Avatar::Bear,
        Avatar::Koala,
        Avatar::Dragon,
        Avatar::Rocket,
        Avatar::Moon,
        Avatar::Star,
        Avatar::Cactus,
        Avatar::Blossom,
        Avatar::Coffee,
        Avatar::Guitar,
        Avatar::Anchor,
        Avatar::Bolt,
        Avatar::Ghost,
        Avatar::Telescope,
        Avatar::Sailboat,
        Avatar::SpoutingWhale,
        Avatar::Dolphin,
        Avatar::Fish,
        Avatar::TropicalFish,
        Avatar::Blowfish,
        Avatar::Shark,
        Avatar::Seal,
        Avatar::Otter,
        Avatar::Squid,
        Avatar::Jellyfish,
        Avatar::Crab,
        Avatar::Lobster,
        Avatar::Shrimp,
        Avatar::Oyster,
        Avatar::Coral,
        Avatar::Crocodile,
        Avatar::Merperson,
        Avatar::TearsOfJoy,
        Avatar::RedHeart,
        Avatar::RollingOnTheFloor,
        Avatar::ThumbsUp,
        Avatar::LoudlyCrying,
        Avatar::FoldedHands,
        Avatar::BlowingAKiss,
        Avatar::SmilingWithHearts,
        Avatar::HeartEyes,
        Avatar::SmilingEyes,
        Avatar::PartyPopper,
        Avatar::Beaming,
        Avatar::TwoHearts,
        Avatar::Pleading,
        Avatar::SweatSmile,
        Avatar::Fire,
        Avatar::Smiling,
        Avatar::Facepalm,
        Avatar::HeartSuit,
        Avatar::Shrug,
        Avatar::EyeRoll,
        Avatar::Laughing,
        Avatar::Hug,
        Avatar::Wink,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Avatar::Initial => "Initial",
            Avatar::Cat => "Cat",
            Avatar::Panda => "Panda",
            Avatar::Whale => "Whale",
            Avatar::Octopus => "Octopus",
            Avatar::Shell => "Shell",
            Avatar::Turtle => "Turtle",
            Avatar::Penguin => "Penguin",
            Avatar::Frog => "Frog",
            Avatar::Rabbit => "Rabbit",
            Avatar::Bear => "Bear",
            Avatar::Koala => "Koala",
            Avatar::Dragon => "Dragon",
            Avatar::Rocket => "Rocket",
            Avatar::Moon => "Moon",
            Avatar::Star => "Star",
            Avatar::Cactus => "Cactus",
            Avatar::Blossom => "Blossom",
            Avatar::Coffee => "Coffee",
            Avatar::Guitar => "Guitar",
            Avatar::Anchor => "Anchor",
            Avatar::Bolt => "Bolt",
            Avatar::Ghost => "Ghost",
            Avatar::Telescope => "Telescope",
            Avatar::Sailboat => "Sailboat",
            Avatar::SpoutingWhale => "Spouting whale",
            Avatar::Dolphin => "Dolphin",
            Avatar::Fish => "Fish",
            Avatar::TropicalFish => "Tropical fish",
            Avatar::Blowfish => "Blowfish",
            Avatar::Shark => "Shark",
            Avatar::Seal => "Seal",
            Avatar::Otter => "Otter",
            Avatar::Squid => "Squid",
            Avatar::Jellyfish => "Jellyfish",
            Avatar::Crab => "Crab",
            Avatar::Lobster => "Lobster",
            Avatar::Shrimp => "Shrimp",
            Avatar::Oyster => "Oyster",
            Avatar::Coral => "Coral",
            Avatar::Crocodile => "Crocodile",
            Avatar::Merperson => "Merperson",
            Avatar::TearsOfJoy => "Tears of joy",
            Avatar::RedHeart => "Red heart",
            Avatar::RollingOnTheFloor => "Rolling on the floor",
            Avatar::ThumbsUp => "Thumbs up",
            Avatar::LoudlyCrying => "Loudly crying",
            Avatar::FoldedHands => "Folded hands",
            Avatar::BlowingAKiss => "Blowing a kiss",
            Avatar::SmilingWithHearts => "Smiling with hearts",
            Avatar::HeartEyes => "Heart eyes",
            Avatar::SmilingEyes => "Smiling eyes",
            Avatar::PartyPopper => "Party popper",
            Avatar::Beaming => "Beaming",
            Avatar::TwoHearts => "Two hearts",
            Avatar::Pleading => "Pleading",
            Avatar::SweatSmile => "Sweat smile",
            Avatar::Fire => "Fire",
            Avatar::Smiling => "Smiling",
            Avatar::Facepalm => "Facepalm",
            Avatar::HeartSuit => "Heart suit",
            Avatar::Shrug => "Shrug",
            Avatar::EyeRoll => "Eye roll",
            Avatar::Laughing => "Laughing",
            Avatar::Hug => "Hug",
            Avatar::Wink => "Wink",
        }
    }

    /// The glyph drawn for the avatar, or `None` for [`Avatar::Initial`],
    /// which draws the name's first letter. Each is one code point (no
    /// variation selector) of the monochrome emoji font the chrome
    /// bundles for avatars; a new glyph must be added to that font too.
    pub fn glyph(self) -> Option<&'static str> {
        Some(match self {
            Avatar::Initial => return None,
            Avatar::Cat => "\u{1F431}",
            Avatar::Panda => "\u{1F43C}",
            Avatar::Whale => "\u{1F433}",
            Avatar::Octopus => "\u{1F419}",
            Avatar::Shell => "\u{1F41A}",
            Avatar::Turtle => "\u{1F422}",
            Avatar::Penguin => "\u{1F427}",
            Avatar::Frog => "\u{1F438}",
            Avatar::Rabbit => "\u{1F430}",
            Avatar::Bear => "\u{1F43B}",
            Avatar::Koala => "\u{1F428}",
            Avatar::Dragon => "\u{1F409}",
            Avatar::Rocket => "\u{1F680}",
            Avatar::Moon => "\u{1F319}",
            Avatar::Star => "\u{2B50}",
            Avatar::Cactus => "\u{1F335}",
            Avatar::Blossom => "\u{1F338}",
            Avatar::Coffee => "\u{2615}",
            Avatar::Guitar => "\u{1F3B8}",
            Avatar::Anchor => "\u{2693}",
            Avatar::Bolt => "\u{26A1}",
            Avatar::Ghost => "\u{1F47B}",
            Avatar::Telescope => "\u{1F52D}",
            Avatar::Sailboat => "\u{26F5}",
            Avatar::SpoutingWhale => "\u{1F40B}",
            Avatar::Dolphin => "\u{1F42C}",
            Avatar::Fish => "\u{1F41F}",
            Avatar::TropicalFish => "\u{1F420}",
            Avatar::Blowfish => "\u{1F421}",
            Avatar::Shark => "\u{1F988}",
            Avatar::Seal => "\u{1F9AD}",
            Avatar::Otter => "\u{1F9A6}",
            Avatar::Squid => "\u{1F991}",
            Avatar::Jellyfish => "\u{1FABC}",
            Avatar::Crab => "\u{1F980}",
            Avatar::Lobster => "\u{1F99E}",
            Avatar::Shrimp => "\u{1F990}",
            Avatar::Oyster => "\u{1F9AA}",
            Avatar::Coral => "\u{1FAB8}",
            Avatar::Crocodile => "\u{1F40A}",
            Avatar::Merperson => "\u{1F9DC}",
            Avatar::TearsOfJoy => "\u{1F602}",
            Avatar::RedHeart => "\u{2764}",
            Avatar::RollingOnTheFloor => "\u{1F923}",
            Avatar::ThumbsUp => "\u{1F44D}",
            Avatar::LoudlyCrying => "\u{1F62D}",
            Avatar::FoldedHands => "\u{1F64F}",
            Avatar::BlowingAKiss => "\u{1F618}",
            Avatar::SmilingWithHearts => "\u{1F970}",
            Avatar::HeartEyes => "\u{1F60D}",
            Avatar::SmilingEyes => "\u{1F60A}",
            Avatar::PartyPopper => "\u{1F389}",
            Avatar::Beaming => "\u{1F601}",
            Avatar::TwoHearts => "\u{1F495}",
            Avatar::Pleading => "\u{1F97A}",
            Avatar::SweatSmile => "\u{1F605}",
            Avatar::Fire => "\u{1F525}",
            Avatar::Smiling => "\u{263A}",
            Avatar::Facepalm => "\u{1F926}",
            Avatar::HeartSuit => "\u{2665}",
            Avatar::Shrug => "\u{1F937}",
            Avatar::EyeRoll => "\u{1F644}",
            Avatar::Laughing => "\u{1F606}",
            Avatar::Hug => "\u{1F917}",
            Avatar::Wink => "\u{1F609}",
        })
    }

    /// What the avatar shows for a profile called `name`: its glyph, or
    /// the name's first letter in upper case ("?" for an empty name).
    pub fn text(self, name: &str) -> String {
        match self.glyph() {
            Some(glyph) => glyph.to_string(),
            None => name
                .trim()
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string()),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// Names are one path component: ASCII letters, digits, `-`, `_`, and
    /// `.`. A name is never empty, never ends in `.` (Windows strips trailing
    /// dots, so `a.` would resolve to `a`, and `.`/`..` are covered by this
    /// rule), and is never a Windows device name such as `CON` or `COM1`,
    /// with or without an extension, in any letter case. These rules apply on
    /// every platform so profile names stay portable.
    InvalidName(String),
    /// The OS reported no home or config directory for this user.
    NoConfigDirectory,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::InvalidName(name) => write!(
                f,
                "invalid profile name {name:?}: use ASCII letters, digits, '-', '_', or '.', without a trailing '.' or a Windows device name"
            ),
            ProfileError::NoConfigDirectory => {
                write!(f, "could not determine the user's configuration directory")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// Resolve `name` under the current user's per-OS config directory.
    pub fn resolve(
        brand: &Brand,
        engine: &EngineInfo,
        name: &str,
    ) -> Result<Profile, ProfileError> {
        let base = BaseDirs::new().ok_or(ProfileError::NoConfigDirectory)?;
        Profile::resolve_under(base.config_dir(), brand, engine, name)
    }

    /// Resolve `name` under an explicit config root. `resolve` passes the
    /// OS config directory; tests pass a temporary directory.
    pub fn resolve_under(
        config_root: &Path,
        brand: &Brand,
        engine: &EngineInfo,
        name: &str,
    ) -> Result<Profile, ProfileError> {
        if !is_valid_name(name) {
            return Err(ProfileError::InvalidName(name.to_string()));
        }
        let dir = config_root
            .join(brand.data_dir)
            .join(PROFILES_DIR_NAME)
            .join(name);
        Ok(Profile {
            name: name.to_string(),
            engine_dir: dir.join(engine.data_dir),
            dir,
        })
    }

    /// The profile called `name` beside this one (same profiles directory,
    /// same engine subdirectory name).
    pub fn sibling(&self, name: &str) -> Result<Profile, ProfileError> {
        if !is_valid_name(name) {
            return Err(ProfileError::InvalidName(name.to_string()));
        }
        let dir = self.profiles_dir().join(name);
        let engine_dir = match self.engine_dir.file_name() {
            Some(engine) => dir.join(engine),
            None => dir.clone(),
        };
        Ok(Profile {
            name: name.to_string(),
            dir,
            engine_dir,
        })
    }

    /// The directory holding every profile.
    pub fn profiles_dir(&self) -> PathBuf {
        self.dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.dir.clone())
    }

    /// `<brand data dir>/updates`, beside the profiles directory: where
    /// downloaded builds wait for the restart that installs them. Shared
    /// by every profile, since an update is the browser's, not a
    /// profile's.
    pub fn updates_dir(&self) -> PathBuf {
        let profiles = self.profiles_dir();
        profiles
            .parent()
            .map(|brand| brand.join("updates"))
            .unwrap_or_else(|| profiles.join("updates"))
    }

    pub fn is_default(&self) -> bool {
        self.name == DEFAULT_PROFILE_NAME
    }

    pub fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.toml")
    }

    pub fn keymap_path(&self) -> PathBuf {
        self.dir.join("keymap.toml")
    }

    pub fn bookmarks_path(&self) -> PathBuf {
        self.dir.join("bookmarks.toml")
    }

    pub fn history_path(&self) -> PathBuf {
        self.dir.join("history.toml")
    }

    /// The favicons of visited sites, one PNG per host, for the
    /// bookmarks bar.
    pub fn favicons_dir(&self) -> PathBuf {
        self.dir.join("favicons")
    }

    /// The installed extensions, as the engine last reported them
    /// ([`InstalledExtensions`]): what another profile copies from.
    ///
    /// [`InstalledExtensions`]: crate::extension::InstalledExtensions
    pub fn extensions_path(&self) -> PathBuf {
        self.dir.join("extensions.toml")
    }

    /// The file a running browser leaves in the profile with its process
    /// id, so another process can tell the profile is in use.
    pub fn pid_path(&self) -> PathBuf {
        self.dir.join("running.pid")
    }

    /// Create the profile directory and the engine's subdirectory.
    pub fn ensure_dirs(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.engine_dir)
    }

    /// The names of the other profiles on this machine: every validly
    /// named directory under the profiles directory except this one,
    /// sorted. Each is a profile another `--profile <name>` process can
    /// run. Returns nothing if the directory cannot be read.
    pub fn others(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.profiles_dir())
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .filter(|name| is_valid_name(name) && *name != self.name)
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The other profiles with what their settings say about them: the
    /// default profile first, then the rest by display name. A profile
    /// whose settings cannot be read is listed by its directory name
    /// with the brand's defaults.
    pub fn siblings(&self, brand: &Brand) -> Vec<ProfileEntry> {
        let mut entries: Vec<ProfileEntry> = self
            .others()
            .into_iter()
            .map(|dir| {
                let settings = self
                    .sibling(&dir)
                    .ok()
                    .and_then(|p| Settings::load(&p.settings_path(), brand).ok())
                    .unwrap_or_else(|| Settings::for_brand(brand));
                ProfileEntry {
                    name: settings.profile.display_name(&dir),
                    avatar: settings.profile.avatar,
                    color: settings.chrome_color,
                    dir,
                }
            })
            .collect();
        entries.sort_by(|a, b| {
            (a.dir != DEFAULT_PROFILE_NAME, a.name.to_lowercase())
                .cmp(&(b.dir != DEFAULT_PROFILE_NAME, b.name.to_lowercase()))
        });
        entries
    }
}

/// The directory name for a new profile called `name`, not among
/// `taken`: the name in lower case, with runs of anything but ASCII
/// letters and digits turned into one `-` ("Work Stuff" is
/// `work-stuff`), `profile` when nothing of it survives, and a numeric
/// suffix when the name is taken (`work-2`, `work-3`, ...). Always a
/// valid profile name.
pub fn directory_name_for(name: &str, taken: &[String]) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    let base = if slug.is_empty() || !is_valid_name(&slug) {
        "profile".to_string()
    } else {
        slug
    };
    let is_taken = |candidate: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(candidate));
    if !is_taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !is_taken(candidate))
        .expect("an unbounded sequence has a free name")
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.ends_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !is_windows_device_name(name.split('.').next().unwrap_or(name))
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, and `LPT0`-`LPT9`, compared
/// case-insensitively. Windows reserves these even with an extension, so
/// callers pass the part of the name before the first `.`.
fn is_windows_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            let bytes = upper.as_bytes();
            bytes.len() == 4
                && (upper.starts_with("COM") || upper.starts_with("LPT"))
                && bytes[3].is_ascii_digit()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::test_support::{BRAND, ENGINE};

    fn under(root: &Path, name: &str) -> Result<Profile, ProfileError> {
        Profile::resolve_under(root, &BRAND, &ENGINE, name)
    }

    #[test]
    fn resolves_under_brand_profiles_and_engine_directories() {
        let p = under(Path::new("/cfg"), "work").unwrap();
        assert_eq!(p.name, "work");
        assert_eq!(p.dir, Path::new("/cfg/Osmena/profiles/work"));
        assert_eq!(
            p.settings_path(),
            Path::new("/cfg/Osmena/profiles/work/settings.toml")
        );
        assert_eq!(
            p.keymap_path(),
            Path::new("/cfg/Osmena/profiles/work/keymap.toml")
        );
        assert_eq!(p.engine_dir, Path::new("/cfg/Osmena/profiles/work/engine"));
        assert_eq!(p.profiles_dir(), Path::new("/cfg/Osmena/profiles"));
        assert!(!p.is_default());
        assert!(under(Path::new("/cfg"), "default").unwrap().is_default());
    }

    #[test]
    fn a_sibling_sits_beside_the_profile_with_the_same_engine_directory() {
        let p = under(Path::new("/cfg"), "work").unwrap();
        let s = p.sibling("play").unwrap();
        assert_eq!(s, under(Path::new("/cfg"), "play").unwrap());
        assert_eq!(s.engine_dir, Path::new("/cfg/Osmena/profiles/play/engine"));
        assert_eq!(
            p.sibling("a/b"),
            Err(ProfileError::InvalidName("a/b".to_string()))
        );
    }

    #[test]
    fn rejects_names_that_are_not_a_single_path_component() {
        for bad in ["", ".", "..", "...", "a.", "a/b", "a\\b", "a b", "ä"] {
            assert_eq!(
                under(Path::new("/cfg"), bad),
                Err(ProfileError::InvalidName(bad.to_string())),
                "{bad:?} should be rejected"
            );
        }
        assert!(under(Path::new("/cfg"), "ok-name_1.2").is_ok());
    }

    #[test]
    fn rejects_windows_device_names_in_any_case_and_with_an_extension() {
        let reserved = [
            "con", "CON", "Prn", "aux", "nul", "com0", "com1", "COM9", "lpt0", "LPT9", "con.txt",
            "Nul.log",
        ];
        for bad in reserved {
            assert_eq!(
                under(Path::new("/cfg"), bad),
                Err(ProfileError::InvalidName(bad.to_string())),
                "{bad:?} should be rejected"
            );
        }
        for ok in ["console", "com10", "COM", "nul2", "a.con"] {
            assert!(
                under(Path::new("/cfg"), ok).is_ok(),
                "{ok:?} should be accepted"
            );
        }
    }

    #[test]
    fn resolve_uses_the_platform_config_directory() {
        let p = Profile::resolve(&BRAND, &ENGINE, DEFAULT_PROFILE_NAME).unwrap();
        let dir = p.dir.to_string_lossy().replace('\\', "/");
        assert!(dir.ends_with("/Osmena/profiles/default"), "{dir}");
        if cfg!(target_os = "macos") {
            assert!(
                dir.contains("/Library/Application Support/Osmena/"),
                "{dir}"
            );
        }
        if cfg!(target_os = "windows") {
            assert!(dir.contains("/AppData/Roaming/Osmena/"), "{dir}");
        }
    }

    #[test]
    fn ensure_dirs_creates_profile_and_engine_directories() {
        let root = tempfile::tempdir().unwrap();
        let p = under(root.path(), "t").unwrap();
        p.ensure_dirs().unwrap();
        assert!(p.engine_dir.is_dir());
        assert!(p.dir.is_dir());
    }

    #[test]
    fn others_lists_sibling_profile_directories_sorted_without_self() {
        let root = tempfile::tempdir().expect("temp dir");
        let me = under(root.path(), "work").unwrap();
        for name in ["work", "zeta", "alpha", "bad name"] {
            std::fs::create_dir_all(root.path().join("Osmena/profiles").join(name)).unwrap();
        }
        std::fs::write(root.path().join("Osmena/profiles/notes.txt"), "x").unwrap();
        assert_eq!(me.others(), vec!["alpha".to_string(), "zeta".to_string()]);
    }

    #[test]
    fn others_is_empty_when_the_profiles_directory_does_not_exist() {
        let root = tempfile::tempdir().expect("temp dir");
        let me = under(root.path(), "solo").unwrap();
        assert!(me.others().is_empty());
    }

    #[test]
    fn siblings_read_each_profiles_settings_with_the_default_profile_first() {
        let root = tempfile::tempdir().expect("temp dir");
        let me = under(root.path(), "work").unwrap();
        for (dir, settings) in [
            ("default", ""),
            (
                "zeta",
                "chrome_color = \"plum\"\n[profile]\nname = \"Alice\"\navatar = \"whale\"\n",
            ),
            ("beta", "this is not = toml ="),
            ("work", "[profile]\nname = \"Me\"\n"),
        ] {
            let dir = root.path().join("Osmena/profiles").join(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("settings.toml"), settings).unwrap();
        }
        let entries = me.siblings(&BRAND);
        let dirs: Vec<&str> = entries.iter().map(|e| e.dir.as_str()).collect();
        assert_eq!(dirs, ["default", "zeta", "beta"]);
        assert_eq!(entries[0].name, "Default");
        assert_eq!(entries[0].avatar, Avatar::Initial);
        assert_eq!(entries[0].color, BRAND.chrome_color);
        assert_eq!(entries[1].name, "Alice");
        assert_eq!(entries[1].avatar, Avatar::Whale);
        assert_eq!(entries[1].color, ChromeColor::Plum);
        // Unreadable settings: the directory name stands in.
        assert_eq!(entries[2].name, "Beta");
    }

    #[test]
    fn a_directory_name_is_the_slug_of_the_display_name_made_unique() {
        let none: Vec<String> = Vec::new();
        assert_eq!(directory_name_for("Work Stuff", &none), "work-stuff");
        assert_eq!(directory_name_for("  Élodie's  ", &none), "lodie-s");
        assert_eq!(directory_name_for("日本", &none), "profile");
        assert_eq!(directory_name_for("", &none), "profile");
        assert_eq!(directory_name_for("con", &none), "profile");
        let taken = vec!["work".to_string(), "Work-2".to_string()];
        assert_eq!(directory_name_for("Work", &taken), "work-3");
        assert_eq!(directory_name_for("---", &none), "profile");
    }

    #[test]
    fn every_avatar_has_a_label_and_the_initial_draws_the_first_letter() {
        assert_eq!(Avatar::ALL.len(), 66);
        for (i, avatar) in Avatar::ALL.iter().enumerate() {
            assert!(!avatar.label().is_empty());
            if let Some(glyph) = avatar.glyph() {
                assert_eq!(glyph.chars().count(), 1, "{avatar:?}");
            }
            assert!(!Avatar::ALL[..i].contains(avatar), "{avatar:?} twice");
        }
        assert_eq!(Avatar::Initial.text(" élodie"), "É");
        assert_eq!(Avatar::Initial.text(""), "?");
        assert_eq!(Avatar::Whale.text("Alice"), "\u{1F433}");
        assert_eq!(Avatar::TropicalFish.text("Alice"), "\u{1F420}");
    }

    #[test]
    fn the_palettes_hold_every_glyph_and_the_sea_creatures_come_first() {
        let [sea, emoji, assorted] = Avatar::PALETTES;
        assert_eq!(sea.name, "Sea creatures");
        assert_eq!(sea.avatars.len(), 22);
        assert_eq!(emoji.name, "Emoji");
        assert_eq!(emoji.avatars.len(), 24);
        assert_eq!(emoji.avatars[0], Avatar::TearsOfJoy);
        assert_eq!(assorted.name, "Assorted");
        assert_eq!(assorted.avatars.len(), 24);
        for palette in Avatar::PALETTES {
            for (i, avatar) in palette.avatars.iter().enumerate() {
                assert!(avatar.glyph().is_some(), "{avatar:?} in {}", palette.name);
                assert!(
                    !palette.avatars[..i].contains(avatar),
                    "{avatar:?} twice in {}",
                    palette.name
                );
            }
        }
        for avatar in Avatar::ALL {
            let offered = Avatar::PALETTES.iter().any(|p| p.avatars.contains(&avatar));
            assert_eq!(offered, avatar.glyph().is_some(), "{avatar:?}");
        }
    }

    #[test]
    fn avatars_are_stored_by_kebab_case_name() {
        #[derive(Serialize, Deserialize)]
        struct Holder {
            avatar: Avatar,
        }
        let text = toml::to_string(&Holder {
            avatar: Avatar::TropicalFish,
        })
        .unwrap();
        assert_eq!(text.trim(), "avatar = \"tropical-fish\"");
        let back: Holder = toml::from_str("avatar = \"spouting-whale\"").unwrap();
        assert_eq!(back.avatar, Avatar::SpoutingWhale);
    }
}
