//! Who a press on the tab strip belongs to, on macOS.
//!
//! A window without the system frame keeps, on macOS, a transparent
//! title bar over the top of its content (`frameless_attributes`), which
//! is where the tab strip is drawn. AppKit moves a window on a press in
//! the title bar when the view under the pointer answers yes to
//! `mouseDownCanMoveWindow`, and `NSView` answers yes for any view that
//! is not opaque. winit's content view overrides neither, so a press on
//! the part of a tab the title bar covers moved the window and the
//! chrome never saw a drag begin.
//!
//! The answer is made "no" for winit's view class: the window is then
//! moved by the strip's own drag area and grip (`WindowRequest::Drag`,
//! `Window::drag_window`), zoomed by a double-click there, and the
//! traffic lights are views of their own, which never asked the content
//! view anything.

/// Whether a window's content view must refuse to move its window on a
/// press: only where the content lies under a title bar that would
/// otherwise take the press, which is a frameless window on macOS. A
/// window with the system's frame has its title bar above the content
/// and is left as it is.
pub fn content_refuses_window_moves(frameless: bool, macos: bool) -> bool {
    frameless && macos
}

/// How far under a window's top edge the probe of [`keep_presses`]
/// looks: inside the title bar's 28 points and inside a tab.
#[cfg(target_os = "macos")]
const PROBE_FROM_TOP: f64 = 22.0;

/// Make a press on `window`'s content the content's own (see the module
/// documentation), and log what a press in the strip did before and
/// does now. Called once per frameless window; adding the method a
/// second time changes nothing.
#[cfg(target_os = "macos")]
pub fn keep_presses(window: &winit::window::Window) {
    use std::ffi::c_char;

    use objc2::ffi;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{msg_send, sel};
    use objc2_foundation::{NSPoint, NSRect};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// `-[NSView mouseDownCanMoveWindow]`, answering no.
    extern "C-unwind" fn never(_this: &AnyObject, _cmd: Sel) -> Bool {
        Bool::NO
    }

    /// Whether a press `PROBE_FROM_TOP` under the top of `view`'s
    /// window, at the middle of its width, would move the window, with
    /// the class of the view the press would land on.
    fn probe(view: &AnyObject) -> Option<(bool, String)> {
        // SAFETY: plain AppKit getters on a live view, on the main
        // thread (windows are only made there); every object returned
        // is nil-checked before use.
        unsafe {
            let ns_window: *mut AnyObject = msg_send![view, window];
            let ns_window = ns_window.as_ref()?;
            let content: *mut AnyObject = msg_send![ns_window, contentView];
            let frame_view: *mut AnyObject = msg_send![content.as_ref()?, superview];
            let frame_view = frame_view.as_ref()?;
            let frame: NSRect = msg_send![frame_view, frame];
            // Window coordinates start at the bottom left.
            let point = NSPoint::new(frame.size.width / 2.0, frame.size.height - PROBE_FROM_TOP);
            let hit: *mut AnyObject = msg_send![frame_view, hitTest: point];
            let hit = hit.as_ref()?;
            let moves: Bool = msg_send![hit, mouseDownCanMoveWindow];
            Some((
                moves.as_bool(),
                hit.class().name().to_string_lossy().into_owned(),
            ))
        }
    }

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: the handle of a live winit window names its content view.
    let view: &AnyObject = unsafe { handle.ns_view.cast::<AnyObject>().as_ref() };
    let before = probe(view);

    let class: &AnyClass = view.class();
    let selector = sel!(mouseDownCanMoveWindow);
    // The inherited method says how the return type is encoded on this
    // architecture ("B" or "c").
    let Some(inherited) = class.instance_method(selector) else {
        log::warn!("the content view has no mouseDownCanMoveWindow; presses left to the system");
        return;
    };
    // SAFETY: `never` has the signature of the method it stands for
    // (`BOOL (id, SEL)`), with the encoding of the inherited one.
    // `class_addMethod` adds an override of a superclass's
    // implementation and leaves a method the class itself defines
    // alone (it then answers NO), so nothing of winit's is replaced and
    // a second call is a no-op. Classes are never deallocated here.
    let added = unsafe {
        let types: *const c_char = ffi::method_getTypeEncoding(std::ptr::from_ref(inherited));
        let imp: Imp =
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(never);
        ffi::class_addMethod(std::ptr::from_ref(class).cast_mut(), selector, imp, types)
    };
    let after = probe(view);
    log::debug!(
        "strip press {PROBE_FROM_TOP} points under the top: lands on {}, could move the window \
         before: {}, can now: {} ({} of {} taught to refuse)",
        after
            .as_ref()
            .map_or("nothing", |(_, class)| class.as_str()),
        before.map_or("unknown".to_string(), |(moves, _)| moves.to_string()),
        after
            .as_ref()
            .map_or("unknown".to_string(), |(moves, _)| moves.to_string()),
        if added.as_bool() {
            "the class"
        } else {
            "an earlier window had the class"
        },
        class.name().to_string_lossy(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_frameless_window_on_macos_refuses_to_move_on_a_press() {
        assert!(content_refuses_window_moves(true, true));
        assert!(
            !content_refuses_window_moves(false, true),
            "the system's title bar is above the content and keeps moving the window"
        );
        assert!(!content_refuses_window_moves(true, false));
        assert!(!content_refuses_window_moves(false, false));
    }
}
