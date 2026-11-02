//! Idle timer bar: after a few seconds without the mouse on it, the pill folds into a small ball
//! drawn like the LockIn mark: a clock ring that fills with the phase, a hand at its end, and the
//! calibration ball's blue in the middle (amber looking away, red on a blocked site or alarm).
//! Hovering it, an alert or a quick add unfolds the pill again.
//!
//! While folded the window shrinks to a small square around the ball so the rest of the strip no
//! longer catches clicks. Shapes are drawn in screen coordinates against the window's actual
//! position (as in `ball`), so a move and a resize landing a frame apart never shows a jump.

use crate::theme;
use eframe::egui::{self, pos2, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2, ViewportCommand};
use std::time::{Duration, Instant};

const MORPH: f32 = 0.4;
/// Ball radius and the folded window's side.
pub const R: f32 = 11.0;
const SIDE: f32 = 64.0;

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

    /// Snap back to the full pill (calibration takes the window over).
    pub fn reset(&mut self, ctx: &egui::Context, home: Pos2, pill: Vec2) {
        if self.stage != Stage::Open {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(pill));
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(home));
        }
        self.stage = Stage::Open;
        self.touched = Instant::now();
    }

    /// Advance one frame; true when the full pill should be drawn. `home` is the pill's saved
    /// outer position, `hold_open` true while something needs the full pill, and `fold_after` the
    /// idle delay (`None` = never fold).
    pub fn frame(&mut self, ui: &mut Ui, home: Pos2, pill: Vec2, look: &Look, hold_open: bool, fold_after: Option<Duration>) -> bool {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let t = self.since.elapsed().as_secs_f32();
        let center = Self::ball_center(home, pill);
        let pill_rect = Rect::from_min_size(home, pill).shrink(2.0);
        // Folded, the hover zone around the ball stays inside the pill's rect: a cursor that
        // unfolds the ball must also hold the pill open, or a cursor resting at the zone's edge
        // would unfold, refold after the delay, and unfold again.
        let hit = (SIDE / 2.0).min(pill_rect.height() / 2.0);
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
                let ball = Rect::from_center_size(full.center(), Vec2::splat(R * 2.0));
                let rect = Rect::from_min_max(full.min.lerp(ball.min, k), full.max.lerp(ball.max, k));
                let fill = look.fill.lerp_to_gamma(look.tone, k);
                p.rect_filled(rect, CornerRadius::same(255), fill);
                p.rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, look.edge.gamma_multiply(1.0 - k)), StrokeKind::Inside);
                if t >= MORPH {
                    if self.stage == Stage::Folding {
                        ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::splat(SIDE)));
                        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(center - Vec2::splat(SIDE / 2.0)));
                        self.go(Stage::Folded);
                    } else {
                        self.go(Stage::Open);
                    }
                }
                false
            }
            Stage::Folded => {
                if over || hold_open {
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(pill));
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(home));
                    self.go(Stage::Unfolding);
                    ctx.request_repaint();
                }
                // Cursor polling: the ball gets no mouse events while click-through.
                ctx.request_repaint_after(Duration::from_millis(150));
                let c = local(ui, center, center - Vec2::splat(SIDE / 2.0));
                clock_ball(&p, c, look);
                false
            }
        }
    }
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
