//! Notifications from the engine and the pure `apply` that folds them into
//! the model.

use url::Url;

use crate::command::{EngineAction, close_tab, open_window};
use crate::extension::Extension;
use crate::model::{Browser, Favicon, LoadState, RgbaImage, Tab, TabId};

/// Handle to an engine tab that page content created (`window.open`) and
/// that the engine is holding until the model adopts it with
/// [`EngineAction::AdoptTab`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PendingTab(pub u64);

/// The pointer shape a page asked for while the pointer is over it, in
/// CSS `cursor` terms (the engine maps its own cursor types to these; the
/// window maps them to the platform's cursors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Pointer,
    Text,
    Crosshair,
    Wait,
    Progress,
    Help,
    Move,
    Grab,
    Grabbing,
    NotAllowed,
    ColResize,
    RowResize,
    NsResize,
    EwResize,
    NeswResize,
    NwseResize,
    ZoomIn,
    ZoomOut,
    /// `cursor: none`.
    Hidden,
}

/// What the pointer was over when the user asked for a page's context
/// menu, as far as the engine can tell. Everything is optional: an engine
/// fills in what it knows, and the chrome builds the menu from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageContext {
    /// The link under the pointer.
    pub link: Option<Url>,
    /// The image, video, or audio under the pointer.
    pub media: Option<Media>,
    /// `Some` when text is selected: the text itself when the engine can
    /// read it, empty otherwise.
    pub selection: Option<String>,
    /// `Some` when the pointer is in an editable field, with what can be
    /// done there.
    pub editable: Option<EditState>,
    /// A misspelled word under the pointer and the dictionary's
    /// suggestions for it.
    pub misspelling: Option<Misspelling>,
}

/// An image, video, or audio element under the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    pub kind: MediaKind,
    pub src: Url,
    /// Whether the media loops; `None` when the engine cannot toggle it.
    pub looping: Option<bool>,
    /// Whether the media shows its controls; `None` when the engine
    /// cannot toggle them.
    pub controls: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

impl MediaKind {
    /// The word for it in menu entries: "Save image as…".
    pub fn noun(self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Video => "video",
            MediaKind::Audio => "audio",
        }
    }
}

/// What an editable field under the pointer allows right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_cut: bool,
    pub can_copy: bool,
    pub can_paste: bool,
    pub can_delete: bool,
    pub can_select_all: bool,
}

impl EditState {
    /// Everything allowed, for an engine that cannot tell.
    pub const ALL: EditState = EditState {
        can_undo: true,
        can_redo: true,
        can_cut: true,
        can_copy: true,
        can_paste: true,
        can_delete: true,
        can_select_all: true,
    };
}

/// A misspelled word the spell checker flagged where the context menu
/// was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misspelling {
    pub word: String,
    pub suggestions: Vec<String>,
}

/// Something the engine reported. Produced by an engine crate from its
/// engine's callbacks, keyed by core ids.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    UrlChanged {
        tab: TabId,
        url: Url,
    },
    TitleChanged {
        tab: TabId,
        title: String,
    },
    LoadStateChanged {
        tab: TabId,
        state: LoadState,
    },
    HistoryChanged {
        tab: TabId,
        can_go_back: bool,
        can_go_forward: bool,
    },
    StatusChanged {
        tab: TabId,
        text: String,
    },
    FaviconChanged {
        tab: TabId,
        favicon: Favicon,
    },
    /// Page content in `opener` asked for a new tab. The engine already
    /// created it and holds it as `pending`. A `background` tab (a
    /// middle-click or Ctrl-click on a link) opens beside the opener
    /// without taking its place; any other becomes the active tab.
    NewTabRequested {
        opener: TabId,
        pending: PendingTab,
        background: bool,
    },
    /// Page content in `opener` asked for `url` in a new window
    /// (Shift-click on a link).
    NewWindowRequested {
        opener: TabId,
        url: Url,
    },
    /// The user asked for the context menu of `tab`'s page, at
    /// `position` (physical pixels from the top left of the page area,
    /// or `None` for wherever the pointer is), over `context`. Not part
    /// of the model: the window shows the menu.
    ContextMenuRequested {
        tab: TabId,
        position: Option<(f32, f32)>,
        context: PageContext,
    },
    /// The image the user asked to copy (`PageCommand::CopyImage`) is
    /// decoded: put it on the clipboard. Not part of the model.
    ClipboardImage(RgbaImage),
    /// Page content closed the tab (`window.close()`).
    TabClosed(TabId),
    TabCrashed {
        tab: TabId,
        reason: String,
    },
    /// The engine painted a new frame for this tab; the window showing it
    /// should redraw.
    FrameReady(TabId),
    /// The page under the pointer changed the pointer shape. Not part of
    /// the model: the window applies it directly.
    CursorChanged {
        tab: TabId,
        cursor: Cursor,
    },
    /// An engine-level error not tied to a tab (for example, losing the
    /// connection to the engine backend).
    EngineError(String),
    /// The installed extensions, or something about one of them, changed:
    /// this is the whole list as it now stands.
    ExtensionsChanged(Vec<Extension>),
}

/// Fold an engine event into the model. Returns follow-up actions; only
/// `NewTabRequested`, `NewWindowRequested`, and `TabClosed` produce any.
pub fn apply(browser: &mut Browser, event: &EngineEvent) -> Vec<EngineAction> {
    match event {
        EngineEvent::UrlChanged { tab, url } => {
            with_tab(browser, *tab, |t| t.url = url.clone());
            Vec::new()
        }
        EngineEvent::TitleChanged { tab, title } => {
            with_tab(browser, *tab, |t| t.title = title.clone());
            Vec::new()
        }
        EngineEvent::LoadStateChanged { tab, state } => {
            with_tab(browser, *tab, |t| t.load = *state);
            Vec::new()
        }
        EngineEvent::HistoryChanged {
            tab,
            can_go_back,
            can_go_forward,
        } => {
            with_tab(browser, *tab, |t| {
                t.can_go_back = *can_go_back;
                t.can_go_forward = *can_go_forward;
            });
            Vec::new()
        }
        EngineEvent::StatusChanged { tab, text } => {
            with_tab(browser, *tab, |t| t.status = text.clone());
            Vec::new()
        }
        EngineEvent::FaviconChanged { tab, favicon } => {
            with_tab(browser, *tab, |t| t.favicon = Some(favicon.clone()));
            Vec::new()
        }
        EngineEvent::NewTabRequested {
            opener,
            pending,
            background,
        } => {
            let Some(window) = browser.window_of_tab(*opener) else {
                return Vec::new();
            };
            let tab = browser.allocate_tab_id();
            let w = browser
                .window_mut(window)
                .expect("window_of_tab returned an existing window");
            let previous = w.active;
            w.insert_tab_after(
                *opener,
                Tab::new(tab, Url::parse("about:blank").expect("about:blank parses")),
            );
            if *background {
                return vec![EngineAction::AdoptTab {
                    window,
                    tab,
                    pending: *pending,
                    background: true,
                }];
            }
            w.active = tab;
            w.select_only(tab);
            vec![
                EngineAction::Hide(previous),
                EngineAction::AdoptTab {
                    window,
                    tab,
                    pending: *pending,
                    background: false,
                },
            ]
        }
        EngineEvent::NewWindowRequested { opener, url } => {
            if browser.tab(*opener).is_none() {
                return Vec::new();
            }
            open_window(browser, url.clone())
        }
        EngineEvent::ContextMenuRequested { .. } | EngineEvent::ClipboardImage(_) => Vec::new(),
        EngineEvent::TabClosed(tab) => close_tab(browser, *tab),
        EngineEvent::TabCrashed { tab, reason } => {
            with_tab(browser, *tab, |t| {
                t.crashed = Some(reason.clone());
                t.load = LoadState::Idle;
            });
            Vec::new()
        }
        EngineEvent::FrameReady(_) => Vec::new(),
        EngineEvent::CursorChanged { .. } => Vec::new(),
        EngineEvent::EngineError(message) => {
            browser.engine_error = Some(message.clone());
            Vec::new()
        }
        EngineEvent::ExtensionsChanged(extensions) => {
            browser.extensions = extensions.clone();
            Vec::new()
        }
    }
}

fn with_tab(browser: &mut Browser, tab: TabId, f: impl FnOnce(&mut Tab)) {
    if let Some(t) = browser.tab_mut(tab) {
        f(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::command::{Command, dispatch, open_window};
    use crate::model::WindowId;
    use crate::model::test_support::{browser, url};

    fn one_window() -> (Browser, WindowId, TabId) {
        let mut b = browser();
        let actions = open_window(&mut b, url("https://a.example/"));
        let EngineAction::OpenWindow { window, tab, .. } = actions[0].clone() else {
            panic!("open_window returns OpenWindow");
        };
        (b, window, tab)
    }

    #[test]
    fn url_changed_updates_tab_url() {
        let (mut b, _, tab) = one_window();
        let actions = apply(
            &mut b,
            &EngineEvent::UrlChanged {
                tab,
                url: url("https://b.example/"),
            },
        );
        assert!(actions.is_empty());
        assert_eq!(b.tab(tab).unwrap().url.as_str(), "https://b.example/");
    }

    #[test]
    fn title_changed_updates_title() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::TitleChanged {
                tab,
                title: "Hello".to_string(),
            },
        );
        assert_eq!(b.tab(tab).unwrap().title, "Hello");
    }

    #[test]
    fn load_state_changed_updates_load() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::LoadStateChanged {
                tab,
                state: LoadState::Complete,
            },
        );
        assert_eq!(b.tab(tab).unwrap().load, LoadState::Complete);
    }

    #[test]
    fn history_changed_updates_navigation_flags() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::HistoryChanged {
                tab,
                can_go_back: true,
                can_go_forward: false,
            },
        );
        let t = b.tab(tab).unwrap();
        assert!(t.can_go_back);
        assert!(!t.can_go_forward);
    }

    #[test]
    fn status_changed_updates_status() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::StatusChanged {
                tab,
                text: "https://link.example/".to_string(),
            },
        );
        assert_eq!(b.tab(tab).unwrap().status, "https://link.example/");
    }

    #[test]
    fn favicon_changed_stores_favicon() {
        let (mut b, _, tab) = one_window();
        let favicon = Favicon {
            width: 1,
            height: 1,
            rgba: Arc::from(vec![255, 0, 0, 255]),
        };
        apply(
            &mut b,
            &EngineEvent::FaviconChanged {
                tab,
                favicon: favicon.clone(),
            },
        );
        assert_eq!(b.tab(tab).unwrap().favicon, Some(favicon));
    }

    #[test]
    fn new_tab_requested_adds_active_tab_and_adopts_pending() {
        let (mut b, window, opener) = one_window();
        let actions = apply(
            &mut b,
            &EngineEvent::NewTabRequested {
                opener,
                pending: PendingTab(7),
                background: false,
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 2);
        let tab = w.tabs[1].id;
        assert_eq!(w.active, tab);
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(opener),
                EngineAction::AdoptTab {
                    window,
                    tab,
                    pending: PendingTab(7),
                    background: false,
                },
            ]
        );
    }

    #[test]
    fn new_tab_requested_for_unknown_opener_does_nothing() {
        let (mut b, _, _) = one_window();
        let actions = apply(
            &mut b,
            &EngineEvent::NewTabRequested {
                opener: TabId(99),
                pending: PendingTab(1),
                background: false,
            },
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn a_background_tab_request_adds_a_tab_beside_the_opener_without_activating_it() {
        let (mut b, window, opener) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://z.example/")),
            },
        );
        dispatch(&mut b, &Command::ActivateTab(opener));
        let actions = apply(
            &mut b,
            &EngineEvent::NewTabRequested {
                opener,
                pending: PendingTab(7),
                background: true,
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.active, opener, "the opener stays active");
        assert_eq!(w.tabs.len(), 3);
        let new = w.tabs[1].id;
        assert_ne!(new, opener);
        assert_eq!(
            actions,
            vec![EngineAction::AdoptTab {
                window,
                tab: new,
                pending: PendingTab(7),
                background: true,
            }]
        );
        assert!(!w.is_selected(new));
    }

    #[test]
    fn a_foreground_tab_request_sits_beside_the_opener_and_takes_over() {
        let (mut b, window, opener) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://z.example/")),
            },
        );
        dispatch(&mut b, &Command::ActivateTab(opener));
        let actions = apply(
            &mut b,
            &EngineEvent::NewTabRequested {
                opener,
                pending: PendingTab(8),
                background: false,
            },
        );
        let w = b.window(window).unwrap();
        let new = w.tabs[1].id;
        assert_eq!(w.active, new);
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(opener),
                EngineAction::AdoptTab {
                    window,
                    tab: new,
                    pending: PendingTab(8),
                    background: false,
                }
            ]
        );
    }

    #[test]
    fn a_new_window_request_opens_a_window_at_the_url() {
        let (mut b, _window, opener) = one_window();
        let actions = apply(
            &mut b,
            &EngineEvent::NewWindowRequested {
                opener,
                url: url("https://w.example/"),
            },
        );
        assert_eq!(b.windows.len(), 2);
        assert!(matches!(
            actions.as_slice(),
            [EngineAction::OpenWindow { url, .. }] if url.as_str() == "https://w.example/"
        ));
        let gone = apply(
            &mut b,
            &EngineEvent::NewWindowRequested {
                opener: TabId(999),
                url: url("https://w.example/"),
            },
        );
        assert!(gone.is_empty());
    }

    #[test]
    fn context_menu_and_clipboard_events_leave_the_model_alone() {
        let (mut b, _window, tab) = one_window();
        let before = format!("{:?}", b.windows);
        assert!(
            apply(
                &mut b,
                &EngineEvent::ContextMenuRequested {
                    tab,
                    position: Some((1.0, 2.0)),
                    context: PageContext::default(),
                },
            )
            .is_empty()
        );
        assert!(
            apply(
                &mut b,
                &EngineEvent::ClipboardImage(RgbaImage {
                    width: 1,
                    height: 1,
                    rgba: Arc::from([0, 0, 0, 0]),
                }),
            )
            .is_empty()
        );
        assert_eq!(format!("{:?}", b.windows), before);
    }

    #[test]
    fn tab_closed_behaves_like_close_tab_command() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = apply(&mut b, &EngineEvent::TabClosed(t2));
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t2), EngineAction::Show(t1)]
        );
        assert_eq!(b.window(window).unwrap().tabs.len(), 1);
    }

    #[test]
    fn tab_crashed_keeps_tab_and_records_reason() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::LoadStateChanged {
                tab,
                state: LoadState::Loading,
            },
        );
        apply(
            &mut b,
            &EngineEvent::TabCrashed {
                tab,
                reason: "script panicked".to_string(),
            },
        );
        let t = b.tab(tab).unwrap();
        assert_eq!(t.crashed.as_deref(), Some("script panicked"));
        assert_eq!(t.load, LoadState::Idle);
        assert_eq!(t.url.as_str(), "https://a.example/");
    }

    #[test]
    fn frame_ready_changes_nothing() {
        let (mut b, _, tab) = one_window();
        let before = b.windows.clone();
        assert!(apply(&mut b, &EngineEvent::FrameReady(tab)).is_empty());
        assert_eq!(b.windows, before);
    }

    #[test]
    fn engine_error_is_recorded() {
        let (mut b, _, _) = one_window();
        apply(
            &mut b,
            &EngineEvent::EngineError("lost backend".to_string()),
        );
        assert_eq!(b.engine_error.as_deref(), Some("lost backend"));
    }

    #[test]
    fn extensions_changed_replaces_the_list() {
        let (mut b, _, _) = one_window();
        let ext = Extension {
            id: "abc".to_string(),
            name: "Abc".to_string(),
            version: "1.0".to_string(),
            description: String::new(),
            enabled: true,
            icon: None,
            action_title: None,
        };
        assert!(apply(&mut b, &EngineEvent::ExtensionsChanged(vec![ext.clone()])).is_empty());
        assert_eq!(b.extensions, vec![ext]);
        apply(&mut b, &EngineEvent::ExtensionsChanged(Vec::new()));
        assert!(b.extensions.is_empty());
    }

    #[test]
    fn tab_closed_echo_after_close_tab_command_is_a_no_op() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let closed = dispatch(&mut b, &Command::CloseTab(t2));
        assert_eq!(
            closed,
            vec![EngineAction::CloseTab(t2), EngineAction::Show(t1)]
        );
        let before = b.windows.clone();
        assert!(apply(&mut b, &EngineEvent::TabClosed(t2)).is_empty());
        assert_eq!(b.windows, before);
    }

    #[test]
    fn tab_closed_echo_after_closing_last_tab_is_a_no_op() {
        let (mut b, window, t1) = one_window();
        let closed = dispatch(&mut b, &Command::CloseTab(t1));
        assert_eq!(
            closed,
            vec![
                EngineAction::CloseTab(t1),
                EngineAction::CloseWindow(window),
                EngineAction::Quit,
            ]
        );
        let before = b.windows.clone();
        assert!(apply(&mut b, &EngineEvent::TabClosed(t1)).is_empty());
        assert_eq!(b.windows, before);
        assert!(b.windows.is_empty());
    }

    #[test]
    fn tab_closed_for_unknown_tab_is_a_no_op() {
        let (mut b, _, _) = one_window();
        let before = b.windows.clone();
        assert!(apply(&mut b, &EngineEvent::TabClosed(TabId(99))).is_empty());
        assert_eq!(b.windows, before);
    }

    #[test]
    fn events_for_unknown_tabs_are_ignored() {
        let (mut b, _, _) = one_window();
        let before = b.windows.clone();
        apply(
            &mut b,
            &EngineEvent::TitleChanged {
                tab: TabId(99),
                title: "x".to_string(),
            },
        );
        assert_eq!(b.windows, before);
    }
}
