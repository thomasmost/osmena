//! winit input to engine input, and the key-routing rule. Coordinates are
//! converted by the window (toolbar offset, physical pixels); this module
//! maps the enums and decides where a key goes.

use keyboard_types::{Code, Modifiers};
use osmena_core::{
    ButtonAction, CommandKind, EngineInput, Extension, KeyChord, Keymap, MouseButton,
};
use winit::event::{ElementState, Ime, MouseScrollDelta};

pub fn mouse_button(button: winit::event::MouseButton) -> MouseButton {
    match button {
        winit::event::MouseButton::Left => MouseButton::Primary,
        winit::event::MouseButton::Right => MouseButton::Secondary,
        winit::event::MouseButton::Middle => MouseButton::Auxiliary,
        winit::event::MouseButton::Back => MouseButton::Back,
        winit::event::MouseButton::Forward => MouseButton::Forward,
        winit::event::MouseButton::Other(n) => MouseButton::Other(n),
    }
}

pub fn button_action(state: ElementState) -> ButtonAction {
    match state {
        ElementState::Pressed => ButtonAction::Down,
        ElementState::Released => ButtonAction::Up,
    }
}

/// Wheel delta in pixels, `(dx, dy)`. winit reports one wheel notch as
/// one line on Windows; `line_px` (the engine's `wheel_line_px`) is how
/// far one line scrolls.
pub fn wheel_delta(delta: MouseScrollDelta, line_px: f32) -> (f64, f64) {
    match delta {
        MouseScrollDelta::LineDelta(dx, dy) => ((dx * line_px) as f64, (dy * line_px) as f64),
        MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
    }
}

pub fn ime(ime: &Ime) -> Option<EngineInput> {
    match ime {
        Ime::Enabled => Some(EngineInput::ImeStart),
        Ime::Preedit(text, _) => Some(EngineInput::ImePreedit(text.clone())),
        Ime::Commit(text) => Some(EngineInput::ImeCommit(text.clone())),
        Ime::Disabled => Some(EngineInput::ImeDismissed),
    }
}

/// One keyboard shortcut an enabled extension declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionChord<'a> {
    pub chord: KeyChord,
    /// The extension's id.
    pub extension: &'a str,
    /// The command's name in the extension's manifest.
    pub name: &'a str,
}

/// The shortcuts of `extensions` that can fire: every command with a
/// chord, of every enabled extension, in the list's order (which decides
/// between two extensions that declare the same chord: the first wins).
pub fn extension_chords(extensions: &[Extension]) -> Vec<ExtensionChord<'_>> {
    extensions
        .iter()
        .filter(|extension| extension.enabled)
        .flat_map(|extension| {
            extension.commands.iter().filter_map(move |command| {
                command.chord.map(|chord| ExtensionChord {
                    chord,
                    extension: &extension.id,
                    name: &command.name,
                })
            })
        })
        .collect()
}

/// Where a key event goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRoute {
    /// egui: a widget (the address bar) has keyboard focus, or egui got
    /// the press of this key.
    Egui,
    /// The initial press of a chord bound in the keymap.
    Command(CommandKind),
    /// The initial press of an extension's chord that the keymap does not
    /// hold: the index into the chords given.
    Extension(usize),
    /// The view that has the keyboard: the active tab's page, or the side
    /// panel's.
    Page,
}

/// The routing rule: a bound chord with Cmd or Ctrl held fires its command
/// on the initial press even while an egui field (the address bar) has
/// focus, so shortcuts keep working there; otherwise egui takes every key
/// while it has focus; otherwise bound chords on their initial press
/// (auto-repeats and releases never trigger commands); otherwise the page.
///
/// "Bound" is the keymap first. A chord the keymap does not hold is then
/// looked up among `extensions` (the enabled extensions' shortcuts, from
/// [`extension_chords`]) under the same conditions, so a shortcut an
/// extension declares never takes a chord from the keymap, whoever has the
/// keyboard.
///
/// This is the stateless first cut for one event, used as is by the
/// settings window (which has no page and passes no extensions). Browser
/// windows go through [`crate::routing::KeyRouter`], which applies this
/// rule to presses and repeats and then sends repeats and releases to the
/// side that got the press, so a key that fired a command never reaches
/// the page.
pub fn route_key(
    egui_wants_keyboard: bool,
    pressed: bool,
    repeat: bool,
    modifiers: Modifiers,
    code: Code,
    keymap: &Keymap,
    extensions: &[ExtensionChord<'_>],
) -> KeyRoute {
    let with_primary = modifiers.intersects(Modifiers::CONTROL | Modifiers::META);
    let chord = KeyChord::new(modifiers, code);
    let initial = pressed && !repeat;
    let bound = if initial { keymap.resolve(chord) } else { None };
    let extension = if initial && bound.is_none() {
        extensions
            .iter()
            .position(|e| KeyChord::new(e.chord.modifiers, e.chord.code) == chord)
    } else {
        None
    };
    match (egui_wants_keyboard, bound, extension) {
        (true, Some(kind), _) if with_primary => KeyRoute::Command(kind),
        (true, None, Some(index)) if with_primary => KeyRoute::Extension(index),
        (true, _, _) => KeyRoute::Egui,
        (false, Some(kind), _) => KeyRoute::Command(kind),
        (false, None, Some(index)) => KeyRoute::Extension(index),
        (false, None, None) => KeyRoute::Page,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osmena_core::extension::ExtensionCommand;
    use osmena_core::{Extension, Platform};

    #[test]
    fn line_deltas_scale_to_pixels_and_pixel_deltas_pass_through() {
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, -1.0), 100.0),
            (0.0, -100.0)
        );
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, -1.0), 76.0),
            (0.0, -76.0)
        );
        assert_eq!(
            wheel_delta(
                MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(3.0, 4.0)),
                100.0
            ),
            (3.0, 4.0)
        );
    }

    #[test]
    fn buttons_map_one_to_one() {
        assert_eq!(
            mouse_button(winit::event::MouseButton::Left),
            MouseButton::Primary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Right),
            MouseButton::Secondary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Middle),
            MouseButton::Auxiliary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Other(7)),
            MouseButton::Other(7)
        );
        assert_eq!(button_action(ElementState::Pressed), ButtonAction::Down);
    }

    fn mac() -> Keymap {
        Keymap::defaults(Platform::MacOs)
    }

    #[test]
    fn egui_focus_takes_unbound_keys_but_primary_chords_still_fire() {
        let r = route_key(
            true,
            true,
            false,
            Modifiers::empty(),
            Code::KeyA,
            &mac(),
            &[],
        );
        assert_eq!(r, KeyRoute::Egui, "typing goes to the address bar");
        let r = route_key(true, true, false, Modifiers::META, Code::KeyA, &mac(), &[]);
        assert_eq!(r, KeyRoute::Egui, "Cmd+A is unbound: egui selects all");
        let r = route_key(true, true, false, Modifiers::META, Code::KeyT, &mac(), &[]);
        assert_eq!(
            r,
            KeyRoute::Command(CommandKind::NewTab),
            "Cmd+T works while the address bar has focus"
        );
        let r = route_key(true, false, false, Modifiers::META, Code::KeyT, &mac(), &[]);
        assert_eq!(r, KeyRoute::Egui, "the release goes to egui");
    }

    #[test]
    fn bound_chord_on_initial_press_is_a_command() {
        let r = route_key(false, true, false, Modifiers::META, Code::KeyT, &mac(), &[]);
        assert_eq!(r, KeyRoute::Command(CommandKind::NewTab));
    }

    #[test]
    fn repeats_and_releases_of_chords_never_fire_commands() {
        let r = route_key(false, true, true, Modifiers::META, Code::KeyT, &mac(), &[]);
        assert_eq!(r, KeyRoute::Page);
        let r = route_key(
            false,
            false,
            false,
            Modifiers::META,
            Code::KeyT,
            &mac(),
            &[],
        );
        assert_eq!(r, KeyRoute::Page);
    }

    #[test]
    fn unbound_keys_reach_the_page() {
        let r = route_key(
            false,
            true,
            false,
            Modifiers::empty(),
            Code::KeyA,
            &mac(),
            &[],
        );
        assert_eq!(r, KeyRoute::Page);
        let r = route_key(
            false,
            true,
            false,
            Modifiers::empty(),
            Code::Escape,
            &mac(),
            &[],
        );
        assert_eq!(r, KeyRoute::Page, "Escape belongs to the page");
        let r = route_key(
            false,
            true,
            false,
            Modifiers::META,
            Code::ArrowLeft,
            &mac(),
            &[],
        );
        assert_eq!(r, KeyRoute::Page, "Cmd+Left edits text in fields");
    }

    /// Cmd+E, which the mac keymap leaves free.
    fn cmd_e() -> KeyChord {
        KeyChord::new(Modifiers::META, Code::KeyE)
    }

    /// Alt+Shift+Y, free too, and without Cmd or Ctrl.
    fn alt_shift_y() -> KeyChord {
        KeyChord::new(Modifiers::ALT | Modifiers::SHIFT, Code::KeyY)
    }

    fn extension(id: &str, enabled: bool, commands: &[(&str, Option<KeyChord>)]) -> Extension {
        Extension {
            id: id.to_string(),
            name: id.to_uppercase(),
            version: "1".to_string(),
            description: String::new(),
            enabled,
            icon: None,
            action_title: None,
            side_panel: false,
            commands: commands
                .iter()
                .map(|(name, chord)| ExtensionCommand {
                    name: name.to_string(),
                    description: String::new(),
                    chord: *chord,
                })
                .collect(),
        }
    }

    #[test]
    fn the_test_chords_are_free_in_the_keymap() {
        assert_eq!(mac().resolve(cmd_e()), None);
        assert_eq!(mac().resolve(alt_shift_y()), None);
    }

    #[test]
    fn only_enabled_extensions_commands_with_a_chord_are_listed() {
        let extensions = [
            extension("aaa", true, &[("open", Some(cmd_e())), ("no-key", None)]),
            extension("bbb", false, &[("off", Some(alt_shift_y()))]),
            extension("ccc", true, &[("save", Some(alt_shift_y()))]),
        ];
        assert_eq!(
            extension_chords(&extensions),
            [
                ExtensionChord {
                    chord: cmd_e(),
                    extension: "aaa",
                    name: "open",
                },
                ExtensionChord {
                    chord: alt_shift_y(),
                    extension: "ccc",
                    name: "save",
                },
            ]
        );
    }

    #[test]
    fn an_extensions_chord_fires_on_its_initial_press() {
        let chords = [ExtensionChord {
            chord: cmd_e(),
            extension: "aaa",
            name: "open",
        }];
        let r = route_key(
            false,
            true,
            false,
            Modifiers::META,
            Code::KeyE,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Extension(0));
    }

    #[test]
    fn a_chord_in_the_keymap_beats_an_extensions() {
        // The extension asks for Cmd+T, which is New tab.
        let chords = [ExtensionChord {
            chord: KeyChord::new(Modifiers::META, Code::KeyT),
            extension: "aaa",
            name: "open",
        }];
        let r = route_key(
            false,
            true,
            false,
            Modifiers::META,
            Code::KeyT,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Command(CommandKind::NewTab));
        let r = route_key(
            true,
            true,
            false,
            Modifiers::META,
            Code::KeyT,
            &mac(),
            &chords,
        );
        assert_eq!(
            r,
            KeyRoute::Command(CommandKind::NewTab),
            "in the address bar too"
        );
    }

    #[test]
    fn repeats_and_releases_never_fire_an_extensions_command() {
        let chords = [ExtensionChord {
            chord: cmd_e(),
            extension: "aaa",
            name: "open",
        }];
        let r = route_key(
            false,
            true,
            true,
            Modifiers::META,
            Code::KeyE,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Page);
        let r = route_key(
            false,
            false,
            false,
            Modifiers::META,
            Code::KeyE,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Page);
    }

    /// As for the keymap: while a field of the chrome has the keyboard, a
    /// chord with Cmd or Ctrl still fires, and one without is typing.
    #[test]
    fn egui_focus_lets_through_only_an_extensions_primary_chords() {
        let chords = [
            ExtensionChord {
                chord: cmd_e(),
                extension: "aaa",
                name: "open",
            },
            ExtensionChord {
                chord: alt_shift_y(),
                extension: "ccc",
                name: "save",
            },
        ];
        let r = route_key(
            true,
            true,
            false,
            Modifiers::META,
            Code::KeyE,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Extension(0));
        let r = route_key(
            true,
            true,
            false,
            Modifiers::ALT | Modifiers::SHIFT,
            Code::KeyY,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Egui);
        let r = route_key(
            false,
            true,
            false,
            Modifiers::ALT | Modifiers::SHIFT,
            Code::KeyY,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Extension(1));
    }

    #[test]
    fn the_first_extension_listed_wins_a_chord_two_declare() {
        let chords = [
            ExtensionChord {
                chord: cmd_e(),
                extension: "aaa",
                name: "open",
            },
            ExtensionChord {
                chord: cmd_e(),
                extension: "ccc",
                name: "other",
            },
        ];
        let r = route_key(
            false,
            true,
            false,
            Modifiers::META,
            Code::KeyE,
            &mac(),
            &chords,
        );
        assert_eq!(r, KeyRoute::Extension(0));
    }
}
