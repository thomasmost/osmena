//! A page frame uploaded as a GL texture egui can paint, for engines that
//! hand the shell pixels (`PagePaint::Frame`).
//!
//! The upload happens inside egui's UI pass, where the painter is not
//! reachable, so a texture is registered with egui only after the pass
//! that created it; the frame it was created in paints a blank page and
//! asks for a redraw. Every later frame updates the pixels in place.

use glow::HasContext;
use osmena_engine::PixelRect;

pub struct PageTexture {
    texture: glow::Texture,
    /// `None` until [`PageTexture::register`] ran after the UI pass.
    id: Option<egui::TextureId>,
    size: (u32, u32),
}

impl PageTexture {
    /// Upload `bgra` (`width` by `height`, tightly packed) into a new,
    /// unregistered texture.
    pub fn new(gl: &glow::Context, width: u32, height: u32, bgra: &[u8]) -> Option<PageTexture> {
        if !fits(width, height, bgra) {
            return None;
        }
        // SAFETY: the context is current; the slice is `width * height`
        // BGRA pixels, as checked above.
        unsafe {
            let texture = match gl.create_texture() {
                Ok(texture) => texture,
                Err(e) => {
                    log::error!("creating a page texture: {e}");
                    return None;
                }
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            allocate(gl, width, height, bgra);
            gl.bind_texture(glow::TEXTURE_2D, None);
            Some(PageTexture {
                texture,
                id: None,
                size: (width, height),
            })
        }
    }

    /// The egui id to paint with, once registered.
    pub fn id(&self) -> Option<egui::TextureId> {
        self.id
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Replace the pixels, or only the `dirty` parts of them when the
    /// size is unchanged and the engine named some; a size change
    /// reallocates the texture's storage under the same handle, so the
    /// egui id stays valid.
    pub fn update(
        &mut self,
        gl: &glow::Context,
        width: u32,
        height: u32,
        bgra: &[u8],
        dirty: &[PixelRect],
    ) {
        if !fits(width, height, bgra) {
            return;
        }
        // SAFETY: the context is current and the texture is ours; every
        // rectangle is clipped to the frame and its bytes are within
        // `bgra`, as `span` guarantees.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            if (width, height) != self.size {
                allocate(gl, width, height, bgra);
                self.size = (width, height);
            } else if dirty.is_empty() {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    glow::BGRA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(bgra)),
                );
            } else {
                // Rows of the rectangle are `width` pixels apart in the
                // frame; the upload starts at the rectangle's first pixel.
                gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, width as i32);
                for (rect, bytes) in dirty.iter().filter_map(|rect| span(*rect, width, height)) {
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as i32,
                        rect.height as i32,
                        glow::BGRA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&bgra[bytes])),
                    );
                }
                gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    /// Register with egui's painter if not yet done. Returns true if it
    /// just happened, so the caller knows a redraw will show the page.
    pub fn register(&mut self, painter: &mut egui_glow::Painter) -> bool {
        if self.id.is_some() {
            return false;
        }
        self.id = Some(painter.register_native_texture(self.texture));
        true
    }

    /// Release the texture. The painter deletes a registered texture when
    /// it is freed; an unregistered one is deleted here.
    pub fn destroy(self, gl: &glow::Context, painter: &mut egui_glow::Painter) {
        match self.id {
            Some(id) => painter.free_texture(id),
            // SAFETY: the context is current and the texture is ours.
            None => unsafe { gl.delete_texture(self.texture) },
        }
    }
}

fn fits(width: u32, height: u32, bgra: &[u8]) -> bool {
    width > 0 && height > 0 && bgra.len() == width as usize * height as usize * 4
}

/// `rect` clipped to a `width` by `height` frame, with the byte range of
/// the frame's tightly packed BGRA buffer that holds it: from its first
/// pixel to the end of its last row's part. `None` if nothing is left.
fn span(rect: PixelRect, width: u32, height: u32) -> Option<(PixelRect, std::ops::Range<usize>)> {
    let right = rect.x.checked_add(rect.width)?.min(width);
    let bottom = rect.y.checked_add(rect.height)?.min(height);
    if rect.x >= right || rect.y >= bottom {
        return None;
    }
    let clipped = PixelRect {
        x: rect.x,
        y: rect.y,
        width: right - rect.x,
        height: bottom - rect.y,
    };
    let pixel = |x: u32, y: u32| (y as usize * width as usize + x as usize) * 4;
    let start = pixel(clipped.x, clipped.y);
    let end = pixel(right, bottom - 1);
    Some((clipped, start..end))
}

/// Allocate the bound texture's storage and fill it. The context is
/// current and a texture is bound.
unsafe fn allocate(gl: &glow::Context, width: u32, height: u32, bgra: &[u8]) {
    // SAFETY: the caller's.
    unsafe {
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width as i32,
            height as i32,
            0,
            glow::BGRA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(bgra)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{PixelRect, fits, span};

    #[test]
    fn a_frame_fits_only_at_its_exact_size() {
        assert!(fits(2, 2, &[0; 16]));
        assert!(!fits(2, 2, &[0; 15]));
        assert!(!fits(0, 2, &[]));
        assert!(!fits(2, 0, &[]));
    }

    fn rect(x: u32, y: u32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_rectangle_spans_from_its_first_pixel_to_the_end_of_its_last_row() {
        // A 10 by 10 frame: rows are 40 bytes.
        let (clipped, bytes) = span(rect(2, 3, 4, 2), 10, 10).unwrap();
        assert_eq!(clipped, rect(2, 3, 4, 2));
        assert_eq!(bytes, (3 * 10 + 2) * 4..(4 * 10 + 6) * 4);
        // The whole frame is the whole buffer.
        assert_eq!(span(rect(0, 0, 10, 10), 10, 10).unwrap().1, 0..400);
    }

    #[test]
    fn a_rectangle_is_clipped_to_the_frame_or_dropped() {
        let (clipped, bytes) = span(rect(8, 9, 5, 5), 10, 10).unwrap();
        assert_eq!(clipped, rect(8, 9, 2, 1));
        assert_eq!(bytes, (9 * 10 + 8) * 4..400);
        assert_eq!(span(rect(10, 0, 1, 1), 10, 10), None);
        assert_eq!(span(rect(0, 0, 0, 5), 10, 10), None);
        assert_eq!(span(rect(u32::MAX, 0, 2, 2), 10, 10), None);
    }
}
