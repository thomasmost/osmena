//! A [`Surface`] through glutin, for engines that paint into memory and
//! bring no GL context of their own. egui paints the chrome with it, and
//! the shell uploads page frames to textures on it.

use std::ffi::CString;
use std::num::NonZeroU32;
use std::sync::Arc;

use dpi::PhysicalSize;
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentGlContext, PossiblyCurrentContext,
    PossiblyCurrentGlContext, Version,
};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::surface::{
    GlSurface as _, Surface as GlutinWindowSurface, SurfaceAttributesBuilder, WindowSurface,
};
use raw_window_handle::{DisplayHandle, WindowHandle};

use crate::{EngineError, Surface};

pub struct GlutinSurface {
    gl: Arc<glow::Context>,
    surface: GlutinWindowSurface<WindowSurface>,
    context: PossiblyCurrentContext,
    // Dropped last: the surface and context belong to it.
    _display: Display,
    size: std::cell::Cell<PhysicalSize<u32>>,
}

impl GlutinSurface {
    /// A double-buffered RGBA8 surface on `window` with a desktop GL 3.3
    /// core context.
    pub fn new(
        display: DisplayHandle<'_>,
        window: WindowHandle<'_>,
        size: PhysicalSize<u32>,
    ) -> Result<GlutinSurface, EngineError> {
        let display = display.as_raw();
        let window = window.as_raw();
        #[cfg(target_os = "windows")]
        let preference = DisplayApiPreference::Wgl(Some(window));
        #[cfg(target_os = "macos")]
        let preference = DisplayApiPreference::Cgl;
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let preference = DisplayApiPreference::Egl;
        // SAFETY: the handles come from a live winit window and event loop.
        let display = unsafe { Display::new(display, preference) }
            .map_err(|e| EngineError(format!("creating the GL display: {e}")))?;
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .compatible_with_native_window(window)
            .build();
        // SAFETY: as above.
        let config = unsafe { display.find_configs(template) }
            .map_err(|e| EngineError(format!("finding a GL config: {e}")))?
            .reduce(|best, candidate| {
                // No multisampling: the chrome is 2D, and the page is a
                // texture.
                if candidate.num_samples() < best.num_samples() {
                    candidate
                } else {
                    best
                }
            })
            .ok_or_else(|| EngineError("no GL config matches the window".to_string()))?;
        let attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .with_profile(GlProfile::Core)
            .build(Some(window));
        // SAFETY: as above.
        let context = unsafe { display.create_context(&config, &attributes) }
            .map_err(|e| EngineError(format!("creating the GL context: {e}")))?;
        let (width, height) = non_zero(size);
        let surface_attributes =
            SurfaceAttributesBuilder::<WindowSurface>::new().build(window, width, height);
        // SAFETY: as above.
        let surface = unsafe { display.create_window_surface(&config, &surface_attributes) }
            .map_err(|e| EngineError(format!("creating the GL surface: {e}")))?;
        let context = context
            .make_current(&surface)
            .map_err(|e| EngineError(format!("making the GL context current: {e}")))?;
        // SAFETY: the context is current; the loader resolves GL entry
        // points for it.
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                let name = CString::new(name).expect("GL function names have no NUL");
                display.get_proc_address(&name).cast()
            })
        };
        Ok(GlutinSurface {
            gl: Arc::new(gl),
            surface,
            context,
            _display: display,
            size: std::cell::Cell::new(size),
        })
    }
}

impl Surface for GlutinSurface {
    fn make_current(&self) -> Result<(), EngineError> {
        self.context
            .make_current(&self.surface)
            .map_err(|e| EngineError(format!("making the GL context current: {e}")))
    }

    fn resize(&self, size: PhysicalSize<u32>) {
        let (width, height) = non_zero(size);
        self.surface.resize(&self.context, width, height);
        self.size.set(size);
    }

    fn gl(&self) -> Arc<glow::Context> {
        self.gl.clone()
    }

    fn begin_frame(&self) {
        if let Err(e) = self.make_current() {
            log::error!("{e}");
        }
        use glow::HasContext;
        // SAFETY: the context is current.
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            let size = self.size.get();
            self.gl
                .viewport(0, 0, size.width as i32, size.height as i32);
        }
    }

    fn present(&self) {
        if let Err(e) = self.surface.swap_buffers(&self.context) {
            log::error!("presenting the GL surface: {e}");
        }
    }
}

fn non_zero(size: PhysicalSize<u32>) -> (NonZeroU32, NonZeroU32) {
    (
        NonZeroU32::new(size.width.max(1)).expect("non-zero"),
        NonZeroU32::new(size.height.max(1)).expect("non-zero"),
    )
}
