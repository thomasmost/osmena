//! The engine-free model of a browser.
//!
//! This crate knows nothing about any engine, egui, or winit. It holds the
//! window and tab model, the named commands the UI emits, the events an
//! engine reports, the input a tab receives, the keymap, settings, profile
//! paths, and the [`Brand`] and [`EngineInfo`] a browser identifies itself
//! with. The shell turns commands into engine calls; an engine crate turns
//! its callbacks into [`EngineEvent`]s. Both go through the pure functions
//! [`dispatch`] and [`apply`], which is what keeps this crate unit-testable
//! without a browser engine.

pub mod bookmarks;
pub mod bookmarks_html;
pub mod brand;
pub mod command;
pub mod download;
pub mod event;
pub mod extension;
pub mod history;
pub mod input;
pub mod keymap;
pub mod model;
pub mod profile;
pub mod settings;
pub mod suggest;
pub mod update;
pub mod zoom;

pub use bookmarks::{Bookmark, BookmarkGroup, Bookmarks, BookmarksError};
pub use brand::{Brand, EngineInfo, ExtensionSupport, PageTools};
pub use command::{
    BookmarksFile, Command, EngineAction, FolderTarget, PageCommand, ProfileForm, Prompt, dispatch,
};
pub use download::{
    Download, DownloadId, DownloadPolicy, DownloadReport, DownloadState, Downloads, DownloadsError,
    describe_bytes, describe_download, system_downloads_dir,
};
pub use event::{
    Cursor, EditState, EngineEvent, Media, MediaKind, Misspelling, PageContext, PendingTab, apply,
};
pub use extension::{
    Extension, ExtensionSource, InstalledExtension, InstalledExtensions, InstalledExtensionsError,
};
pub use history::{History, HistoryEntry, HistoryError};
pub use input::{ButtonAction, EngineInput, KeyInput, MouseButton};
pub use keymap::{ChordParseError, CommandKind, KeyChord, Keymap, KeymapError, Platform};
pub use model::{
    Browser, Favicon, FindState, Folder, FolderId, LoadState, RgbaImage, StripItem, Tab, TabId,
    Window, WindowId, site_key,
};
pub use profile::{Avatar, Profile, ProfileEntry, ProfileError};
pub use settings::{
    ChromeColor, DownloadSettings, ProfileSettings, SearchSettings, Settings, SettingsError,
};
pub use suggest::{Suggestion, SuggestionSource, suggestions};
pub use update::{Asset, AvailableUpdate, BuildId, Manifest, UpdateSource, UpdateStatus};
pub use zoom::{SiteZoom, ZOOM_MAX, ZOOM_MIN, ZOOM_STEPS, ZoomError, next_step};
