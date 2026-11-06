//! Icons fitted to the pixel grid. A favicon arrives at whatever size the
//! site served (16, 32, 64 … pixels, not always square) and is drawn in
//! a slot a few points wide. Handing the GPU a texture of one size for a
//! box of another leaves it to resample on the fly, which blurs a
//! 16-pixel icon the moment its box sits between pixels (an odd tab
//! width is enough) and aliases a large one. So an icon is resampled
//! here, once, to the exact number of physical pixels its slot covers
//! ([`fit`]), and the chrome draws that texture on pixel boundaries
//! ([`snap`]): every texel lands on one pixel.
//!
//! The filter is Lanczos with three lobes, the one Chromium resizes its
//! favicons with, applied to premultiplied colour so a transparent edge
//! does not darken, separably (rows, then columns), and skipped when
//! the sizes already agree.

use egui::emath::GuiRounding;
use egui::{Color32, ColorImage, Rect, Vec2};
use osmena_core::RgbaImage;

/// The side, in physical pixels, of a square icon slot `points` wide at
/// `pixels_per_point`: whole pixels, at least one.
pub(crate) fn icon_px(points: f32, pixels_per_point: f32) -> u32 {
    (points * pixels_per_point).round().max(1.0) as u32
}

/// `slot` moved onto the pixel grid and sized to exactly `px` pixels: the
/// box to draw an icon [`fit`]ted to `px` in, so each texel covers one
/// pixel. The top-left corner moves by less than half a pixel.
pub(crate) fn snap(slot: Rect, px: u32, pixels_per_point: f32) -> Rect {
    let side = px as f32 / pixels_per_point;
    Rect::from_min_size(
        slot.min.round_to_pixels(pixels_per_point),
        Vec2::splat(side),
    )
}

/// `image` resampled to fit a `px`-pixel square, its aspect kept and the
/// rest of the square clear, as the premultiplied pixels egui uploads.
pub(crate) fn fit(image: &RgbaImage, px: u32) -> ColorImage {
    let px = px.max(1) as usize;
    let (w, h) = (image.width as usize, image.height as usize);
    if w == 0 || h == 0 || image.rgba.len() < w * h * 4 {
        return ColorImage::filled([px, px], Color32::TRANSPARENT);
    }
    let (tw, th) = fitted(w, h, px);
    let source = premultiplied(&image.rgba, w * h);
    let rows = resample_rows(&source, w, h, tw);
    let fitted = resample_columns(&rows, tw, h, th);
    let (x0, y0) = ((px - tw) / 2, (px - th) / 2);
    let mut pixels = vec![Color32::TRANSPARENT; px * px];
    for y in 0..th {
        for x in 0..tw {
            pixels[(y0 + y) * px + x0 + x] = to_color(fitted[y * tw + x]);
        }
    }
    ColorImage::new([px, px], pixels)
}

/// The size of a `w` by `h` image fitted in a `px` square: the longer
/// side fills it, the other keeps the aspect (at least one pixel).
fn fitted(w: usize, h: usize, px: usize) -> (usize, usize) {
    let scaled = |short: usize, long: usize| {
        ((short as f32 * px as f32 / long as f32).round() as usize).clamp(1, px)
    };
    if w >= h {
        (px, scaled(h, w))
    } else {
        (scaled(w, h), px)
    }
}

/// Straight RGBA8 as premultiplied colour in 0..1.
fn premultiplied(rgba: &[u8], pixels: usize) -> Vec<[f32; 4]> {
    rgba[..pixels * 4]
        .chunks_exact(4)
        .map(|p| {
            let a = f32::from(p[3]) / 255.0;
            [
                f32::from(p[0]) / 255.0 * a,
                f32::from(p[1]) / 255.0 * a,
                f32::from(p[2]) / 255.0 * a,
                a,
            ]
        })
        .collect()
}

/// Premultiplied colour in 0..1 as egui's premultiplied `Color32`. The
/// filter can overshoot a little at a hard edge: colour is kept within
/// alpha so the pixel stays a valid premultiplied one.
fn to_color(p: [f32; 4]) -> Color32 {
    let a = p[3].clamp(0.0, 1.0);
    let channel = |c: f32| (c.clamp(0.0, a) * 255.0).round() as u8;
    Color32::from_rgba_premultiplied(
        channel(p[0]),
        channel(p[1]),
        channel(p[2]),
        (a * 255.0).round() as u8,
    )
}

/// Every row of a `w` by `h` image resampled to `tw` samples.
fn resample_rows(source: &[[f32; 4]], w: usize, h: usize, tw: usize) -> Vec<[f32; 4]> {
    let taps = weights(w, tw);
    let mut out = vec![[0.0; 4]; tw * h];
    for y in 0..h {
        let row = &source[y * w..(y + 1) * w];
        for (x, (first, weights)) in taps.iter().enumerate() {
            out[y * tw + x] = weighted(weights.iter().zip(&row[*first..]));
        }
    }
    out
}

/// Every column of a `w` by `h` image resampled to `th` samples.
fn resample_columns(source: &[[f32; 4]], w: usize, h: usize, th: usize) -> Vec<[f32; 4]> {
    let taps = weights(h, th);
    let mut out = vec![[0.0; 4]; w * th];
    for x in 0..w {
        for (y, (first, weights)) in taps.iter().enumerate() {
            let column = (0..weights.len()).map(|k| &source[(first + k) * w + x]);
            out[y * w + x] = weighted(weights.iter().zip(column));
        }
    }
    out
}

fn weighted<'a>(taps: impl Iterator<Item = (&'a f32, &'a [f32; 4])>) -> [f32; 4] {
    let mut sum = [0.0; 4];
    for (weight, p) in taps {
        for (s, c) in sum.iter_mut().zip(p) {
            *s += weight * c;
        }
    }
    sum
}

/// For each of `dst` output samples along a line of `src` input samples:
/// the index of the first input it draws on and its weights, which sum
/// to one. A Lanczos-3 kernel, stretched to the input's scale when it is
/// the larger so that no input sample is skipped; at equal sizes it is
/// the identity.
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f32 / dst as f32;
    let footprint = scale.max(1.0);
    let support = 3.0 * footprint;
    (0..dst)
        .map(|i| {
            let centre = (i as f32 + 0.5) * scale;
            let first = (centre - support).floor().max(0.0) as usize;
            let end = (((centre + support).ceil()) as usize).min(src);
            let mut taps: Vec<f32> = (first..end)
                .map(|j| lanczos3((j as f32 + 0.5 - centre) / footprint))
                .collect();
            let sum: f32 = taps.iter().sum();
            if sum.abs() > f32::EPSILON {
                for tap in &mut taps {
                    *tap /= sum;
                }
            }
            (first, taps)
        })
        .collect()
}

/// The Lanczos kernel with three lobes.
fn lanczos3(x: f32) -> f32 {
    use std::f32::consts::PI;
    if x == 0.0 {
        1.0
    } else if x.abs() >= 3.0 {
        0.0
    } else {
        let px = PI * x;
        3.0 * px.sin() * (px / 3.0).sin() / (px * px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn image(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> RgbaImage {
        let pixel = &pixel;
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| pixel(x, y)))
            .collect();
        RgbaImage {
            width: w,
            height: h,
            rgba: Arc::from(rgba),
        }
    }

    #[test]
    fn slots_are_whole_pixels() {
        assert_eq!(icon_px(16.0, 1.0), 16);
        assert_eq!(icon_px(16.0, 1.25), 20);
        assert_eq!(icon_px(16.0, 1.5), 24);
        assert_eq!(icon_px(14.0, 1.25), 18);
        assert_eq!(icon_px(0.1, 1.0), 1);
    }

    #[test]
    fn a_snapped_box_starts_on_a_pixel_and_covers_exactly_its_pixels() {
        let slot = Rect::from_min_size(egui::pos2(10.4, 3.6), Vec2::splat(16.0));
        let at_one = snap(slot, 16, 1.0);
        assert_eq!(at_one.min, egui::pos2(10.0, 4.0));
        assert_eq!(at_one.size(), Vec2::splat(16.0));
        let at_five_quarters = snap(slot, 20, 1.25);
        // 10.4 points is 13 pixels; 3.6 points is 4.5, rounded up to 5.
        assert_eq!(at_five_quarters.min, egui::pos2(13.0 / 1.25, 5.0 / 1.25));
        assert_eq!(at_five_quarters.size(), Vec2::splat(16.0));
    }

    #[test]
    fn an_icon_of_the_slots_size_is_copied_as_is() {
        let source = image(16, 16, |x, y| [(x * 16) as u8, (y * 16) as u8, 77, 255]);
        let fitted = fit(&source, 16);
        assert_eq!(fitted.size, [16, 16]);
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(
                    fitted.pixels[y * 16 + x],
                    Color32::from_rgb((x * 16) as u8, (y * 16) as u8, 77),
                    "pixel {x},{y}"
                );
            }
        }
    }

    #[test]
    fn translucent_pixels_come_through_premultiplied() {
        let source = image(2, 2, |_, _| [200, 100, 0, 128]);
        let fitted = fit(&source, 2);
        for pixel in &fitted.pixels {
            assert_eq!(*pixel, Color32::from_rgba_unmultiplied(200, 100, 0, 128));
        }
    }

    #[test]
    fn shrinking_averages_colour_by_coverage_not_by_channel() {
        // Opaque red beside clear: half-covered red, not dark red.
        let source = image(2, 2, |x, y| {
            if x == y {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let fitted = fit(&source, 1);
        assert_eq!(
            fitted.pixels,
            vec![Color32::from_rgba_premultiplied(128, 0, 0, 128)]
        );
    }

    #[test]
    fn a_flat_icon_shrinks_without_ringing() {
        let source = image(64, 64, |_, _| [10, 20, 250, 255]);
        let fitted = fit(&source, 16);
        assert_eq!(fitted.size, [16, 16]);
        assert!(
            fitted
                .pixels
                .iter()
                .all(|p| *p == Color32::from_rgb(10, 20, 250))
        );
        // And grows the same way.
        let grown = fit(&image(16, 16, |_, _| [10, 20, 250, 255]), 24);
        assert!(
            grown
                .pixels
                .iter()
                .all(|p| *p == Color32::from_rgb(10, 20, 250))
        );
    }

    #[test]
    fn a_wide_icon_is_letterboxed_in_the_square() {
        let source = image(32, 16, |_, _| [0, 0, 0, 255]);
        let square = fit(&source, 16);
        assert_eq!(square.size, [16, 16]);
        for y in 0..16 {
            let row = &square.pixels[y * 16..(y + 1) * 16];
            if (4..12).contains(&y) {
                assert!(row.iter().all(|p| *p == Color32::BLACK), "row {y}");
            } else {
                assert!(row.iter().all(|p| *p == Color32::TRANSPARENT), "row {y}");
            }
        }
        assert_eq!(fitted(16, 32, 16), (8, 16));
        assert_eq!(fitted(100, 1, 16), (16, 1));
    }

    #[test]
    fn a_broken_image_is_a_clear_square() {
        let short = RgbaImage {
            width: 4,
            height: 4,
            rgba: Arc::from(vec![0u8; 8]),
        };
        let fitted = fit(&short, 3);
        assert_eq!(fitted.size, [3, 3]);
        assert!(fitted.pixels.iter().all(|p| *p == Color32::TRANSPARENT));
    }
}
