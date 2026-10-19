//! A page frame uploaded as a GL texture egui can paint, for engines that
//! hand the shell pixels (`PagePaint::Frame`).
//!
//! The upload happens inside egui's UI pass, where the painter is not
//! reachable, so a texture is registered with egui only after the pass
//! that created it; the frame it was created in paints a blank page and
//! asks for a redraw. Every later frame updates the pixels in place.

use glow::HasContext;

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

    /// Replace the pixels; a size change reallocates the texture's storage
    /// under the same handle, so the egui id stays valid.
    pub fn update(&mut self, gl: &glow::Context, width: u32, height: u32, bgra: &[u8]) {
        if !fits(width, height, bgra) {
            return;
        }
        // SAFETY: the context is current and the texture is ours.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            if (width, height) == self.size {
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
                allocate(gl, width, height, bgra);
                self.size = (width, height);
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
    use super::fits;

    #[test]
    fn a_frame_fits_only_at_its_exact_size() {
        assert!(fits(2, 2, &[0; 16]));
        assert!(!fits(2, 2, &[0; 15]));
        assert!(!fits(0, 2, &[]));
        assert!(!fits(2, 0, &[]));
    }
}
