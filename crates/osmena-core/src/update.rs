//! Updates over the air: what a build knows about itself, what a newer
//! build announces, and where the browser is in picking it up.
//!
//! A browser's CI publishes every build with a manifest (`latest.toml`)
//! at a fixed URL ([`UpdateSource::manifest_url`]). The shell fetches it
//! now and then, compares it with the running build ([`BuildId`]), and
//! downloads the asset for this platform; the chrome then offers to
//! restart into it. This module is the vocabulary: the manifest's
//! format, the comparison, and the [`UpdateStatus`] the model carries.
//! Nothing here does I/O.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where a browser's builds are announced. One `static` per brand, in
/// [`crate::Brand::updates`]; `None` there means the browser never
/// checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateSource {
    /// The manifest of the newest build, a TOML document in the shape of
    /// [`Manifest`]. For GitHub Releases that is
    /// `https://github.com/<owner>/<repo>/releases/latest/download/latest.toml`.
    pub manifest_url: &'static str,
}

/// The environment variable a browser's CI sets at build time, which the
/// brand reads with `option_env!` into [`crate::Brand::build`]. A local
/// build has none, and never updates itself.
pub const BUILD_NUMBER_ENV: &str = "OSMENA_BUILD_NUMBER";

/// Parse a build number at compile time: `build_number(option_env!(..))`
/// in a `static`. `None` for no value, an empty value, or anything that is
/// not a decimal number.
pub const fn build_number(text: Option<&str>) -> Option<u64> {
    let Some(text) = text else {
        return None;
    };
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let digit = bytes[i];
        if digit < b'0' || digit > b'9' {
            return None;
        }
        let Some(shifted) = value.checked_mul(10) else {
            return None;
        };
        let Some(next) = shifted.checked_add((digit - b'0') as u64) else {
            return None;
        };
        value = next;
        i += 1;
    }
    Some(value)
}

/// A build's place in the order of builds: the crate version, then the
/// CI build number. Builds compare by version first (semantic versions,
/// numerically per part) and by number within a version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildId {
    pub version: String,
    pub build: u64,
}

impl BuildId {
    pub fn new(version: &str, build: u64) -> BuildId {
        BuildId {
            version: version.to_string(),
            build,
        }
    }

    /// The version as numbers, for comparing: "0.1.0" is `[0, 1, 0]`;
    /// a part that is not a number counts as 0, so "1.2.0-rc.1" orders
    /// as "1.2.0".
    fn parts(&self) -> Vec<u64> {
        self.version
            .split('.')
            .map(|part| {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().unwrap_or(0)
            })
            .collect()
    }

    /// Whether this build is newer than `other`.
    pub fn is_newer_than(&self, other: &BuildId) -> bool {
        match self.parts().cmp(&other.parts()) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => self.build > other.build,
        }
    }
}

impl fmt::Display for BuildId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (build {})", self.version, self.build)
    }
}

/// The key of the asset for this platform in [`Manifest::assets`]:
/// `<os>-<arch>` as Rust names them, "windows-x86_64", "macos-aarch64",
/// "macos-x86_64".
pub fn platform_key() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// What a browser's CI publishes with every build: `latest.toml`.
///
/// ```toml
/// version = "0.1.0"
/// build = 150
/// commit = "0123abcd"
/// published = "2026-10-29T12:00:00Z"
/// notes_url = "https://github.com/owner/repo/releases/tag/build-150"
///
/// [assets.windows-x86_64]
/// name = "nautilus_0.1.0_x64-setup.exe"
/// url = "https://github.com/owner/repo/releases/download/build-150/nautilus_0.1.0_x64-setup.exe"
/// sha256 = "…"
/// size = 123456789
/// ```
///
/// An asset is the installer (Windows: an NSIS installer that takes `/S`)
/// or a zip of the application bundle (macOS). A platform without an
/// asset gets no update from that build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub build: u64,
    #[serde(default)]
    pub commit: String,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub notes_url: Option<String>,
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
}

/// One downloadable file of a build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// The file name to save it as.
    pub name: String,
    pub url: String,
    /// The SHA-256 of the file, lower-case hex, checked after the
    /// download.
    pub sha256: String,
    /// The size in bytes, for the progress.
    pub size: u64,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let manifest: Manifest = toml::from_str(text).map_err(ManifestError::Parse)?;
        if manifest.version.trim().is_empty() {
            return Err(ManifestError::Invalid("the version is empty"));
        }
        for (key, asset) in &manifest.assets {
            if asset.name.contains(['/', '\\']) || asset.name.trim().is_empty() {
                return Err(ManifestError::Invalid("an asset has no plain file name"));
            }
            if asset.sha256.len() != 64 || !asset.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ManifestError::Invalid(
                    "an asset's sha256 is not 64 hex digits",
                ));
            }
            if key.trim().is_empty() {
                return Err(ManifestError::Invalid("an asset has no platform key"));
            }
        }
        Ok(manifest)
    }

    pub fn build_id(&self) -> BuildId {
        BuildId::new(&self.version, self.build)
    }

    /// The update this manifest offers a `current` build on `platform`:
    /// `None` when the manifest is not newer, or has no asset for the
    /// platform.
    pub fn update_for(&self, current: &BuildId, platform: &str) -> Option<AvailableUpdate> {
        let id = self.build_id();
        if !id.is_newer_than(current) {
            return None;
        }
        let asset = self.assets.get(platform)?.clone();
        Some(AvailableUpdate {
            id,
            notes_url: self.notes_url.clone().filter(|url| !url.is_empty()),
            asset,
        })
    }
}

#[derive(Debug)]
pub enum ManifestError {
    Parse(toml::de::Error),
    Invalid(&'static str),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Parse(e) => write!(f, "the update manifest is not valid TOML: {e}"),
            ManifestError::Invalid(what) => write!(f, "the update manifest is invalid: {what}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// A newer build this platform can install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub id: BuildId,
    /// The release notes, if the manifest names a page.
    pub notes_url: Option<String>,
    pub asset: Asset,
}

/// Where the browser is with updates, as the shell last reported it
/// (`Browser::update`). The chrome draws from it: the "Restart to
/// update" button when [`UpdateStatus::Ready`], the failure in the status
/// area, the rest in the settings window and the menus.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Nothing checked yet, or this build cannot update itself (no
    /// build number, no source).
    #[default]
    Idle,
    Checking,
    /// The last check found nothing newer.
    UpToDate,
    /// The asset is on its way: bytes received of its size.
    Downloading {
        update: AvailableUpdate,
        received: u64,
    },
    /// The asset is downloaded and verified at `file`; a restart
    /// installs it.
    Ready {
        update: AvailableUpdate,
        file: PathBuf,
    },
    /// The user asked for the restart; the browser is on its way out.
    Installing(AvailableUpdate),
    /// The last check or download failed; shown until dismissed or the
    /// next check.
    Failed(String),
}

impl UpdateStatus {
    /// The update this status is about, if any.
    pub fn update(&self) -> Option<&AvailableUpdate> {
        match self {
            UpdateStatus::Downloading { update, .. }
            | UpdateStatus::Ready { update, .. }
            | UpdateStatus::Installing(update) => Some(update),
            _ => None,
        }
    }

    /// Whether a check may start now (none is running, and nothing is
    /// being downloaded or installed).
    pub fn can_check(&self) -> bool {
        matches!(
            self,
            UpdateStatus::Idle | UpdateStatus::UpToDate | UpdateStatus::Failed(_)
        )
    }

    /// One line for the settings window and the menus: what is going on.
    pub fn describe(&self, brand: &str) -> String {
        match self {
            UpdateStatus::Idle => String::new(),
            UpdateStatus::Checking => "Checking for updates…".to_string(),
            UpdateStatus::UpToDate => format!("{brand} is up to date."),
            UpdateStatus::Downloading { update, received } => {
                match received.saturating_mul(100).checked_div(update.asset.size) {
                    Some(percent) => {
                        format!("Downloading {brand} {}… {}%", update.id, percent.min(100))
                    }
                    None => format!("Downloading {brand} {}…", update.id),
                }
            }
            UpdateStatus::Ready { update, .. } => {
                format!("{brand} {} is ready to install.", update.id)
            }
            UpdateStatus::Installing(update) => {
                format!("Installing {brand} {}…", update.id)
            }
            UpdateStatus::Failed(message) => format!("Update failed: {message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
version = "0.1.0"
build = 150
commit = "0123abcd"
published = "2026-10-29T12:00:00Z"
notes_url = "https://github.com/owner/repo/releases/tag/build-150"

[assets.windows-x86_64]
name = "nautilus_0.1.0_x64-setup.exe"
url = "https://github.com/owner/repo/releases/download/build-150/nautilus_0.1.0_x64-setup.exe"
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
size = 123

[assets.macos-aarch64]
name = "Nautilus-macos-arm64.zip"
url = "https://github.com/owner/repo/releases/download/build-150/Nautilus-macos-arm64.zip"
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
size = 456
"#;

    #[test]
    fn build_numbers_parse_at_compile_time() {
        const NONE: Option<u64> = build_number(None);
        const EMPTY: Option<u64> = build_number(Some(""));
        const SOME: Option<u64> = build_number(Some("150"));
        const BAD: Option<u64> = build_number(Some("15a"));
        const BIG: Option<u64> = build_number(Some("99999999999999999999"));
        assert_eq!(NONE, None);
        assert_eq!(EMPTY, None);
        assert_eq!(SOME, Some(150));
        assert_eq!(BAD, None);
        assert_eq!(BIG, None);
    }

    #[test]
    fn builds_order_by_version_then_number() {
        let a = BuildId::new("0.1.0", 150);
        assert!(BuildId::new("0.1.0", 151).is_newer_than(&a));
        assert!(!BuildId::new("0.1.0", 150).is_newer_than(&a));
        assert!(!BuildId::new("0.1.0", 149).is_newer_than(&a));
        // A version bump wins over any build number.
        assert!(BuildId::new("0.2.0", 1).is_newer_than(&a));
        assert!(BuildId::new("1.0.0", 0).is_newer_than(&BuildId::new("0.9.9", 999)));
        assert!(!BuildId::new("0.0.9", 999).is_newer_than(&a));
        // Numeric, not lexical.
        assert!(BuildId::new("0.10.0", 0).is_newer_than(&BuildId::new("0.9.0", 0)));
        // A pre-release suffix is ignored.
        assert!(BuildId::new("1.2.0-rc.1", 5).is_newer_than(&BuildId::new("1.1.0", 9)));
        assert_eq!(a.to_string(), "0.1.0 (build 150)");
    }

    #[test]
    fn the_manifest_parses_and_offers_the_platform_its_asset() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.build_id(), BuildId::new("0.1.0", 150));
        assert_eq!(manifest.commit, "0123abcd");
        assert_eq!(manifest.assets.len(), 2);

        let older = BuildId::new("0.1.0", 142);
        let update = manifest.update_for(&older, "windows-x86_64").unwrap();
        assert_eq!(update.id, BuildId::new("0.1.0", 150));
        assert_eq!(update.asset.name, "nautilus_0.1.0_x64-setup.exe");
        assert_eq!(update.asset.size, 123);
        assert_eq!(
            update.notes_url.as_deref(),
            Some("https://github.com/owner/repo/releases/tag/build-150")
        );
        // No asset for the platform: nothing to offer.
        assert_eq!(manifest.update_for(&older, "macos-x86_64"), None);
        // Not newer: nothing.
        assert_eq!(
            manifest.update_for(&BuildId::new("0.1.0", 150), "windows-x86_64"),
            None
        );
        assert_eq!(
            manifest.update_for(&BuildId::new("0.2.0", 0), "windows-x86_64"),
            None
        );
    }

    #[test]
    fn a_minimal_manifest_is_enough_and_a_bad_one_is_refused() {
        let m = Manifest::parse("version = \"0.1.0\"\nbuild = 3\n").unwrap();
        assert!(m.assets.is_empty());
        assert_eq!(m.notes_url, None);
        assert!(matches!(
            Manifest::parse("version = \"0.1.0\""),
            Err(ManifestError::Parse(_))
        ));
        assert!(matches!(
            Manifest::parse("version = \"\"\nbuild = 3\n"),
            Err(ManifestError::Invalid(_))
        ));
        let bad_hash = SAMPLE.replace(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "abc",
        );
        assert!(matches!(
            Manifest::parse(&bad_hash),
            Err(ManifestError::Invalid(_))
        ));
        let bad_name = SAMPLE.replace("nautilus_0.1.0_x64-setup.exe\"\nurl", "../x.exe\"\nurl");
        assert!(matches!(
            Manifest::parse(&bad_name),
            Err(ManifestError::Invalid(_))
        ));
    }

    #[test]
    fn the_platform_key_is_os_dash_arch() {
        let key = platform_key();
        assert!(
            key == "windows-x86_64"
                || key == "macos-aarch64"
                || key == "macos-x86_64"
                || key == "linux-x86_64"
                || key == "linux-aarch64"
                || key == "windows-aarch64",
            "{key}"
        );
    }

    #[test]
    fn the_status_describes_itself() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        let update = manifest
            .update_for(&BuildId::new("0.1.0", 1), "windows-x86_64")
            .unwrap();
        assert_eq!(UpdateStatus::Idle.describe("Test"), "");
        assert_eq!(
            UpdateStatus::UpToDate.describe("Test"),
            "Test is up to date."
        );
        assert_eq!(
            UpdateStatus::Downloading {
                update: update.clone(),
                received: 61,
            }
            .describe("Test"),
            "Downloading Test 0.1.0 (build 150)… 49%"
        );
        let ready = UpdateStatus::Ready {
            update: update.clone(),
            file: PathBuf::from("x"),
        };
        assert_eq!(
            ready.describe("Test"),
            "Test 0.1.0 (build 150) is ready to install."
        );
        assert_eq!(ready.update(), Some(&update));
        assert!(!ready.can_check());
        assert!(UpdateStatus::Failed("boom".to_string()).can_check());
        assert_eq!(
            UpdateStatus::Failed("boom".to_string()).describe("Test"),
            "Update failed: boom"
        );
        assert_eq!(UpdateStatus::default(), UpdateStatus::Idle);
    }
}
