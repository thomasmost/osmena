//! Two fingers spread or pinched on a touchpad zoom the page.
//!
//! The two systems say it differently, and winit passes on only one of
//! them. macOS has a gesture of its own, which winit delivers as
//! `WindowEvent::PinchGesture` with the magnification as a fraction.
//! Windows, for an application that does not take raw touchpad input,
//! turns the gesture into the wheel with Ctrl: a `WM_MOUSEWHEEL` whose
//! key-state word carries `MK_CONTROL` although nobody is holding the
//! key. winit takes the wheel from that message and the modifiers from
//! the keyboard, so the window saw a plain wheel and scrolled the page.
//! The event loop's message hook (installed by `alt_code`) sees the
//! message first and notes the flag here ([`wheel_message`]); the
//! window reads it while it handles the wheel event winit makes of the
//! same message ([`wheel_has_control`]).
//!
//! Either way the outcome is the model's zoom, a step at a time
//! ([`ZoomSteps`]), the same as Ctrl and a real wheel: per site, kept,
//! and shown in the address bar's chip. It is not Chromium's own pinch,
//! which magnifies the picture without laying the page out again.

use std::sync::atomic::{AtomicBool, Ordering};

const WM_MOUSEWHEEL: u32 = 0x020A;
/// The Ctrl bit of a mouse message's key-state word (the low word of
/// its `wParam`).
const MK_CONTROL: usize = 0x0008;

/// How much magnification, as winit reports it on macOS (a fraction:
/// 0.1 is ten per cent larger), makes one zoom step.
pub(crate) const PINCH_STEP: f64 = 0.15;

/// Whether the wheel message being dispatched carries Ctrl. One event
/// loop, one thread: the hook writes it just before winit handles the
/// message and the window reads it within that handling.
static WHEEL_CONTROL: AtomicBool = AtomicBool::new(false);

/// What a message says about the wheel: `Some(ctrl)` for a wheel
/// message, with whether its key-state word carries Ctrl; `None` for
/// any other message.
pub(crate) fn wheel_control(message: u32, wparam: usize) -> Option<bool> {
    (message == WM_MOUSEWHEEL).then_some(wparam & MK_CONTROL != 0)
}

/// Feed one message from the event loop's hook.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn wheel_message(message: u32, wparam: usize) {
    if let Some(control) = wheel_control(message, wparam) {
        WHEEL_CONTROL.store(control, Ordering::Relaxed);
    }
}

/// Whether the wheel event being handled came with Ctrl in its own
/// message: the key held, or a pinch on a touchpad. Always `false`
/// where no hook feeds it.
pub(crate) fn wheel_has_control() -> bool {
    WHEEL_CONTROL.load(Ordering::Relaxed)
}

/// Travel that adds up to zoom steps: wheel distance against a notch,
/// or magnification against [`PINCH_STEP`]. What is left over stays
/// for the next event, so a slow gesture still gets there.
#[derive(Debug, Default)]
pub(crate) struct ZoomSteps {
    travelled: f64,
}

impl ZoomSteps {
    /// Add `amount` and take out the whole steps of `step` it makes:
    /// positive to zoom in that many times, negative to zoom out. A
    /// step that is not a positive number, and an amount that is no
    /// number at all, give none.
    pub(crate) fn add(&mut self, amount: f64, step: f64) -> i32 {
        if !amount.is_finite() || !step.is_finite() || step <= 0.0 {
            return 0;
        }
        self.travelled += amount;
        let steps = (self.travelled / step).trunc();
        self.travelled -= steps * step;
        steps.clamp(-64.0, 64.0) as i32
    }

    /// Forget what is left over: the gesture ended, or the wheel went
    /// back to scrolling.
    pub(crate) fn reset(&mut self) {
        self.travelled = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WM_MOUSEMOVE: u32 = 0x0200;
    const MK_SHIFT: usize = 0x0004;

    #[test]
    fn only_a_wheel_message_says_anything_and_only_its_ctrl_bit_counts() {
        // A wheel message's wParam: the key state in the low word, the
        // distance in the high word.
        let up = 120usize << 16;
        assert_eq!(wheel_control(WM_MOUSEWHEEL, up), Some(false));
        assert_eq!(wheel_control(WM_MOUSEWHEEL, up | MK_CONTROL), Some(true));
        assert_eq!(wheel_control(WM_MOUSEWHEEL, up | MK_SHIFT), Some(false));
        // The distance never reads as the flag.
        assert_eq!(wheel_control(WM_MOUSEWHEEL, 0xFFF8usize << 16), Some(false));
        assert_eq!(wheel_control(WM_MOUSEMOVE, MK_CONTROL), None);
    }

    #[test]
    fn the_flag_follows_the_last_wheel_message() {
        wheel_message(WM_MOUSEWHEEL, MK_CONTROL);
        assert!(wheel_has_control());
        // Another message in between changes nothing.
        wheel_message(WM_MOUSEMOVE, 0);
        assert!(wheel_has_control());
        wheel_message(WM_MOUSEWHEEL, 0);
        assert!(!wheel_has_control());
    }

    #[test]
    fn travel_adds_up_to_steps_and_keeps_the_rest() {
        let mut zoom = ZoomSteps::default();
        // Small pinch deltas: nothing until they make a step.
        assert_eq!(zoom.add(0.06, PINCH_STEP), 0);
        assert_eq!(zoom.add(0.06, PINCH_STEP), 0);
        assert_eq!(zoom.add(0.06, PINCH_STEP), 1);
        // The other way, past what was left over.
        assert_eq!(zoom.add(-0.10, PINCH_STEP), 0);
        assert_eq!(zoom.add(-0.10, PINCH_STEP), -1);
        // A big notch of the wheel makes several at once.
        let mut zoom = ZoomSteps::default();
        assert_eq!(zoom.add(250.0, 100.0), 2);
        assert_eq!(zoom.add(50.0, 100.0), 1);
        // A reset drops the remainder.
        assert_eq!(zoom.add(90.0, 100.0), 0);
        zoom.reset();
        assert_eq!(zoom.add(90.0, 100.0), 0);
        // What is no number moves nothing.
        assert_eq!(zoom.add(f64::NAN, PINCH_STEP), 0);
        assert_eq!(zoom.add(1.0, 0.0), 0);
        assert_eq!(zoom.add(20.0, 100.0), 1);
    }
}
