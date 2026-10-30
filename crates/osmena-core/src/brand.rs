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
    /// The chrome colour of a fresh profile (the settings window offers
    /// every [`ChromeColor`]).
    pub chrome_color: ChromeColor,
    /// The application icon as a PNG, straight RGBA8, 256 px square.
    pub icon_png: &'static [u8],
    /// The version of this build: `env!("CARGO_PKG_VERSION")` of the
    /// binary crate.
    pub version: &'static str,
    /// The build number CI stamped into this build, from the
    /// [`crate::update::BUILD_NUMBER_ENV`] environment variable at compile
    /// time (`crate::update::build_number(option_env!(..))`). `None` for
    /// a build made outside CI, which never updates itself.
    pub build: Option<u64>,
    /// Where this brand's builds are announced, so the browser can check
    /// for a newer one; `None` for a browser that never checks.
    pub updates: Option<UpdateSource>,
}

impl Brand {
    /// The running build, if CI numbered it.
    pub fn build_id(&self) -> Option<BuildId> {
        self.build.map(|build| BuildId::new(self.version, build))
    }

    /// Whether this build can check for and install newer ones: it has
    /// a number and the brand has a source.
    pub fn can_update(&self) -> bool {
        self.build.is_some() && self.updates.is_some()
    }

    /// "Paladin 0.1.0 (build 150)", or "Paladin 0.1.0 (local build)".
    pub fn describe_build(&self) -> String {
        match self.build {
            Some(build) => format!("{} {} (build {build})", self.name, self.version),
            None => format!("{} {} (local build)", self.name, self.version),
        }
    }
}

use url::Url;

use crate::settings::ChromeColor;
use crate::update::{BuildId, UpdateSource};

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

/// What an engine can do to a page beyond showing it, for the page's
/// context menu: an entry whose tool the engine lacks is left out of the
/// menu. Each maps to a `PageCommand` the engine's tab host receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageTools {
    /// Save a link's target, an image, a video, or the page to disk,
    /// asking where (`PageCommand::Save`).
    pub save: bool,
    /// Put an image on the clipboard (`PageCommand::CopyImage`).
    pub copy_image: bool,
    /// Paste without formatting (`PageCommand::PasteAsPlainText`).
    pub paste_plain: bool,
    /// Toggle a video's or audio's looping and controls
    /// (`PageCommand::SetMediaLoop`, `SetMediaControls`).
    pub media_controls: bool,
    /// Print the page (`PageCommand::Print`).
    pub print: bool,
    /// Show the page's source (`PageCommand::ViewSource`).
    pub view_source: bool,
    /// Open the developer tools on an element (`PageCommand::Inspect`).
    pub inspect: bool,
}

impl PageTools {
    /// An engine with none of them: the menu keeps what the model can do
    /// alone (open links, copy addresses) and the editing basics.
    pub const NONE: PageTools = PageTools {
        save: false,
        copy_image: false,
        paste_plain: false,
        media_controls: false,
        print: false,
        view_source: false,
        inspect: false,
    };

    pub const ALL: PageTools = PageTools {
        save: true,
        copy_image: true,
        paste_plain: true,
        media_controls: true,
        print: true,
        view_source: true,
        inspect: true,
    };
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
    /// What the page's context menu may offer beyond the model's own
    /// entries.
    pub page_tools: PageTools,
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
        chrome_color: ChromeColor::Slate,
        icon_png: &[],
        version: "0.1.0",
        build: Some(7),
        updates: Some(UpdateSource {
            manifest_url: "https://updates.example/latest.toml",
        }),
    };

    /// An engine for tests.
    pub(crate) static ENGINE: EngineInfo = EngineInfo {
        name: "TestEngine",
        version: "0.0",
        data_dir: "engine",
        supports_stop: true,
        wheel_line_px: 40.0,
        extensions: None,
        page_tools: PageTools::ALL,
    };
}
