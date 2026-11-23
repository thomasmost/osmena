//! The chrome's own glyphs: the toolbar's arrows, the star, the puzzle
//! piece, the close cross, a page's stand-in for an icon.
//!
//! They are drawings, not strokes worked out in code. Each is an SVG
//! under `assets/icons`, on a 16-unit grid, and `tools/glyphs/build.py`
//! compiles them into `assets/fonts/OsmenaIcons.ttf`, one glyph per
//! [`Icon`] at the code point [`Icon::code`] gives it. Drawn at
//! [`SIZE`] points a grid unit is a pixel on an unscaled display, so
//! the lines the drawings put on the grid are sharp there; at any
//! other scale the text renderer rasterises the outline afresh, which
//! no texture stretched over a quad would be.
//!
//! What moves (the loading arc, the downloads' ring, the recording
//! mark) and what follows the system's own look (the window controls)
//! is still painted in `chrome.rs`.

use egui::emath::GuiRounding;
use egui::{Color32, FontFamily, FontId, Painter, Pos2, Shape, Vec2};

use crate::theme;

/// The side of the box a glyph is drawn for, in points: the size at
/// which a grid unit of its drawing is a point.
pub(crate) const SIZE: f32 = 16.0;

/// A glyph of the chrome's icon font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Icon {
    /// The toolbar's Back and Forward: an arrow with a shaft.
    Back,
    Forward,
    /// An arc turning clockwise into an arrowhead.
    Reload,
    /// The cross that stands where Reload does while a page loads.
    Stop,
    /// The strip's "new tab".
    Plus,
    /// Three lines: the window menu.
    Menu,
    /// The bookmark star, in outline (not bookmarked) and solid.
    Star,
    StarFilled,
    /// A puzzle piece in outline: the extensions menu.
    Puzzle,
    /// An arrow into a tray: the downloads.
    Download,
    /// The small cross that closes a tab, a bar or a panel.
    Close,
    /// A page with no icon of its own.
    Globe,
    /// A magnifier: the Launch Panel's field while it searches.
    Search,
    /// Two four-pointed stars: the same field while it asks.
    Sparkles,
    /// A cog: the Launch Settings.
    Gear,
    /// The find bar's "previous" and "next".
    ChevronUp,
    ChevronDown,
}

impl Icon {
    /// Every glyph, in the order of their code points.
    #[cfg(test)]
    pub(crate) const ALL: [Icon; 17] = [
        Icon::Back,
        Icon::Forward,
        Icon::Reload,
        Icon::Stop,
        Icon::Plus,
        Icon::Menu,
        Icon::Star,
        Icon::StarFilled,
        Icon::Puzzle,
        Icon::Download,
        Icon::Close,
        Icon::Globe,
        Icon::Search,
        Icon::Sparkles,
        Icon::Gear,
        Icon::ChevronUp,
        Icon::ChevronDown,
    ];

    /// Where the glyph sits in the icon font: a code point of the
    /// Private Use Area, which no other font of the chrome draws. The
    /// font's builder reads these lines, so a new glyph is a variant, a
    /// line here, and a drawing named after the variant.
    pub(crate) fn code(self) -> char {
        match self {
            Icon::Back => '\u{E000}',
            Icon::Forward => '\u{E001}',
            Icon::Reload => '\u{E002}',
            Icon::Stop => '\u{E003}',
            Icon::Plus => '\u{E004}',
            Icon::Menu => '\u{E005}',
            Icon::Star => '\u{E006}',
            Icon::StarFilled => '\u{E007}',
            Icon::Puzzle => '\u{E008}',
            Icon::Download => '\u{E009}',
            Icon::Close => '\u{E00A}',
            Icon::Globe => '\u{E00B}',
            Icon::Search => '\u{E00C}',
            Icon::Sparkles => '\u{E00D}',
            Icon::Gear => '\u{E00E}',
            Icon::ChevronUp => '\u{E00F}',
            Icon::ChevronDown => '\u{E010}',
        }
    }
}

/// `icon` in `ink` as a shape, in a box of `size` points centred on
/// `center`. The box is moved onto the pixel grid first: a drawing's
/// lines are sharp only when its grid falls on the screen's.
///
/// A drawing with a line through its middle (the plus, an arrow's
/// shaft) keeps that line on a row of pixels, which in a box of an
/// even number of them is half a pixel right of and under the centre.
/// Nothing here corrects for it: the half pixel does not show in a
/// button, and a stroke split over two rows does.
pub(crate) fn shape(painter: &Painter, icon: Icon, center: Pos2, size: f32, ink: Color32) -> Shape {
    let corner = (center - Vec2::splat(size / 2.0)).round_to_pixels(painter.pixels_per_point());
    let font = FontId::new(size, FontFamily::Name(theme::ICONS.into()));
    let galley = painter.layout_no_wrap(icon.code().to_string(), font, ink);
    Shape::galley(corner, galley, ink)
}

/// Paint `icon` (see [`shape`]).
pub(crate) fn paint(painter: &Painter, icon: Icon, center: Pos2, size: f32, ink: Color32) {
    painter.add(shape(painter, icon, center, size, ink));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster;

    const GROUND: Color32 = Color32::WHITE;
    const INK: Color32 = Color32::BLACK;
    /// The side of the picture a glyph is rendered into, in pixels;
    /// its box is the sixteen in the middle, from `EDGE`.
    const SIDE: usize = 24;
    const EDGE: usize = 4;

    /// How much of each pixel `icon` inks, drawn at its natural size in
    /// the middle of the picture: `[row][column]`.
    fn ink_of(icon: Icon) -> Vec<Vec<f32>> {
        let middle = Pos2::new(SIDE as f32 / 2.0, SIDE as f32 / 2.0);
        let picture = raster::render(1.0, Vec2::splat(SIDE as f32), false, GROUND, |ui| {
            paint(ui.painter(), icon, middle, SIZE, INK);
        });
        (0..SIDE)
            .map(|y| {
                (0..SIDE)
                    .map(|x| picture.coverage(x, y, GROUND, INK))
                    .collect()
            })
            .collect()
    }

    /// The ink at a point of the drawing's own grid.
    fn at(ink: &[Vec<f32>], x: f32, y: f32) -> f32 {
        ink[EDGE + y.floor() as usize][EDGE + x.floor() as usize]
    }

    #[test]
    fn every_icon_is_drawn_inside_its_box_and_about_its_middle() {
        for icon in Icon::ALL {
            let ink = ink_of(icon);
            let mut total = 0.0;
            let (mut left, mut right, mut top, mut bottom) = (SIDE, 0, SIDE, 0);
            for (y, row) in ink.iter().enumerate() {
                for (x, covered) in row.iter().enumerate() {
                    total += covered;
                    if *covered > 0.05 {
                        left = left.min(x);
                        right = right.max(x + 1);
                        top = top.min(y);
                        bottom = bottom.max(y + 1);
                    }
                }
            }
            assert!(total > 12.0, "{icon:?} is hardly drawn ({total} pixels)");
            assert!(
                left >= EDGE && top >= EDGE && right <= SIDE - EDGE && bottom <= SIDE - EDGE,
                "{icon:?} leaves its box: {left}..{right} by {top}..{bottom}"
            );
            // A glyph is ten pixels or more each way, or it is too
            // small beside the others; and its middle is the box's, to
            // within the pixel a line on a row of pixels costs.
            assert!(right - left >= 9 && bottom - top >= 6, "{icon:?} is small");
            let middle = SIDE as f32 / 2.0;
            let (x, y) = ((left + right) as f32 / 2.0, (top + bottom) as f32 / 2.0);
            assert!(
                (x - middle).abs() <= 1.0 && (y - middle).abs() <= 1.0,
                "{icon:?} is off its middle: ({x}, {y})"
            );
        }
    }

    #[test]
    fn a_line_through_the_middle_of_a_glyph_is_one_row_of_pixels() {
        // The plus: its bar is the ninth row, dark, with a faint row on
        // each side and nothing beyond; the same down its upright.
        let ink = ink_of(Icon::Plus);
        assert!(at(&ink, 5.0, 8.5) > 0.95);
        assert!(at(&ink, 5.0, 7.5) < 0.35 && at(&ink, 5.0, 9.5) < 0.35);
        assert!(at(&ink, 5.0, 6.5) < 0.02 && at(&ink, 5.0, 10.5) < 0.02);
        assert!(at(&ink, 8.5, 5.0) > 0.95);
        assert!(at(&ink, 7.5, 5.0) < 0.35 && at(&ink, 9.5, 5.0) < 0.35);
        // The menu's three lines likewise, four rows apart.
        let ink = ink_of(Icon::Menu);
        for row in [4.5, 8.5, 12.5] {
            assert!(at(&ink, 8.0, row) > 0.95, "row {row}");
            assert!(at(&ink, 8.0, row + 2.0) < 0.02, "under row {row}");
        }
    }

    #[test]
    fn reload_is_an_open_arc_with_a_head_not_a_ring() {
        let ink = ink_of(Icon::Reload);
        // The arc is a circle of six about (8.5, 8.5): inked at nine,
        // six and twelve o'clock...
        assert!(at(&ink, 2.5, 8.5) > 0.9, "nine o'clock");
        assert!(at(&ink, 8.5, 14.5) > 0.9, "six o'clock");
        assert!(at(&ink, 8.5, 2.5) > 0.9, "twelve o'clock");
        // ...and open at three, between the tail and the head.
        assert!(at(&ink, 14.5, 8.5) < 0.05, "three o'clock");
        assert!(at(&ink, 14.5, 7.5) < 0.05, "above three o'clock");
        // The head: its corner and both of its legs.
        assert!(at(&ink, 14.5, 5.5) > 0.9, "the head's corner");
        assert!(at(&ink, 14.5, 3.5) > 0.9, "the upright leg");
        assert!(at(&ink, 12.5, 5.5) > 0.9, "the level leg");
    }

    #[test]
    fn the_star_and_the_puzzle_piece_are_outlines_and_only_the_bookmarked_star_is_solid() {
        // Each drawing's middle: empty in outline, inked when solid.
        assert!(at(&ink_of(Icon::Star), 8.5, 9.5) < 0.02);
        assert!(at(&ink_of(Icon::StarFilled), 8.5, 9.5) > 0.98);
        let puzzle = ink_of(Icon::Puzzle);
        for (x, y) in [(7.5, 10.5), (6.5, 8.5), (9.5, 12.5)] {
            assert!(at(&puzzle, x, y) < 0.02, "({x}, {y}) is inked");
        }
        // Its outline is there: the foot of the piece, a row of pixels.
        assert!(at(&puzzle, 7.5, 14.5) > 0.9);
    }

    #[test]
    fn a_glyph_is_drawn_at_any_scale_and_in_any_size() {
        // Twice the pixels, the same drawing: about four times the ink.
        let middle = Pos2::new(12.0, 12.0);
        let inked = |pixels_per_point: f32, size: f32| -> f32 {
            let picture =
                raster::render(pixels_per_point, Vec2::splat(24.0), false, GROUND, |ui| {
                    paint(ui.painter(), Icon::Globe, middle, size, INK);
                });
            (0..picture.height)
                .flat_map(|y| (0..picture.width).map(move |x| (x, y)))
                .map(|(x, y)| picture.coverage(x, y, GROUND, INK))
                .sum()
        };
        let (one, two) = (inked(1.0, SIZE), inked(2.0, SIZE));
        assert!((two / one - 4.0).abs() < 0.4, "{one} then {two}");
        // A smaller box, a smaller glyph.
        assert!(inked(1.0, 12.0) < one * 0.7);
    }
}
