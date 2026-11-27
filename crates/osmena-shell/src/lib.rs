//! The osmena browser shell: everything between a browser's `main` and
//! its engine. A browser is a [`Brand`], an [`EngineInfo`], an
//! [`Engine`] implementation, and a `main` that parses its command line
//! and calls [`run`].
//!
//! The process is `shell::Shell`; each open profile is an
//! `app::ProfileApp` with its own model, engine and windows (one of
//! them, or several when the engine runs profiles together).
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

// The state machine is tested everywhere; only Windows installs it.
#[cfg_attr(not(windows), allow(dead_code))]
mod alt_code;
mod app;
mod clipboard;
mod default_browser;
mod favicons;
mod instance;
mod launch;
mod load;
mod locale;
mod media_prompt;
mod page;
mod pinch;
pub mod process;
mod profiles;
mod repaint;
mod settings_window;
mod shell;
mod tab_drop;
mod titlebar;
mod update;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use osmena_chrome::fatal::report_fatal;
use osmena_core::profile::DEFAULT_PROFILE_NAME;
use osmena_core::{Brand, EngineInfo, Profile};
use osmena_engine::{Engine, EngineError, Waker};
use winit::event_loop::{ControlFlow, EventLoop};

pub use osmena_chrome;
pub use osmena_core;
pub use osmena_engine;

use crate::app::AppEvent;
use crate::shell::{Shell, Startup};

/// The command-line arguments every osmena browser takes. Flatten it into
/// the browser's own `clap` parser with `#[command(flatten)]`.
#[derive(Args, Debug, Clone)]
pub struct CliArgs {
    /// Profile name; state lives under the per-user data directory.
    /// Without it the profiles open at the last quit open again, or the
    /// default profile.
    #[arg(long)]
    pub profile: Option<String>,
    /// URLs to open, one tab each; text that is not a URL is searched.
    /// With none, the homepage opens.
    pub urls: Vec<String>,
    /// Quit this many seconds after the engine started, as Quit would,
    /// logging the open tabs first. For scripted smoke tests of a build
    /// (does the engine start, does a page load, does shutdown finish).
    #[arg(long, hide = true, value_name = "SECONDS")]
    pub exit_after: Option<f64>,
    /// Check for updates against this manifest instead of the brand's
    /// (a local server, for trying the update flow). The build still
    /// needs a build number to compare with.
    #[arg(long, hide = true, value_name = "URL")]
    pub update_manifest: Option<String>,
    /// Open the windows and tabs of the last run, whatever the setting
    /// says (the update's restart and a profile handover pass it).
    #[arg(long, hide = true)]
    pub restore: bool,
    /// Keep every profile under this directory instead of the per-user
    /// data directory: a browser apart from the installed one, with its
    /// own profiles and its own process (for scripted runs and for
    /// trying a build beside the one in use).
    #[arg(long, hide = true, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
}

/// Starts an engine for a profile once the event loop is running: the
/// profile (its engine directory, and the one every profile shares) and
/// the waker the engine calls when it has work for the main thread.
/// Called once per process, or, when `EngineInfo::profiles_in_process`
/// is true, once for each profile opened in it, while the engines of the
/// profiles opened before still live. Every engine it made is dropped
/// before the closure is, inside the event loop: what the engines share
/// (a process-wide runtime) belongs in the closure and ends with it.
pub type StartEngine<E> = Box<dyn FnMut(&Profile, Arc<dyn Waker>) -> Result<E, EngineError>>;

/// What a browser hands [`run`].
pub struct Launch<E: Engine> {
    pub brand: &'static Brand,
    pub engine: &'static EngineInfo,
    pub args: CliArgs,
    pub start_engine: StartEngine<E>,
}

/// Run the browser to completion: make sure this is the one browser
/// process (or hand the command line to it), resolve the profile, load
/// its settings, build the initial model, run the event loop. Logging is
/// the caller's: some engines install the process logger themselves.
pub fn run<E: Engine>(launch: Launch<E>) -> ExitCode {
    let Launch {
        brand,
        engine,
        args,
        start_engine,
    } = launch;
    let bin = brand.binary;
    let fatal = |message: String| {
        report_fatal(brand, &format!("{bin}: {message}"));
        ExitCode::FAILURE
    };

    // Before any profile is loaded: a fresh one's defaults follow the
    // system's locale, and on Windows only the system can say what it
    // is.
    if let Some(name) = locale::system() {
        osmena_core::settings::set_system_locale(name);
    }

    let data_dir = args.data_dir.as_deref();
    let default = match load::resolve(brand, engine, data_dir, DEFAULT_PROFILE_NAME) {
        Ok(profile) => profile,
        Err(e) => return fatal(e),
    };
    // An engine that runs every profile in one process has one browser
    // process: a later start hands over and is done.
    let mut primary = None;
    if engine.profiles_in_process {
        match instance::claim(&profiles::brand_dir(&default)) {
            Ok(instance::Claim::Primary(this)) => primary = Some(this),
            Ok(instance::Claim::Running(running)) => {
                let relaunch = instance::Relaunch {
                    profile: args.profile.clone(),
                    urls: args.urls.clone(),
                    restore: args.restore,
                };
                return match running.forward(&relaunch) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => fatal(format!(
                        "{} is already running but did not answer: {e}",
                        brand.name
                    )),
                };
            }
            // Without the lock the browser still runs; a second start
            // would then be a second browser.
            Err(e) => log::warn!("cannot tell whether {} is already running: {e}", brand.name),
        }
    }
    // The profile named, else (in one process) the ones open at the last
    // quit, else the default one.
    let mut names = match &args.profile {
        Some(name) => vec![name.clone()],
        None if engine.profiles_in_process => profiles::read_open(&default),
        None => Vec::new(),
    };
    if names.is_empty() {
        names.push(DEFAULT_PROFILE_NAME.to_string());
    }
    let first = names.remove(0);
    let profile = match load::resolve(brand, engine, data_dir, &first) {
        Ok(profile) => profile,
        Err(e) => return fatal(e),
    };
    if engine.profiles_in_process && profiles::in_use(&profile.pid_path()) {
        return fatal(format!(
            "profile {first:?} is open in another {} process; quit that one first",
            brand.name
        ));
    }
    let loaded = match load::load(brand, profile) {
        Ok(loaded) => loaded,
        Err(e) => return fatal(e),
    };
    let urls = match loaded.urls(&args.urls) {
        Ok(urls) => urls,
        Err(e) => return fatal(e),
    };
    let opened = loaded.open(urls, args.restore);

    let mut builder = EventLoop::<AppEvent>::with_user_event();
    // winit's default macOS menu owns Cmd+Q: AppKit runs `terminate:`
    // before the window sees the key. Without the menu, the keymap's Quit
    // command runs the normal shutdown instead.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_default_menu(false);
    }
    // Alt and a keypad code: the character winit drops (alt_code.rs).
    #[cfg(windows)]
    let alt_codes = alt_code::install(&mut builder);
    let event_loop = match builder.build() {
        Ok(l) => l,
        Err(e) => return fatal(format!("cannot create event loop: {e}")),
    };
    #[cfg(windows)]
    let _ = alt_codes.set(event_loop.create_proxy());
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell::new(
        brand,
        engine,
        opened,
        Startup {
            others: names,
            restore: args.restore,
            data_dir: args.data_dir.clone(),
            exit_after: args.exit_after.map(std::time::Duration::from_secs_f64),
            update_manifest: args.update_manifest.clone(),
            primary,
        },
        start_engine,
        &event_loop,
    );
    match event_loop.run_app(&mut shell) {
        Ok(()) if shell.started_cleanly() => ExitCode::SUCCESS,
        Ok(()) => ExitCode::FAILURE,
        Err(e) => fatal(format!("event loop error: {e}")),
    }
}
