//! Native windows: the always-on-top timer pill, the alert card and the calculator.
//! Visual language matches the dashboard (assets/app.css): soft slate, regular weights, gentle fades.

use crate::app::{App, Level};
use crate::memes;
use crate::policy::{Gaze, Kind};
use crate::pomodoro::Phase;
use crate::theme::{self, display, icon, strong};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Shape, Stroke, StrokeKind, Ui, ViewportBuilder,
    ViewportCommand, ViewportId,
};

pub const PILL: egui::Vec2 = vec2(554.0, 60.0);

fn mmss(secs: f64) -> String {
    let s = secs.max(0.0).ceil() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// Colour that glides to `target` instead of snapping.
fn soft_color(ctx: &egui::Context, id: Id, target: Color32) -> Color32 {
    let ch = |i: usize, v: u8| ctx.animate_value_with_time(id.with(i), v as f32, 0.45) as u8;
    Color32::from_rgb(ch(0, target.r()), ch(1, target.g()), ch(2, target.b()))
}

/// Round Phosphor-icon button with a fading hover wash.
fn icon_button(ui: &mut Ui, rect: Rect, id: &str, tip: &str, glyph: char, ink: Option<Color32>) -> bool {
    let r = ui.interact(rect, ui.scope_id().with(id), Sense::click()).on_hover_text(tip);
    let hover = ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.15);
    let p = ui.painter();
    let wash = if r.is_pointer_button_down_on() { theme::HOVER } else { theme::RAISED };
    p.circle_filled(rect.center(), rect.width() / 2.0, wash.gamma_multiply(hover));
    let ink = ink.unwrap_or_else(|| theme::MUTED.lerp_to_gamma(theme::TEXT, hover));
    p.text(rect.center(), Align2::CENTER_CENTER, glyph, theme::icons(18.0), ink);
    r.clicked()
}

/// Camera toggle drawn with the painter (the Phosphor subset has no camera glyph); slashed when off.
fn camera_button(ui: &mut Ui, rect: Rect, on: bool) -> bool {
    let tip = if on { "Camera on: click to turn it off (Ctrl+Alt+K)" } else { "Camera off: click to turn it on (Ctrl+Alt+K)" };
    let r = ui.interact(rect, ui.scope_id().with("cam"), Sense::click()).on_hover_text(tip);
    let hover = ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.15);
    let p = ui.painter();
    let wash = if r.is_pointer_button_down_on() { theme::HOVER } else { theme::RAISED };
    p.circle_filled(rect.center(), rect.width() / 2.0, wash.gamma_multiply(hover));
    let ink = if on { theme::MUTED.lerp_to_gamma(theme::TEXT, hover) } else { theme::CAUTION };
    let stroke = Stroke::new(1.5, ink);
    let c = rect.center();
    let body = Rect::from_center_size(c + vec2(0.0, 1.0), vec2(16.0, 11.0));
    p.rect_stroke(body, CornerRadius::same(3), stroke, StrokeKind::Middle);
    p.line_segment([pos2(c.x - 3.5, body.top()), pos2(c.x - 2.0, body.top() - 2.5)], stroke);
    p.line_segment([pos2(c.x - 2.0, body.top() - 2.5), pos2(c.x + 2.0, body.top() - 2.5)], stroke);
    p.line_segment([pos2(c.x + 2.0, body.top() - 2.5), pos2(c.x + 3.5, body.top())], stroke);
    p.circle_stroke(body.center(), 3.0, stroke);
    if !on {
        p.line_segment([pos2(c.x - 9.0, c.y - 8.0), pos2(c.x + 9.0, c.y + 9.0)], Stroke::new(3.5, Color32::from_rgb(27, 31, 39)));
        p.line_segment([pos2(c.x - 9.0, c.y - 8.0), pos2(c.x + 9.0, c.y + 9.0)], stroke);
    }
    r.clicked()
}

fn text_icon_button(ui: &mut Ui, rect: Rect, id: &str, tip: &str, text: &str, ink: Color32) -> bool {
    let r = ui.interact(rect, ui.scope_id().with(id), Sense::click()).on_hover_text(tip);
    let hover = ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.15);
    ui.painter().circle_filled(rect.center(), rect.width() / 2.0, theme::RAISED.gamma_multiply(hover));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, strong(15.0), ink.lerp_to_gamma(theme::TEXT, hover));
    r.clicked()
}

/// Arc from 12 o'clock, clockwise, covering `frac` of the circle.
fn arc(p: &egui::Painter, c: egui::Pos2, r: f32, frac: f32, stroke: Stroke) {
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.0 {
        return;
    }
    let n = (64.0 * frac).ceil().max(2.0) as usize;
    let pts = (0..=n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, stroke));
}

impl App {
    /// The task strip deliberately lives outside the timer/button layout so concurrent pill work
    /// only needs to preserve this one call and its rectangle.
    fn pill_task_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        if self.pill_adding {
            let toggle = Rect::from_min_size(pos2(rect.right() - 61.0, rect.top()), vec2(58.0, rect.height()));
            let edit_rect = Rect::from_min_max(rect.min, pos2(toggle.left() - 5.0, rect.bottom()));
            let id = ui.scope_id().with("pill-task-add-field");
            let response = ui.put(
                edit_rect,
                egui::TextEdit::singleline(&mut self.pill_add_text)
                    .id(id)
                    .hint_text(if self.pill_add_idea { "Park an idea…" } else { "Add a task…" })
                    .font(FontId::proportional(11.5))
                    .frame(egui::Frame::NONE)
                    .margin(vec2(2.0, 1.0)),
            );
            if !response.has_focus() {
                response.request_focus();
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
            let chip = ui.interact(toggle, ui.scope_id().with("pill-task-kind"), Sense::click());
            ui.painter().rect_filled(toggle, CornerRadius::same(255), theme::RAISED);
            ui.painter().text(
                toggle.center(),
                Align2::CENTER_CENTER,
                if self.pill_add_idea { "Idea" } else { "Task" },
                FontId::proportional(10.5),
                if self.pill_add_idea { theme::CAUTION } else { theme::GAZE },
            );
            if chip.clicked() {
                self.pill_add_idea = !self.pill_add_idea;
            }
            let (enter, shift, escape) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift, i.key_pressed(egui::Key::Escape)));
            if enter {
                let idea = self.pill_add_idea || shift;
                if idea {
                    self.tasks.add_idea(&self.pill_add_text, crate::tasks::now_secs());
                } else {
                    self.tasks.add_task(&self.pill_add_text, 1, crate::tasks::now_secs());
                }
                self.tasks.save();
                self.finish_pill_add();
            } else if escape {
                self.finish_pill_add();
            }
            return;
        }

        let plus = Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(20.0, 20.0));
        if self.pill_return_hwnd == 0 && ui.rect_contains_pointer(plus) {
            // Capture on hover, before Windows activates the pill for the click.
            self.pill_return_hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize };
        }
        if text_icon_button(ui, plus, "task-add", "Quick add task or idea", "+", theme::MUTED) {
            self.pill_adding = true;
            self.pill_add_text.clear();
            self.pill_add_idea = false;
            ctx.request_repaint();
        }

        let Some(task) = self.tasks.active().cloned() else { return };
        let check = Rect::from_center_size(pos2(plus.left() - 15.0, rect.center().y), vec2(20.0, 20.0));
        if text_icon_button(ui, check, "task-done", "Mark active task done", "✓", theme::FOCUS) {
            self.complete_task_now(task.id);
        }

        let dots = task.estimate.max(task.actual).min(12);
        let dot_w = dots as f32 * 7.0 + if task.estimate.max(task.actual) > 12 { 20.0 } else { 0.0 };
        let right = check.left() - 6.0;
        let dot_x = right - dot_w;
        for i in 0..dots {
            let c = pos2(dot_x + 3.0 + i as f32 * 7.0, rect.center().y);
            if i < task.actual.min(task.estimate) {
                ui.painter().circle_filled(c, 2.2, theme::FOCUS);
            } else if i < task.estimate {
                ui.painter().circle_stroke(c, 2.2, Stroke::new(1.0, theme::MUTED));
            } else {
                ui.painter().circle_filled(c, 2.2, theme::CAUTION);
            }
        }
        if task.estimate.max(task.actual) > 12 {
            ui.painter().text(pos2(right - 17.0, rect.center().y), Align2::LEFT_CENTER, format!("+{}", task.estimate.max(task.actual) - 12), FontId::proportional(9.5), theme::MUTED);
        }
        let title_width = (dot_x - rect.left() - 4.0).max(15.0);
        let original_chars = task.title.chars().count();
        let mut title = task.title;
        while title.chars().count() > 1 {
            let shown = format!("{title}…");
            let width = ctx.fonts_mut(|f| f.layout_no_wrap(shown, FontId::proportional(11.5), theme::TEXT).size().x);
            if width <= title_width {
                break;
            }
            title.pop();
        }
        if title.chars().count() < original_chars {
            title.push('…');
        }
        ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, title, FontId::proportional(11.5), theme::MUTED);
    }

    fn finish_pill_add(&mut self) {
        self.pill_adding = false;
        self.pill_add_text.clear();
        if self.pill_return_hwnd != 0 {
            let hwnd = windows::Win32::Foundation::HWND(self.pill_return_hwnd as *mut core::ffi::c_void);
            let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd) };
            self.pill_return_hwnd = 0;
        }
    }

    fn status(&self) -> (Color32, String) {
        let brain = self.s.brainrot;
        if let Some(left) = self.pomo.brb {
            return if left < 0.0 {
                (theme::ALERT, format!("Away {} too long", mmss(-left)))
            } else {
                (theme::BREAK, format!("Back in {}", mmss(left)))
            };
        }
        if self.alarm_active() {
            return (theme::ALERT, if brain { "Not locked in".into() } else { "Distracted".into() });
        }
        match self.pomo.phase {
            Phase::ShortBreak | Phase::LongBreak => {
                let label = if self.pomo.phase == Phase::LongBreak { "Long break" } else { "Break" };
                (theme::BREAK, if self.pomo.running { label.into() } else { format!("{label}, paused") })
            }
            Phase::Focus if !self.pomo.running => (theme::MUTED, if self.pomo_started() { "Paused".into() } else { "Ready to focus".into() }),
            Phase::Focus if self.pomo.distracted_hold => (theme::CAUTION, "Timer waits for you".into()),
            Phase::Focus if !self.s.camera_enabled => (theme::FOCUS, if brain { "Locked in".into() } else { "Focusing".into() }),
            Phase::Focus if self.cam.error.is_some() => (theme::CAUTION, "Camera unavailable".into()),
            Phase::Focus => match self.cam.gaze {
                Some(Gaze::Screen) => (theme::GAZE, "On your screen".into()),
                Some(Gaze::Book) => (theme::FOCUS, if brain { "Locked in on the book".into() } else { "On your book".into() }),
                Some(Gaze::Away) => (theme::CAUTION, "Looking away".into()),
                Some(Gaze::Absent) => (theme::CAUTION, "Not at your desk".into()),
                None => (theme::MUTED, "Starting the camera".into()),
            },
        }
    }

    /// Share of the current phase already done (penalty time included), for the ring.
    fn phase_progress(&self) -> f32 {
        let mins = match self.pomo.phase {
            Phase::Focus => self.s.focus_min,
            Phase::ShortBreak => self.s.short_break_min,
            Phase::LongBreak => self.s.long_break_min,
        };
        let total = mins as f64 * 60.0 + if self.pomo.phase == Phase::Focus { self.pomo.block_added_secs as f64 } else { 0.0 };
        (1.0 - self.pomo.remaining / total.max(1.0)) as f32
    }

    /// Root viewport: the always-on-top timer bar.
    pub fn pill_ui(&mut self, ui: &mut Ui) {
        if (self.cal.ball_pending || self.cal.ball.is_some()) && self.ball_frame(ui) {
            return;
        }
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        let rect = Rect::from_min_size(full.min, PILL).shrink(2.0);

        // Place top-centre on first run, then remember where the user drags it.
        let info = ctx.input(|i| i.viewport().clone());
        match (self.s.overlay_pos, info.monitor_size, info.outer_rect) {
            (None, Some(m), _) => {
                let p = pos2((m.x - PILL.x) / 2.0, 12.0);
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
        if bg.secondary_clicked() || bg.double_clicked() {
            self.open_dashboard();
        }
        // Configured opacity at rest; fully opaque while the pointer is over the bar.
        let hovered = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| rect.contains(p));
        let lift = ctx.animate_bool_with_time(Id::unique("pill-hover"), hovered, 0.2);
        let rest = self.s.pill_opacity.clamp(0.3, 1.0);
        ui.multiply_opacity(rest + (1.0 - rest) * lift);

        let (tone, label) = self.status();
        let tone = soft_color(&ctx, Id::unique("pill-tone"), tone);
        let edge = soft_color(&ctx, Id::unique("pill-edge"), if self.alarm_active() { theme::ALERT.gamma_multiply(0.55) } else { theme::LINE });
        let p = ui.painter().clone();
        p.rect_filled(rect, CornerRadius::same(255), Color32::from_rgba_unmultiplied(27, 31, 39, 248));
        p.rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, edge), StrokeKind::Inside);

        // Progress ring with the state dot in the middle.
        let cy = rect.center().y;
        let ring = pos2(rect.left() + 30.0, cy);
        let progress = ctx.animate_value_with_time(Id::unique("pill-progress"), self.phase_progress(), 0.6);
        p.circle_stroke(ring, 14.0, Stroke::new(2.5, theme::RAISED));
        arc(&p, ring, 14.0, progress, Stroke::new(2.5, tone));
        p.circle_filled(ring, 4.0, tone);

        // Clock and status as one vertically centred block.
        let text_x = rect.left() + 56.0;
        let clock = ctx.fonts_mut(|f| f.layout_no_wrap(mmss(self.pomo.remaining), display(24.0), theme::TEXT));
        // With a task row under the clock, the status moves up beside the clock so the gaze
        // label stays visible.
        let task_row = self.tasks.active().is_some() || self.pill_adding;
        let mut side = if task_row { label.clone() } else { String::new() };
        let mut sub = if task_row { String::new() } else { label };
        if self.pomo.block_added_secs > 0 && self.pomo.phase == Phase::Focus {
            let extra = format!("   +{}m", self.pomo.block_added_secs.div_ceil(60));
            if task_row { side.push_str(&extra) } else { sub.push_str(&extra) }
        }
        let status = ctx.fonts_mut(|f| f.layout_no_wrap(sub, FontId::proportional(11.5), theme::MUTED));
        // Font line boxes carry extra leading; tighten the gap so the pair reads as one unit.
        let gap = -5.0;
        let block = clock.size().y + gap + status.size().y;
        let top = cy - block / 2.0;
        p.galley(pos2(text_x, top), clock.clone(), theme::TEXT);
        p.galley(pos2(text_x + 1.0, top + clock.size().y + gap), status, theme::MUTED);
        if !side.is_empty() {
            let clock_mid = top + clock.size().y / 2.0 + 1.0;
            p.text(pos2(text_x + clock.size().x + 10.0, clock_mid), Align2::LEFT_CENTER, side, FontId::proportional(11.5), tone);
        }

        // Buttons, right to left, with a hairline divider before them.
        let size = 32.0;
        let mut x = rect.right() - 12.0 - size / 2.0;
        let mut next = || {
            let r = Rect::from_center_size(pos2(x, cy), vec2(size, size));
            x -= size + 2.0;
            r
        };
        let (app, calc, cam, brb, play) = (next(), next(), next(), next(), next());
        let divider = play.left() - 8.0;
        p.line_segment([pos2(divider, cy - 13.0), pos2(divider, cy + 13.0)], Stroke::new(1.0, theme::LINE));
        let task_rect = Rect::from_min_max(pos2(text_x + 1.0, top + clock.size().y + gap), pos2(divider - 7.0, top + clock.size().y + gap + 17.0));
        self.pill_task_ui(ui, task_rect);
        if icon_button(ui, app, "app", "Open LockIn (Ctrl+Alt+O)", icon::APP, None) {
            self.open_dashboard();
        }
        if icon_button(ui, calc, "calc", "Calculator (Ctrl+Alt+C)", icon::CALCULATOR, None) {
            self.calc_open = !self.calc_open;
            if self.calc_open {
                self.calc.open();
                self.calc_min = false;
            }
        }
        if camera_button(ui, cam, self.s.camera_enabled) {
            self.toggle_camera();
        }
        let back = self.pomo.brb.is_some() || self.alarm_active();
        let can_step_away = self.pomo.focusing();
        let (glyph, ink, tip) = if back {
            (icon::BACK, Some(theme::FOCUS), "I'm back (Ctrl+Alt+B)")
        } else if can_step_away {
            (icon::COFFEE, None, "Step away without alarms (Ctrl+Alt+B)")
        } else {
            (icon::COFFEE, Some(theme::MUTED.gamma_multiply(0.45)), "Step away works during a focus block (Ctrl+Alt+B)")
        };
        if icon_button(ui, brb, "brb", tip, glyph, ink) && (back || can_step_away) {
            self.brb_or_back();
        }
        let glyph = if self.pomo.running { icon::PAUSE } else { icon::PLAY };
        if icon_button(ui, play, "play", "Start or pause (Ctrl+Alt+P)", glyph, Some(theme::TEXT)) {
            self.toggle_timer();
        }
        if lift > 0.0 && lift < 1.0 {
            ctx.request_repaint();
        }
    }

    pub fn alert_window(&mut self, ctx: &egui::Context) {
        let Some(alert) = &self.alert else { return };
        let has_meme = alert.cat.is_some() || self.user_meme.is_some();
        let size = vec2(440.0, if has_meme { 410.0 } else { 196.0 });
        let monitor = ctx.input_for(ViewportId::ROOT, |i| i.viewport().monitor_size).unwrap_or(vec2(1920.0, 1080.0));
        let level = alert.level;
        let builder = ViewportBuilder::default()
            .with_title("LockIn alert")
            .with_inner_size(size)
            .with_position(pos2((monitor.x - size.x) / 2.0, 84.0))
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
        let t = alert.opened.elapsed().as_secs_f32();
        // Fade and settle in.
        let a = (t / 0.25).min(1.0);
        let ease = 1.0 - (1.0 - a).powi(3);
        ui.multiply_opacity(ease);
        let rect = ui.max_rect().shrink(4.0).translate(vec2(0.0, (1.0 - ease) * 10.0));
        let bg = ui.interact(rect, ui.scope_id().with("alert-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let (accent, tag) = match alert.level {
            Level::Alarm => (theme::ALERT, "Alarm"),
            Level::Warn => (theme::CAUTION, "Heads up"),
            Level::Ask => (theme::GAZE, "Quick question"),
            Level::Info => (theme::FOCUS, "LockIn"),
        };
        let p = ui.painter().clone();
        p.rect_filled(rect, CornerRadius::same(20), theme::SURFACE);
        p.rect_stroke(rect, CornerRadius::same(20), Stroke::new(1.0, theme::LINE), StrokeKind::Inside);

        let mut y = rect.top() + 16.0;
        let meme_rect = Rect::from_min_size(pos2(rect.left() + 16.0, y), vec2(rect.width() - 32.0, 180.0));
        if let Some(m) = &self.user_meme {
            m.paint(&p, meme_rect, t);
            y += 196.0;
        } else if let Some(cat) = alert.cat {
            memes::paint_cat(&p, meme_rect, t, cat);
            y += 196.0;
        }
        p.circle_filled(pos2(rect.left() + 28.0, y + 8.0), 3.5, accent);
        p.text(pos2(rect.left() + 38.0, y + 8.0), Align2::LEFT_CENTER, tag, FontId::proportional(12.5), theme::MUTED);
        y += 22.0;
        let w = rect.width() - 48.0;
        let title = ctx.fonts_mut(|f| f.layout(alert.line.title.clone(), strong(20.0), theme::TEXT, w));
        let th = title.size().y;
        p.galley(pos2(rect.left() + 24.0, y), title, theme::TEXT);
        y += th + 2.0;
        let body = ctx.fonts_mut(|f| f.layout(alert.line.body.clone(), FontId::proportional(14.0), theme::MUTED, w));
        let bh = body.size().y;
        p.galley(pos2(rect.left() + 24.0, y), body, theme::MUTED);
        y += bh + 8.0;
        if alert.level == Level::Alarm {
            let mut chips = Vec::new();
            if alert.added_secs > 0 {
                chips.push(format!("+{} min focus", alert.added_secs.div_ceil(60)));
            }
            if self.s.penalty_score && alert.kind.as_ref().is_some_and(|k| *k != Kind::BrbOverdue || self.s.penalty_brb_overdue) {
                chips.push(format!("-{} score", self.s.penalty_score_points));
            }
            if self.s.penalty_pause_timer && self.pomo.distracted_hold {
                chips.push("timer waits for you".into());
            }
            let mut x = rect.left() + 24.0;
            for c in chips {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(c, FontId::proportional(12.0), theme::ALERT));
                let r = Rect::from_min_size(pos2(x, y), g.size() + vec2(18.0, 8.0));
                p.rect_filled(r, CornerRadius::same(255), theme::ALERT.gamma_multiply(0.14));
                p.galley(r.min + vec2(9.0, 4.0), g, theme::ALERT);
                x = r.right() + 6.0;
            }
        }

        // Buttons.
        let btn_rect = Rect::from_min_max(pos2(rect.left() + 20.0, rect.bottom() - 56.0), pos2(rect.right() - 20.0, rect.bottom() - 16.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let button = |text: &str, primary: bool| {
            let (fill, ink) = if primary { (theme::TEXT, theme::BG) } else { (theme::RAISED, theme::TEXT) };
            egui::Button::new(egui::RichText::new(text).font(strong(14.0)).color(ink)).fill(fill).corner_radius(255).min_size(vec2(0.0, 38.0))
        };
        let level = alert.level;
        let site = match &alert.kind {
            Some(Kind::Site(d) | Kind::ScreenSite(d)) => Some(d.clone()),
            _ => None,
        };
        match level {
            Level::Alarm => {
                if ui.add(button(if self.s.brainrot { "I'm locked in" } else { "I'm back" }, true)).clicked() {
                    self.im_back();
                }
                if let Some(d) = site {
                    if ui.add(button("It's for study", false)).clicked() {
                        self.policy.session_allowed.insert(d);
                        self.im_back();
                    }
                }
            }
            Level::Warn => {
                if ui.add(button("Got it", true)).clicked() {
                    self.alert = None;
                    self.user_meme = None;
                }
                if let Some(d) = site {
                    if ui.add(button("It's for study", false)).clicked() {
                        self.policy.session_allowed.insert(d);
                        self.alert = None;
                        self.user_meme = None;
                    }
                }
            }
            Level::Ask => {
                if self.alert.as_ref().is_some_and(|a| a.task_question.is_some()) {
                    if ui.add(button("Done", true)).clicked() {
                        self.answer_task_question(true);
                    } else if ui.add(button("Carry on", false)).clicked() {
                        self.answer_task_question(false);
                    }
                } else {
                    if ui.add(button("For study", true)).clicked() {
                        self.answer_ask(true);
                    } else if ui.add(button("Distraction", false)).clicked() {
                        self.answer_ask(false);
                    } else if let Some(a) = &mut self.alert {
                        ui.checkbox(&mut a.remember, "Remember");
                    }
                }
            }
            Level::Info => {
                if ui.add(button("OK", true)).clicked() {
                    self.alert = None;
                }
            }
        }
        if t < 0.3 {
            ctx.request_repaint();
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
        // Test-calculator look: 214 px wide, grey body, blue title bar, red <- C +/-, green tall =.
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
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, strong(13.0), Color32::WHITE);
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
            p.text(r2.left_top() + vec2(5.0, 2.0), Align2::LEFT_TOP, "M", strong(12.0), Color32::from_rgb(0x44, 0x44, 0x44));
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
                1 => (RED.0, RED.1, Color32::WHITE, strong(12.0)),
                2 => (GREEN.0, GREEN.1, Color32::WHITE, strong(20.0)),
                _ => (if resp.hovered() { KEY_HOVER } else { KEY }, EDGE, KEY_TEXT, strong(12.0)),
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
