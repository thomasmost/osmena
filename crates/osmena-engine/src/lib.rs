//! The contract between a browser engine and the osmena shell.
//!
//! An engine crate (Paladin's Servo glue, Nautilus's CEF glue) implements
//! [`Engine`], [`TabHost`], and [`Surface`]; `osmena-shell` drives them
//! and never sees the engine's own types. Everything that crosses the
//! boundary is expressed in `osmena-core`'s terms: core ids,
//! [`EngineEvent`]s, and [`EngineInput`]. This crate holds only the
//! traits, the two shared error and wake types, and the page-paint
//! handoff; with the `glutin` feature it also provides a [`Surface`]
//! implementation for engines that do not bring a GL context of their own.
//!
//! [`EngineEvent`]: osmena_core::EngineEvent
//! [`EngineInput`]: osmena_core::EngineInput

use std::fmt;
use std::sync::Arc;

use osmena_core::{EngineEvent, EngineInput, ExtensionSource, PageCommand, PendingTab, TabId};
use raw_window_handle::{DisplayHandle, WindowHandle};
use url::Url;

pub use dpi::PhysicalSize;
pub use keyboard_types;
pub use raw_window_handle;

#[cfg(feature = "glutin")]
mod glutin;
#[cfg(feature = "glutin")]
pub use crate::glutin::GlutinSurface;

/// Wakes the application's event loop from any thread. An engine calls
/// this when it has work for the main thread; the shell then calls
/// [`Engine::spin`].
pub use osmena_core::DownloadPolicy;

pub trait Waker: Send + Sync + 'static {
    fn wake(&self);
}

/// An error from the engine, in words meant for the user.
#[derive(Debug)]
pub struct EngineError(pub String);

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EngineError {}

/// The GL context and surface of one native window. The shell's egui
/// paints the chrome through it; an engine that renders with GL (Servo)
/// provides its own so the tabs share the context, and one that paints
/// into memory (CEF) uses [`GlutinSurface`].
pub trait Surface {
    fn make_current(&self) -> Result<(), EngineError>;
    /// Call on every window resize, in physical pixels, never zero.
    fn resize(&self, size: PhysicalSize<u32>);
    /// The glow handle egui renders through.
    fn gl(&self) -> Arc<glow::Context>;
    /// Make current and bind the window's framebuffer before egui paints.
    fn begin_frame(&self);
    /// Swap buffers.
    fn present(&self);
}

/// A target rectangle for a blit, in GL window coordinates (origin at
/// the bottom left, physical pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlitRect {
    pub x: i32,
    pub y_from_bottom: i32,
    pub width: i32,
    pub height: i32,
}

/// Copies a tab's offscreen framebuffer into the window.
pub type BlitFn = Box<dyn Fn(&glow::Context, BlitRect) + Send + Sync>;

/// A rectangle on the screen, in physical pixels from the primary
/// monitor's top-left: where a toolbar button is, for the popup an engine
/// opens below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A rectangle of pixels in a frame, from its top left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A tab's latest pixels, BGRA8 with straight alpha, row-major from the
/// top left, tightly packed.
#[derive(Clone, Copy, Debug)]
pub struct FrameRef<'a> {
    pub width: u32,
    pub height: u32,
    pub bgra: &'a [u8],
    /// True if the pixels changed since the last time they were handed
    /// out, so the shell can skip the upload otherwise.
    pub fresh: bool,
    /// The parts that changed since the last time the frame was handed
    /// out, when the engine knows them, so the shell uploads only those;
    /// empty means all of it. Read only when `fresh`.
    pub dirty: &'a [PixelRect],
}

/// How a tab's content reaches the window, handed to the shell by
/// [`TabHost::paint_page`] once per frame.
pub enum PagePaint<'a> {
    /// The engine painted the tab into GL itself; the shell calls the
    /// function from egui's paint pass to blit it under the toolbar.
    Blit(BlitFn),
    /// The engine paints into memory; the shell uploads the frame to a
    /// texture and draws it under the toolbar.
    Frame(FrameRef<'a>),
}

/// The engine side of one browser window: its tabs. Positions in
/// [`EngineInput`] are physical pixels from the top left of the page area.
pub trait TabHost {
    /// Create a visible tab loading `url`.
    fn create_tab(&mut self, tab: TabId, url: Url);
    /// Take over a tab that page content opened (an
    /// `EngineEvent::NewTabRequested`). Returns false if the engine no
    /// longer holds it.
    fn adopt_tab(&mut self, pending: PendingTab, tab: TabId) -> bool;
    fn close_tab(&mut self, tab: TabId);
    fn has_tab(&self, tab: TabId) -> bool;
    fn navigate(&self, tab: TabId, url: Url);
    fn back(&self, tab: TabId);
    fn forward(&self, tab: TabId);
    fn reload(&self, tab: TabId);
    /// Stop the load in progress. Engines whose `EngineInfo::supports_stop`
    /// is false may do nothing.
    fn stop(&self, tab: TabId);
    /// The tab is the window's visible tab: paint it.
    fn show(&self, tab: TabId);
    /// The tab is behind another: stop painting it.
    fn hide(&self, tab: TabId);
    fn set_focused(&self, tab: TabId, focused: bool);
    /// The page area changed size (physical pixels). Called every frame
    /// before [`TabHost::paint_page`]; the host ignores an unchanged size.
    fn resize_content(&self, tab: TabId, size: PhysicalSize<u32>);
    /// Where the page area sits on screen, in logical pixels, for engines
    /// that place menus and widgets themselves. Default: ignored.
    fn set_content_origin(&self, _x: i32, _y: i32) {}
    fn set_hidpi(&self, scale: f32);
    fn input(&self, tab: TabId, input: EngineInput);
    /// Hand the shell the tab's content for this frame, if there is any
    /// yet. Called with the window's GL context current, from inside the
    /// shell's UI pass, after [`TabHost::resize_content`].
    fn paint_page(&self, tab: TabId, sink: &mut dyn FnMut(PagePaint<'_>));
    /// Open the popup of `extension`'s toolbar action, for `tab`, below
    /// `anchor` (the button, in physical screen pixels). An engine without
    /// extensions ignores it. Default: nothing.
    fn open_extension_action(&self, _tab: TabId, _extension: &str, _anchor: ScreenRect) {}
    /// Close the extension popup open over this window, if any: the user
    /// clicked back into the window, or moved or resized it. Default:
    /// nothing.
    fn close_extension_popups(&self) {}
    /// Do `command` to `tab`'s page, where the user opened its context
    /// menu (the engine reported `EngineEvent::ContextMenuRequested` and
    /// remembers the point). An engine advertises what it can do in
    /// `EngineInfo::page_tools`; the editing commands are expected of
    /// every engine. Default: nothing.
    fn page_command(&self, _tab: TabId, _command: PageCommand) {}
    /// The context menu of `tab`'s page closed, after any command it
    /// produced: let go of what was held for it. Default: nothing.
    fn context_menu_closed(&self, _tab: TabId) {}
    /// Search `tab`'s page for `text`: afresh from the top, or on to the
    /// next match (`find_next`) `forward` or back, wrapping. The engine
    /// highlights the matches, scrolls the active one into view, and
    /// reports `EngineEvent::FindResult`s. Only engines whose
    /// `EngineInfo::find` is true receive it. Default: nothing.
    fn find(&self, _tab: TabId, _text: &str, _forward: bool, _match_case: bool, _find_next: bool) {}
    /// End the search in `tab`: drop the highlights, and the selection of
    /// the active match too when `clear_selection`. Default: nothing.
    fn stop_finding(&self, _tab: TabId, _clear_selection: bool) {}
    /// Zoom `tab`'s page to `percent` (100 is the page's own size). The
    /// model keeps the level per site and asks again after every
    /// navigation to another site. Default: nothing.
    fn set_zoom(&self, _tab: TabId, _percent: u16) {}
    /// Download `url` from `tab`'s page (a retry of a failed download).
    /// Default: nothing.
    fn start_download(&self, _tab: TabId, _url: Url) {}
}

/// One engine instance, one per process. The shell builds it through a
/// closure the browser supplies (engines differ in their options), then
/// spins it from the event loop and drains its events into the model.
pub trait Engine: 'static {
    type Surface: Surface;
    type Host: TabHost;

    /// Run the work the engine has queued for the main thread: delivers
    /// the callbacks that become [`EngineEvent`]s and paints. Called after
    /// every window event and every [`Waker::wake`].
    fn spin(&self);
    /// Take every event produced since the last call.
    fn drain_events(&self) -> Vec<EngineEvent>;
    /// Forget a tab that page content asked for but that no window adopted.
    fn discard_pending(&self, pending: PendingTab);
    /// A GL context and surface on a native window, for the chrome.
    fn open_surface(
        &self,
        display: DisplayHandle<'_>,
        window: WindowHandle<'_>,
        size: PhysicalSize<u32>,
    ) -> Result<Self::Surface, EngineError>;
    /// The tab host of a window drawn on `surface`, at the window's scale
    /// factor.
    fn open_host(
        &self,
        surface: &Self::Surface,
        window: WindowHandle<'_>,
        scale: f32,
    ) -> Result<Self::Host, EngineError>;
    /// Install an extension. The engine reports the outcome itself: an
    /// `EngineEvent::ExtensionsChanged` with the new list, or an
    /// `EngineEvent::EngineError` saying why not. An engine whose
    /// `EngineInfo::extensions` is `None` never receives it. Default:
    /// nothing.
    fn install_extension(&self, _source: ExtensionSource) {}
    /// Enable or disable an installed extension. Default: nothing.
    fn set_extension_enabled(&self, _extension: &str, _enabled: bool) {}
    /// Remove an installed extension. Default: nothing.
    fn uninstall_extension(&self, _extension: &str) {}
    /// Where downloads go and whether to ask for each file: sent once
    /// the engine is up and whenever the setting changes. An engine whose
    /// `EngineInfo::downloads` is false never receives it, nor the three
    /// below. The engine reports every download it runs with
    /// `EngineEvent::DownloadUpdated`, under an id of its own, which the
    /// commands below name. Default: nothing.
    fn configure_downloads(&self, _policy: DownloadPolicy) {}
    fn cancel_download(&self, _id: u32) {}
    fn pause_download(&self, _id: u32) {}
    fn resume_download(&self, _id: u32) {}
}
