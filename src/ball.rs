//! Screen calibration: the timer pill folds into the gaze-coloured ball, glides across the monitor
//! (the user follows it with their eyes), rests in the middle for a head nod, then flies home and
//! unfolds back into the pill. The vision worker samples while the ball travels.

use crate::theme;
use eframe::egui::{self, pos2, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2, ViewportCommand};
use std::time::Instant;

/// Ball spots as fractions of the monitor, same order as the old in-window dot.
const DOTS: [[f32; 2]; 6] = [[0.5, 0.45], [0.14, 0.16], [0.86, 0.16], [0.86, 0.82], [0.14, 0.82], [0.5, 0.45]];
/// Rising C-major pentatonic, one note per spot.
const NOTES: [f32; 6] = [523.25, 587.33, 659.25, 783.99, 880.0, 1046.5];
const MORPH: f32 = 0.45;
const FLY: f32 = 0.55;
const BEAT: f32 = 1.0;
const MOVE: f32 = 0.42;
/// Resting in the middle while the user tips their head down to the keyboard and back.
const NOD_BEATS: usize = 3;
/// Window frames to wait after resizing/moving before the pill trusts its own position again.
const SETTLE: f32 = 0.2;
const HOLD_MAX: f32 = 6.0;
pub const BALL_WIN: f32 = 76.0;
const R: f32 = 11.0;

/// Seconds of sampling the vision worker needs for the screen target: six spots plus the nod.
pub const SAMPLE_SECS: f32 = DOTS.len() as f32 * BEAT + NOD_BEATS as f32 * BEAT;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    /// Pill folding into the ball inside the pill window.
    Fold,
    /// Ball window flying from the pill to the first spot.
    Out,
    /// Following the spots and the nod (vision sampling runs).
    Tour,
    /// Waiting at the centre for the worker to finish.
    Hold,
    /// Flying back to where the pill lives.
    Home,
    /// Ball unfolding into the pill inside the pill window.
    Unfold,
    /// Window is pill-sized again; wait for the OS to apply it.
    Settle,
}

pub struct Ball {
    stage: Stage,
    since: Instant,
    /// Pill outer position (points) to return to.
    home: Pos2,
    monitor: Vec2,
    last_beat: Option<usize>,
    done_sampling: bool,
}

/// Side effects the pill performs for the ball this frame.
pub enum Step {
    /// Start the vision worker's screen sampling now.
    StartSampling,
    /// Play a note (Hz).
    Note(f32),
    /// Animation finished; the pill takes the window back.
    Finished,
}

fn ease(t: f32) -> f32 {
    // cubic-bezier(.3,.7,.2,1)-like ease-out, as on the old CSS dot.
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn lerp(a: Pos2, b: Pos2, t: f32) -> Pos2 {
    a + (b - a) * t
}

impl Ball {
    pub fn new(home: Pos2, monitor: Vec2) -> Ball {
        Ball {
            stage: Stage::Fold,
            since: Instant::now(),
            home,
            monitor,
            last_beat: None,
            done_sampling: false,
        }
    }

    /// The worker reported the screen step finished (or failed).
    pub fn sampling_done(&mut self) {
        self.done_sampling = true;
    }

    fn spot(&self, i: usize) -> Pos2 {
        pos2(DOTS[i][0] * self.monitor.x, DOTS[i][1] * self.monitor.y)
    }

    fn pill_center(&self, pill: Vec2) -> Pos2 {
        self.home + pill / 2.0
    }

    fn go(&mut self, stage: Stage) {
        self.stage = stage;
        self.since = Instant::now();
    }

    fn place(ctx: &egui::Context, center: Pos2) {
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(center - Vec2::splat(BALL_WIN / 2.0)));
    }

    /// Advance and draw one frame. `pill` is the pill window size.
    pub fn frame(&mut self, ui: &mut Ui, pill: Vec2) -> Vec<Step> {
        let ctx = ui.ctx().clone();
        ctx.request_repaint();
        let t = self.since.elapsed().as_secs_f32();
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let origin = ui.max_rect().min;
        match self.stage {
            Stage::Fold | Stage::Unfold => {
                let k = ease(t / MORPH);
                let k = if self.stage == Stage::Fold { k } else { 1.0 - k };
                let full = Rect::from_min_size(origin, pill).shrink(2.0);
                let ball = Rect::from_center_size(full.center(), Vec2::splat(R * 2.0));
                let rect = Rect::from_min_max(lerp(full.min, ball.min, k), lerp(full.max, ball.max, k));
                let fill = theme::SURFACE.lerp_to_gamma(theme::GAZE, k);
                p.rect_filled(rect, CornerRadius::same(255), fill);
                p.rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, theme::LINE.gamma_multiply(1.0 - k)), StrokeKind::Inside);
                if t >= MORPH {
                    if self.stage == Stage::Fold {
                        ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::splat(BALL_WIN)));
                        Self::place(&ctx, self.pill_center(pill));
                        self.go(Stage::Out);
                    } else {
                        self.go(Stage::Settle);
                    }
                }
            }
            Stage::Out | Stage::Home => {
                let (from, to) = if self.stage == Stage::Out {
                    (self.pill_center(pill), self.spot(0))
                } else {
                    (self.spot(DOTS.len() - 1), self.pill_center(pill))
                };
                Self::place(&ctx, lerp(from, to, ease(t / FLY)));
                self.draw(&p, origin, f32::INFINITY, None);
                if t >= FLY {
                    if self.stage == Stage::Out {
                        self.last_beat = None;
                        out.push(Step::StartSampling);
                        self.go(Stage::Tour);
                    } else {
                        ctx.send_viewport_cmd(ViewportCommand::InnerSize(pill));
                        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(self.home));
                        self.go(Stage::Unfold);
                    }
                }
            }
            Stage::Tour | Stage::Hold => {
                let beat = (t / BEAT) as usize;
                let in_beat = t - beat as f32 * BEAT;
                let spots = DOTS.len();
                let (center, pulse_at) = if self.stage == Stage::Hold || beat >= spots {
                    // Nod: rest in the middle and breathe slowly.
                    (self.spot(spots - 1), None)
                } else if beat == 0 {
                    (self.spot(0), Some(0.0))
                } else {
                    (lerp(self.spot(beat - 1), self.spot(beat), ease(in_beat / MOVE)), Some(MOVE))
                };
                Self::place(&ctx, center);
                let since_pulse = pulse_at.map(|at| in_beat - at).filter(|d| *d >= 0.0);
                if self.stage == Stage::Tour && beat < spots && since_pulse.is_some() && self.last_beat != Some(beat) {
                    self.last_beat = Some(beat);
                    out.push(Step::Note(NOTES[beat]));
                }
                let breathe = (beat >= spots || self.stage == Stage::Hold).then(|| (t * std::f32::consts::PI).sin().abs());
                self.draw(&p, origin, since_pulse.unwrap_or(f32::INFINITY), breathe);
                if self.stage == Stage::Tour && t >= SAMPLE_SECS {
                    self.go(Stage::Hold);
                }
                let held = self.stage == Stage::Hold && self.since.elapsed().as_secs_f32() >= HOLD_MAX;
                // `held`: the worker never answered (camera gone); give the pill back anyway.
                if (self.done_sampling && self.stage == Stage::Hold) || held {
                    self.go(Stage::Home);
                }
            }
            Stage::Settle => {
                if t >= SETTLE {
                    return vec![Step::Finished];
                }
            }
        }
        out
    }

    /// The ball centred in the ball window: halo, a thump on the beat and a fading ping ring.
    fn draw(&self, p: &egui::Painter, origin: Pos2, since_pulse: f32, breathe: Option<f32>) {
        let c = origin + Vec2::splat(BALL_WIN / 2.0);
        let thump = if since_pulse < 0.3 { (1.0 - (since_pulse / 0.3 - 0.4).abs() / 0.6).max(0.0) } else { 0.0 };
        let halo = 6.0 + 4.0 * thump.max(breathe.unwrap_or(0.0));
        let halo_alpha = 0.2 + 0.1 * thump;
        p.circle_filled(c, R + halo, theme::GAZE.gamma_multiply(halo_alpha));
        p.circle_filled(c, R, theme::GAZE);
        if since_pulse < 0.6 {
            let k = since_pulse / 0.6;
            let ring = (R + 6.0) * (1.0 + 1.6 * k);
            let ring = ring.min(BALL_WIN / 2.0 - 1.0);
            p.circle_stroke(c, ring, Stroke::new(2.0, theme::GAZE.gamma_multiply(0.8 * (1.0 - k))));
        }
    }
}

