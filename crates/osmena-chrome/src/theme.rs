//! The chrome's look, installed once per egui context: the typeface and
//! the spacing, rounding and input padding every window shares, so the
//! browser window and the settings window read as one product.
//!
//! The face is Inter (SIL Open Font License, `assets/fonts`), a
//! geometric UI face with even metrics at small sizes; egui's own fonts
//! stay behind it as fallbacks for the glyphs it lacks (emoji, the
//! toolbar's arrows). Inputs are pills with generous side padding;
//! widgets round at 8 px, like the tabs.

use std::sync::Arc;

use egui::{
    Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Style, TextStyle,
    Vec2,
};

/// The padding inside every text field of the chrome.
pub const INPUT_MARGIN: Margin = Margin::symmetric(10, 5);

/// The rounding of widgets, inputs, and the tabs' top corners.
pub const CORNER: u8 = 8;

/// The rounding of a field that is a pill (the address bar, the
/// launcher's field): more than half of any field's height.
pub const PILL: u8 = 20;

/// The family of the medium weight, for headings.
pub const MEDIUM: &str = "Inter-Medium";

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
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".to_owned());
    let mut medium = proportional.clone();
    medium[0] = MEDIUM.to_owned();
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), medium);
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
    visuals.menu_corner_radius = CornerRadius::same(10);
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
    }
}
