//! Input for a tab, in the browser's own terms. The shell builds these from
//! window events; each engine crate converts them to its engine's events.
//! This keeps the windowing library out of the engines and the engines out
//! of the shell.

use keyboard_types::{Code, Key, KeyState, Location, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Primary,
    Secondary,
    Auxiliary,
    Back,
    Forward,
    Other(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    Down,
    Up,
}

/// A key event in `keyboard-types` terms (the W3C UI Events model).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    pub state: KeyState,
    pub key: Key,
    pub code: Code,
    pub location: Location,
    pub modifiers: Modifiers,
    pub repeat: bool,
}

/// Input for a tab. Positions are physical pixels relative to the top-left
/// of the page area (below the toolbar). Every pointer event carries the
/// keyboard modifiers held at that moment, as the window knows them: an
/// engine never infers them from the key events it saw, because a
/// modifier can go down or up while the window is not focused (Alt+Tab
/// takes the Alt release with it).
#[derive(Clone, Debug, PartialEq)]
pub enum EngineInput {
    MouseMove {
        x: f32,
        y: f32,
        modifiers: Modifiers,
    },
    MouseButton {
        action: ButtonAction,
        button: MouseButton,
        x: f32,
        y: f32,
        modifiers: Modifiers,
    },
    /// The pointer left the page area.
    MouseLeft,
    /// Scroll by `(dx, dy)` pixels with the pointer at `(x, y)`; positive
    /// `dy` scrolls towards the top of the page (wheel rolled away from
    /// the user), as in winit.
    Wheel {
        dx: f64,
        dy: f64,
        x: f32,
        y: f32,
        modifiers: Modifiers,
    },
    Key(KeyInput),
    ImeStart,
    ImePreedit(String),
    ImeCommit(String),
    ImeDismissed,
}
