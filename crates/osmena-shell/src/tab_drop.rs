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

/// Where a drop of tabs goes. `under` are the windows of the profile the
/// pointer is over, the source among them when it is; `recent` is the
/// focus order (see [`frontmost`]); `source` is the window the tabs came
/// from; `whole_window` says the tabs are all that window has.
///
/// The front window among `under`, source included, decides. Another
/// window in front takes the tabs. Otherwise they are torn off into a
/// new window where they were dropped, over the source too (the drag was
/// detached from its strip, or it would have been a reorder), except that
/// when the source is in front and the tabs are all it has, nothing moves:
/// a window dropped on itself keeps what it has. A source behind another
/// window under the pointer is not the drop's: the window in front is.
pub fn drop_destination<W: Copy + PartialEq>(
    under: &[W],
    recent: &[W],
    source: W,
    whole_window: bool,
) -> DropTarget<W> {
    match frontmost(under, recent) {
        Some(window) if window != source => DropTarget::Window(window),
        Some(_) if whole_window => DropTarget::Nothing,
        _ => DropTarget::NewWindow,
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

    /// Window 1 is the source; 7 is another window of the profile.
    #[test]
    fn a_window_under_the_pointer_takes_the_tabs_when_the_source_is_not_there() {
        assert_eq!(
            drop_destination(&[7], &[7], 1, false),
            DropTarget::Window(7)
        );
        assert_eq!(drop_destination(&[7], &[7], 1, true), DropTarget::Window(7));
    }

    /// Window 7 is hidden behind the source, and the pointer is over both.
    /// The source had the keyboard, so it is in front: the drop is on the
    /// source, not on the window behind it.
    #[test]
    fn the_source_in_front_of_another_window_takes_the_drop_itself() {
        assert_eq!(
            drop_destination(&[1, 7], &[1, 7], 1, false),
            DropTarget::NewWindow
        );
        assert_eq!(
            drop_destination(&[1, 7], &[1, 7], 1, true),
            DropTarget::Nothing,
            "every tab of the source, dropped over it, stays"
        );
    }

    /// The source is behind window 7 under the pointer: 7 is in front, and
    /// takes the tabs, whether or not they are all of the source's.
    #[test]
    fn the_source_behind_another_window_hands_the_tabs_to_it() {
        assert_eq!(
            drop_destination(&[1, 7], &[7, 1], 1, false),
            DropTarget::Window(7)
        );
        assert_eq!(
            drop_destination(&[1, 7], &[7, 1], 1, true),
            DropTarget::Window(7)
        );
    }

    #[test]
    fn the_source_alone_under_the_pointer_is_its_own_drop() {
        assert_eq!(
            drop_destination(&[1], &[1], 1, false),
            DropTarget::NewWindow
        );
        assert_eq!(drop_destination(&[1], &[1], 1, true), DropTarget::Nothing);
    }

    #[test]
    fn nothing_under_the_pointer_tears_the_tabs_off_whatever_the_source_holds() {
        assert_eq!(
            drop_destination::<u8>(&[], &[7], 1, false),
            DropTarget::NewWindow
        );
        assert_eq!(
            drop_destination::<u8>(&[], &[7], 1, true),
            DropTarget::NewWindow
        );
    }

    /// A window that never had the keyboard is behind the source, which did.
    #[test]
    fn a_window_that_never_had_the_keyboard_is_behind_the_source() {
        assert_eq!(
            drop_destination(&[1, 7], &[1], 1, false),
            DropTarget::NewWindow
        );
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
