//! User settings, stored as TOML in the profile directory. Every field has
//! a default, so a file from an older or newer version still loads.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::brand::Brand;
use crate::profile::{Avatar, DEFAULT_PROFILE_NAME};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Loaded in the first window at startup when no URL is given. Empty
    /// in [`Settings::default`]; [`Settings::load`] fills it from the
    /// brand, so a file that never set it follows the brand's default.
    pub homepage: String,
    /// Loaded in every new window. A new tab asks for its address first
    /// (the launcher), so this is not what it opens on.
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
    /// Where address-bar text that is not a URL goes.
    pub search: SearchSettings,
    /// How the profile presents itself and what it takes from the
    /// default profile.
    pub profile: ProfileSettings,
    /// Look for a newer build now and then, download it, and offer to
    /// restart into it. Off, the browser checks only when asked
    /// ("Check for updates" in the menu or the settings window).
    pub check_for_updates: bool,
    /// Show the bookmarks bar (the group [`Bookmarks::BAR`]) under the
    /// toolbar.
    ///
    /// [`Bookmarks::BAR`]: crate::bookmarks::Bookmarks::BAR
    pub show_bookmarks_bar: bool,
    /// The group the star put the last bookmark in, which is where the
    /// next one goes until the user picks another; empty, the bar.
    pub last_bookmark_group: String,
    /// The zoom level, in percent, of sites without one of their own
    /// (`crate::zoom`).
    pub page_zoom: u16,
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
            search: SearchSettings::default(),
            profile: ProfileSettings::default(),
            check_for_updates: true,
            show_bookmarks_bar: false,
            last_bookmark_group: String::new(),
            page_zoom: 100,
        }
    }
}

/// The profile as the user sees it (`[profile]` in `settings.toml`): its
/// display name and avatar (its colour is [`Settings::chrome_color`]),
/// and what it shares with or took from the default profile. The
/// history is never shared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSettings {
    /// The display name; empty, the directory name stands in
    /// ([`ProfileSettings::display_name`]).
    pub name: String,
    pub avatar: Avatar,
    /// The profile's bookmarks are the default profile's: it reads and
    /// writes the default profile's `bookmarks.toml` instead of its own.
    /// Meaningless for the default profile.
    pub share_bookmarks: bool,
    /// Install the default profile's extensions in this profile at its
    /// next start, then clear the flag. A new profile made with "copy
    /// extensions" starts with it set, since it has no engine yet to
    /// install them with.
    pub copy_extensions: bool,
}

impl ProfileSettings {
    /// The name shown for the profile whose directory is `dir`: the
    /// display name, or the directory name with its first letter in
    /// upper case ("default" is "Default").
    pub fn display_name(&self, dir: &str) -> String {
        let name = self.name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
        let mut chars = dir.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => DEFAULT_PROFILE_NAME.to_string(),
        }
    }
}

/// The search the address bar runs on text that is not a URL, and that
/// the page's context menu offers for selected text (`[search]` in
/// `settings.toml`). `url` names the provider: `{query}` in it stands
/// for the text, form-encoded, and without the placeholder the text is
/// appended. `name` labels the menu entry ("Search Google for …"). The
/// default is a Google search; a browser may later offer a choice of
/// engines, or something other than a search engine, through this one
/// setting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    pub name: String,
    pub url: String,
}

impl Default for SearchSettings {
    fn default() -> SearchSettings {
        SearchSettings {
            name: "Google".to_string(),
            url: "https://www.google.com/search?q={query}".to_string(),
        }
    }
}

impl SearchSettings {
    /// What `url` holds where the query goes.
    pub const PLACEHOLDER: &'static str = "{query}";

    /// The URL that answers `query`, or `None` when the query is blank or
    /// the setting does not make a URL.
    pub fn url_for(&self, query: &str) -> Option<Url> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let text = if self.url.contains(Self::PLACEHOLDER) {
            self.url.replace(Self::PLACEHOLDER, &encoded)
        } else {
            format!("{}{encoded}", self.url.trim())
        };
        Url::parse(&text).ok()
    }
}

/// A chrome colour swatch. Stored by name (`chrome_color = "ocean"`); the
/// UI derives every shade it needs from the swatch's base colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChromeColor {
    /// A warm sand, like the inside of a shell: light, so the text on
    /// the strip is dark.
    Shell,
    Driftwood,
    Walnut,
    Mist,
    #[default]
    Slate,
    Graphite,
    Lagoon,
    Ocean,
    Abyss,
    Mint,
    Forest,
    Pine,
    Lavender,
    Plum,
    Aubergine,
    Blush,
    Rose,
    Wine,
    Peach,
    Amber,
    Umber,
}

impl ChromeColor {
    /// The swatches by hue, each from its pastel through the middle
    /// shade to a deeper, more natural one: what the colour picker lays
    /// out as a grid, a hue per column.
    pub const FAMILIES: [[ChromeColor; 3]; 7] = [
        [
            ChromeColor::Shell,
            ChromeColor::Driftwood,
            ChromeColor::Walnut,
        ],
        [ChromeColor::Mist, ChromeColor::Slate, ChromeColor::Graphite],
        [ChromeColor::Lagoon, ChromeColor::Ocean, ChromeColor::Abyss],
        [ChromeColor::Mint, ChromeColor::Forest, ChromeColor::Pine],
        [
            ChromeColor::Lavender,
            ChromeColor::Plum,
            ChromeColor::Aubergine,
        ],
        [ChromeColor::Blush, ChromeColor::Rose, ChromeColor::Wine],
        [ChromeColor::Peach, ChromeColor::Amber, ChromeColor::Umber],
    ];

    /// Every swatch, hue by hue, pastel first.
    pub const ALL: [ChromeColor; 21] = [
        ChromeColor::Shell,
        ChromeColor::Driftwood,
        ChromeColor::Walnut,
        ChromeColor::Mist,
        ChromeColor::Slate,
        ChromeColor::Graphite,
        ChromeColor::Lagoon,
        ChromeColor::Ocean,
        ChromeColor::Abyss,
        ChromeColor::Mint,
        ChromeColor::Forest,
        ChromeColor::Pine,
        ChromeColor::Lavender,
        ChromeColor::Plum,
        ChromeColor::Aubergine,
        ChromeColor::Blush,
        ChromeColor::Rose,
        ChromeColor::Wine,
        ChromeColor::Peach,
        ChromeColor::Amber,
        ChromeColor::Umber,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChromeColor::Shell => "Shell",
            ChromeColor::Driftwood => "Driftwood",
            ChromeColor::Walnut => "Walnut",
            ChromeColor::Mist => "Mist",
            ChromeColor::Slate => "Slate",
            ChromeColor::Graphite => "Graphite",
            ChromeColor::Lagoon => "Lagoon",
            ChromeColor::Ocean => "Ocean",
            ChromeColor::Abyss => "Abyss",
            ChromeColor::Mint => "Mint",
            ChromeColor::Forest => "Forest",
            ChromeColor::Pine => "Pine",
            ChromeColor::Lavender => "Lavender",
            ChromeColor::Plum => "Plum",
            ChromeColor::Aubergine => "Aubergine",
            ChromeColor::Blush => "Blush",
            ChromeColor::Rose => "Rose",
            ChromeColor::Wine => "Wine",
            ChromeColor::Peach => "Peach",
            ChromeColor::Amber => "Amber",
            ChromeColor::Umber => "Umber",
        }
    }

    /// The base colour, sRGB. The chrome picks dark or light text by the
    /// swatch's luminance (`Palette`), so a swatch may be light.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            ChromeColor::Shell => [229, 214, 194],
            ChromeColor::Driftwood => [156, 130, 102],
            ChromeColor::Walnut => [92, 66, 48],
            ChromeColor::Mist => [208, 215, 226],
            ChromeColor::Slate => [71, 85, 105],
            ChromeColor::Graphite => [40, 41, 46],
            ChromeColor::Lagoon => [184, 222, 230],
            ChromeColor::Ocean => [14, 110, 140],
            ChromeColor::Abyss => [12, 56, 76],
            ChromeColor::Mint => [196, 228, 204],
            ChromeColor::Forest => [31, 112, 72],
            ChromeColor::Pine => [22, 66, 46],
            ChromeColor::Lavender => [218, 206, 238],
            ChromeColor::Plum => [96, 60, 142],
            ChromeColor::Aubergine => [58, 36, 82],
            ChromeColor::Blush => [244, 208, 220],
            ChromeColor::Rose => [168, 36, 86],
            ChromeColor::Wine => [100, 26, 50],
            ChromeColor::Peach => [250, 218, 188],
            ChromeColor::Amber => [166, 86, 12],
            ChromeColor::Umber => [102, 56, 22],
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
            chrome_color: brand.chrome_color,
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
            search: SearchSettings {
                name: "Duck".to_string(),
                url: "https://d.example/?q={query}".to_string(),
            },
            profile: ProfileSettings {
                name: "Alice".to_string(),
                avatar: Avatar::Whale,
                share_bookmarks: true,
                copy_extensions: false,
            },
            check_for_updates: false,
            show_bookmarks_bar: true,
            last_bookmark_group: "Work".to_string(),
            page_zoom: 110,
        };
        let text = s.to_toml().unwrap();
        assert!(text.contains("check_for_updates = false"), "{text}");
        assert!(text.contains("show_bookmarks_bar = true"), "{text}");
        assert!(text.contains("chrome_color = \"plum\""), "{text}");
        assert!(text.contains("[search]"), "{text}");
        assert!(text.contains("[profile]"), "{text}");
        assert!(text.contains("avatar = \"whale\""), "{text}");
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
        assert_eq!(ChromeColor::ALL.len(), 21);
        assert_eq!(ChromeColor::default(), ChromeColor::Slate);
    }

    #[test]
    fn the_families_are_every_swatch_once_from_light_to_dark() {
        let flat: Vec<ChromeColor> = ChromeColor::FAMILIES.iter().flatten().copied().collect();
        assert_eq!(flat, ChromeColor::ALL);
        // The colours saved before the families keep their place in the
        // middle (Shell and Graphite at the ends of theirs).
        for color in [
            ChromeColor::Slate,
            ChromeColor::Ocean,
            ChromeColor::Forest,
            ChromeColor::Plum,
            ChromeColor::Rose,
            ChromeColor::Amber,
        ] {
            assert!(
                ChromeColor::FAMILIES.iter().any(|f| f[1] == color),
                "{color:?}"
            );
        }
        let luma = |c: ChromeColor| {
            let [r, g, b] = c.rgb();
            0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)
        };
        for [pastel, middle, deep] in ChromeColor::FAMILIES {
            assert!(luma(pastel) > luma(middle), "{pastel:?}");
            assert!(luma(middle) > luma(deep), "{deep:?}");
        }
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
        assert!(s.check_for_updates, "updates are checked by default");
    }

    #[test]
    fn the_default_search_is_google_and_queries_are_form_encoded() {
        let s = Settings::parse("").unwrap();
        assert_eq!(s.search.name, "Google");
        assert_eq!(
            s.search.url_for("rust  browser & engine").unwrap().as_str(),
            "https://www.google.com/search?q=rust++browser+%26+engine"
        );
        assert_eq!(s.search.url_for("  "), None);
        // A file that names only the engine keeps the default URL.
        let s = Settings::parse("[search]\nname = \"G\"\n").unwrap();
        assert_eq!(s.search.name, "G");
        assert_eq!(s.search.url, SearchSettings::default().url);
    }

    #[test]
    fn a_search_url_without_the_placeholder_takes_the_query_at_its_end() {
        let s = SearchSettings {
            name: "Lex".to_string(),
            url: "https://lex.example/find/ ".to_string(),
        };
        assert_eq!(
            s.url_for("a b").unwrap().as_str(),
            "https://lex.example/find/a+b"
        );
        let bad = SearchSettings {
            name: "x".to_string(),
            url: "not a url {query}".to_string(),
        };
        assert_eq!(bad.url_for("a"), None);
    }

    #[test]
    fn a_profile_without_a_name_is_shown_by_its_directory() {
        let s = Settings::parse("").unwrap();
        assert_eq!(s.profile, ProfileSettings::default());
        assert_eq!(s.profile.display_name("default"), "Default");
        assert_eq!(s.profile.display_name("work-2"), "Work-2");
        let named = Settings::parse("[profile]\nname = \"  Alice \"\n").unwrap();
        assert_eq!(named.profile.display_name("whatever"), "Alice");
        let s = Settings::parse("[profile]\nshare_bookmarks = true\n").unwrap();
        assert!(s.profile.share_bookmarks);
        assert!(!s.profile.copy_extensions);
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
