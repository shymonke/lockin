//! Overlay pill (root window), alert card and the CAT calculator window.

use crate::app::{App, Level};
use crate::memes;
use crate::policy::{Gaze, Kind};
use crate::pomodoro::Phase;
use crate::theme::{self, heavy};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Shape, Stroke, StrokeKind, Ui, ViewportBuilder,
    ViewportCommand, ViewportId,
};

pub const PILL: egui::Vec2 = vec2(340.0, 64.0);

fn mmss(secs: f64) -> String {
    let s = secs.max(0.0).ceil() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// A round icon button painted by hand (no icon font needed).
fn icon_button(ui: &mut Ui, rect: Rect, id: &str, tip: &str, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> bool {
    let r = ui.interact(rect, ui.scope_id().with(id), Sense::click()).on_hover_text(tip);
    let fill = if r.is_pointer_button_down_on() {
        theme::LIME.linear_multiply(0.45)
    } else if r.hovered() {
        Color32::from_rgb(56, 56, 72)
    } else {
        theme::CARD_HI
    };
    let p = ui.painter();
    p.circle_filled(rect.center(), rect.width() / 2.0, fill);
    paint(p, rect.shrink(9.0), theme::TEXT);
    r.clicked()
}

fn paint_play(p: &egui::Painter, r: Rect, c: Color32) {
    p.add(Shape::convex_polygon(vec![r.left_top() + vec2(2.0, 0.0), r.left_bottom() + vec2(2.0, 0.0), pos2(r.right(), r.center().y)], c, Stroke::NONE));
}
fn paint_pause(p: &egui::Painter, r: Rect, c: Color32) {
    let w = r.width() * 0.32;
    p.rect_filled(Rect::from_min_size(r.left_top() + vec2(1.0, 0.0), vec2(w, r.height())), 2.0, c);
    p.rect_filled(Rect::from_min_size(pos2(r.right() - w - 1.0, r.top()), vec2(w, r.height())), 2.0, c);
}
fn paint_calc(p: &egui::Painter, r: Rect, c: Color32) {
    p.rect_stroke(r, 2.0, Stroke::new(1.6, c), StrokeKind::Inside);
    let cell = r.width() / 3.0;
    for i in 0..3 {
        for j in 1..3 {
            p.circle_filled(pos2(r.left() + cell * (i as f32 + 0.5), r.top() + cell * (j as f32 + 0.5) - 1.0), 1.4, c);
        }
    }
    p.line_segment([pos2(r.left() + 3.0, r.top() + cell * 0.6), pos2(r.right() - 3.0, r.top() + cell * 0.6)], Stroke::new(1.6, c));
}
fn paint_gear(p: &egui::Painter, r: Rect, c: Color32) {
    let ctr = r.center();
    let rad = r.width() * 0.3;
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([ctr + d * rad, ctr + d * (rad + 4.0)], Stroke::new(3.0, c));
    }
    p.circle_stroke(ctr, rad, Stroke::new(2.2, c));
    p.circle_filled(ctr, 2.0, c);
}

impl App {
    fn status(&self) -> (Color32, String) {
        let brain = self.s.brainrot;
        if let Some(left) = self.pomo.brb {
            return if left < 0.0 {
                (theme::RED, format!("BRB OVERDUE {}", mmss(-left)))
            } else {
                (theme::PURPLE, format!("BRB · BACK IN {}", mmss(left)))
            };
        }
        if self.alarm_active() {
            return (theme::RED, if brain { "NOT LOCKED IN".into() } else { "DISTRACTED".into() });
        }
        match self.pomo.phase {
            Phase::ShortBreak | Phase::LongBreak => {
                let label = if self.pomo.phase == Phase::LongBreak { "LONG BREAK" } else { "BREAK" };
                (theme::BLUE, if self.pomo.running { label.into() } else { format!("{label} · PAUSED") })
            }
            Phase::Focus if !self.pomo.running => (theme::MUTED, "PAUSED · CTRL+ALT+P".into()),
            Phase::Focus => {
                if self.pomo.distracted_hold {
                    return (theme::AMBER, "TIMER HELD".into());
                }
                let what = if !self.s.camera_enabled {
                    "FOCUS".into()
                } else if self.cam.error.is_some() {
                    "FOCUS · CAMERA ERROR".into()
                } else {
                    match self.cam.gaze {
                        Some(Gaze::Screen) => if brain { "LOCKED IN · SCREEN" } else { "FOCUS · ON SCREEN" }.into(),
                        Some(Gaze::Book) => if brain { "LOCKED IN · BOOK" } else { "FOCUS · ON BOOK" }.into(),
                        Some(Gaze::Away) => "LOOKING AWAY".into(),
                        Some(Gaze::Absent) => "NOT AT DESK".into(),
                        None => "FOCUS · STARTING CAMERA".into(),
                    }
                };
                let col = match self.cam.gaze {
                    Some(Gaze::Away | Gaze::Absent) => theme::AMBER,
                    _ => theme::LIME,
                };
                (col, what)
            }
        }
    }

    /// Root viewport: the always-on-top pill.
    pub fn pill_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        let rect = Rect::from_min_size(full.min, PILL).shrink(2.0);

        // Place top-centre on first run, then remember where the user drags it.
        let info = ctx.input(|i| i.viewport().clone());
        match (self.s.overlay_pos, info.monitor_size, info.outer_rect) {
            (None, Some(m), _) => {
                let p = pos2((m.x - PILL.x) / 2.0, 14.0);
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(p));
                self.s.overlay_pos = Some([p.x, p.y]);
            }
            (Some(saved), _, Some(r)) if (r.min.x - saved[0]).abs() > 1.0 || (r.min.y - saved[1]).abs() > 1.0 => {
                self.s.overlay_pos = Some([r.min.x.round(), r.min.y.round()]);
            }
            _ => {}
        }

        let bg = ui.interact(rect, ui.scope_id().with("pill-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.secondary_clicked() {
            self.settings_open = true;
        }

        let (dot, label) = self.status();
        let p = ui.painter();
        let border = if self.alarm_active() {
            let t = ctx.input(|i| i.time) as f32;
            theme::RED.linear_multiply(0.6 + 0.4 * (t * 6.0).sin().abs())
        } else if self.s.brainrot {
            theme::PINK
        } else {
            theme::LINE
        };
        p.rect_filled(rect, CornerRadius::same(30), Color32::from_rgba_unmultiplied(14, 14, 18, 240));
        p.rect_stroke(rect, CornerRadius::same(30), Stroke::new(1.5, border), StrokeKind::Inside);
        let dc = pos2(rect.left() + 26.0, rect.center().y);
        p.circle_filled(dc, 11.0, dot.linear_multiply(0.18));
        p.circle_filled(dc, 6.0, dot);

        let shown = self.pomo.remaining;
        p.text(pos2(rect.left() + 46.0, rect.top() + 7.0), Align2::LEFT_TOP, mmss(shown), heavy(27.0), theme::TEXT);
        if self.pomo.block_added_secs > 0 && self.pomo.phase == Phase::Focus {
            // Compact penalty chip beside the clock, clear of the buttons.
            let m = self.pomo.block_added_secs / 60;
            let s = self.pomo.block_added_secs % 60;
            let chip = if s == 0 { format!("+{m}m") } else { format!("+{m}:{s:02}") };
            p.text(pos2(rect.left() + 134.0, rect.top() + 11.0), Align2::LEFT_TOP, chip, heavy(11.0), theme::RED);
        }
        p.text(pos2(rect.left() + 47.0, rect.bottom() - 8.0), Align2::LEFT_BOTTOM, label, FontId::proportional(10.5), theme::MUTED);

        // Buttons, right to left.
        let size = 34.0;
        let cy = rect.center().y;
        let mut x = rect.right() - 12.0 - size / 2.0;
        let mut next = || {
            let r = Rect::from_center_size(pos2(x, cy), vec2(size, size));
            x -= size + 6.0;
            r
        };
        let (gear, calc, brb, play) = (next(), next(), next(), next());
        if icon_button(ui, gear, "gear", "Settings (Ctrl+Alt+O, or right-click the pill)", paint_gear) {
            self.settings_open = !self.settings_open;
        }
        if icon_button(ui, calc, "calc", "CAT calculator (Ctrl+Alt+C)", paint_calc) {
            self.calc_open = !self.calc_open;
            if self.calc_open {
                self.calc.open();
                self.calc_min = false;
            }
        }
        let back = self.pomo.brb.is_some() || self.alarm_active();
        let brb_tip = if back { "I'm back (Ctrl+Alt+B)" } else { "BRB pause (Ctrl+Alt+B)" };
        let brb_enabled = back || self.pomo.focusing();
        if icon_button(ui, brb, "brb", brb_tip, |p, r, c| {
            let (txt, col) = if back { ("BACK", theme::LIME) } else { ("BRB", if brb_enabled { c } else { theme::MUTED }) };
            p.text(r.center(), Align2::CENTER_CENTER, txt, heavy(if back { 9.5 } else { 11.0 }), col);
        }) && brb_enabled
        {
            self.brb_or_back();
        }
        let running = self.pomo.running;
        if icon_button(ui, play, "play", "Start / pause (Ctrl+Alt+P)", |p, r, c| if running { paint_pause(p, r, c) } else { paint_play(p, r, c) }) {
            self.toggle_timer();
        }
    }

    pub fn alert_window(&mut self, ctx: &egui::Context) {
        let Some(alert) = &self.alert else { return };
        let has_meme = alert.cat.is_some() || self.user_meme.is_some();
        let size = vec2(460.0, if has_meme { 430.0 } else { 200.0 });
        let monitor = ctx.input_for(ViewportId::ROOT, |i| i.viewport().monitor_size).unwrap_or(vec2(1920.0, 1080.0));
        let level = alert.level;
        let builder = ViewportBuilder::default()
            .with_title("LockIn alert")
            .with_inner_size(size)
            .with_position(pos2((monitor.x - size.x) / 2.0, 90.0))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(matches!(level, Level::Alarm | Level::Ask));
        let id = ViewportId::from_hash_of(("alert", alert.opened));
        ctx.show_viewport_immediate(id, builder, |ui, _| self.alert_ui(ui));
    }

    fn alert_ui(&mut self, ui: &mut Ui) {
        let Some(alert) = &mut self.alert else { return };
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect().shrink(4.0);
        let bg = ui.interact(rect, ui.scope_id().with("alert-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let accent = match alert.level {
            Level::Alarm => theme::RED,
            Level::Warn => theme::AMBER,
            Level::Ask => theme::BLUE,
            Level::Info => theme::LIME,
        };
        let t = alert.opened.elapsed().as_secs_f32();
        let p = ui.painter().clone();
        p.rect_filled(rect, CornerRadius::same(22), theme::CARD);
        p.rect_stroke(rect, CornerRadius::same(22), Stroke::new(2.0, accent.linear_multiply(0.8)), StrokeKind::Inside);
        p.rect_filled(Rect::from_min_size(rect.min + vec2(22.0, 0.0), vec2(rect.width() - 44.0, 5.0)), 3.0, accent);

        let mut y = rect.top() + 18.0;
        let meme_rect = Rect::from_min_size(pos2(rect.left() + 18.0, y), vec2(rect.width() - 36.0, 190.0));
        if let Some(m) = &self.user_meme {
            m.paint(&p, meme_rect, t);
            y += 202.0;
        } else if let Some(cat) = alert.cat {
            memes::paint_cat(&p, meme_rect, t, cat);
            y += 202.0;
        }
        let tag = match alert.level {
            Level::Alarm => "ALARM",
            Level::Warn => "WARNING",
            Level::Ask => "QUICK CHECK",
            Level::Info => "LOCKIN",
        };
        p.text(pos2(rect.left() + 24.0, y), Align2::LEFT_TOP, tag, FontId::proportional(11.0), accent);
        y += 16.0;
        let title = ctx.fonts_mut(|f| f.layout(alert.line.title.clone(), heavy(25.0), theme::TEXT, rect.width() - 48.0));
        let th = title.size().y;
        p.galley(pos2(rect.left() + 24.0, y), title, theme::TEXT);
        y += th + 4.0;
        let body = ctx.fonts_mut(|f| f.layout(alert.line.body.clone(), FontId::proportional(15.0), theme::MUTED, rect.width() - 48.0));
        let bh = body.size().y;
        p.galley(pos2(rect.left() + 24.0, y), body, theme::MUTED);
        y += bh + 8.0;
        if alert.level == Level::Alarm {
            let mut chips = Vec::new();
            if alert.added_secs > 0 {
                chips.push(format!("+{} focus time", mmss(alert.added_secs as f64)));
            }
            if self.s.penalty_score && alert.kind.as_ref().is_some_and(|k| *k != Kind::BrbOverdue || self.s.penalty_brb_overdue) {
                chips.push(format!("-{} score", self.s.penalty_score_points));
            }
            if self.s.penalty_pause_timer && self.pomo.distracted_hold {
                chips.push("timer held".into());
            }
            let mut x = rect.left() + 24.0;
            for c in chips {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(c, FontId::proportional(12.5), theme::RED));
                let r = Rect::from_min_size(pos2(x, y), g.size() + vec2(16.0, 6.0));
                p.rect_filled(r, 10.0, theme::RED.linear_multiply(0.18));
                p.galley(r.min + vec2(8.0, 3.0), g, theme::RED);
                x = r.right() + 6.0;
            }
        }

        // Buttons.
        let btn_rect = Rect::from_min_max(pos2(rect.left() + 22.0, rect.bottom() - 58.0), pos2(rect.right() - 22.0, rect.bottom() - 16.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let big = |text: &str, fill: Color32, dark: bool| {
            egui::Button::new(egui::RichText::new(text).font(heavy(15.0)).color(if dark { theme::INK } else { theme::TEXT }))
                .fill(fill)
                .corner_radius(12)
                .min_size(vec2(0.0, 40.0))
        };
        let level = alert.level;
        let site = match &alert.kind {
            Some(Kind::Site(d) | Kind::ScreenSite(d)) => Some(d.clone()),
            _ => None,
        };
        match level {
            Level::Alarm => {
                if ui.add(big(if self.s.brainrot { "I'M LOCKED IN" } else { "I'M BACK" }, theme::LIME, true)).clicked() {
                    self.im_back();
                }
                if let Some(d) = site {
                    if ui.add(big("It's for study", theme::CARD_HI, false)).clicked() {
                        self.policy.session_allowed.insert(d);
                        self.im_back();
                    }
                }
            }
            Level::Warn => {
                if ui.add(big("Got it", theme::AMBER, true)).clicked() {
                    self.alert = None;
                    self.user_meme = None;
                }
                if let Some(d) = site {
                    if ui.add(big("It's for study", theme::CARD_HI, false)).clicked() {
                        self.policy.session_allowed.insert(d);
                        self.alert = None;
                        self.user_meme = None;
                    }
                }
            }
            Level::Ask => {
                if ui.add(big("Studying", theme::LIME, true)).clicked() {
                    self.answer_ask(true);
                } else if ui.add(big("Distraction", theme::RED, false)).clicked() {
                    self.answer_ask(false);
                } else if let Some(a) = &mut self.alert {
                    ui.checkbox(&mut a.remember, "Remember");
                }
            }
            Level::Info => {
                if ui.add(big("OK", theme::LIME, true)).clicked() {
                    self.alert = None;
                }
            }
        }
    }

    pub fn calc_window(&mut self, ctx: &egui::Context) {
        if !self.calc_open {
            self.calc_spawn = None;
            return;
        }
        // Position only at spawn; afterwards the user drags it and we just record where it went.
        let spawn = *self.calc_spawn.get_or_insert_with(|| {
            self.s.calc_pos.unwrap_or_else(|| {
                let m = ctx.input_for(ViewportId::ROOT, |i| i.viewport().monitor_size).unwrap_or(vec2(1920.0, 1080.0));
                [m.x - 260.0, 120.0]
            })
        });
        let h = if self.calc_min { 44.0 } else { 316.0 };
        let b = ViewportBuilder::default()
            .with_title("Normal Calculator")
            .with_inner_size(vec2(216.0, h))
            .with_position(pos2(spawn[0], spawn[1]))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top();
        ctx.show_viewport_immediate(ViewportId::from_hash_of("calc"), b, |ui, _| self.calc_ui(ui));
    }

    fn calc_ui(&mut self, ui: &mut Ui) {
        // TCS iON look: 214 px wide, grey body, blue title bar, red <- C +/-, green tall =.
        const BODY: Color32 = Color32::from_rgb(0xda, 0xda, 0xda);
        const EDGE: Color32 = Color32::from_rgb(0xaa, 0xaa, 0xaa);
        const KEY: Color32 = Color32::from_rgb(0xf1, 0xf1, 0xf1);
        const KEY_HOVER: Color32 = Color32::from_rgb(0xea, 0xea, 0xea);
        const KEY_TEXT: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
        const TITLE: Color32 = Color32::from_rgb(0x42, 0x86, 0xf3);
        const RED: (Color32, Color32) = (Color32::from_rgb(0xe7, 0x4c, 0x3c), Color32::from_rgb(0xc0, 0x39, 0x2b));
        const GREEN: (Color32, Color32) = (Color32::from_rgb(0x2e, 0xcc, 0x71), Color32::from_rgb(0x27, 0xae, 0x60));

        let ctx = ui.ctx().clone();
        if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
            let pos = [r.min.x.round(), r.min.y.round()];
            if self.s.calc_pos != Some(pos) {
                self.s.calc_pos = Some(pos);
            }
        }
        let full = ui.max_rect();
        let w = 214.0;
        let total = Rect::from_min_size(full.min, vec2(w, full.height().min(if self.calc_min { 43.0 } else { 312.0 })));
        let p = ui.painter().clone();
        p.rect_filled(total, 5.0, Color32::from_rgb(0x99, 0x99, 0x99));
        p.rect_filled(Rect::from_min_max(total.min, total.max - vec2(0.0, 7.0)), 5.0, BODY);
        p.rect_stroke(total, 5.0, Stroke::new(1.0, EDGE), StrokeKind::Inside);

        // Title bar (drag handle).
        let title = Rect::from_min_size(total.min, vec2(w, 36.0));
        p.rect_filled(title, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, TITLE);
        let drag = ui.interact(title, ui.scope_id().with("calc-title"), Sense::click_and_drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        p.text(title.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, "Normal Calculator", FontId::proportional(14.0), Color32::WHITE);
        let tb = |ui: &mut Ui, x: f32, id: &str, label: &str| {
            let r = Rect::from_center_size(pos2(title.left() + x, title.center().y), vec2(22.0, 22.0));
            let resp = ui.interact(r, ui.scope_id().with(id), Sense::click());
            ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, Color32::from_white_alpha(128)), StrokeKind::Inside);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, heavy(13.0), Color32::WHITE);
            resp.clicked()
        };
        if tb(ui, w - 50.0, "min", if self.calc_min { "□" } else { "_" }) {
            self.calc_min = !self.calc_min;
        }
        if tb(ui, w - 22.0, "close", "×") {
            self.calc_open = false;
        }
        if self.calc_min {
            return;
        }

        // Keyboard (our addition; the exam calculator is mouse-only).
        let mut keys = Vec::new();
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Text(t) => {
                        for ch in t.chars() {
                            let k = match ch {
                                '0'..='9' | '.' | '+' | '-' | '*' | '/' | '%' => Some(ch.to_string()),
                                'x' | 'X' => Some("*".into()),
                                '=' => Some("equals".into()),
                                _ => None,
                            };
                            keys.extend(k);
                        }
                    }
                    egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.ctrl && !modifiers.alt => match key {
                        egui::Key::Enter => keys.push("equals".into()),
                        egui::Key::Backspace => keys.push("back".into()),
                        egui::Key::Delete | egui::Key::Escape => keys.push("clear".into()),
                        _ => {}
                    },
                    _ => {}
                }
            }
        });

        let body = Rect::from_min_size(title.left_bottom() + vec2(10.0, 10.0), vec2(w - 20.0, 0.0));
        let screen = |p: &egui::Painter, y: f32, text: &str, color: Color32| {
            let r = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), 30.0));
            p.rect_filled(r, 3.0, EDGE);
            p.rect_filled(Rect::from_min_max(r.min + vec2(1.0, 1.0), r.max - vec2(1.0, 3.0)), 2.0, Color32::WHITE);
            let clip = p.with_clip_rect(r.shrink(2.0));
            clip.text(r.right_center() - vec2(6.0, 1.0), Align2::RIGHT_CENTER, text, FontId::proportional(17.0), color);
            r
        };
        let view = self.calc.view();
        let (expr, disp, mem) = (view.expression.to_string(), view.display.to_string(), view.memory);
        screen(&p, body.top(), &expr, Color32::from_rgb(0x33, 0x33, 0x33));
        let r2 = screen(&p, body.top() + 37.0, &disp, Color32::BLACK);
        if mem {
            p.text(r2.left_top() + vec2(5.0, 2.0), Align2::LEFT_TOP, "M", heavy(12.0), Color32::from_rgb(0x44, 0x44, 0x44));
        }

        // (id, label, col, row, colspan, rowspan, style) matching the CSS grid auto-placement.
        #[rustfmt::skip]
        const GRID: [(&str, &str, u8, u8, u8, u8, u8); 27] = [
            ("MC","MC",0,0,1,1,0),("MR","MR",1,0,1,1,0),("MS","MS",2,0,1,1,0),("M+","M+",3,0,1,1,0),("M-","M-",4,0,1,1,0),
            ("back","←",0,1,2,1,1),("clear","C",2,1,1,1,1),("negate","+/-",3,1,1,1,1),("sqrt","√",4,1,1,1,0),
            ("7","7",0,2,1,1,0),("8","8",1,2,1,1,0),("9","9",2,2,1,1,0),("/","/",3,2,1,1,0),("%","%",4,2,1,1,0),
            ("4","4",0,3,1,1,0),("5","5",1,3,1,1,0),("6","6",2,3,1,1,0),("*","*",3,3,1,1,0),("reciprocal","1/x",4,3,1,1,0),
            ("1","1",0,4,1,1,0),("2","2",1,4,1,1,0),("3","3",2,4,1,1,0),("-","-",3,4,1,1,0),("equals","=",4,4,1,2,2),
            ("0","0",0,5,2,1,0),(".",".",2,5,1,1,0),("+","+",3,5,1,1,0),
        ];
        let top = body.top() + 74.0;
        for (id, label, c, r, cs, rs, style) in GRID {
            let min = pos2(body.left() + c as f32 * 40.0, top + r as f32 * 30.0);
            let size = vec2(cs as f32 * 35.0 + (cs - 1) as f32 * 5.0, rs as f32 * 25.0 + (rs - 1) as f32 * 5.0);
            let rect = Rect::from_min_size(min, size);
            let resp = ui.interact(rect, ui.scope_id().with(id), Sense::click());
            let down = resp.is_pointer_button_down_on();
            let (fill, edge, text_col, font) = match style {
                1 => (RED.0, RED.1, Color32::WHITE, heavy(12.0)),
                2 => (GREEN.0, GREEN.1, Color32::WHITE, heavy(20.0)),
                _ => (if resp.hovered() { KEY_HOVER } else { KEY }, EDGE, KEY_TEXT, heavy(12.0)),
            };
            let bottom = if down { 1.0 } else { 3.0 };
            p.rect_filled(rect, 4.0, edge);
            p.rect_filled(Rect::from_min_max(rect.min + vec2(1.0, 1.0), rect.max - vec2(1.0, bottom)), 3.0, fill);
            p.text(rect.center() - vec2(0.0, bottom / 2.0), Align2::CENTER_CENTER, label, font, text_col);
            if resp.clicked() {
                keys.push(id.to_string());
            }
        }
        for k in keys {
            let _ = self.calc.press(&k);
        }
    }
}
