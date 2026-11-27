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

/// A monitor in physical pixels, with its scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// The size a window opens at when it is given no place, in logical
/// points: the default of `BrowserWindow::open`.
pub const NEW_WINDOW_SIZE: (f64, f64) = (1024.0, 768.0);

/// Where tabs were let go off a window, as the source and the chrome saw
/// it at the drop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TornOff {
    /// The source's outer top-left corner and inner size, in physical
    /// pixels. `maximized` is set when the source is maximized or full
    /// screen: its size is then not one a window should copy.
    pub source: WindowGeometry,
    /// The source's scale factor (physical pixels a point).
    pub source_scale: f64,
    /// From the source's outer corner to its content's, in physical
    /// pixels: the system frame's left and top, or zero without a frame.
    pub frame: (i32, i32),
    /// Where the new window's content corner goes, in points from the
    /// source's content corner (`TabDrop::origin`), so that the tabs stay
    /// under the pointer as they were held.
    pub origin: (f32, f32),
}

/// The place of a window made for tabs torn off as `torn`, on the monitor
/// `screen` under the drop. The window is the size it was in points,
/// scaled to that monitor, unless the source was maximized, when it is
/// [`NEW_WINDOW_SIZE`]; it is never larger than the monitor, and pulled
/// back onto the monitor if the drop was near its edge. Its outer corner
/// is placed (`set_outer_position`, which means the same on every
/// platform), so its content corner is where the origin puts it.
pub fn torn_off_place(torn: &TornOff, screen: Screen) -> WindowGeometry {
    let source = torn.source;
    let (points_width, points_height) = if source.maximized {
        NEW_WINDOW_SIZE
    } else {
        (
            f64::from(source.width) / torn.source_scale,
            f64::from(source.height) / torn.source_scale,
        )
    };
    let fit =
        |points: f64, span: u32| ((points * screen.scale).round() as u32).clamp(1, span.max(1));
    let width = fit(points_width, screen.width);
    let height = fit(points_height, screen.height);

    // The content corner, in the physical pixels of the screen: the source's
    // content corner moved by the origin, which is in the source's points.
    let content_x = f64::from(source.x) + f64::from(torn.frame.0);
    let content_y = f64::from(source.y) + f64::from(torn.frame.1);
    let content_x = content_x + f64::from(torn.origin.0) * torn.source_scale;
    let content_y = content_y + f64::from(torn.origin.1) * torn.source_scale;
    // The outer corner is that less the frame, which is measured in the
    // monitor's pixels.
    let frame_scale = screen.scale / torn.source_scale;
    let x = (content_x - f64::from(torn.frame.0) * frame_scale).round() as i64;
    let y = (content_y - f64::from(torn.frame.1) * frame_scale).round() as i64;

    WindowGeometry {
        x: fit_on(x, screen.x, screen.width, width),
        y: fit_on(y, screen.y, screen.height, height),
        width,
        height,
        maximized: false,
    }
}

/// `at`, moved so that a span of `size` starting there lies within the
/// `span` that starts at `start`, as far as it can (a span larger than the
/// monitor starts at its edge).
fn fit_on(at: i64, start: i32, span: u32, size: u32) -> i32 {
    let low = i64::from(start);
    let high = (low + i64::from(span) - i64::from(size)).max(low);
    at.clamp(low, high) as i32
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

    fn source(x: i32, y: i32, width: u32, height: u32, maximized: bool) -> WindowGeometry {
        WindowGeometry {
            x,
            y,
            width,
            height,
            maximized,
        }
    }

    fn screen(width: u32, height: u32, scale: f64) -> Screen {
        Screen {
            x: 0,
            y: 0,
            width,
            height,
            scale,
        }
    }

    fn torn(source: WindowGeometry, source_scale: f64, origin: (f32, f32)) -> TornOff {
        TornOff {
            source,
            source_scale,
            frame: (0, 0),
            origin,
        }
    }

    /// A maximized source is 3840 by 2160 pixels at scale 2, which is 1920
    /// by 1080 points; a window torn off it is the size a new window opens
    /// at, 1024 by 768 points, at the scale of the monitor it lands on.
    #[test]
    fn a_torn_off_window_from_a_maximized_source_opens_at_the_default_size() {
        let maximized = source(0, 0, 3840, 2160, true);
        let place = torn_off_place(
            &torn(maximized, 2.0, (300.0, 40.0)),
            screen(3840, 2160, 2.0),
        );
        assert_eq!(
            (place.width, place.height),
            (2048, 1536),
            "the default size, in the monitor's pixels"
        );
        assert!(!place.maximized);
        assert_eq!(
            (place.x, place.y),
            (600, 80),
            "300 and 40 points at scale 2"
        );
    }

    /// A window that was not maximized keeps its size in points, so it is
    /// the same window on a monitor of another scale.
    #[test]
    fn a_torn_off_window_keeps_its_size_in_points_on_another_scale() {
        let window = source(100, 50, 2400, 1600, false);
        let place = torn_off_place(&torn(window, 2.0, (0.0, 0.0)), screen(3840, 2160, 1.5));
        assert_eq!((place.width, place.height), (1800, 1200));
    }

    /// Bigger than the monitor it lands on: the monitor's size, not more.
    #[test]
    fn a_torn_off_window_is_never_larger_than_its_monitor() {
        let maximized = source(0, 0, 1280, 720, true);
        let place = torn_off_place(&torn(maximized, 1.0, (10.0, 10.0)), screen(1280, 720, 1.0));
        assert_eq!((place.width, place.height), (1024, 720));
    }

    /// A drop near the right edge of a 1920 by 1080 monitor: the window's
    /// 1024 points would run off it, so it is pulled back until it fits.
    #[test]
    fn a_drop_near_a_monitors_edge_pulls_the_window_back_onto_it() {
        let window = source(1200, 100, 800, 600, false);
        let place = torn_off_place(&torn(window, 1.0, (700.0, 40.0)), screen(1920, 1080, 1.0));
        assert_eq!(place.width, 800);
        assert_eq!(place.x, 1920 - 800, "the right edge is on the monitor's");
        assert_eq!(place.y, 140, "its top is where the origin put it");
        // And near the bottom: the same, for a window that would run off it.
        let tall = source(100, 900, 800, 600, false);
        let place = torn_off_place(&torn(tall, 1.0, (50.0, 300.0)), screen(1920, 1080, 1.0));
        assert_eq!(place.height, 600);
        assert_eq!(place.y, 1080 - 600);
    }

    /// The outer corner is placed so that the content corner is where the
    /// origin says, on a system frame of 8 pixels left and 31 on top.
    #[test]
    fn a_system_frame_is_taken_off_the_content_corner() {
        let window = source(100, 50, 800, 600, false);
        let framed = TornOff {
            frame: (8, 31),
            ..torn(window, 1.0, (300.0, 20.0))
        };
        let place = torn_off_place(&framed, screen(3840, 2160, 1.0));
        assert_eq!(
            (place.x + 8, place.y + 31),
            (408, 101),
            "the content corner is 300 and 20 points from the source's"
        );
        assert_eq!((place.x, place.y), (400, 70));
    }

    /// On a monitor of another scale the frame is measured in that
    /// monitor's pixels, and the content corner still lands where it should.
    #[test]
    fn a_system_frame_is_taken_off_in_the_monitors_pixels() {
        let window = source(100, 50, 800, 600, false);
        let framed = TornOff {
            frame: (8, 31),
            ..torn(window, 1.0, (300.0, 20.0))
        };
        let place = torn_off_place(&framed, screen(3840, 2160, 2.0));
        // The content corner is at (408, 101) in the source's pixels; the
        // frame is 16 by 62 of this monitor's, so the outer corner is
        // (408 - 16, 101 - 62).
        assert_eq!((place.x, place.y), (392, 39));
    }
}
