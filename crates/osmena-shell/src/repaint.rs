//! When egui wants the next frame. egui asks through a callback, either
//! at once (an animation, a response to input) or after a delay (the
//! caret blink in a focused text field, a tooltip about to appear). A
//! request that is due becomes a redraw request on the window; one that
//! is not becomes a deadline the application waits for with
//! `ControlFlow::WaitUntil`. Treating a delayed request as an immediate
//! one, which `Context::has_requested_repaint` invites, redraws the
//! window continuously while the address bar has focus.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use winit::window::Window;

/// The repaint requests of one egui context, and the deadline not yet
/// reached.
pub struct RepaintSchedule {
    /// The earliest moment requested since the last poll; written by
    /// egui's callback from inside a pass.
    requested: Arc<Mutex<Option<Instant>>>,
    /// The earliest moment a frame is wanted at, past the last poll.
    due: Option<Instant>,
}

impl RepaintSchedule {
    /// Install the callback on `ctx` and return the schedule to poll.
    pub fn install(ctx: &egui::Context) -> RepaintSchedule {
        let requested: Arc<Mutex<Option<Instant>>> = Arc::default();
        let shared = requested.clone();
        ctx.set_request_repaint_callback(move |info| request(&shared, info.delay));
        RepaintSchedule {
            requested,
            due: None,
        }
    }

    /// Act on what egui asked for since the last poll: request a redraw
    /// of `window` if a frame is due, and return the moment the next one
    /// is wanted at otherwise, for the event loop to wait until.
    pub fn poll(&mut self, window: &Window) -> Option<Instant> {
        let requested = lock(&self.requested).take();
        self.due = earliest(self.due, requested);
        if self.due.is_some_and(|at| at <= Instant::now()) {
            self.due = None;
            window.request_redraw();
        }
        self.due
    }
}

fn request(requested: &Mutex<Option<Instant>>, delay: Duration) {
    let Some(at) = Instant::now().checked_add(delay) else {
        return;
    };
    let mut earliest_requested = lock(requested);
    *earliest_requested = earliest(*earliest_requested, Some(at));
}

fn lock(requested: &Mutex<Option<Instant>>) -> std::sync::MutexGuard<'_, Option<Instant>> {
    requested
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The earlier of two optional moments.
pub fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_earliest_of_two_moments_wins_and_none_is_ignored() {
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        assert_eq!(earliest(Some(later), Some(now)), Some(now));
        assert_eq!(earliest(None, Some(later)), Some(later));
        assert_eq!(earliest(Some(now), None), Some(now));
        assert_eq!(earliest(None, None), None);
    }

    #[test]
    fn a_request_keeps_the_earliest_moment() {
        let requested = Mutex::new(None);
        request(&requested, Duration::from_secs(5));
        let far = lock(&requested).expect("a moment was requested");
        request(&requested, Duration::ZERO);
        let near = lock(&requested).expect("a moment was requested");
        assert!(near < far);
        request(&requested, Duration::from_secs(5));
        assert_eq!(
            *lock(&requested),
            Some(near),
            "a later request does not push it back"
        );
        request(&requested, Duration::MAX);
        assert_eq!(
            *lock(&requested),
            Some(near),
            "an overflowing delay is ignored"
        );
    }
}
