//! The osmena browser shell: everything between a browser's `main` and
//! its engine. A browser is a [`Brand`], an [`EngineInfo`], an
//! [`Engine`] implementation, and a `main` that parses its command line
//! and calls [`run`].
//!
//! One iteration of the loop, for every winit event:
//! 1. The window the event belongs to handles it: chrome events go to
//!    egui, content events go to the active tab through the engine, key
//!    chords are resolved through the keymap. This yields `Command`s.
//! 2. `dispatch` turns each `Command` into model changes plus
//!    `EngineAction`s, which the app forwards to the engine or handles.
//! 3. The engine spins; its `EngineEvent`s are drained into `apply`, whose
//!    follow-up actions are performed too.
//! 4. Windows that were touched redraw: the chrome, then the active tab
//!    under it, however the engine delivers it.

mod app;
mod clipboard;
mod page;
pub mod process;
mod repaint;
mod settings_window;
mod window;

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use osmena_chrome::chrome::location_input_to_url;
use osmena_chrome::fatal::report_fatal;
use osmena_core::command::open_window;
use osmena_core::profile::DEFAULT_PROFILE_NAME;
use osmena_core::{
    Bookmarks, Brand, Browser, Command, EngineInfo, History, Keymap, Platform, Profile, Settings,
    dispatch,
};
use osmena_engine::{Engine, EngineError, Waker};
use url::Url;
use winit::event_loop::{ControlFlow, EventLoop};

pub use osmena_chrome;
pub use osmena_core;
pub use osmena_engine;

use crate::app::{App, AppEvent};

/// The command-line arguments every osmena browser takes. Flatten it into
/// the browser's own `clap` parser with `#[command(flatten)]`.
#[derive(Args, Debug, Clone)]
pub struct CliArgs {
    /// Profile name; state lives under the per-user data directory.
    #[arg(long, default_value = DEFAULT_PROFILE_NAME)]
    pub profile: String,
    /// URLs to open, one tab each; text that is not a URL is searched.
    /// With none, the homepage opens.
    pub urls: Vec<String>,
    /// Quit this many seconds after the engine started, as Quit would,
    /// logging the open tabs first. For scripted smoke tests of a build
    /// (does the engine start, does a page load, does shutdown finish).
    #[arg(long, hide = true, value_name = "SECONDS")]
    pub exit_after: Option<f64>,
}

/// Builds the engine once the event loop is running: the engine's data
/// directory (the profile's engine subdirectory) and the waker it calls
/// when it has work for the main thread.
pub type StartEngine<E> = Box<dyn FnOnce(&Path, Arc<dyn Waker>) -> Result<E, EngineError>>;

/// What a browser hands [`run`].
pub struct Launch<E: Engine> {
    pub brand: &'static Brand,
    pub engine: &'static EngineInfo,
    pub args: CliArgs,
    pub start_engine: StartEngine<E>,
}

/// Run the browser to completion: resolve the profile, load its settings,
/// build the initial model, run the event loop. Logging is the caller's:
/// some engines install the process logger themselves.
pub fn run<E: Engine>(launch: Launch<E>) -> ExitCode {
    let Launch {
        brand,
        engine,
        args,
        start_engine,
    } = launch;
    let bin = brand.binary;

    let profile = match Profile::resolve(brand, engine, &args.profile) {
        Ok(p) => p,
        Err(e) => {
            report_fatal(brand, &format!("{bin}: {e}"));
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = profile.ensure_dirs() {
        report_fatal(
            brand,
            &format!(
                "{bin}: cannot create profile directory {}: {e}",
                profile.dir.display()
            ),
        );
        return ExitCode::FAILURE;
    }
    let settings = match Settings::load(&profile.settings_path(), brand) {
        Ok(s) => s,
        Err(e) => {
            report_fatal(
                brand,
                &format!("{bin}: {e} (in {})", profile.settings_path().display()),
            );
            return ExitCode::FAILURE;
        }
    };
    let mut urls = Vec::new();
    for arg in &args.urls {
        // A URL opens; other text is searched, as in the address bar.
        match location_input_to_url(arg).or_else(|| settings.search.url_for(arg)) {
            Some(url) => urls.push(url),
            None => {
                report_fatal(brand, &format!("{bin}: not a URL: {arg:?}"));
                return ExitCode::FAILURE;
            }
        }
    }

    let keymap = match Keymap::load(&profile.keymap_path(), Platform::current()) {
        Ok(k) => k,
        Err(e) => {
            report_fatal(
                brand,
                &format!("{bin}: {e} (in {})", profile.keymap_path().display()),
            );
            return ExitCode::FAILURE;
        }
    };
    let bookmarks = match Bookmarks::load(&profile.bookmarks_path()) {
        Ok(b) => b,
        Err(e) => {
            report_fatal(
                brand,
                &format!("{bin}: {e} (in {})", profile.bookmarks_path().display()),
            );
            return ExitCode::FAILURE;
        }
    };
    // A history that cannot be read is not worth refusing to start over:
    // it is logged and starts empty (the file is replaced on the next
    // visit).
    let history = match History::load(&profile.history_path()) {
        Ok(h) => h,
        Err(e) => {
            log::warn!(
                "{e} (in {}); starting with an empty history",
                profile.history_path().display()
            );
            History::default()
        }
    };
    let mut browser = Browser::new(profile, settings);
    browser.bookmarks = bookmarks;
    browser.history = history;
    let first: Url = urls.first().cloned().unwrap_or_else(|| browser.homepage());
    let mut actions = open_window(&mut browser, first);
    let window = browser.windows[0].id;
    for url in urls.into_iter().skip(1) {
        actions.extend(dispatch(
            &mut browser,
            &Command::NewTab {
                window,
                url: Some(url),
            },
        ));
    }

    let mut builder = EventLoop::<AppEvent>::with_user_event();
    // winit's default macOS menu owns Cmd+Q: AppKit runs `terminate:`
    // before the window sees the key. Without the menu, the keymap's Quit
    // command runs the normal shutdown instead.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_default_menu(false);
    }
    let event_loop = match builder.build() {
        Ok(l) => l,
        Err(e) => {
            report_fatal(brand, &format!("{bin}: cannot create event loop: {e}"));
            return ExitCode::FAILURE;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(
        brand,
        engine,
        browser,
        keymap,
        actions,
        start_engine,
        &event_loop,
        args.exit_after.map(std::time::Duration::from_secs_f64),
    );
    match event_loop.run_app(&mut app) {
        Ok(()) if app.started_cleanly() => ExitCode::SUCCESS,
        Ok(()) => ExitCode::FAILURE,
        Err(e) => {
            report_fatal(brand, &format!("{bin}: event loop error: {e}"));
            ExitCode::FAILURE
        }
    }
}
