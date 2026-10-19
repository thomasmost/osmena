//! User settings, stored as TOML in the profile directory. Every field has
//! a default, so a file from an older or newer version still loads.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::brand::Brand;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Loaded in the first window at startup when no URL is given. Empty
    /// in [`Settings::default`]; [`Settings::load`] fills it from the
    /// brand, so a file that never set it follows the brand's default.
    pub homepage: String,
    /// Loaded in every new tab and new window.
    pub new_tab_url: String,
    /// Reserved for session restore; stored but not acted on in Phase 1.
    pub restore_windows_on_launch: bool,
    /// The colour of this profile's chrome (tab strip, toolbar tint), one of
    /// a few swatches so windows of different profiles tell apart at a glance.
    pub chrome_color: ChromeColor,
    /// Give browser windows the operating system's title bar and borders.
    /// Off by default: the tab strip is then the title bar, as in Chromium,
    /// with the window controls at its right end.
    pub system_title_bar: bool,
    /// The ids of the extensions whose toolbar actions sit in the
    /// toolbar, in order. An id of an extension no longer installed is
    /// kept, so it comes back pinned if the extension does.
    pub pinned_extensions: Vec<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            homepage: String::new(),
            new_tab_url: "about:blank".to_string(),
            restore_windows_on_launch: false,
            chrome_color: ChromeColor::default(),
            system_title_bar: false,
            pinned_extensions: Vec::new(),
        }
    }
}

/// A chrome colour swatch. Stored by name (`chrome_color = "ocean"`); the
/// UI derives every shade it needs from the swatch's base colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChromeColor {
    #[default]
    Slate,
    Ocean,
    Forest,
    Plum,
    Rose,
    Amber,
    Graphite,
}

impl ChromeColor {
    /// Every swatch, in the order the settings window shows them.
    pub const ALL: [ChromeColor; 7] = [
        ChromeColor::Slate,
        ChromeColor::Ocean,
        ChromeColor::Forest,
        ChromeColor::Plum,
        ChromeColor::Rose,
        ChromeColor::Amber,
        ChromeColor::Graphite,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChromeColor::Slate => "Slate",
            ChromeColor::Ocean => "Ocean",
            ChromeColor::Forest => "Forest",
            ChromeColor::Plum => "Plum",
            ChromeColor::Rose => "Rose",
            ChromeColor::Amber => "Amber",
            ChromeColor::Graphite => "Graphite",
        }
    }

    /// The base colour, sRGB. All swatches are dark enough for white text.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            ChromeColor::Slate => [71, 85, 105],
            ChromeColor::Ocean => [14, 110, 140],
            ChromeColor::Forest => [31, 112, 72],
            ChromeColor::Plum => [96, 60, 142],
            ChromeColor::Rose => [168, 36, 86],
            ChromeColor::Amber => [166, 86, 12],
            ChromeColor::Graphite => [40, 41, 46],
        }
    }
}

#[derive(Debug)]
pub enum SettingsError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::Io(e) => write!(f, "settings file: {e}"),
            SettingsError::Parse(e) => write!(f, "settings file is not valid TOML: {e}"),
            SettingsError::Serialize(e) => write!(f, "could not serialize settings: {e}"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<io::Error> for SettingsError {
    fn from(e: io::Error) -> Self {
        SettingsError::Io(e)
    }
}

impl Settings {
    /// The defaults of a fresh profile of `brand`.
    pub fn for_brand(brand: &Brand) -> Settings {
        Settings {
            homepage: brand.homepage.to_string(),
            ..Settings::default()
        }
    }

    /// Parse without a brand: a missing homepage stays empty.
    pub fn parse(text: &str) -> Result<Settings, SettingsError> {
        toml::from_str(text).map_err(SettingsError::Parse)
    }

    /// Fill the fields a file may leave unset with the brand's defaults.
    fn with_brand_defaults(mut self, brand: &Brand) -> Settings {
        if self.homepage.is_empty() {
            self.homepage = brand.homepage.to_string();
        }
        self
    }

    pub fn to_toml(&self) -> Result<String, SettingsError> {
        toml::to_string_pretty(self).map_err(SettingsError::Serialize)
    }

    /// Load from `path`. A missing file yields the brand's defaults; a
    /// malformed file is an error so a typo never silently resets the
    /// user's settings.
    pub fn load(path: &Path, brand: &Brand) -> Result<Settings, SettingsError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::parse(&text).map(|s| s.with_brand_defaults(brand)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::for_brand(brand)),
            Err(e) => Err(SettingsError::Io(e)),
        }
    }

    /// Write to `path` atomically (see [`write_atomically`]), creating
    /// parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let text = self.to_toml()?;
        write_atomically(path, &text)?;
        Ok(())
    }
}

/// Write `text` to `path`, creating parent directories as needed. The text
/// goes to a sibling `<path>.tmp` first, is synced to disk, and is then
/// renamed over `path`, so a crash mid-write leaves the previous file intact
/// rather than a truncated one. The temporary file is removed if any step
/// fails.
pub(crate) fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temp_path(path);
    let result = write_and_sync(&tmp, text.as_bytes()).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The sibling temporary path used while saving: `<path>.tmp`, in the same
/// directory so the final rename stays on one filesystem.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::test_support::BRAND;

    #[test]
    fn round_trips_through_toml() {
        let s = Settings {
            homepage: "https://a.example/".to_string(),
            new_tab_url: "https://b.example/".to_string(),
            restore_windows_on_launch: true,
            chrome_color: ChromeColor::Plum,
            system_title_bar: true,
            pinned_extensions: vec!["abc".to_string(), "def".to_string()],
        };
        let text = s.to_toml().unwrap();
        assert!(text.contains("chrome_color = \"plum\""), "{text}");
        assert!(text.contains("system_title_bar = true"), "{text}");
        assert!(text.contains("pinned_extensions = ["), "{text}");
        assert_eq!(Settings::parse(&text).unwrap(), s);
    }

    #[test]
    fn chrome_color_names_are_kebab_case_and_unknown_names_are_errors() {
        let s = Settings::parse("chrome_color = \"graphite\"\n").unwrap();
        assert_eq!(s.chrome_color, ChromeColor::Graphite);
        assert!(matches!(
            Settings::parse("chrome_color = \"neon\"\n"),
            Err(SettingsError::Parse(_))
        ));
        assert_eq!(ChromeColor::ALL.len(), 7);
        assert_eq!(ChromeColor::default(), ChromeColor::Slate);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s = Settings::parse("homepage = \"https://a.example/\"\n").unwrap();
        assert_eq!(s.homepage, "https://a.example/");
        assert_eq!(s.new_tab_url, Settings::default().new_tab_url);
        assert_eq!(
            s.restore_windows_on_launch,
            Settings::default().restore_windows_on_launch
        );
    }

    #[test]
    fn empty_file_is_all_defaults() {
        let s = Settings::parse("").unwrap();
        assert_eq!(s, Settings::default());
        assert!(!s.system_title_bar, "windows are frameless by default");
    }

    #[test]
    fn malformed_toml_is_an_error() {
        assert!(matches!(
            Settings::parse("homepage = "),
            Err(SettingsError::Parse(_))
        ));
    }

    #[test]
    fn load_of_missing_file_is_the_brand_default_and_save_creates_parents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.toml");
        let fresh = Settings::load(&path, &BRAND).unwrap();
        assert_eq!(fresh, Settings::for_brand(&BRAND));
        assert_eq!(fresh.homepage, "https://home.example/");

        let s = Settings {
            homepage: "https://saved.example/".to_string(),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path, &BRAND).unwrap(), s);
    }

    #[test]
    fn a_file_without_a_homepage_takes_the_brand_homepage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "chrome_color = \"rose\"
",
        )
        .unwrap();
        let s = Settings::load(&path, &BRAND).unwrap();
        assert_eq!(s.homepage, "https://home.example/");
        assert_eq!(s.chrome_color, ChromeColor::Rose);
    }

    #[test]
    fn save_replaces_existing_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let first = Settings {
            homepage: "https://first.example/".to_string(),
            ..Settings::default()
        };
        let second = Settings {
            homepage: "https://second.example/".to_string(),
            ..Settings::default()
        };
        first.save(&path).unwrap();
        second.save(&path).unwrap();
        assert_eq!(Settings::load(&path, &BRAND).unwrap(), second);
        assert!(!temp_path(&path).exists());
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("settings.toml")]);
    }

    #[test]
    fn failed_save_leaves_no_temp_file_and_reports_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        // Renaming a file over a directory fails on every platform.
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(
            Settings::default().save(&path),
            Err(SettingsError::Io(_))
        ));
        assert!(!temp_path(&path).exists());
    }
}
