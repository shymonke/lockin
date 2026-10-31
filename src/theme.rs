//! Bold dark theme, Segoe UI system fonts (read from Windows, not bundled).

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;

pub const INK: Color32 = Color32::from_rgb(14, 14, 18);
pub const CARD: Color32 = Color32::from_rgb(26, 26, 33);
pub const CARD_HI: Color32 = Color32::from_rgb(38, 38, 48);
pub const LINE: Color32 = Color32::from_rgb(58, 58, 72);
pub const TEXT: Color32 = Color32::from_rgb(244, 244, 248);
pub const MUTED: Color32 = Color32::from_rgb(150, 150, 168);
pub const LIME: Color32 = Color32::from_rgb(198, 255, 61);
pub const RED: Color32 = Color32::from_rgb(255, 59, 92);
pub const AMBER: Color32 = Color32::from_rgb(255, 176, 32);
pub const BLUE: Color32 = Color32::from_rgb(77, 163, 255);
pub const PURPLE: Color32 = Color32::from_rgb(178, 120, 255);
pub const PINK: Color32 = Color32::from_rgb(255, 92, 205);

pub fn black() -> FontFamily {
    FontFamily::Name("black".into())
}
pub fn heavy(size: f32) -> FontId {
    FontId::new(size, black())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    let mut add = |name: &str, file: &str| -> bool {
        match std::fs::read(dir.join(file)) {
            Ok(bytes) => {
                fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    let has_ui = add("segoe", "segoeuib.ttf") || add("segoe", "segoeui.ttf");
    let has_black = add("segoe-black", "seguibl.ttf");
    if has_ui {
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "segoe".into());
    }
    let mut black_family = fonts.families[&FontFamily::Proportional].clone();
    if has_black {
        black_family.insert(0, "segoe-black".into());
    }
    fonts.families.insert(black(), black_family);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = INK;
        v.window_fill = CARD;
        v.extreme_bg_color = Color32::from_rgb(20, 20, 26);
        v.faint_bg_color = CARD;
        v.selection.bg_fill = LIME.linear_multiply(0.35);
        v.selection.stroke.color = LIME;
        v.hyperlink_color = LIME;
        v.override_text_color = Some(TEXT);
        let r = egui::CornerRadius::same(8);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
            w.corner_radius = r;
        }
        v.widgets.inactive.bg_fill = CARD_HI;
        v.widgets.inactive.weak_bg_fill = CARD_HI;
        v.widgets.hovered.bg_fill = Color32::from_rgb(52, 52, 66);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 52, 66);
        v.widgets.active.bg_fill = LIME.linear_multiply(0.5);
        v.widgets.noninteractive.bg_stroke.color = LINE;
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 26.0;
        style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Heading, heavy(22.0));
        style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
    });
}
