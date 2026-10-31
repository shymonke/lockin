//! Alert memes: procedurally painted, animated cats (zero assets) and user GIF/PNG/JPG files.

use crate::policy::Kind;
use crate::theme;
use eframe::egui::{self, pos2, vec2, Color32, ColorImage, Painter, Rect, Shape, Stroke, TextureHandle, TextureOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const BUILTINS: [&str; 4] = ["Scuba cat", "Judging cat", "Sleepy cat", "Alarm cat"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CatStyle {
    Scuba,
    Judging,
    Sleepy,
    Alarm,
}

impl CatStyle {
    pub fn from_name(name: &str) -> CatStyle {
        match name {
            "Judging cat" => CatStyle::Judging,
            "Sleepy cat" => CatStyle::Sleepy,
            "Alarm cat" => CatStyle::Alarm,
            _ => CatStyle::Scuba,
        }
    }
    /// The meme that fits the distraction best.
    pub fn for_kind(kind: &Kind, seed: u64) -> CatStyle {
        match kind {
            Kind::Absent => CatStyle::Sleepy,
            Kind::Phone => CatStyle::Judging,
            Kind::BrbOverdue => CatStyle::Alarm,
            Kind::LookingAway => CatStyle::Scuba,
            Kind::Site(_) | Kind::ScreenSite(_) => {
                if seed % 2 == 0 { CatStyle::Scuba } else { CatStyle::Judging }
            }
        }
    }
}

const FUR: Color32 = Color32::from_rgb(255, 170, 70);
const FUR_DARK: Color32 = Color32::from_rgb(214, 120, 40);
const EAR_IN: Color32 = Color32::from_rgb(255, 140, 170);
const EYE: Color32 = Color32::from_rgb(20, 20, 26);

/// Paint an animated cat filling `rect`. `t` is seconds since the alert opened.
pub fn paint_cat(p: &Painter, rect: Rect, t: f32, style: CatStyle) {
    let c = rect.center();
    let s = rect.height().min(rect.width()) / 200.0;
    let clip = p.with_clip_rect(rect);

    // Scene.
    match style {
        CatStyle::Scuba => {
            clip.rect_filled(rect, 14.0, Color32::from_rgb(18, 70, 140));
            let mut wave = Vec::new();
            for i in 0..=40 {
                let x = rect.left() + rect.width() * i as f32 / 40.0;
                wave.push(pos2(x, rect.top() + 18.0 * s + (x * 0.05 + t * 2.0).sin() * 5.0 * s));
            }
            clip.add(Shape::line(wave, Stroke::new(3.0 * s, Color32::from_rgb(120, 200, 255))));
            for i in 0..9 {
                let fi = i as f32;
                let speed = 30.0 + fi * 7.0;
                let y = rect.bottom() - ((t * speed + fi * 41.0) % rect.height());
                let x = rect.left() + rect.width() * ((fi * 0.137 + 0.05) % 1.0) + (t * 3.0 + fi).sin() * 6.0;
                clip.circle_stroke(pos2(x, y), (3.0 + (i % 3) as f32 * 2.5) * s, Stroke::new(1.5 * s, Color32::from_rgb(190, 235, 255)));
            }
        }
        CatStyle::Alarm => {
            let flash = if (t * 4.0) as i32 % 2 == 0 { theme::RED } else { Color32::from_rgb(120, 20, 40) };
            clip.rect_filled(rect, 14.0, flash.linear_multiply(0.55));
        }
        CatStyle::Sleepy => {
            clip.rect_filled(rect, 14.0, Color32::from_rgb(34, 30, 70));
        }
        CatStyle::Judging => {
            clip.rect_filled(rect, 14.0, Color32::from_rgb(60, 40, 52));
        }
    }

    let shake = match style {
        CatStyle::Alarm => vec2((t * 40.0).sin() * 4.0 * s, 0.0),
        CatStyle::Judging => vec2(0.0, 0.0),
        _ => vec2(0.0, (t * 2.0).sin() * 5.0 * s),
    };
    let h = c + shake + vec2(0.0, 12.0 * s);
    let at = |x: f32, y: f32| h + vec2(x * s, y * s);

    // Ears.
    for dir in [-1.0f32, 1.0] {
        clip.add(Shape::convex_polygon(vec![at(dir * 70.0, -20.0), at(dir * 62.0, -92.0), at(dir * 18.0, -58.0)], FUR, Stroke::NONE));
        clip.add(Shape::convex_polygon(vec![at(dir * 60.0, -32.0), at(dir * 57.0, -78.0), at(dir * 30.0, -56.0)], EAR_IN, Stroke::NONE));
    }
    // Head.
    clip.add(Shape::ellipse_filled(h, vec2(82.0 * s, 68.0 * s), FUR));
    for i in 0..3 {
        let x = (i as f32 - 1.0) * 14.0;
        clip.line_segment([at(x, -66.0), at(x * 0.8, -48.0)], Stroke::new(4.0 * s, FUR_DARK));
    }
    // Muzzle.
    clip.add(Shape::ellipse_filled(at(0.0, 22.0), vec2(30.0 * s, 20.0 * s), Color32::from_rgb(255, 225, 190)));
    clip.add(Shape::convex_polygon(vec![at(-7.0, 10.0), at(7.0, 10.0), at(0.0, 18.0)], EAR_IN, Stroke::NONE));
    let mouth = Stroke::new(2.5 * s, EYE);
    clip.add(Shape::line(vec![at(-12.0, 26.0), at(-6.0, 30.0), at(0.0, 24.0), at(6.0, 30.0), at(12.0, 26.0)], mouth));
    for dir in [-1.0f32, 1.0] {
        for k in 0..3 {
            let y = 18.0 + k as f32 * 8.0;
            clip.line_segment([at(dir * 34.0, y), at(dir * 92.0, y - 10.0 + k as f32 * 10.0)], Stroke::new(1.6 * s, Color32::from_rgb(250, 240, 230)));
        }
    }

    // Eyes.
    let eye_l = at(-30.0, -12.0);
    let eye_r = at(30.0, -12.0);
    match style {
        CatStyle::Scuba => {
            // Mask: strap, frame, glass.
            clip.line_segment([at(-82.0, -14.0), at(82.0, -14.0)], Stroke::new(7.0 * s, Color32::from_rgb(30, 30, 40)));
            let mask = Rect::from_center_size(at(0.0, -12.0), vec2(118.0 * s, 46.0 * s));
            clip.rect_filled(mask, 18.0 * s, Color32::from_rgb(255, 214, 0));
            clip.rect_filled(mask.shrink(6.0 * s), 14.0 * s, Color32::from_rgba_unmultiplied(160, 230, 255, 200));
            for e in [eye_l, eye_r] {
                clip.circle_filled(e, 9.0 * s, EYE);
                clip.circle_filled(e + vec2(-3.0, -3.0) * s, 3.0 * s, Color32::WHITE);
            }
            // Snorkel.
            clip.add(Shape::line(vec![at(56.0, 30.0), at(78.0, 26.0), at(84.0, -60.0), at(84.0, -96.0)], Stroke::new(8.0 * s, Color32::from_rgb(255, 92, 60))));
            clip.circle_filled(at(84.0, -98.0), 6.0 * s, Color32::from_rgb(255, 92, 60));
        }
        CatStyle::Judging => {
            for (e, dir) in [(eye_l, -1.0f32), (eye_r, 1.0)] {
                let lid = Rect::from_center_size(e, vec2(30.0 * s, 9.0 * s));
                clip.rect_filled(lid, 4.0 * s, Color32::from_rgb(250, 250, 240));
                clip.circle_filled(e + vec2(dir * 6.0 * s, 0.0), 4.0 * s, EYE);
                clip.line_segment([e + vec2(-18.0, -10.0 + dir * 4.0) * s, e + vec2(18.0, -10.0 - dir * 4.0) * s], Stroke::new(4.0 * s, FUR_DARK));
            }
            let dots = ((t * 2.0) as usize % 4).max(1);
            clip.text(at(70.0, -78.0), egui::Align2::CENTER_CENTER, ".".repeat(dots), theme::heavy(30.0 * s), Color32::WHITE);
        }
        CatStyle::Sleepy => {
            for e in [eye_l, eye_r] {
                let mut arc = Vec::new();
                for i in 0..=10 {
                    let a = std::f32::consts::PI * i as f32 / 10.0;
                    arc.push(e + vec2(-a.cos() * 12.0 * s, a.sin() * 6.0 * s));
                }
                clip.add(Shape::line(arc, Stroke::new(3.0 * s, EYE)));
            }
            for i in 0..3 {
                let phase = (t * 0.6 + i as f32 / 3.0) % 1.0;
                let pos = at(60.0 + phase * 40.0, -60.0 - phase * 70.0);
                let col = Color32::from_rgba_unmultiplied(220, 220, 255, ((1.0 - phase) * 255.0) as u8);
                clip.text(pos, egui::Align2::CENTER_CENTER, "z", theme::heavy((14.0 + phase * 16.0) * s), col);
            }
        }
        CatStyle::Alarm => {
            for e in [eye_l, eye_r] {
                clip.circle_filled(e, 15.0 * s, Color32::WHITE);
                clip.circle_filled(e, 6.0 * s, EYE);
            }
            for i in 0..6 {
                let a = i as f32 / 6.0 * std::f32::consts::TAU + t;
                let r0 = 100.0 * s;
                let dir = vec2(a.cos(), a.sin());
                clip.line_segment([h + dir * r0, h + dir * (r0 + 14.0 * s)], Stroke::new(4.0 * s, theme::AMBER));
            }
        }
    }
}

/// A user meme file decoded for display (GIFs keep their frames, capped and downscaled).
pub struct UserMeme {
    frames: Vec<(TextureHandle, Duration)>,
    total: Duration,
    pub size: egui::Vec2,
}

const MAX_SIDE: u32 = 280;
const MAX_FRAMES: usize = 90;

fn shrink(img: image::RgbaImage) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    let k = (MAX_SIDE as f32 / w.max(h) as f32).min(1.0);
    if k >= 1.0 {
        return img;
    }
    image::imageops::resize(&img, (w as f32 * k) as u32, (h as f32 * k) as u32, image::imageops::FilterType::Triangle)
}

impl UserMeme {
    pub fn load(ctx: &egui::Context, path: &Path) -> Option<UserMeme> {
        use image::AnimationDecoder;
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        let mut raw: Vec<(image::RgbaImage, Duration)> = Vec::new();
        if ext == "gif" {
            let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
            let dec = image::codecs::gif::GifDecoder::new(file).ok()?;
            for f in dec.into_frames().take(MAX_FRAMES) {
                let f = f.ok()?;
                let (n, d) = f.delay().numer_denom_ms();
                let ms = if d == 0 { 100 } else { (n / d).max(20) };
                raw.push((shrink(f.into_buffer()), Duration::from_millis(ms as u64)));
            }
        } else {
            raw.push((shrink(image::open(path).ok()?.to_rgba8()), Duration::from_secs(3600)));
        }
        let first = raw.first()?;
        let size = vec2(first.0.width() as f32, first.0.height() as f32);
        let mut total = Duration::ZERO;
        let frames = raw
            .into_iter()
            .enumerate()
            .map(|(i, (img, d))| {
                total += d;
                let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                (ctx.load_texture(format!("meme-{i}"), ci, TextureOptions::LINEAR), d)
            })
            .collect();
        Some(UserMeme { frames, total, size })
    }

    pub fn paint(&self, p: &Painter, rect: Rect, t: f32) {
        let mut at = Duration::from_secs_f32(t).as_millis() % self.total.as_millis().max(1);
        let mut tex = &self.frames[0].0;
        for (h, d) in &self.frames {
            if at < d.as_millis() {
                tex = h;
                break;
            }
            at -= d.as_millis();
        }
        let k = (rect.width() / self.size.x).min(rect.height() / self.size.y);
        let r = Rect::from_center_size(rect.center(), self.size * k);
        p.rect_filled(rect, 14.0, Color32::from_rgb(10, 10, 14));
        p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

pub fn media_files(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e.to_ascii_lowercase().as_str())))
        .collect();
    v.sort();
    v
}
