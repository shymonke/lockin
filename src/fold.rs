//! Idle timer bar: after a few seconds without the mouse on it, the pill folds into a small ball
//! drawn like the LockIn mark: a clock ring that fills with the phase, a hand at its end, and the
//! calibration ball's blue in the middle (amber looking away, red on a blocked site or alarm).
//! Hovering it, an alert or a quick add unfolds the pill again.
//!
//! The window keeps the pill's size and place the whole time: resizing it to a small square made
//! Windows flash the old picture stretched or squeezed for a frame. Folded, it turns click-through
//! instead, so the empty rest of the strip lets clicks pass, and hovering is read from the OS
//! cursor.

use crate::theme;
use eframe::egui::{self, pos2, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2, ViewportCommand};
use std::time::{Duration, Instant};

/// Pill ⇄ ball morph time, shared with the calibration ball so both unfold alike.
pub const MORPH: f32 = 0.45;
/// Ball radius.
pub const R: f32 = 11.0;
/// Folded, a cursor within this distance of the ball's centre unfolds it.
const HOVER: f32 = 32.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Open,
    Folding,
    Folded,
    Unfolding,
}

pub struct Fold {
    pub stage: Stage,
    since: Instant,
    /// Last time the cursor was over the pill (or the ball).
    touched: Instant,
}

impl Default for Fold {
    fn default() -> Self {
        Fold { stage: Stage::Open, since: Instant::now(), touched: Instant::now() }
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Cursor in points, from the OS so it also works while the bar is click-through.
fn cursor(ctx: &egui::Context) -> Option<Pos2> {
    let mut p = windows::Win32::Foundation::POINT::default();
    // SAFETY: plain out-parameter call.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.ok()?;
    let ppp = ctx.pixels_per_point();
    Some(pos2(p.x as f32 / ppp, p.y as f32 / ppp))
}

/// Screen point (points) → this frame's local coordinates, using where the OS has the window now.
fn local(ui: &Ui, screen: Pos2, fallback: Pos2) -> Pos2 {
    let at = ui.ctx().input(|i| i.viewport().inner_rect.map(|r| r.min)).unwrap_or(fallback);
    ui.max_rect().min + (screen - at)
}

impl Fold {
    /// Ball centre on screen: the middle of the pill at `home`.
    pub fn ball_center(home: Pos2, pill: Vec2) -> Pos2 {
        home + pill / 2.0
    }

    pub fn is_open(&self) -> bool {
        self.stage == Stage::Open
    }

    fn go(&mut self, stage: Stage) {
        self.stage = stage;
        self.since = Instant::now();
    }

    /// Snap back to the full pill (calibration takes the window over and restores click-through
    /// when it hands it back).
    pub fn reset(&mut self) {
        self.stage = Stage::Open;
        self.touched = Instant::now();
    }

    /// Advance one frame; true when the full pill should be drawn. `home` is the pill's saved
    /// outer position, `hold_open` true while something needs the full pill, `fold_after` the
    /// idle delay (`None` = never fold), and `click_through` the user's setting, restored on
    /// unfolding.
    pub fn frame(&mut self, ui: &mut Ui, home: Pos2, pill: Vec2, look: &Look, hold_open: bool, fold_after: Option<Duration>, click_through: bool) -> bool {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let t = self.since.elapsed().as_secs_f32();
        let center = Self::ball_center(home, pill);
        let pill_rect = Rect::from_min_size(home, pill).shrink(2.0);
        // Folded, the hover zone around the ball stays inside the pill's rect: a cursor that
        // unfolds the ball must also hold the pill open, or a cursor resting at the zone's edge
        // would unfold, refold after the delay, and unfold again.
        let hit = HOVER.min(pill_rect.height() / 2.0);
        let over = cursor(&ctx).is_some_and(|c| match self.stage {
            Stage::Folded => c.distance(center) <= hit,
            _ => pill_rect.contains(c),
        });
        if over || hold_open {
            self.touched = now;
        }
        let p = ui.painter().clone();
        match self.stage {
            Stage::Open => {
                match fold_after {
                    Some(after) if !hold_open => {
                        let idle = self.touched.elapsed();
                        if idle >= after {
                            self.go(Stage::Folding);
                            ctx.request_repaint();
                        } else {
                            ctx.request_repaint_after(after - idle);
                        }
                    }
                    _ => {}
                }
                true
            }
            Stage::Folding | Stage::Unfolding => {
                ctx.request_repaint();
                if self.stage == Stage::Folding && (over || hold_open) {
                    // Changed course mid-fold: unfold from the current shape (k stays put).
                    let back = 1.0 - ease(t / MORPH).cbrt();
                    self.stage = Stage::Unfolding;
                    self.since = now - Duration::from_secs_f32(back * MORPH);
                }
                let t = self.since.elapsed().as_secs_f32();
                let k = if self.stage == Stage::Folding { ease(t / MORPH) } else { 1.0 - ease(t / MORPH) };
                let full = Rect::from_min_size(local(ui, home, home), pill).shrink(2.0);
                morph(&p, full, k, look.fill, look.tone, look.edge);
                if t >= MORPH {
                    if self.stage == Stage::Folding {
                        ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(true));
                        self.go(Stage::Folded);
                    } else {
                        self.go(Stage::Open);
                    }
                }
                false
            }
            Stage::Folded => {
                if over || hold_open {
                    ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(click_through));
                    self.go(Stage::Unfolding);
                    ctx.request_repaint();
                }
                // Cursor polling: the ball gets no mouse events while click-through.
                ctx.request_repaint_after(Duration::from_millis(150));
                let c = local(ui, center, home);
                clock_ball(&p, c, look);
                false
            }
        }
    }
}

/// The pill ⇄ ball morph: `k` 0 is the full pill rect, 1 the ball at its centre. The fill blends
/// from the pill's to the ball's colour and the edge fades out on the way to the ball.
pub fn morph(p: &egui::Painter, full: Rect, k: f32, fill: egui::Color32, ball: egui::Color32, edge: egui::Color32) {
    let small = Rect::from_center_size(full.center(), Vec2::splat(R * 2.0));
    let rect = Rect::from_min_max(full.min.lerp(small.min, k), full.max.lerp(small.max, k));
    p.rect_filled(rect, CornerRadius::same(255), fill.lerp_to_gamma(ball, k));
    p.rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, edge.gamma_multiply(1.0 - k)), StrokeKind::Inside);
}

/// Colours and progress for the folded ball (and the fold morph).
pub struct Look {
    /// Ball colour: calibration blue, amber looking away, red on a blocked site or alarm.
    pub tone: egui::Color32,
    /// Clock ring colour (green focus, blue break, like the logo's ring).
    pub ring: egui::Color32,
    /// Theme background and edge of the pill.
    pub fill: egui::Color32,
    pub edge: egui::Color32,
    /// Share of the phase done, 0..1.
    pub progress: f32,
    /// Clock ring and hand around the ball; off draws the plain ball.
    pub clock: bool,
}

/// The LockIn mark as a live clock: the phase fills the ring with round ends, a hand points at
/// where it has reached, and the state-coloured ball sits in the middle.
fn clock_ball(p: &egui::Painter, c: Pos2, look: &Look) {
    if !look.clock {
        p.circle_filled(c, R, look.tone);
        return;
    }
    let ring_r = R + 5.5;
    let width = 3.5;
    p.circle_filled(c, ring_r + width / 2.0 + 3.0, look.fill);
    p.circle_stroke(c, ring_r, Stroke::new(width, theme::RAISED));
    let frac = look.progress.clamp(0.0, 1.0);
    let angle = |f: f32| -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * f;
    let at = |f: f32, r: f32| c + egui::vec2(angle(f).cos(), angle(f).sin()) * r;
    if frac > 0.0 {
        crate::ui::arc(p, c, ring_r, frac, Stroke::new(width, look.ring));
        // Round caps, as in the logo.
        p.circle_filled(at(0.0, ring_r), width / 2.0, look.ring);
        p.circle_filled(at(frac, ring_r), width / 2.0, look.ring);
    }
    p.circle_filled(c, R, look.tone);
    // Hand toward the ring's end, drawn into the ball in the theme's dark, with the logo's light hub.
    p.line_segment([c, at(frac, R - 2.5)], Stroke::new(2.2, theme::BG));
    p.circle_filled(c, 2.4, theme::TEXT);
}
