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

pub mod brand;
pub mod command;
pub mod event;
pub mod input;
pub mod keymap;
pub mod model;
pub mod profile;
pub mod settings;

pub use brand::{Brand, EngineInfo};
pub use command::{Command, EngineAction, dispatch};
pub use event::{Cursor, EngineEvent, PendingTab, apply};
pub use input::{ButtonAction, EngineInput, KeyInput, MouseButton};
pub use keymap::{ChordParseError, CommandKind, KeyChord, Keymap, KeymapError, Platform};
pub use model::{Browser, Favicon, LoadState, Tab, TabId, Window, WindowId};
pub use profile::{Profile, ProfileError};
pub use settings::{ChromeColor, Settings, SettingsError};
