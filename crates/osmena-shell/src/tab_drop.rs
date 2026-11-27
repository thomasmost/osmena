//! Where tabs let go off their strip end up: the decisions, apart from
//! the windows they are made about.

use osmena_core::WindowGeometry;

/// What a drop of tabs off their strip does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget<W> {
    /// The tabs join this other window of the profile.
    Window(W),
    /// The tabs get a window of their own, where they were dropped.
    NewWindow,
    /// Nothing moves.
    Nothing,
}

/// What a drop does, from what is under the pointer: `other` is another
/// window of this profile there (the front one, when several are),
/// `over_source` whether the pointer is over the window the tabs came
/// from, and `whole_window` whether the tabs are all that window has.
///
/// Another window takes the tabs. Anywhere else they are torn off into
/// a new window, over their own window too (the drag was detached from
/// its strip, or it would have been a reorder); except that every tab
/// of a window dropped on that window is torn from nothing, and stays.
pub fn drop_target<W>(other: Option<W>, over_source: bool, whole_window: bool) -> DropTarget<W> {
    match other {
        Some(window) => DropTarget::Window(window),
        None if over_source && whole_window => DropTarget::Nothing,
        None => DropTarget::NewWindow,
    }
}

/// The window in front among `under` (those the pointer is over), going
/// by `recent`, the windows in the order they last had the keyboard,
/// latest first: the stand-in for the system's stacking order, which
/// winit does not tell. A window that never had the keyboard is behind
/// the ones that did.
pub fn frontmost<W: Copy + PartialEq>(under: &[W], recent: &[W]) -> Option<W> {
    recent
        .iter()
        .find(|window| under.contains(window))
        .or(under.first())
        .copied()
}

/// `window` got the keyboard: it is now the first of `recent`.
pub fn note_focus<W: Copy + PartialEq>(recent: &mut Vec<W>, window: W) {
    recent.retain(|w| *w != window);
    recent.insert(0, window);
}

/// The place of a window made for tabs dragged out of the window at
/// `source`: the source's size, with its corner at `origin` (the
/// chrome's `TabDrop::origin`, in points from the source's corner at
/// `scale` pixels a point), which puts the tabs under the pointer as
/// they were held. Never maximized: it is a window let go at a point.
pub fn torn_off_place(source: WindowGeometry, origin: (f32, f32), scale: f64) -> WindowGeometry {
    let pixels = |points: f32| (f64::from(points) * scale).round() as i32;
    WindowGeometry {
        x: source.x.saturating_add(pixels(origin.0)),
        y: source.y.saturating_add(pixels(origin.1)),
        maximized: false,
        ..source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_window_under_the_pointer_takes_the_tabs() {
        assert_eq!(drop_target(Some(7), false, false), DropTarget::Window(7));
        // Whatever else is true: it is in front of the source, or the
        // tabs are all the source has.
        assert_eq!(drop_target(Some(7), true, true), DropTarget::Window(7));
    }

    #[test]
    fn tabs_dropped_on_no_other_window_are_torn_off() {
        assert_eq!(drop_target::<u8>(None, false, false), DropTarget::NewWindow);
        assert_eq!(
            drop_target::<u8>(None, true, false),
            DropTarget::NewWindow,
            "over their own window's page too"
        );
        assert_eq!(drop_target::<u8>(None, false, true), DropTarget::NewWindow);
    }

    #[test]
    fn every_tab_of_a_window_dropped_on_it_stays() {
        assert_eq!(drop_target::<u8>(None, true, true), DropTarget::Nothing);
    }

    #[test]
    fn the_window_that_last_had_the_keyboard_is_the_one_in_front() {
        let mut recent = Vec::new();
        note_focus(&mut recent, 1);
        note_focus(&mut recent, 2);
        note_focus(&mut recent, 3);
        note_focus(&mut recent, 2);
        assert_eq!(recent, vec![2, 3, 1]);
        assert_eq!(frontmost(&[1, 3], &recent), Some(3));
        assert_eq!(frontmost(&[1, 2, 3], &recent), Some(2));
        assert_eq!(frontmost(&[1], &recent), Some(1));
        assert_eq!(frontmost(&[], &recent), None);
        // One that never had the keyboard is behind the rest, but is
        // still a window the tabs can go to.
        assert_eq!(frontmost(&[9, 1], &recent), Some(1));
        assert_eq!(frontmost(&[9], &recent), Some(9));
    }

    #[test]
    fn a_torn_off_window_takes_the_sources_size_at_the_drop() {
        let source = WindowGeometry {
            x: 100,
            y: 50,
            width: 2048,
            height: 1536,
            maximized: true,
        };
        assert_eq!(
            torn_off_place(source, (300.5, -20.0), 2.0),
            WindowGeometry {
                x: 701,
                y: 10,
                width: 2048,
                height: 1536,
                maximized: false,
            }
        );
    }
}
