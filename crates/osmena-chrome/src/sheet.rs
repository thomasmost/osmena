//! The sheets: pictures of the chrome and of its glyphs, for judging
//! them by eye. Nothing here asserts anything; the tests are ignored
//! and write PNGs when asked:
//!
//! ```text
//! cargo test -p osmena-chrome sheet -- --ignored
//! ```
//!
//! They land in `target/sheets` (or `OSMENA_SHEET_DIR`), each at one
//! pixel per point and again magnified, since a glyph of a dozen
//! pixels is judged a pixel at a time.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Color32, Pos2, Vec2};
use osmena_core::{
    Avatar, Brand, Browser, ChromeColor, EngineInfo, ExtensionSupport, FindState, Keymap,
    PageTools, Platform, Profile, ProfileForm, Prompt, Settings, Tab, TabId, Window, WindowId,
};
use url::Url;

use crate::chrome::{self, ChromeContext, ChromeState};
use crate::icons::{self, Icon};
use crate::raster::{self, Picture};

static BRAND: Brand = Brand {
    name: "Osmena",
    binary: "osmena",
    data_dir: "Osmena",
    homepage: "https://home.example/",
    chrome_color: ChromeColor::Shell,
    icon_png: &[],
    version: "0.1.0",
    build: None,
    updates: None,
    google_client_id: None,
};

fn no_item(_: &Url) -> Option<String> {
    None
}

static ENGINE: EngineInfo = EngineInfo {
    name: "TestEngine",
    version: "0.0",
    data_dir: "engine",
    supports_stop: true,
    find: true,
    downloads: true,
    wheel_line_px: 40.0,
    extensions: Some(ExtensionSupport {
        store_name: "Store",
        store_url: "https://store.example/",
        store_item_id: no_item,
        side_panels: false,
    }),
    page_tools: PageTools::ALL,
    profiles_in_process: false,
};

/// Where the sheets are written.
fn sheet_dir() -> PathBuf {
    match std::env::var_os("OSMENA_SHEET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/sheets"),
    }
}

/// Write `picture` as `<name>.png` and, magnified, as `<name>@<n>x.png`.
fn save(picture: &Picture, name: &str, magnify: usize) {
    let dir = sheet_dir();
    picture.save_png(&dir.join(format!("{name}.png")));
    if magnify > 1 {
        picture
            .magnified(magnify)
            .save_png(&dir.join(format!("{name}@{magnify}x.png")));
    }
}

/// A browser with one window of three tabs, the second one active.
fn browser(color: ChromeColor, avatar: Avatar) -> Browser {
    let profile = Profile {
        name: "default".to_string(),
        dir: PathBuf::from("profile"),
        engine_dir: PathBuf::from("profile/engine"),
        guest: false,
    };
    let mut settings = Settings::for_brand(&BRAND);
    settings.chrome_color = color;
    settings.profile.avatar = avatar;
    let mut browser = Browser::new(profile, settings);
    let page = |n: u64, address: &str, title: &str| {
        let mut tab = Tab::new(TabId(n), Url::parse(address).expect("a test address"));
        tab.title = title.to_string();
        tab
    };
    let mut window = Window::with_tab(
        WindowId(1),
        page(1, "https://a.example/", "A page with a long enough title"),
    );
    window.tabs.push(page(
        2,
        "https://docs.example/guide/start",
        "Getting started",
    ));
    window.tabs.push(page(3, "https://c.example/", "Third"));
    window.active = TabId(2);
    browser.windows.push(window);
    browser
}

/// The whole chrome of `browser`'s window, `size` points, after
/// `prepare` has put the chrome's state as the picture wants it (a
/// panel open, a bar showing).
fn chrome_picture(
    browser: &Browser,
    size: Vec2,
    pixels_per_point: f32,
    dark: bool,
    prepare: impl FnOnce(&mut ChromeState),
) -> Picture {
    let keymap = Keymap::defaults(Platform::Windows);
    let site_icons = HashMap::new();
    let mut state = ChromeState::default();
    prepare(&mut state);
    let ground = if dark {
        Color32::from_gray(30)
    } else {
        Color32::WHITE
    };
    raster::render(pixels_per_point, size, dark, ground, |ui| {
        let context = ChromeContext {
            color: browser.settings.chrome_color,
            brand: &BRAND,
            profile_dir: &browser.profile.name,
            profile_name: "Thomas",
            avatar: browser.settings.profile.avatar,
            engine: &ENGINE,
            engine_error: None,
            frameless: true,
            maximized: false,
            pinned: &[],
            history: &browser.history,
            update: &browser.update,
            bookmarks: &browser.bookmarks,
            show_bookmarks_bar: false,
            site_icons: &site_icons,
            page_zoom: browser.settings.page_zoom,
            downloads: &browser.downloads,
            restore_offer: false,
            default_browser_offer: false,
            launch: &browser.launch,
            launch_settings: &browser.settings.launch,
            search: &browser.settings.search,
        };
        let _ = chrome::draw(ui, &browser.windows[0], &context, &mut state, &keymap);
    })
}

#[test]
#[ignore = "writes pictures of the chrome to target/sheets"]
fn sheet_of_the_chrome() {
    for (color, avatar, tag) in [
        (ChromeColor::Shell, Avatar::Whale, "shell"),
        (ChromeColor::Slate, Avatar::Initial, "slate"),
    ] {
        let browser = browser(color, avatar);
        for dark in [false, true] {
            let theme = if dark { "dark" } else { "light" };
            let picture = chrome_picture(&browser, Vec2::new(760.0, 120.0), 1.0, dark, |_| {});
            save(&picture, &format!("chrome-{tag}-{theme}"), 4);
        }
        let picture = chrome_picture(&browser, Vec2::new(760.0, 120.0), 2.0, false, |_| {});
        save(&picture, &format!("chrome-{tag}-light-2x"), 1);
    }
}

/// Every glyph of the icon font in a row, at its natural size, on a
/// light and a dark ground.
fn icons_picture(pixels_per_point: f32) -> Picture {
    let pitch = 28.0;
    let width = pitch * Icon::ALL.len() as f32;
    raster::render(
        pixels_per_point,
        Vec2::new(width, 2.0 * pitch),
        false,
        Color32::from_rgb(250, 247, 243),
        |ui| {
            let painter = ui.painter();
            painter.rect_filled(
                egui::Rect::from_min_size(Pos2::new(0.0, pitch), Vec2::new(width, pitch)),
                0.0,
                Color32::from_gray(38),
            );
            for (index, icon) in Icon::ALL.into_iter().enumerate() {
                let x = pitch * (index as f32 + 0.5);
                icons::paint(
                    painter,
                    icon,
                    Pos2::new(x, pitch * 0.5),
                    icons::SIZE,
                    Color32::from_gray(40),
                );
                icons::paint(
                    painter,
                    icon,
                    Pos2::new(x, pitch * 1.5),
                    icons::SIZE,
                    Color32::from_gray(230),
                );
            }
        },
    )
}

#[test]
#[ignore = "writes pictures of the icons to target/sheets"]
fn sheet_of_the_icons() {
    save(&icons_picture(1.0), "icons", 8);
    for (scale, tag) in [(1.25, "125"), (1.5, "150"), (2.0, "200")] {
        save(&icons_picture(scale), &format!("icons-{tag}"), 4);
    }
}

#[test]
#[ignore = "writes pictures of the Launch Panel to target/sheets"]
fn sheet_of_the_launch_panel() {
    let browser = browser(ChromeColor::Shell, Avatar::Whale);
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let picture = chrome_picture(&browser, Vec2::new(1100.0, 640.0), 1.0, dark, |state| {
            state.open_launcher()
        });
        save(&picture, &format!("launch-{theme}"), 1);
    }
}

/// Every avatar on its disc at the three sizes the chrome draws it
/// (the toolbar's, the dialog's choice, the dialog's preview), on a
/// pale and a deep swatch.
fn avatars_picture(pixels_per_point: f32) -> Picture {
    let columns = 6;
    let cell = Vec2::new(210.0, 64.0);
    let rows = Avatar::ALL.len().div_ceil(columns);
    raster::render(
        pixels_per_point,
        Vec2::new(cell.x * columns as f32, cell.y * rows as f32),
        false,
        Color32::from_rgb(250, 247, 243),
        |ui| {
            for (index, avatar) in Avatar::ALL.into_iter().enumerate() {
                let corner = Pos2::new(
                    cell.x * (index % columns) as f32,
                    cell.y * (index / columns) as f32,
                );
                let mut x = corner.x + 14.0;
                for color in [ChromeColor::Shell, ChromeColor::Ocean] {
                    for radius in [10.0, 16.0] {
                        x += radius;
                        let center = Pos2::new(x, corner.y + 20.0);
                        chrome::paint_avatar(ui, center, radius, color, avatar, "Thomas");
                        x += radius + 4.0;
                    }
                }
                let center = Pos2::new(x + 28.0, corner.y + 32.0);
                chrome::paint_avatar(ui, center, 28.0, ChromeColor::Shell, avatar, "Thomas");
            }
        },
    )
}

#[test]
#[ignore = "writes pictures of the avatars to target/sheets"]
fn sheet_of_the_avatars() {
    save(&avatars_picture(1.0), "avatars", 1);
    save(&avatars_picture(1.5), "avatars-150", 1);
    save(&avatars_picture(2.0), "avatars-200", 1);
}

#[test]
#[ignore = "writes pictures of the profile dialog to target/sheets"]
fn sheet_of_the_profile_dialog() {
    // One picture per tab of avatars: the dialog opens on the tab of
    // the avatar it is given.
    for (avatar, tag) in [
        (Avatar::Whale, "sea"),
        (Avatar::Wink, "emoji"),
        (Avatar::Rocket, "assorted"),
    ] {
        let browser = browser(ChromeColor::Shell, avatar);
        let picture = chrome_picture(&browser, Vec2::new(900.0, 760.0), 1.0, false, |state| {
            state.open_prompt(Prompt::Profile {
                form: ProfileForm {
                    name: "Thomas".to_string(),
                    color: ChromeColor::Shell,
                    avatar,
                    share_bookmarks: false,
                    copy_extensions: false,
                },
                new: false,
                deletable: false,
            });
        });
        save(&picture, &format!("profile-{tag}"), 1);
    }
}

#[test]
#[ignore = "writes pictures of the find bar to target/sheets"]
fn sheet_of_the_find_bar() {
    let mut browser = browser(ChromeColor::Shell, Avatar::Whale);
    browser.windows[0].find = Some(FindState {
        text: "needle".to_string(),
        match_case: false,
        current: 2,
        total: 7,
    });
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let picture = chrome_picture(&browser, Vec2::new(760.0, 140.0), 1.0, dark, |_| {});
        save(&picture, &format!("find-{theme}"), 4);
    }
}
