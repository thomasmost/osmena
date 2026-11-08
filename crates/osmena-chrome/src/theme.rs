//! The chrome's look, installed once per egui context: the typeface and
//! the spacing, rounding and input padding every window shares, so the
//! browser window and the settings window read as one product.
//!
//! The face is Inter (SIL Open Font License, `assets/fonts`), a
//! geometric UI face with even metrics at small sizes; egui's own fonts
//! stay behind it as fallbacks for the glyphs it lacks (emoji, the
//! toolbar's arrows). Profile avatars are drawn from their own family
//! ([`AVATARS`]): a subset of the current monochrome Noto Emoji (also
//! OFL), which has the newer emoji egui's older copy lacks. Inputs are
//! pills with generous side padding;
//! widgets round at 8 px, like the tabs.

use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Margin,
    Stroke, Style, TextStyle, Theme, Vec2, epaint::Shadow,
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
/// Its font, `assets/fonts/NotoEmoji-Avatars.ttf`, is Google Fonts'
/// `NotoEmoji[wght].ttf` (version 3.002) pinned to weight 400 and cut
/// down to the glyphs of `osmena_core::Avatar` with fontTools
/// (`varLib.instancer`, then `subset` with no layout features). A new
/// avatar's code point goes into a new cut; a test checks the font has
/// every avatar.
pub const AVATARS: &str = "Avatars";

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
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/NotoEmoji-Avatars.ttf"
        ))),
    );
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".to_owned());
    let mut medium = proportional.clone();
    medium[0] = MEDIUM.to_owned();
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), medium);
    // The proportional faces behind it only lend the family a
    // replacement glyph; every avatar is in the cut itself.
    let mut avatars = vec![AVATARS.to_owned()];
    avatars.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(AVATARS.into()), avatars);
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
        // The cut alone, without the fallbacks behind it.
        definitions
            .families
            .insert(FontFamily::Name("cut".into()), vec![AVATARS.to_owned()]);
        let mut fonts = egui::epaint::text::Fonts::new(Default::default(), definitions);
        let font = FontId::new(16.0, FontFamily::Name("cut".into()));
        for avatar in osmena_core::Avatar::ALL {
            if let Some(glyph) = avatar.glyph() {
                assert!(fonts.has_glyphs(&font, glyph), "{avatar:?} {glyph}");
            }
        }
        // The flamingo is in Noto Emoji but no avatar: the cut is a cut.
        let flamingo = char::from_u32(0x1F9A9).unwrap();
        assert!(!fonts.has_glyph(&font, flamingo));
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
