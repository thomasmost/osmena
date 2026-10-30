//! Page zoom: Chromium's step table, and the level remembered for each
//! site (`zoom.toml` in the profile). A site without a level of its own
//! takes the default, `Settings::page_zoom`; a tab's level is
//! `Tab::zoom`, which every tab on the same host shares.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::model::site_key;
use crate::settings::write_atomically;

/// The levels a step moves between, in percent.
pub const ZOOM_STEPS: [u16; 17] = [
    25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500,
];
pub const ZOOM_MIN: u16 = ZOOM_STEPS[0];
pub const ZOOM_MAX: u16 = ZOOM_STEPS[ZOOM_STEPS.len() - 1];

/// The step above (`up`) or below `percent`: the next entry of the
/// table past it, or the end of the table when there is none.
pub fn next_step(percent: u16, up: bool) -> u16 {
    if up {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|&step| step > percent)
            .unwrap_or(ZOOM_MAX)
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&step| step < percent)
            .unwrap_or(ZOOM_MIN)
    }
}

/// The zoom levels of sites that differ from the default, by host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteZoom {
    pub sites: BTreeMap<String, u16>,
}

#[derive(Debug)]
pub enum ZoomError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for ZoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZoomError::Io(e) => write!(f, "cannot read or write the zoom levels: {e}"),
            ZoomError::Parse(e) => write!(f, "the zoom file is not valid: {e}"),
            ZoomError::Serialize(e) => write!(f, "cannot write the zoom levels: {e}"),
        }
    }
}

impl std::error::Error for ZoomError {}

impl SiteZoom {
    /// The level for a page at `url`: its site's, or `default`.
    pub fn level_for(&self, url: &Url, default: u16) -> u16 {
        site_key(url)
            .and_then(|host| self.sites.get(&host).copied())
            .unwrap_or(default)
    }

    /// Remember `percent` for `host`; the default is remembered as no
    /// entry. Returns whether the table changed.
    pub fn set(&mut self, host: &str, percent: u16, default: u16) -> bool {
        if percent == default {
            self.sites.remove(host).is_some()
        } else {
            self.sites.insert(host.to_string(), percent) != Some(percent)
        }
    }

    pub fn forget(&mut self, host: &str) -> bool {
        self.sites.remove(host).is_some()
    }

    pub fn parse(text: &str) -> Result<SiteZoom, ZoomError> {
        toml::from_str(text).map_err(ZoomError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, ZoomError> {
        toml::to_string_pretty(self).map_err(ZoomError::Serialize)
    }

    /// Read the file; a missing file is no levels.
    pub fn load(path: &Path) -> Result<SiteZoom, ZoomError> {
        match std::fs::read_to_string(path) {
            Ok(text) => SiteZoom::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(SiteZoom::default()),
            Err(e) => Err(ZoomError::Io(e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), ZoomError> {
        let text = self.to_toml()?;
        write_atomically(path, &text).map_err(ZoomError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_the_table_and_stop_at_its_ends() {
        assert_eq!(next_step(100, true), 110);
        assert_eq!(next_step(100, false), 90);
        assert_eq!(next_step(500, true), 500);
        assert_eq!(next_step(25, false), 25);
        // Off the table: the next entry past it either way.
        assert_eq!(next_step(105, true), 110);
        assert_eq!(next_step(105, false), 100);
        assert_eq!(next_step(0, false), 25);
        assert_eq!(next_step(1000, true), 500);
    }

    #[test]
    fn levels_are_per_host_and_the_default_is_no_entry() {
        let mut z = SiteZoom::default();
        let a = Url::parse("https://A.example/page").unwrap();
        let b = Url::parse("https://b.example/").unwrap();
        assert_eq!(z.level_for(&a, 100), 100);
        assert!(z.set("a.example", 125, 100));
        assert!(!z.set("a.example", 125, 100));
        assert_eq!(z.level_for(&a, 100), 125);
        assert_eq!(z.level_for(&b, 110), 110);
        // Back to the default forgets the site.
        assert!(z.set("a.example", 100, 100));
        assert!(z.sites.is_empty());
        assert!(!z.set("a.example", 100, 100));
        assert!(!z.forget("a.example"));
        assert_eq!(z.level_for(&Url::parse("about:blank").unwrap(), 90), 90);
    }

    #[test]
    fn round_trips_through_toml_and_files() {
        let mut z = SiteZoom::default();
        z.set("docs.rs", 110, 100);
        z.set("news.ycombinator.com", 125, 100);
        let text = z.to_toml().unwrap();
        assert!(text.contains("[sites]"), "{text}");
        assert!(text.contains("\"docs.rs\" = 110"), "{text}");
        assert_eq!(SiteZoom::parse(&text).unwrap(), z);
        assert_eq!(SiteZoom::parse("").unwrap(), SiteZoom::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zoom.toml");
        assert_eq!(SiteZoom::load(&path).unwrap(), SiteZoom::default());
        z.save(&path).unwrap();
        assert_eq!(SiteZoom::load(&path).unwrap(), z);
        std::fs::write(&path, "sites = 3").unwrap();
        assert!(matches!(SiteZoom::load(&path), Err(ZoomError::Parse(_))));
    }
}
