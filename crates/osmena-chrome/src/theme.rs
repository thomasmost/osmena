//! The chrome's look, installed once per egui context: the typeface and
//! the spacing, rounding and input padding every window shares, so the
//! browser window and the settings window read as one product.
//!
//! The face is Inter (SIL Open Font License, `assets/fonts`), a
//! geometric UI face with even metrics at small sizes; egui's own fonts
//! stay behind it as fallbacks for the glyphs it lacks. The chrome's
//! glyphs ([`ICONS`]) and the profile avatars ([`AVATARS`]) are two
//! small fonts of this repository's own drawings, built from the SVGs
//! under `assets/icons` and `assets/avatars`. Inputs are pills with
//! generous side padding; widgets round at 8 px, like the tabs.

use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Id,
    Margin, Stroke, Style, TextStyle, Theme, Vec2, epaint::Shadow,
};
use osmena_core::ChromeColor;

use crate::chrome::Palette;

/// The padding inside every text field of the chrome.
pub const INPUT_MARGIN: Margin = Margin::symmetric(10, 5);

/// The rounding of widgets, inputs, and the tabs' top corners.
pub const CORNER: u8 = 8;

/// The rounding of a field that is a pill (the address bar, the
/// launcher's field): more than half of any field's height.
pub const PILL: u8 = 20;

/// The rounding of a card (a tile of the Launch Panel).
pub const CARD: u8 = 14;

/// The rounding of a sheet over the page (the Launch Panel).
pub const PANEL: u8 = 18;

/// The rounding of a popup (a suggestions list, a popover, a prompt).
pub const POPUP: u8 = 12;

/// The width of the side panel docked on the right of the page area, in
/// points. Fixed: the panel is not resizable.
pub const SIDE_PANEL_WIDTH: f32 = 360.0;

/// The height of the side panel's header (the extension's icon and name,
/// the close control), in points.
pub const SIDE_PANEL_HEADER: f32 = 36.0;

/// The narrowest the page may get beside a side panel, in points: a
/// content area narrower than this plus [`SIDE_PANEL_WIDTH`] shows no
/// panel.
pub const SIDE_PANEL_MIN_PAGE: f32 = 320.0;

/// The space between the side panel's edge and its header's icon, in
/// points. The close control sits half of it in from the other edge:
/// its disc is larger than its glyph.
pub const SIDE_PANEL_PADDING: f32 = 10.0;

/// The space between the parts of the side panel's header, in points.
pub const SIDE_PANEL_GAP: f32 = 8.0;

/// The diameter of the disc behind the side panel's close control, in
/// points.
pub const SIDE_PANEL_CLOSE: f32 = 24.0;

/// The shadow under a popup: wide and soft, cast straight down, so a
/// list reads as lifted off the page rather than outlined.
pub const POPUP_SHADOW: Shadow = Shadow {
    offset: [0, 4],
    blur: 18,
    spread: 0,
    color: Color32::from_black_alpha(36),
};

/// The shadow under a sheet (the Launch Panel): deeper than a popup's.
pub const PANEL_SHADOW: Shadow = Shadow {
    offset: [0, 10],
    blur: 40,
    spread: 0,
    color: Color32::from_black_alpha(70),
};

/// The family of the medium weight, for headings.
pub const MEDIUM: &str = "Inter-Medium";

/// The family the profile avatars' glyphs are drawn in.
///
/// Its font, `assets/fonts/OsmenaAvatars.ttf`, is built from the
/// drawings in `assets/avatars` by `tools/glyphs/build.py`, one glyph
/// per `osmena_core::Avatar` at the emoji code point the avatar names:
/// plain silhouettes made to read on a disc twenty pixels across. A new
/// avatar is a variant, its code point and a drawing named after it;
/// tests check the font has every avatar and every avatar a drawing.
/// The whale is Noto Emoji's, which is why this one font is under the
/// SIL Open Font License (`LICENSE-OsmenaAvatars.txt`).
pub const AVATARS: &str = "Avatars";

/// The family of the chrome's own glyphs (`crate::icons`).
///
/// Its font, `assets/fonts/OsmenaIcons.ttf`, is built from the drawings
/// in `assets/icons` by `tools/glyphs/build.py`; a test checks it has
/// every glyph the chrome names.
pub const ICONS: &str = "Icons";

/// A font of drawings, as its builder made it: the outlines are placed
/// on the pixel grid by hand, and a hinter moving them to suit a
/// letter's stems would only blur them.
fn drawings(bytes: &'static [u8]) -> Arc<FontData> {
    Arc::new(FontData::from_static(bytes).tweak(FontTweak {
        hinting_override: Some(false),
        ..FontTweak::default()
    }))
}

/// Install the fonts and the style on `ctx`.
pub fn install(ctx: &Context) {
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(style);
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        MEDIUM.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        AVATARS.to_owned(),
        drawings(include_bytes!("../assets/fonts/OsmenaAvatars.ttf")),
    );
    fonts.font_data.insert(
        ICONS.to_owned(),
        drawings(include_bytes!("../assets/fonts/OsmenaIcons.ttf")),
    );
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".to_owned());
    let mut medium = proportional.clone();
    medium[0] = MEDIUM.to_owned();
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), medium);
    // The proportional faces behind it only lend the family a
    // replacement glyph; every avatar is in the font itself.
    let mut avatars = vec![AVATARS.to_owned()];
    avatars.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(AVATARS.into()), avatars);
    // Likewise the icons: a family with no replacement glyph makes
    // egui warn at every start, and a test checks the font alone has
    // every glyph the chrome names.
    let mut icons = vec![ICONS.to_owned()];
    icons.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name(ICONS.into()), icons);
    fonts
}

fn style(style: &mut Style) {
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(19.0, FontFamily::Name(MEDIUM.into())),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.item_spacing = Vec2::new(8.0, 4.0);
    style.spacing.interact_size.y = 22.0;
    let visuals = &mut style.visuals;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(CORNER);
    }
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(POPUP);
    visuals.popup_shadow = POPUP_SHADOW;
    visuals.window_shadow = PANEL_SHADOW;
    // Buttons show no outline on hover or press: the fill changes.
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
}

/// Tint the context's styles with the profile's colour: the accent
/// (the focus ring of every field, the text selection, the picked
/// row, a lit chip, a link) and the surface every popup is drawn on.
/// Both the light and the dark style are set, each from its own
/// [`Palette`], so a system theme change needs no new call. The colour
/// last applied is kept in the context's memory, so a call per frame
/// costs a lookup until the colour changes.
pub fn tint(ctx: &Context, color: ChromeColor) {
    let key = Id::new("osmena-tint");
    if ctx.data(|d| d.get_temp::<ChromeColor>(key)) == Some(color) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(key, color));
    for (theme, dark) in [(Theme::Light, false), (Theme::Dark, true)] {
        let palette = Palette::new(color, dark);
        ctx.style_mut_of(theme, |style| tint_style(style, &palette));
    }
}

fn tint_style(style: &mut Style, palette: &Palette) {
    let visuals = &mut style.visuals;
    visuals.selection.stroke = Stroke::new(1.5_f32, palette.accent);
    visuals.selection.bg_fill = palette.selection;
    visuals.hyperlink_color = palette.accent;
    visuals.window_fill = palette.surface;
    visuals.panel_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0_f32, palette.hairline);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, palette.hairline);
}

/// Round the fields drawn inside `ui` as pills.
pub fn pill_fields(ui: &mut egui::Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    for widget in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
    ] {
        widget.corner_radius = CornerRadius::same(PILL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inter_leads_the_proportional_family_and_the_fallbacks_stay() {
        let fonts = fonts();
        let proportional = &fonts.families[&FontFamily::Proportional];
        assert_eq!(proportional[0], "Inter");
        assert!(proportional.len() > 1, "egui's fallbacks were dropped");
        let medium = &fonts.families[&FontFamily::Name(MEDIUM.into())];
        assert_eq!(medium[0], MEDIUM);
        assert_eq!(&medium[1..], &proportional[1..]);
        assert!(fonts.font_data.contains_key("Inter"));
        assert!(fonts.font_data.contains_key(MEDIUM));
    }

    #[test]
    fn the_avatar_font_has_every_avatar_glyph() {
        let mut definitions = fonts();
        assert_eq!(
            definitions.families[&FontFamily::Name(AVATARS.into())][0],
            AVATARS
        );
        // The font alone, without the fallbacks behind it.
        definitions
            .families
            .insert(FontFamily::Name("alone".into()), vec![AVATARS.to_owned()]);
        let mut fonts = egui::epaint::text::Fonts::new(Default::default(), definitions);
        let font = FontId::new(16.0, FontFamily::Name("alone".into()));
        for avatar in osmena_core::Avatar::ALL {
            if let Some(glyph) = avatar.glyph() {
                assert!(fonts.has_glyphs(&font, glyph), "{avatar:?} {glyph}");
            }
        }
        // The flamingo is an emoji but no avatar: nothing is in the
        // font that no avatar asks for.
        let flamingo = char::from_u32(0x1F9A9).unwrap();
        assert!(!fonts.has_glyph(&font, flamingo));
    }

    #[test]
    fn the_icon_font_has_every_icon() {
        let mut definitions = fonts();
        assert_eq!(
            definitions.families[&FontFamily::Name(ICONS.into())][0],
            ICONS
        );
        // The font alone, without the fallbacks behind it.
        definitions
            .families
            .insert(FontFamily::Name("alone".into()), vec![ICONS.to_owned()]);
        let mut fonts = egui::epaint::text::Fonts::new(Default::default(), definitions);
        let font = FontId::new(crate::icons::SIZE, FontFamily::Name("alone".into()));
        for icon in crate::icons::Icon::ALL {
            assert!(fonts.has_glyph(&font, icon.code()), "{icon:?}");
        }
        // And nothing else: a letter is not an icon.
        assert!(!fonts.has_glyph(&font, 'a'));
    }

    /// The name of a variant as its drawing's file has it:
    /// `StarFilled` is `star-filled`.
    fn file_name(variant: &str) -> String {
        let mut name = String::new();
        for (index, letter) in variant.chars().enumerate() {
            if letter.is_uppercase() && index > 0 {
                name.push('-');
            }
            name.extend(letter.to_lowercase());
        }
        name
    }

    /// The drawings in a directory of `assets`, by file name.
    fn drawings_in(directory: &str) -> Vec<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(directory);
        let mut names: Vec<String> = std::fs::read_dir(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .map(|entry| entry.unwrap().path())
            .filter(|file| file.extension().is_some_and(|e| e == "svg"))
            .map(|file| file.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn every_glyph_has_its_drawing_and_every_drawing_its_glyph() {
        let mut icons: Vec<String> = crate::icons::Icon::ALL
            .iter()
            .map(|icon| file_name(&format!("{icon:?}")))
            .collect();
        icons.sort();
        assert_eq!(drawings_in("icons"), icons);
        let mut avatars: Vec<String> = osmena_core::Avatar::ALL
            .iter()
            .filter(|avatar| avatar.glyph().is_some())
            .map(|avatar| file_name(&format!("{avatar:?}")))
            .collect();
        avatars.sort();
        assert_eq!(drawings_in("avatars"), avatars);
    }

    #[test]
    fn the_style_rounds_widgets_and_pads_buttons() {
        let mut s = Style::default();
        style(&mut s);
        assert_eq!(
            s.visuals.widgets.inactive.corner_radius,
            CornerRadius::same(CORNER)
        );
        assert_eq!(s.spacing.button_padding, Vec2::new(8.0, 4.0));
        assert_eq!(
            s.text_styles[&TextStyle::Heading].family,
            FontFamily::Name(MEDIUM.into())
        );
        assert_eq!(s.visuals.widgets.hovered.bg_stroke, Stroke::NONE);
    }

    #[test]
    fn the_tint_sets_both_styles_once_per_colour() {
        let ctx = Context::default();
        install(&ctx);
        tint(&ctx, ChromeColor::Shell);
        let light = ctx.style_of(Theme::Light);
        let dark = ctx.style_of(Theme::Dark);
        assert_eq!(
            light.visuals.selection.stroke.color,
            Palette::new(ChromeColor::Shell, false).accent
        );
        assert_eq!(
            dark.visuals.selection.stroke.color,
            Palette::new(ChromeColor::Shell, true).accent
        );
        assert_ne!(
            light.visuals.selection.stroke.color,
            dark.visuals.selection.stroke.color
        );
        // The same colour again changes nothing; another colour does.
        tint(&ctx, ChromeColor::Shell);
        assert_eq!(
            ctx.style_of(Theme::Light).visuals.selection.stroke.color,
            light.visuals.selection.stroke.color
        );
        tint(&ctx, ChromeColor::Ocean);
        assert_eq!(
            ctx.style_of(Theme::Light).visuals.selection.stroke.color,
            Palette::new(ChromeColor::Ocean, false).accent
        );
    }
}
