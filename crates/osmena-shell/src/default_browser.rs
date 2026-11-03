//! The system's default browser, and the web links and pages the system
//! hands this one to open. macOS only, so far; elsewhere the status is
//! `Unsupported` and nothing is installed.
//!
//! - [`status`]: whether this browser opens the system's `http` links.
//!   Supported only when running from an app bundle whose `Info.plist`
//!   claims the `http` scheme (`CFBundleURLTypes`), which is what makes
//!   it a browser the system can choose at all.
//! - [`make_default`]: ask the system to make it so. macOS asks the user
//!   to confirm ("Do you want to change your default web browser…") and
//!   calls back either way.
//! - [`install_open_handler`]: the URLs the system asks this process to
//!   open, as Apple Events: `GURL` for a link clicked in another app
//!   (when this is the default browser), `odoc` for a page opened with
//!   it from the Finder. winit's application delegate implements
//!   neither, so the shell installs Apple Event handlers itself.

use osmena_core::DefaultBrowser;
use url::Url;

/// Whether this browser is the system's default for web links.
pub fn status() -> DefaultBrowser {
    #[cfg(target_os = "macos")]
    {
        mac::status()
    }
    #[cfg(not(target_os = "macos"))]
    {
        DefaultBrowser::Unsupported
    }
}

/// Ask the system to make this browser the default for web links; `done`
/// runs when the system has answered (on any thread), and the caller
/// reads [`status`] again then.
pub fn make_default(done: impl Fn() + Send + Sync + 'static) {
    #[cfg(target_os = "macos")]
    mac::make_default(done);
    #[cfg(not(target_os = "macos"))]
    {
        log::warn!("making this browser the default is not supported on this system");
        done();
    }
}

/// Hand the URLs the system asks this process to open to `open`, on the
/// main thread, from now on. Call once, before the event loop runs (the
/// link that launched the browser arrives during its launch).
pub fn install_open_handler(open: impl Fn(Vec<Url>) + Send + Sync + 'static) {
    #[cfg(target_os = "macos")]
    mac::install_open_handler(open);
    #[cfg(not(target_os = "macos"))]
    let _ = open;
}

/// The web URLs among what an open request named: `http`, `https` and
/// `file` URLs, parsed; anything else (another scheme, text that is not
/// a URL) is dropped with a log line.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn openable(texts: impl IntoIterator<Item = String>) -> Vec<Url> {
    texts
        .into_iter()
        .filter_map(|text| match Url::parse(text.trim()) {
            Ok(url) if matches!(url.scheme(), "http" | "https" | "file") => Some(url),
            Ok(url) => {
                log::warn!("not opening {url}: not a web or file URL");
                None
            }
            Err(e) => {
                log::warn!("not opening {text:?}: {e}");
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::os::raw::c_long;
    use std::sync::OnceLock;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSError, NSString, NSURL, ns_string};
    use osmena_core::DefaultBrowser;
    use url::Url;

    pub(super) fn status() -> DefaultBrowser {
        let bundle = NSBundle::mainBundle();
        let Some(own) = bundle.bundleIdentifier() else {
            return DefaultBrowser::Unsupported;
        };
        if !claims_http(&bundle) {
            return DefaultBrowser::Unsupported;
        }
        match handler_of("http") {
            Some(handler) if handler.eq_ignore_ascii_case(&own.to_string()) => DefaultBrowser::Yes,
            _ => DefaultBrowser::No,
        }
    }

    /// The bundle identifier of the application that opens `scheme`
    /// URLs, if any does.
    fn handler_of(scheme: &str) -> Option<String> {
        let probe = NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://example.com/")))?;
        let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&probe)?;
        let bundle = NSBundle::bundleWithURL(&app)?;
        Some(bundle.bundleIdentifier()?.to_string())
    }

    /// Whether the bundle's `Info.plist` lists `http` among the schemes
    /// of its `CFBundleURLTypes`.
    fn claims_http(bundle: &NSBundle) -> bool {
        let Some(types) = bundle.objectForInfoDictionaryKey(ns_string!("CFBundleURLTypes")) else {
            return false;
        };
        let Ok(types) = types.downcast::<NSArray>() else {
            return false;
        };
        types.iter().any(|entry| {
            let Some(entry) = entry.downcast_ref::<NSDictionary>() else {
                return false;
            };
            // SAFETY: `objectForKey:` of a dictionary, with a string key;
            // it returns the value or nil.
            let schemes: Option<Retained<AnyObject>> =
                unsafe { msg_send![entry, objectForKey: ns_string!("CFBundleURLSchemes")] };
            schemes
                .and_then(|schemes| schemes.downcast::<NSArray>().ok())
                .is_some_and(|schemes| {
                    schemes.iter().any(|scheme| {
                        scheme
                            .downcast_ref::<NSString>()
                            .is_some_and(|s| s.to_string().eq_ignore_ascii_case("http"))
                    })
                })
        })
    }

    pub(super) fn make_default(done: impl Fn() + Send + Sync + 'static) {
        let bundle = NSBundle::mainBundle();
        let app = bundle.bundleURL();
        let own = bundle.bundleIdentifier().map(|id| id.to_string());
        let done = std::sync::Arc::new(done);
        // The `http` scheme is the one that makes a browser the system's
        // default (macOS asks the user here); `https` normally follows
        // it, and is asked for on its own only when it did not.
        let after_http = {
            let app = app.clone();
            let done = done.clone();
            move |error: *mut NSError| {
                report("http", error);
                let https_too = own
                    .as_deref()
                    .is_some_and(|own| handler_of("https").as_deref() != Some(own))
                    && handler_of("http").as_deref() == own.as_deref();
                if https_too {
                    let done = done.clone();
                    set_handler(&app, "https", move |error| {
                        report("https", error);
                        done();
                    });
                } else {
                    done();
                }
            }
        };
        set_handler(&app, "http", after_http);
    }

    fn set_handler(app: &NSURL, scheme: &str, then: impl Fn(*mut NSError) + 'static) {
        let block = block2::RcBlock::new(then);
        NSWorkspace::sharedWorkspace()
            .setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
                app,
                &NSString::from_str(scheme),
                Some(&block),
            );
    }

    fn report(scheme: &str, error: *mut NSError) {
        // SAFETY: the completion handler's error is nil or a live NSError
        // for the duration of the call.
        match unsafe { error.as_ref() } {
            None => log::info!("asked the system to open {scheme} links here"),
            Some(error) => log::info!(
                "the system did not make this the {scheme} browser: {}",
                error.localizedDescription()
            ),
        }
    }

    // Apple Events, through the Apple Event Manager's C interface
    // (CoreServices). The handler gets the event, a reply, and the
    // reference constant it was installed with.
    #[repr(C)]
    struct AEDesc {
        descriptor_type: u32,
        data_handle: *mut c_void,
    }

    impl AEDesc {
        const NULL: AEDesc = AEDesc {
            descriptor_type: TYPE_NULL,
            data_handle: std::ptr::null_mut(),
        };
    }

    type OSErr = i16;
    type EventHandler = extern "C" fn(*const AEDesc, *mut AEDesc, *mut c_void) -> OSErr;

    const fn four_cc(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }
    const INTERNET_EVENT_CLASS: u32 = four_cc(b"GURL");
    const GET_URL: u32 = four_cc(b"GURL");
    const CORE_EVENT_CLASS: u32 = four_cc(b"aevt");
    const OPEN_DOCUMENTS: u32 = four_cc(b"odoc");
    const DIRECT_OBJECT: u32 = four_cc(b"----");
    const TYPE_UTF8_TEXT: u32 = four_cc(b"utf8");
    const TYPE_FILE_URL: u32 = four_cc(b"furl");
    const TYPE_AE_LIST: u32 = four_cc(b"list");
    const TYPE_NULL: u32 = four_cc(b"null");
    const NO_ERR: OSErr = 0;

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn AEInstallEventHandler(
            class: u32,
            id: u32,
            handler: EventHandler,
            refcon: *mut c_void,
            is_sys_handler: u8,
        ) -> OSErr;
        fn AEGetParamDesc(
            event: *const AEDesc,
            keyword: u32,
            desired_type: u32,
            result: *mut AEDesc,
        ) -> OSErr;
        fn AECountItems(list: *const AEDesc, count: *mut c_long) -> OSErr;
        fn AEGetNthDesc(
            list: *const AEDesc,
            index: c_long,
            desired_type: u32,
            keyword: *mut u32,
            result: *mut AEDesc,
        ) -> OSErr;
        fn AEGetDescDataSize(desc: *const AEDesc) -> c_long;
        fn AEGetDescData(desc: *const AEDesc, data: *mut c_void, max: c_long) -> OSErr;
        fn AEDisposeDesc(desc: *mut AEDesc) -> OSErr;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFNotificationCenterGetLocalCenter() -> *mut c_void;
        fn CFNotificationCenterAddObserver(
            center: *mut c_void,
            observer: *const c_void,
            callback: extern "C" fn(
                *mut c_void,
                *mut c_void,
                *const c_void,
                *const c_void,
                *const c_void,
            ),
            name: *const c_void,
            object: *const c_void,
            behavior: c_long,
        );
    }
    /// `CFNotificationSuspensionBehaviorDeliverImmediately`.
    const DELIVER_IMMEDIATELY: c_long = 4;

    type Open = Box<dyn Fn(Vec<Url>) + Send + Sync>;
    static OPEN: OnceLock<Open> = OnceLock::new();

    pub(super) fn install_open_handler(open: impl Fn(Vec<Url>) + Send + Sync + 'static) {
        if OPEN.set(Box::new(open)).is_err() {
            log::warn!("the open-URL handler is installed already");
            return;
        }
        install_handlers();
        // AppKit installs its own `odoc` handler while it finishes
        // launching, over one installed before; one installed when it
        // posts "will finish launching" stays, and still comes before the
        // open requests the launch carries (Apple's advice for
        // `applicationWillFinishLaunching:`, whose notification this is:
        // winit owns the delegate).
        let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
        // SAFETY: the local center is the process's default notification
        // center; the name is a live string (toll-free bridged to
        // CFString), kept alive for the process; the callback has the
        // signature the center calls.
        unsafe {
            CFNotificationCenterAddObserver(
                CFNotificationCenterGetLocalCenter(),
                std::ptr::null(),
                will_finish_launching,
                Retained::into_raw(name).cast(),
                std::ptr::null(),
                DELIVER_IMMEDIATELY,
            );
        }
    }

    extern "C" fn will_finish_launching(
        _center: *mut c_void,
        _observer: *mut c_void,
        _name: *const c_void,
        _object: *const c_void,
        _user_info: *const c_void,
    ) {
        install_handlers();
    }

    fn install_handlers() {
        for (class, id, handler) in [
            (INTERNET_EVENT_CLASS, GET_URL, get_url as EventHandler),
            (
                CORE_EVENT_CLASS,
                OPEN_DOCUMENTS,
                open_documents as EventHandler,
            ),
        ] {
            // SAFETY: a handler with the signature the Apple Event Manager
            // calls, for the process's lifetime; no reference constant.
            let err = unsafe { AEInstallEventHandler(class, id, handler, std::ptr::null_mut(), 0) };
            if err != NO_ERR {
                log::warn!("cannot install the Apple Event handler {class:#x}/{id:#x}: {err}");
            }
        }
    }

    /// A link to open: the direct object is the URL as text.
    extern "C" fn get_url(event: *const AEDesc, _reply: *mut AEDesc, _: *mut c_void) -> OSErr {
        let mut desc = AEDesc::NULL;
        // SAFETY: `event` is the live event the manager passed; `desc` is
        // ours to fill and dispose of.
        let text = unsafe {
            if AEGetParamDesc(event, DIRECT_OBJECT, TYPE_UTF8_TEXT, &mut desc) != NO_ERR {
                return NO_ERR;
            }
            let text = desc_text(&desc);
            AEDisposeDesc(&mut desc);
            text
        };
        deliver(text.into_iter().collect());
        NO_ERR
    }

    /// Files to open: the direct object is a list of file references,
    /// each coerced to its `file://` URL.
    extern "C" fn open_documents(
        event: *const AEDesc,
        _reply: *mut AEDesc,
        _: *mut c_void,
    ) -> OSErr {
        let mut list = AEDesc::NULL;
        let mut urls = Vec::new();
        // SAFETY: as in `get_url`; each item descriptor is disposed of
        // after reading.
        unsafe {
            if AEGetParamDesc(event, DIRECT_OBJECT, TYPE_AE_LIST, &mut list) != NO_ERR {
                return NO_ERR;
            }
            let mut count: c_long = 0;
            if AECountItems(&list, &mut count) == NO_ERR {
                for index in 1..=count {
                    let mut item = AEDesc::NULL;
                    let mut keyword = 0;
                    if AEGetNthDesc(&list, index, TYPE_FILE_URL, &mut keyword, &mut item) == NO_ERR
                    {
                        urls.extend(desc_text(&item));
                        AEDisposeDesc(&mut item);
                    }
                }
            }
            AEDisposeDesc(&mut list);
        }
        deliver(urls);
        NO_ERR
    }

    /// A descriptor's data as UTF-8 text.
    ///
    /// # Safety
    /// `desc` must be a live descriptor.
    unsafe fn desc_text(desc: &AEDesc) -> Option<String> {
        // SAFETY: the caller's; the buffer is as large as the manager
        // says the data is.
        unsafe {
            let size = AEGetDescDataSize(desc);
            let mut data = vec![0u8; usize::try_from(size).ok()?];
            if AEGetDescData(desc, data.as_mut_ptr().cast(), size) != NO_ERR {
                return None;
            }
            String::from_utf8(data).ok()
        }
    }

    fn deliver(texts: Vec<String>) {
        let urls = super::openable(texts);
        if urls.is_empty() {
            return;
        }
        log::info!(
            "the system asks to open {}",
            urls.iter().map(Url::as_str).collect::<Vec<_>>().join(", ")
        );
        if let Some(open) = OPEN.get() {
            open(urls);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_and_file_urls_are_opened() {
        let urls = openable([
            "https://example.com/a".to_string(),
            " http://example.com/b\n".to_string(),
            "file:///Users/me/page.html".to_string(),
            "mailto:someone@example.com".to_string(),
            "not a url".to_string(),
        ]);
        let urls: Vec<&str> = urls.iter().map(Url::as_str).collect();
        assert_eq!(
            urls,
            [
                "https://example.com/a",
                "http://example.com/b",
                "file:///Users/me/page.html"
            ]
        );
    }
}
