//! The egui chrome of an osmena browser, and the pure input rules around
//! it. Nothing here touches an engine: the chrome draws the model from
//! `osmena-core` and returns the [`Command`]s the user issued.
//!
//! - [`chrome`]: the tab strip, toolbar, status overlay, and crash panel;
//!   `launch` (private) draws the Launch Panel for it.
//! - [`menus`]: the window and tab menus as [`MenuEntry`] data, and
//!   [`native_menu`], which shows them as native popups through `muda`.
//! - [`routing`]: where a pointer or key event goes (chrome, page, or a
//!   command), unit-tested without a window.
//! - [`input`]: winit input to [`EngineInput`], and the one key-routing
//!   rule; [`keyutils`]: winit keys to `keyboard-types`.
//! - [`icon`] and [`fatal`]: the brand's icon and the fatal-error dialog.
//!
//! [`Command`]: osmena_core::Command
//! [`EngineInput`]: osmena_core::EngineInput
//! [`MenuEntry`]: menus::MenuEntry

pub mod chrome;
pub mod fatal;
pub mod icon;
pub mod input;
pub mod keyutils;
mod launch;
pub mod menus;
pub mod native_menu;
pub mod routing;
pub mod theme;

pub use keyboard_types;
