//! Soft dark theme shared by the native windows (pill, alert card); matches assets/app.css.
//! Fonts are read from Windows (Segoe UI regular / semilight / semibold), nothing is bundled.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(20, 23, 29);
pub const SURFACE: Color32 = Color32::from_rgb(27, 31, 39);
pub const RAISED: Color32 = Color32::from_rgb(35, 40, 51);
pub const HOVER: Color32 = Color32::from_rgb(43, 49, 64);
pub const LINE: Color32 = Color32::from_rgb(46, 52, 64);
pub const TEXT: Color32 = Color32::from_rgb(228, 231, 236);
pub const MUTED: Color32 = Color32::from_rgb(138, 147, 163);
pub const FOCUS: Color32 = Color32::from_rgb(140, 199, 165);
pub const GAZE: Color32 = Color32::from_rgb(155, 176, 240);
pub const CAUTION: Color32 = Color32::from_rgb(226, 190, 126);
pub const ALERT: Color32 = Color32::from_rgb(227, 145, 145);
pub const BREAK: Color32 = Color32::from_rgb(134, 182, 230);

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}
/// Phosphor icons (MIT), subset to the glyphs LockIn uses; shared with the dashboard.
pub static ICON_FONT: &[u8] = include_bytes!("../assets/fonts/phosphor-lockin.ttf");

pub mod icon {
    pub const PLAY: char = '\u{e3d0}';
    pub const PAUSE: char = '\u{e39e}';
    pub const COFFEE: char = '\u{e1c2}';
    pub const BACK: char = '\u{e08a}';
    pub const CALCULATOR: char = '\u{e538}';
    pub const APP: char = '\u{e5da}';
}

pub fn icons(size: f32) -> FontId {
    FontId::new(size, family("icons"))
}

/// Large light numerals (timer).
pub fn display(size: f32) -> FontId {
    FontId::new(size, family("light"))
}
/// Emphasis (titles, primary buttons).
pub fn strong(size: f32) -> FontId {
    FontId::new(size, family("semibold"))
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
    let regular = add("segoe", "segoeui.ttf");
    let light = add("segoe-light", "segoeuisl.ttf");
    let semibold = add("segoe-semibold", "seguisb.ttf");
    let base = fonts.families[&FontFamily::Proportional].clone();
    let with = |first: Option<&str>| {
        let mut v = base.clone();
        if let Some(f) = first {
            v.insert(0, f.into());
        }
        v
    };
    let prop = with(regular.then_some("segoe"));
    fonts.families.insert(family("light"), with(light.then_some("segoe-light")).into_iter().chain(prop.clone()).collect());
    fonts.families.insert(family("semibold"), with(semibold.then_some("segoe-semibold")).into_iter().chain(prop.clone()).collect());
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.font_data.insert("phosphor".into(), Arc::new(FontData::from_static(ICON_FONT)));
    fonts.families.insert(family("icons"), vec!["phosphor".into()]);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = GAZE.linear_multiply(0.35);
        v.widgets.inactive.weak_bg_fill = RAISED;
        v.widgets.hovered.weak_bg_fill = HOVER;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
            w.corner_radius = egui::CornerRadius::same(255);
            w.bg_stroke = egui::Stroke::NONE;
        }
        style.spacing.button_padding = egui::vec2(16.0, 8.0);
        style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style.animation_time = 0.18;
    });
}
