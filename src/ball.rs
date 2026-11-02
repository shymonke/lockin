//! Screen calibration: the timer pill folds into the gaze-coloured ball, glides across the monitor
//! (the user follows it with their eyes), rests in the middle for a head nod, then flies home and
//! unfolds back into the pill. The vision worker samples while the ball travels; if the camera or
//! models are still starting, the ball breathes at its first spot until they are ready.
//!
//! During the tour the pill window shrinks to a small click-through square that follows the ball.
//! The ball is drawn in screen coordinates against the window's actual position, so it stays put
//! on screen even when a move and a resize land a frame apart.

use crate::fold::{morph, MORPH};
use crate::theme;
use eframe::egui::{self, pos2, Pos2, Rect, Stroke, Ui, Vec2, ViewportCommand};
use std::time::Instant;

/// Ball spots as fractions of the monitor, same order as the old in-window dot.
const DOTS: [[f32; 2]; 6] = [[0.5, 0.45], [0.14, 0.16], [0.86, 0.16], [0.86, 0.82], [0.14, 0.82], [0.5, 0.45]];
/// Rising C-major pentatonic, one note per spot.
const NOTES: [f32; 6] = [523.25, 587.33, 659.25, 783.99, 880.0, 1046.5];
const FLY: f32 = 0.55;
const BEAT: f32 = 1.0;
const MOVE: f32 = 0.42;
/// Resting in the middle while the user tips their head down to the keyboard and back.
const NOD_BEATS: usize = 3;
/// Time for the OS to apply the final pill size/position before the pill trusts its own position.
const SETTLE: f32 = 0.2;
/// Longest wait in the middle for the worker's result (camera gone) before flying home anyway.
const HOLD_MAX: f32 = 6.0;
const R: f32 = 11.0;
/// Ball window side. The window trails the ball by a frame or two while it glides, so it is
/// large enough that the ball never leaves it mid-move.
const BALL_WIN: f32 = 360.0;

/// Seconds of sampling the vision worker needs for the screen target: six spots plus the nod.
pub const SAMPLE_SECS: f32 = DOTS.len() as f32 * BEAT + NOD_BEATS as f32 * BEAT;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    /// Pill folding into the ball.
    Fold,
    /// Ball flying from the pill to the first spot.
    Out,
    /// At the first spot, breathing until the camera and models are ready.
    Wait,
    /// Following the spots and the nod (vision sampling runs).
    Tour,
    /// Waiting at the centre for the worker to finish.
    Hold,
    /// Flying back to where the pill lives.
    Home,
    /// Ball unfolding into the pill.
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
    // Ease-out like the old CSS dot's cubic-bezier(.3,.7,.2,1).
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Smoothstep for the ball's glides: half the peak speed of `ease`, so the window keeps up.
fn glide(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: Pos2, b: Pos2, t: f32) -> Pos2 {
    a + (b - a) * t
}

impl Ball {
    pub fn new(home: Pos2, monitor: Vec2) -> Ball {
        Ball { stage: Stage::Fold, since: Instant::now(), home, monitor, last_beat: None, done_sampling: false }
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

    /// Move the ball window over screen point `c` and draw the ball there.
    fn show_at(&self, ui: &Ui, p: &egui::Painter, c: Pos2, since_pulse: f32, breathe: Option<f32>) {
        ui.ctx().send_viewport_cmd(ViewportCommand::OuterPosition(c - Vec2::splat(BALL_WIN / 2.0)));
        self.draw(p, self.local(ui, c), since_pulse, breathe);
    }

    /// Screen point (points) → this frame's local coordinates, using where the OS has the window now.
    fn local(&self, ui: &Ui, screen: Pos2) -> Pos2 {
        // Painter coordinates are relative to the client area, so use its origin, not the frame's.
        let at = ui.ctx().input(|i| i.viewport().inner_rect.map(|r| r.min)).unwrap_or(self.home);
        ui.max_rect().min + (screen - at)
    }

    /// Advance and draw one frame. `pill` is the pill window size; `ready` whether the vision
    /// worker has the camera and models up (sampling starts no earlier).
    pub fn frame(&mut self, ui: &mut Ui, pill: Vec2, ready: bool) -> Vec<Step> {
        let ctx = ui.ctx().clone();
        ctx.request_repaint();
        let t = self.since.elapsed().as_secs_f32();
        let mut out = Vec::new();
        let p = ui.painter().clone();
        match self.stage {
            Stage::Fold | Stage::Unfold | Stage::Settle => {
                // Settle keeps drawing the bare pill shell so nothing flickers before the pill
                // takes over.
                let k = match self.stage {
                    Stage::Fold => ease(t / MORPH),
                    Stage::Unfold => 1.0 - ease(t / MORPH),
                    _ => 0.0,
                };
                let full = Rect::from_min_size(self.local(ui, self.home), pill).shrink(2.0);
                morph(&p, full, k, theme::SURFACE, theme::GAZE, theme::LINE);
                match self.stage {
                    Stage::Fold if t >= MORPH => {
                        ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::splat(BALL_WIN)));
                        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(self.pill_center(pill) - Vec2::splat(BALL_WIN / 2.0)));
                        self.go(Stage::Out);
                    }
                    Stage::Unfold if t >= MORPH => self.go(Stage::Settle),
                    Stage::Settle if t >= SETTLE => return vec![Step::Finished],
                    _ => {}
                }
            }
            Stage::Out | Stage::Home => {
                let (from, to) = if self.stage == Stage::Out {
                    (self.pill_center(pill), self.spot(0))
                } else {
                    (self.spot(DOTS.len() - 1), self.pill_center(pill))
                };
                self.show_at(ui, &p, lerp(from, to, glide(t / FLY)), f32::INFINITY, None);
                if t >= FLY {
                    if self.stage == Stage::Out {
                        self.go(Stage::Wait);
                    } else {
                        ctx.send_viewport_cmd(ViewportCommand::InnerSize(pill));
                        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(self.home));
                        self.go(Stage::Unfold);
                    }
                }
            }
            Stage::Wait => {
                let breathe = (t * std::f32::consts::PI).sin().abs();
                self.show_at(ui, &p, self.spot(0), f32::INFINITY, Some(breathe));
                if self.done_sampling {
                    // Cancelled, or the camera failed, before sampling began.
                    self.go(Stage::Home);
                } else if ready {
                    self.last_beat = None;
                    out.push(Step::StartSampling);
                    self.go(Stage::Tour);
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
                    (lerp(self.spot(beat - 1), self.spot(beat), glide(in_beat / MOVE)), Some(MOVE))
                };
                let since_pulse = pulse_at.map(|at| in_beat - at).filter(|d| *d >= 0.0);
                if self.stage == Stage::Tour && beat < spots && since_pulse.is_some() && self.last_beat != Some(beat) {
                    self.last_beat = Some(beat);
                    out.push(Step::Note(NOTES[beat]));
                }
                let breathe = (beat >= spots || self.stage == Stage::Hold).then(|| (t * std::f32::consts::PI).sin().abs());
                self.show_at(ui, &p, center, since_pulse.unwrap_or(f32::INFINITY), breathe);
                if self.stage == Stage::Tour && t >= SAMPLE_SECS {
                    self.go(Stage::Hold);
                }
                let held = self.stage == Stage::Hold && self.since.elapsed().as_secs_f32() >= HOLD_MAX;
                if (self.done_sampling && self.stage == Stage::Hold) || held {
                    self.go(Stage::Home);
                }
            }
        }
        out
    }

    /// The ball at `c` (local): halo, a thump on the beat and a fading ping ring.
    fn draw(&self, p: &egui::Painter, c: Pos2, since_pulse: f32, breathe: Option<f32>) {
        let thump = if since_pulse < 0.3 { (1.0 - (since_pulse / 0.3 - 0.4).abs() / 0.6).max(0.0) } else { 0.0 };
        let halo = 6.0 + 4.0 * thump.max(breathe.unwrap_or(0.0));
        let halo_alpha = 0.2 + 0.1 * thump;
        p.circle_filled(c, R + halo, theme::GAZE.gamma_multiply(halo_alpha));
        p.circle_filled(c, R, theme::GAZE);
        if since_pulse < 0.6 {
            let k = since_pulse / 0.6;
            let ring = (R + 6.0) * (1.0 + 1.6 * k);
            p.circle_stroke(c, ring, Stroke::new(2.0, theme::GAZE.gamma_multiply(0.8 * (1.0 - k))));
        }
    }
}
