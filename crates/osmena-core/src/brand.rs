//! What a browser built on osmena tells the shared layers about itself.
//!
//! Two constants, both owned by the binary crate: the [`Brand`] (the
//! product: its name, where its data lives, its icon) and the
//! [`EngineInfo`] (the engine: its name and version, the directory its
//! state goes in, what it can and cannot do). They are kept apart so a
//! brand can sit on another engine without touching either.

/// The product identity. One `static` per browser, passed by reference to
/// the shared layers that need it; there is no global.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brand {
    /// Shown to the user: window titles, dialogs. "Paladin".
    pub name: &'static str,
    /// The executable and command-line name, and the prefix of error
    /// messages on stderr. "paladin".
    pub binary: &'static str,
    /// The directory under the per-user config directory that holds every
    /// profile. Usually the same as `name`.
    pub data_dir: &'static str,
    /// The homepage of a fresh profile.
    pub homepage: &'static str,
    /// The application icon as a PNG, straight RGBA8, 256 px square.
    pub icon_png: &'static [u8],
}

use url::Url;

/// What an engine offers for browser extensions. `None` in
/// [`EngineInfo::extensions`] means the engine runs none, and the chrome
/// shows nothing about them.
#[derive(Clone, Copy, Debug)]
pub struct ExtensionSupport {
    /// The store extensions come from. "Chrome Web Store".
    pub store_name: &'static str,
    /// The store's front page, for the "visit the store" entry.
    pub store_url: &'static str,
    /// The id of the extension a page of the store describes, when `url`
    /// is such a page, so the chrome can offer to install it.
    pub store_item_id: fn(&Url) -> Option<String>,
}

/// Two supports are the same store; the function is not compared
/// (function pointers compare unreliably).
impl PartialEq for ExtensionSupport {
    fn eq(&self, other: &ExtensionSupport) -> bool {
        self.store_name == other.store_name && self.store_url == other.store_url
    }
}

/// The engine identity. One `static` per engine crate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineInfo {
    /// "Servo", "Chromium".
    pub name: &'static str,
    /// Shown next to the name where a limitation is explained. "0.7.0".
    pub version: &'static str,
    /// The subdirectory of a profile that holds the engine's own state:
    /// cookies, local storage, the HTTP cache.
    pub data_dir: &'static str,
    /// Whether the engine can stop a load in progress. The chrome shows a
    /// disabled Stop button, with the reason, when it cannot.
    pub supports_stop: bool,
    /// Pixels one wheel notch scrolls, when the platform reports the
    /// wheel in lines. Engines scroll different distances per notch.
    pub wheel_line_px: f32,
    /// Browser extensions, if the engine runs them.
    pub extensions: Option<ExtensionSupport>,
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A brand for tests.
    pub(crate) static BRAND: Brand = Brand {
        name: "Osmena",
        binary: "osmena",
        data_dir: "Osmena",
        homepage: "https://home.example/",
        icon_png: &[],
    };

    /// An engine for tests.
    pub(crate) static ENGINE: EngineInfo = EngineInfo {
        name: "TestEngine",
        version: "0.0",
        data_dir: "engine",
        supports_stop: true,
        wheel_line_px: 40.0,
        extensions: None,
    };
}
