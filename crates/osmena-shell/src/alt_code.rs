//! Characters typed on Windows by holding Alt and keying a code on the
//! numeric keypad (Alt+0151 for an em dash).
//!
//! Windows collects the digits itself and, when Alt is released, posts
//! one `WM_CHAR` with the character. winit builds its key events from a
//! key-down and the `WM_CHAR`s that follow it, so a `WM_CHAR` that
//! follows a key-up belongs to no event of its and is dropped: neither
//! the page nor a field of the chrome ever saw the character. The
//! event loop's message hook sees every message first; [`AltCodes`]
//! picks that one out and the shell delivers it as typed text
//! ([`crate::app::AppEvent::Typed`]).

/// `WM_KEYFIRST` and `WM_KEYLAST`: the range of keyboard messages.
const WM_KEYFIRST: u32 = 0x0100;
const WM_KEYLAST: u32 = 0x0109;
const WM_KEYUP: u32 = 0x0101;
const WM_CHAR: u32 = 0x0102;
const WM_SYSKEYUP: u32 = 0x0105;
/// The virtual-key code of either Alt key.
const VK_MENU: usize = 0x12;

/// Follows a thread's keyboard messages and answers with the character
/// of a `WM_CHAR` that comes straight after Alt's release, which is the
/// only keyboard message the system sends between the two for an Alt
/// code. Any other keyboard message in between (a key-down, with the
/// `WM_CHAR`s winit attaches to it) ends the wait; messages that are
/// not the keyboard's (the pointer, a timer) may be retrieved between
/// the release and its character and change nothing.
#[derive(Debug, Default)]
pub(crate) struct AltCodes {
    alt_released: bool,
}

impl AltCodes {
    /// Feed one message; `Some` is a character to deliver as typed.
    pub(crate) fn message(&mut self, message: u32, wparam: usize) -> Option<char> {
        if !(WM_KEYFIRST..=WM_KEYLAST).contains(&message) {
            return None;
        }
        let was_released = std::mem::replace(
            &mut self.alt_released,
            matches!(message, WM_KEYUP | WM_SYSKEYUP) && wparam == VK_MENU,
        );
        if message != WM_CHAR || !was_released {
            return None;
        }
        // One UTF-16 unit: a surrogate is no character by itself, and a
        // control character is nothing to type.
        u32::try_from(wparam)
            .ok()
            .and_then(char::from_u32)
            .filter(|c| !c.is_control())
    }
}

/// Install the message hook on the event loop being built. The proxy
/// goes into the returned cell once the loop exists; until then nothing
/// is delivered.
#[cfg(windows)]
pub(crate) fn install(
    builder: &mut winit::event_loop::EventLoopBuilder<crate::app::AppEvent>,
) -> std::rc::Rc<std::cell::OnceCell<winit::event_loop::EventLoopProxy<crate::app::AppEvent>>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::MSG;
    use winit::platform::windows::EventLoopBuilderExtWindows;

    let proxy = std::rc::Rc::new(std::cell::OnceCell::<
        winit::event_loop::EventLoopProxy<crate::app::AppEvent>,
    >::new());
    let for_hook = proxy.clone();
    let mut codes = AltCodes::default();
    builder.with_msg_hook(move |msg| {
        // SAFETY: winit calls the hook with a pointer to the `MSG` it
        // just retrieved, valid for the call.
        let msg = unsafe { &*msg.cast::<MSG>() };
        if let Some(character) = codes.message(msg.message, msg.wParam)
            && let Some(proxy) = for_hook.get()
            && let Err(e) = proxy.send_event(crate::app::AppEvent::Typed(character.to_string()))
        {
            log::warn!("failed to deliver an Alt code's character: {e}");
        }
        // Never swallowed: winit and the system see the message as before.
        false
    });
    proxy
}

#[cfg(test)]
mod tests {
    use super::*;

    const WM_KEYDOWN: u32 = 0x0100;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_MOUSEMOVE: u32 = 0x0200;
    const VK_NUMPAD0: usize = 0x60;

    #[test]
    fn the_character_after_alts_release_is_typed() {
        let mut codes = AltCodes::default();
        // Alt down, 0 1 5 1 on the keypad, Alt up, then the character.
        assert_eq!(codes.message(WM_SYSKEYDOWN, VK_MENU), None);
        for digit in [0, 1, 5, 1] {
            assert_eq!(codes.message(WM_SYSKEYDOWN, VK_NUMPAD0 + digit), None);
            assert_eq!(codes.message(WM_SYSKEYUP, VK_NUMPAD0 + digit), None);
        }
        assert_eq!(codes.message(WM_KEYUP, VK_MENU), None);
        // The pointer moved meanwhile: not the keyboard's business.
        assert_eq!(codes.message(WM_MOUSEMOVE, 0), None);
        assert_eq!(codes.message(WM_CHAR, 0x2014), Some('—'));
        // Once only.
        assert_eq!(codes.message(WM_CHAR, 0x2014), None);
    }

    #[test]
    fn a_key_presss_own_character_is_winits() {
        let mut codes = AltCodes::default();
        assert_eq!(codes.message(WM_KEYDOWN, 0x41), None);
        assert_eq!(codes.message(WM_CHAR, 0x61), None);
        // Alt tapped, then a key: the tap is forgotten at the key-down.
        assert_eq!(codes.message(WM_SYSKEYDOWN, VK_MENU), None);
        assert_eq!(codes.message(WM_SYSKEYUP, VK_MENU), None);
        assert_eq!(codes.message(WM_KEYDOWN, 0x41), None);
        assert_eq!(codes.message(WM_CHAR, 0x61), None);
        // Another key's release starts no wait.
        assert_eq!(codes.message(WM_KEYUP, 0x41), None);
        assert_eq!(codes.message(WM_CHAR, 0x61), None);
    }

    #[test]
    fn control_characters_and_lone_surrogates_type_nothing() {
        for unit in [0x1B, 0x0D, 0xD83D, 0xDE00] {
            let mut codes = AltCodes::default();
            assert_eq!(codes.message(WM_KEYUP, VK_MENU), None);
            assert_eq!(codes.message(WM_CHAR, unit), None, "{unit:#x}");
        }
    }
}
