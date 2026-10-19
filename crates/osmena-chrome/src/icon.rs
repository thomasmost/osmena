//! The application icon: the window icon (Windows title bar) and, on
//! macOS, the Dock icon, so `cargo run` shows the logo without an `.app`
//! bundle. Both come from the brand's embedded PNG.

use osmena_core::Brand;
use winit::window::Icon;

/// Decode a PNG to straight RGBA8.
fn decode_icon(png_bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), png::DecodingError> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer)?;
    buffer.truncate(info.buffer_size());
    debug_assert_eq!(info.color_type, png::ColorType::Rgba);
    debug_assert_eq!(info.bit_depth, png::BitDepth::Eight);
    Ok((buffer, info.width, info.height))
}

/// The window icon. winit ignores it on macOS, where the Dock icon is
/// set separately.
pub fn window_icon(brand: &Brand) -> Option<Icon> {
    let (rgba, width, height) = match decode_icon(brand.icon_png) {
        Ok(decoded) => decoded,
        Err(e) => {
            log::warn!("cannot decode embedded icon: {e}");
            return None;
        }
    };
    match Icon::from_rgba(rgba, width, height) {
        Ok(icon) => Some(icon),
        Err(e) => {
            log::warn!("cannot build window icon: {e}");
            None
        }
    }
}

/// Set the Dock icon from the brand's PNG. Must run on the main thread
/// after the event loop (and so `NSApplication`) exists.
#[cfg(target_os = "macos")]
pub fn set_dock_icon(brand: &Brand) {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("set_dock_icon called off the main thread; skipping");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let data = NSData::with_bytes(brand.icon_png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        log::warn!("AppKit could not decode the embedded icon");
        return;
    };
    // SAFETY: on the main thread with a valid NSImage; AppKit retains it.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon(_brand: &Brand) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x2 RGBA PNG, encoded here so the test needs no file.
    fn tiny_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 0])
                .unwrap();
        }
        bytes
    }

    #[test]
    fn a_png_decodes_to_straight_rgba() {
        let (rgba, width, height) = decode_icon(&tiny_png()).unwrap();
        assert_eq!((width, height), (2, 2));
        assert_eq!(rgba.len(), 2 * 2 * 4);
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn garbage_is_not_an_icon() {
        assert!(decode_icon(b"not a png").is_err());
    }
}
