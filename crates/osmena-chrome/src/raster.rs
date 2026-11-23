//! A software rasteriser for what egui paints, so a test can look at
//! the chrome's pixels with no window and no GPU.
//!
//! [`render`] runs one egui pass, tessellates its shapes and fills the
//! triangles the way `egui_glow`'s shader does: the vertex colour times
//! the texture, in gamma space, laid over what is there with
//! premultiplied alpha. Text and icons come out as the browser draws
//! them, to within the GPU's rounding, at any `pixels_per_point`.
//!
//! Two things use it: tests that hold a glyph to its box and its centre,
//! and the sheets (`sheet.rs`), which are how the icons are judged by
//! eye.

use std::collections::HashMap;
use std::path::Path;

use egui::epaint::{ImageDelta, Primitive, Vertex};
use egui::{
    Color32, Context, ImageData, Pos2, RawInput, Rect, TextureFilter, TextureId, Theme, Ui, Vec2,
    ViewportId,
};

/// A rendered frame: opaque pixels, row by row from the top.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Picture {
    pub width: usize,
    pub height: usize,
    /// Gamma-space colour, each channel from 0 to 1.
    pixels: Vec<[f32; 3]>,
}

impl Picture {
    fn new(width: usize, height: usize, ground: Color32) -> Picture {
        let ground = [ground.r(), ground.g(), ground.b()].map(|c| f32::from(c) / 255.0);
        Picture {
            width,
            height,
            pixels: vec![ground; width * height],
        }
    }

    /// The pixel at (`x`, `y`) as 8-bit red, green and blue.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        self.pixels[y * self.width + x].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    }

    /// How far the pixel at (`x`, `y`) is from `ground`, from 0 (the
    /// ground itself) to 1 (as unlike it as `ink` is): the ink's
    /// coverage of that pixel, when nothing else is painted there.
    pub fn coverage(&self, x: usize, y: usize, ground: Color32, ink: Color32) -> f32 {
        let pixel = self.pixels[y * self.width + x];
        let mut along = 0.0;
        let mut span = 0.0;
        for (channel, (g, i)) in [
            (ground.r(), ink.r()),
            (ground.g(), ink.g()),
            (ground.b(), ink.b()),
        ]
        .into_iter()
        .enumerate()
        {
            let (g, i) = (f32::from(g) / 255.0, f32::from(i) / 255.0);
            along += (pixel[channel] - g) * (i - g);
            span += (i - g) * (i - g);
        }
        if span == 0.0 {
            0.0
        } else {
            (along / span).clamp(0.0, 1.0)
        }
    }

    /// The same picture with every pixel `factor` pixels wide and
    /// high, for looking at single pixels.
    pub fn magnified(&self, factor: usize) -> Picture {
        let mut pixels = Vec::with_capacity(self.pixels.len() * factor * factor);
        for y in 0..self.height * factor {
            for x in 0..self.width * factor {
                pixels.push(self.pixels[(y / factor) * self.width + x / factor]);
            }
        }
        Picture {
            width: self.width * factor,
            height: self.height * factor,
            pixels,
        }
    }

    /// The part of the picture inside `rect`, given in pixels.
    pub fn crop(&self, x: usize, y: usize, width: usize, height: usize) -> Picture {
        let mut pixels = Vec::with_capacity(width * height);
        for row in y..y + height {
            let start = row * self.width + x;
            pixels.extend_from_slice(&self.pixels[start..start + width]);
        }
        Picture {
            width,
            height,
            pixels,
        }
    }

    /// Write the picture as a PNG, making the directories on the way.
    pub fn save_png(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the sheet's directory");
        }
        let file = std::fs::File::create(path).expect("the sheet's file");
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            self.width as u32,
            self.height as u32,
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("a PNG header");
        let mut bytes = Vec::with_capacity(self.pixels.len() * 3);
        for y in 0..self.height {
            for x in 0..self.width {
                bytes.extend_from_slice(&self.pixel(x, y));
            }
        }
        writer.write_image_data(&bytes).expect("the PNG's pixels");
    }
}

/// A texture as egui handed it over: premultiplied colour.
struct Texture {
    size: [usize; 2],
    pixels: Vec<Color32>,
    linear: bool,
}

impl Texture {
    /// Lay `delta` over `current`: the whole texture, or a patch of it.
    fn apply(current: Option<Texture>, delta: &ImageDelta) -> Texture {
        let ImageData::Color(image) = &delta.image;
        let linear = delta.options.magnification == TextureFilter::Linear;
        match (delta.pos, current) {
            (Some([left, top]), Some(mut texture)) => {
                for y in 0..image.size[1] {
                    for x in 0..image.size[0] {
                        let at = (top + y) * texture.size[0] + left + x;
                        texture.pixels[at] = image.pixels[y * image.size[0] + x];
                    }
                }
                texture
            }
            _ => Texture {
                size: image.size,
                pixels: image.pixels.clone(),
                linear,
            },
        }
    }

    fn texel(&self, x: isize, y: isize) -> [f32; 4] {
        let x = x.clamp(0, self.size[0] as isize - 1) as usize;
        let y = y.clamp(0, self.size[1] as isize - 1) as usize;
        self.pixels[y * self.size[0] + x]
            .to_array()
            .map(|c| f32::from(c) / 255.0)
    }

    /// The colour at (`u`, `v`), each from 0 to 1 across the texture.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = u * self.size[0] as f32 - 0.5;
        let y = v * self.size[1] as f32 - 0.5;
        if !self.linear {
            return self.texel(x.round() as isize, y.round() as isize);
        }
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let mut out = [0.0; 4];
        for (dx, dy, weight) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let texel = self.texel(x0 + dx, y0 + dy);
            for channel in 0..4 {
                out[channel] += texel[channel] * weight;
            }
        }
        out
    }
}

/// Paint what `add` draws into a picture of `size` points at
/// `pixels_per_point`, over `ground`, in the light or the dark theme,
/// with the chrome's fonts and style installed.
pub(crate) fn render(
    pixels_per_point: f32,
    size: Vec2,
    dark: bool,
    ground: Color32,
    mut add: impl FnMut(&mut Ui),
) -> Picture {
    let ctx = Context::default();
    ctx.set_theme(if dark { Theme::Dark } else { Theme::Light });
    crate::theme::install(&ctx);
    let mut textures: HashMap<TextureId, Texture> = HashMap::new();
    let mut shapes = Vec::new();
    // Three passes a second apart: a widget that measures itself on
    // its first pass (a cached texture, an animated width) has settled
    // by the last, and whatever fades or slides in has arrived.
    for pass in 0..3 {
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(f64::from(pass)),
            ..RawInput::default()
        };
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(pixels_per_point);
        let output = ctx.run_ui(input, |ui| add(ui));
        for (id, delta) in &output.textures_delta.set {
            let texture = Texture::apply(textures.remove(id), delta);
            textures.insert(*id, texture);
        }
        shapes = output.shapes;
    }
    let width = (size.x * pixels_per_point).round() as usize;
    let height = (size.y * pixels_per_point).round() as usize;
    let mut picture = Picture::new(width, height, ground);
    for clipped in ctx.tessellate(shapes, pixels_per_point) {
        let Primitive::Mesh(mesh) = clipped.primitive else {
            continue;
        };
        let Some(texture) = textures.get(&mesh.texture_id) else {
            continue;
        };
        let clip = clipped.clip_rect * pixels_per_point;
        for triangle in mesh.indices.chunks_exact(3) {
            let corners = [
                mesh.vertices[triangle[0] as usize],
                mesh.vertices[triangle[1] as usize],
                mesh.vertices[triangle[2] as usize],
            ];
            fill_triangle(&mut picture, clip, corners, pixels_per_point, texture);
        }
    }
    picture
}

/// Twice the signed area of the triangle `a`, `b`, `p`.
fn edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Whether a pixel centre exactly on the edge from `a` to `b` belongs
/// to the triangle: only on a top or a left edge, so two triangles
/// sharing an edge never both paint a pixel.
fn owns_edge(a: [f64; 2], b: [f64; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    dy < 0.0 || (dy == 0.0 && dx > 0.0)
}

fn fill_triangle(
    picture: &mut Picture,
    clip: Rect,
    mut corners: [Vertex; 3],
    pixels_per_point: f32,
    texture: &Texture,
) {
    let at = |vertex: &Vertex| {
        [
            f64::from(vertex.pos.x * pixels_per_point),
            f64::from(vertex.pos.y * pixels_per_point),
        ]
    };
    let mut area = edge(at(&corners[0]), at(&corners[1]), at(&corners[2]));
    if area == 0.0 {
        return;
    }
    if area < 0.0 {
        corners.swap(1, 2);
        area = -area;
    }
    let [a, b, c] = [at(&corners[0]), at(&corners[1]), at(&corners[2])];
    let colours = corners.map(|vertex| vertex.color.to_array().map(|c| f32::from(c) / 255.0));
    let low = |values: [f64; 3], floor: f32| {
        values
            .into_iter()
            .fold(f64::INFINITY, f64::min)
            .max(f64::from(floor))
            .floor()
            .max(0.0) as usize
    };
    let high = |values: [f64; 3], ceiling: f32, limit: usize| {
        (values
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max)
            .min(f64::from(ceiling))
            .ceil()
            .max(0.0) as usize)
            .min(limit)
    };
    let (left, right) = (
        low([a[0], b[0], c[0]], clip.min.x.round()),
        high([a[0], b[0], c[0]], clip.max.x.round(), picture.width),
    );
    let (top, bottom) = (
        low([a[1], b[1], c[1]], clip.min.y.round()),
        high([a[1], b[1], c[1]], clip.max.y.round(), picture.height),
    );
    let inside = |weight: f64, from: [f64; 2], to: [f64; 2]| {
        weight > 0.0 || (weight == 0.0 && owns_edge(from, to))
    };
    for y in top..bottom {
        for x in left..right {
            let p = [x as f64 + 0.5, y as f64 + 0.5];
            let weights = [edge(b, c, p), edge(c, a, p), edge(a, b, p)];
            if !(inside(weights[0], b, c) && inside(weights[1], c, a) && inside(weights[2], a, b)) {
                continue;
            }
            let weights = weights.map(|w| (w / area) as f32);
            let mut colour = [0.0_f32; 4];
            let mut uv = [0.0_f32; 2];
            for corner in 0..3 {
                for channel in 0..4 {
                    colour[channel] += colours[corner][channel] * weights[corner];
                }
                uv[0] += corners[corner].uv.x * weights[corner];
                uv[1] += corners[corner].uv.y * weights[corner];
            }
            let texel = texture.sample(uv[0], uv[1]);
            let source = [
                colour[0] * texel[0],
                colour[1] * texel[1],
                colour[2] * texel[2],
                colour[3] * texel[3],
            ];
            let pixel = &mut picture.pixels[y * picture.width + x];
            for channel in 0..3 {
                pixel[channel] = source[channel] + pixel[channel] * (1.0 - source[3]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filled_rect_on_the_pixel_grid_covers_exactly_its_pixels() {
        let ground = Color32::WHITE;
        let ink = Color32::BLACK;
        let picture = render(1.0, Vec2::new(8.0, 8.0), false, ground, |ui| {
            ui.painter().rect_filled(
                Rect::from_min_max(Pos2::new(2.0, 3.0), Pos2::new(5.0, 6.0)),
                0.0,
                ink,
            );
        });
        assert_eq!((picture.width, picture.height), (8, 8));
        for y in 0..8 {
            for x in 0..8 {
                let expected = if (2..5).contains(&x) && (3..6).contains(&y) {
                    1.0
                } else {
                    0.0
                };
                let got = picture.coverage(x, y, ground, ink);
                assert!((got - expected).abs() < 0.02, "({x}, {y}): {got}");
            }
        }
    }

    #[test]
    fn two_pixels_per_point_doubles_the_picture() {
        let picture = render(2.0, Vec2::new(8.0, 4.0), false, Color32::WHITE, |ui| {
            ui.painter().rect_filled(
                Rect::from_min_max(Pos2::new(1.0, 1.0), Pos2::new(2.0, 2.0)),
                0.0,
                Color32::BLACK,
            );
        });
        assert_eq!((picture.width, picture.height), (16, 8));
        assert_eq!(picture.pixel(2, 2), [0, 0, 0]);
        assert_eq!(picture.pixel(3, 3), [0, 0, 0]);
        assert_eq!(picture.pixel(4, 4), [255, 255, 255]);
    }

    #[test]
    fn text_is_painted_from_the_font_atlas() {
        let ground = Color32::WHITE;
        let ink = Color32::BLACK;
        let picture = render(1.0, Vec2::new(40.0, 24.0), false, ground, |ui| {
            ui.painter().text(
                Pos2::new(20.0, 12.0),
                egui::Align2::CENTER_CENTER,
                "H",
                egui::FontId::proportional(16.0),
                ink,
            );
        });
        let inked: f32 = (0..picture.height)
            .flat_map(|y| (0..picture.width).map(move |x| (x, y)))
            .map(|(x, y)| picture.coverage(x, y, ground, ink))
            .sum();
        // An "H" at 16 points is two stems and a bar: some dozens of
        // pixels, nowhere near the whole picture.
        assert!((20.0..200.0).contains(&inked), "{inked}");
    }

    #[test]
    fn a_magnified_picture_repeats_each_pixel() {
        let picture = render(1.0, Vec2::new(8.0, 8.0), false, Color32::WHITE, |ui| {
            ui.painter().rect_filled(
                Rect::from_min_max(Pos2::new(2.0, 3.0), Pos2::new(4.0, 5.0)),
                0.0,
                Color32::BLACK,
            );
        });
        let big = picture.magnified(3);
        assert_eq!((big.width, big.height), (24, 24));
        // Each black pixel is nine, and their neighbours stay white.
        assert_eq!(big.pixel(6, 9), [0, 0, 0]);
        assert_eq!(big.pixel(11, 14), [0, 0, 0]);
        assert_eq!(big.pixel(12, 9), [255, 255, 255]);
        assert_eq!(big.pixel(6, 8), [255, 255, 255]);
        let cut = big.crop(6, 9, 6, 6);
        assert_eq!((cut.width, cut.height), (6, 6));
        assert_eq!(cut.pixel(5, 5), [0, 0, 0]);
    }
}
