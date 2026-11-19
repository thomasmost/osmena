//! Reading a profile off disk into a model: at start for the profile the
//! command line names, and later for each profile opened beside it in
//! the same process.

use std::path::Path;

use osmena_chrome::chrome::location_input_to_url;
use osmena_core::command::{open_window, restore_session};
use osmena_core::{
    BlockedSites, Bookmarks, Brand, Browser, Command, Downloads, EngineAction, EngineInfo, History,
    Keymap, Platform, Profile, Session, Settings, SiteZoom, TabId, dispatch,
};
use url::Url;

use crate::profiles;

/// A profile read into a model that has no windows yet.
pub(crate) struct Loaded {
    pub browser: Browser,
    pub keymap: Keymap,
    /// The last run's windows and tabs.
    session: Session,
    /// The last run did not end cleanly: its claim on the profile is
    /// still there and names no live process.
    crashed: bool,
}

/// A loaded profile with its first windows in the model, and what the
/// engine must be asked to show them.
pub(crate) struct Opened {
    pub browser: Browser,
    pub keymap: Keymap,
    pub actions: Vec<EngineAction>,
    /// The tab the first window opened on the home page because nothing
    /// was named: a link the system launched the browser for takes its
    /// place.
    pub homepage_tab: Option<TabId>,
}

/// Where profiles live: under the system's per-user directory, or under
/// the directory `--data-dir` named.
pub(crate) fn resolve(
    brand: &Brand,
    engine: &EngineInfo,
    data_dir: Option<&Path>,
    name: &str,
) -> Result<Profile, String> {
    match data_dir {
        Some(root) => Profile::resolve_under(root, brand, engine, name),
        None => Profile::resolve(brand, engine, name),
    }
    .map_err(|e| e.to_string())
}

/// Read `profile` into a model. What the browser cannot do without (the
/// directory, the settings, the keymap, the bookmarks) is an error, in
/// words for the user; the rest is logged and starts empty.
pub(crate) fn load(brand: &Brand, profile: Profile) -> Result<Loaded, String> {
    profile.ensure_dirs().map_err(|e| {
        format!(
            "cannot create profile directory {}: {e}",
            profile.dir.display()
        )
    })?;
    let settings = Settings::load(&profile.settings_path(), brand)
        .map_err(|e| format!("{e} (in {})", profile.settings_path().display()))?;
    let keymap = Keymap::load(&profile.keymap_path(), Platform::current())
        .map_err(|e| format!("{e} (in {})", profile.keymap_path().display()))?;
    let mut browser = Browser::new(profile, settings);
    // The default profile's file when this profile shares its bookmarks.
    let bookmarks_path = browser.bookmarks_path();
    browser.bookmarks = Bookmarks::load(&bookmarks_path)
        .map_err(|e| format!("{e} (in {})", bookmarks_path.display()))?;
    let profile = &browser.profile;
    // A history that cannot be read is not worth refusing to start over:
    // it is logged and starts empty (the file is replaced on the next
    // visit). The zoom levels, the downloads list and the blocked sites
    // likewise.
    let history = History::load(&profile.history_path()).unwrap_or_else(|e| {
        log::warn!(
            "{e} (in {}); starting with an empty history",
            profile.history_path().display()
        );
        History::default()
    });
    let zoom = SiteZoom::load(&profile.zoom_path()).unwrap_or_else(|e| {
        log::warn!(
            "{e} (in {}); starting with no site zoom levels",
            profile.zoom_path().display()
        );
        SiteZoom::default()
    });
    let downloads = Downloads::load(&profile.downloads_path()).unwrap_or_else(|e| {
        log::warn!(
            "{e} (in {}); starting with no downloads",
            profile.downloads_path().display()
        );
        Downloads::default()
    });
    let blocked = BlockedSites::load(&profile.blocked_path()).unwrap_or_else(|e| {
        log::warn!(
            "{e} (in {}); starting with no blocked sites",
            profile.blocked_path().display()
        );
        BlockedSites::default()
    });
    // The last run's windows: opened again when the setting (or the
    // flag) says so, offered when the last run did not end cleanly.
    let session = Session::load(&profile.session_path()).unwrap_or_else(|e| {
        log::warn!(
            "{e} (in {}); starting without a session",
            profile.session_path().display()
        );
        Session::default()
    });
    let pid_path = profile.pid_path();
    let crashed = pid_path.exists() && !profiles::in_use(&pid_path);
    browser.history = history;
    browser.zoom = zoom;
    browser.downloads = downloads;
    browser.blocked = blocked;
    Ok(Loaded {
        browser,
        keymap,
        session,
        crashed,
    })
}

impl Loaded {
    /// The addresses `args` name: a URL opens, other text is searched
    /// with this profile's search engine, as in the address bar.
    pub fn urls(&self, args: &[String]) -> Result<Vec<Url>, String> {
        args.iter()
            .map(|arg| {
                location_input_to_url(arg)
                    .or_else(|| self.browser.settings.search.url_for(arg))
                    .ok_or_else(|| format!("not a URL: {arg:?}"))
            })
            .collect()
    }

    /// Open the first windows in the model: the last run's when the
    /// setting (or `restore`) says so, else one on the first of `urls`
    /// or the home page, and a tab for each further URL.
    pub fn open(self, urls: Vec<Url>, restore: bool) -> Opened {
        let Loaded {
            mut browser,
            keymap,
            session,
            crashed,
        } = self;
        let mut actions = Vec::new();
        if (browser.settings.restore_windows_on_launch || restore) && !session.is_empty() {
            log::info!(
                "restoring {} window(s) with {} tab(s) of profile {:?}",
                session.windows.len(),
                session.tab_count(),
                browser.profile.name
            );
            actions.extend(restore_session(&mut browser, &session));
        }
        let mut urls = urls.into_iter();
        let mut homepage_tab = None;
        if browser.windows.is_empty() {
            let named = urls.next();
            let on_homepage = named.is_none();
            let first: Url = named.unwrap_or_else(|| browser.homepage());
            actions.extend(open_window(&mut browser, first));
            if on_homepage {
                homepage_tab = browser.windows.last().map(|w| w.active);
            }
            if crashed && !session.is_empty() {
                log::info!("the last run did not end cleanly; offering its session");
                browser.crashed_session = Some(session);
            }
        }
        let window = browser.windows.last().expect("a window was opened").id;
        for url in urls {
            actions.extend(dispatch(
                &mut browser,
                &Command::NewTab {
                    window,
                    url: Some(url),
                },
            ));
        }
        Opened {
            browser,
            keymap,
            actions,
            homepage_tab,
        }
    }
}
