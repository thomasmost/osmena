//! The system clipboard, for the page context menu's "Copy link address"
//! and "Copy image", and the text a menu bar's Paste hands a text field
//! of the chrome. The page itself handles its own copy and paste through
//! the engine.

use std::borrow::Cow;

use osmena_core::RgbaImage;

pub fn set_text(text: &str) {
    let result = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text));
    if let Err(e) = result {
        log::error!("cannot put text on the clipboard: {e}");
    }
}

/// The clipboard's text, if it holds any.
pub fn get_text() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .ok()
}

pub fn set_image(image: &RgbaImage) {
    let data = arboard::ImageData {
        width: image.width as usize,
        height: image.height as usize,
        bytes: Cow::Borrowed(&image.rgba),
    };
    let result = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_image(data));
    if let Err(e) = result {
        log::error!("cannot put an image on the clipboard: {e}");
    }
}
