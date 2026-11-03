//! Pomodoro timer with BRB grace pauses and distraction penalties. Pure logic: the app feeds `tick(dt)`.

use crate::config::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    PhaseEnded(Phase),
    /// BRB grace ran out and the user has not said they are back.
    BrbOverdue,
}

#[derive(Clone, Debug)]
pub struct Pomodoro {
    pub phase: Phase,
    /// Seconds left in the current phase (includes penalty time).
    pub remaining: f64,
    pub running: bool,
    /// The current phase has run at some point. Until then it follows the duration settings.
    pub started: bool,
    /// Seconds left of a BRB pause; negative once overdue.
    pub brb: Option<f64>,
    brb_overdue_sent: bool,
    /// Frozen because the user is distracted (penalty: pause timer while distracted).
    pub distracted_hold: bool,
    pub completed_focus: u32,
    /// Alarms this focus block (drives escalating penalties).
    pub block_alarms: u32,
    pub block_added_secs: u32,
    /// Focus seconds actually earned (only while running, not BRB, not distracted). Running total
    /// for the app's lifetime: the app books the growth since its last look, so a reset keeps it.
    pub earned_secs: f64,
}

impl Pomodoro {
    pub fn new(s: &Settings) -> Self {
        Pomodoro {
            phase: Phase::Focus,
            remaining: s.focus_min as f64 * 60.0,
            running: false,
            started: false,
            brb: None,
            brb_overdue_sent: false,
            distracted_hold: false,
            completed_focus: 0,
            block_alarms: 0,
            block_added_secs: 0,
            earned_secs: 0.0,
        }
    }

    fn duration(&self, s: &Settings, phase: Phase) -> f64 {
        60.0 * match phase {
            Phase::Focus => s.focus_min,
            Phase::ShortBreak => s.short_break_min,
            Phase::LongBreak => s.long_break_min,
        } as f64
    }

    /// True when the focus clock is actually counting down: monitoring should run.
    pub fn focusing(&self) -> bool {
        self.phase == Phase::Focus && self.running && self.brb.is_none()
    }

    pub fn toggle(&mut self) {
        self.running = !self.running;
        if self.running {
            self.started = true;
        } else {
            self.brb = None;
            self.brb_overdue_sent = false;
        }
    }

    /// Settings changed: a phase that has not started yet takes its new length.
    pub fn apply_durations(&mut self, s: &Settings) {
        if !self.started {
            self.remaining = self.duration(s, self.phase);
        }
    }

    pub fn reset(&mut self, s: &Settings) {
        *self = Pomodoro { completed_focus: self.completed_focus, earned_secs: self.earned_secs, ..Pomodoro::new(s) };
    }

    pub fn skip(&mut self, s: &Settings) {
        self.advance(s);
    }

    fn advance(&mut self, s: &Settings) {
        let next = match self.phase {
            Phase::Focus => {
                self.completed_focus += 1;
                if s.long_break_every > 0 && self.completed_focus % s.long_break_every == 0 {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            _ => Phase::Focus,
        };
        self.phase = next;
        self.remaining = self.duration(s, next);
        self.brb = None;
        self.brb_overdue_sent = false;
        self.distracted_hold = false;
        self.block_alarms = 0;
        self.block_added_secs = 0;
        self.running = match next {
            Phase::Focus => s.auto_start_focus,
            _ => s.auto_start_breaks,
        };
        self.started = self.running;
    }

    /// Start a BRB pause (only during a running focus block). Returns false when not applicable.
    pub fn brb_start(&mut self, s: &Settings) -> bool {
        if self.phase != Phase::Focus || !self.running || self.brb.is_some() {
            return false;
        }
        self.brb = Some(s.brb_secs as f64);
        self.brb_overdue_sent = false;
        true
    }

    /// "I'm back": ends the BRB pause. Returns true when the user came back late.
    pub fn back(&mut self) -> bool {
        let late = matches!(self.brb, Some(r) if r < 0.0);
        self.brb = None;
        self.brb_overdue_sent = false;
        late
    }

    /// Apply the time penalty for a distraction alarm; returns seconds added.
    pub fn penalize(&mut self, s: &Settings) -> u32 {
        self.block_alarms += 1;
        if !s.penalty_add_time || self.phase != Phase::Focus {
            return 0;
        }
        let step = if s.penalty_escalate {
            s.penalty_add_secs.saturating_mul(1 << (self.block_alarms - 1).min(8))
        } else {
            s.penalty_add_secs
        };
        let room = s.penalty_max_add_secs.saturating_sub(self.block_added_secs);
        let add = step.min(room);
        self.block_added_secs += add;
        self.remaining += add as f64;
        add
    }

    pub fn tick(&mut self, dt: f64, s: &Settings) -> Vec<Event> {
        let mut events = Vec::new();
        if !self.running {
            return events;
        }
        if let Some(r) = self.brb.as_mut() {
            *r -= dt;
            if *r < 0.0 && !self.brb_overdue_sent {
                self.brb_overdue_sent = true;
                events.push(Event::BrbOverdue);
            }
            return events;
        }
        if self.phase == Phase::Focus && self.distracted_hold {
            return events;
        }
        self.remaining -= dt;
        if self.phase == Phase::Focus {
            self.earned_secs += dt;
        }
        if self.remaining <= 0.0 {
            let ended = self.phase;
            self.advance(s);
            events.push(Event::PhaseEnded(ended));
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings { focus_min: 1, short_break_min: 1, long_break_min: 2, long_break_every: 2, ..Default::default() }
    }

    #[test]
    fn duration_change_applies_until_the_phase_starts() {
        let mut s = Settings { auto_start_breaks: false, ..settings() };
        let mut p = Pomodoro::new(&s);
        s.focus_min = 50;
        p.apply_durations(&s);
        assert_eq!(p.remaining, 3000.0, "unstarted focus takes the new length");
        p.toggle();
        p.tick(10.0, &s);
        p.toggle();
        s.focus_min = 20;
        p.apply_durations(&s);
        assert_eq!(p.remaining, 2990.0, "a started block keeps its time");
        p.skip(&s);
        s.short_break_min = 7;
        p.apply_durations(&s);
        assert_eq!((p.phase, p.remaining), (Phase::ShortBreak, 420.0), "the next unstarted break follows too");
    }

    #[test]
    fn brb_freezes_timer_and_fires_overdue_once() {
        let s = Settings { brb_secs: 120, ..settings() };
        let mut p = Pomodoro::new(&s);
        p.toggle();
        p.tick(10.0, &s);
        assert!(p.brb_start(&s));
        assert!(!p.focusing());
        assert!(p.tick(119.0, &s).is_empty());
        assert_eq!(p.remaining, 50.0, "timer must not move during BRB");
        assert_eq!(p.tick(2.0, &s), vec![Event::BrbOverdue]);
        assert!(p.tick(5.0, &s).is_empty());
        assert!(p.back(), "came back late");
        p.tick(5.0, &s);
        assert_eq!(p.remaining, 45.0, "resumes where it paused");
    }

    #[test]
    fn back_within_grace_is_not_late() {
        let s = settings();
        let mut p = Pomodoro::new(&s);
        p.toggle();
        p.brb_start(&s);
        p.tick(30.0, &s);
        assert!(!p.back());
    }

    #[test]
    fn escalating_penalty_is_capped_per_block_and_resets_next_block() {
        let s = Settings { penalty_add_secs: 60, penalty_max_add_secs: 200, ..settings() };
        let mut p = Pomodoro::new(&s);
        p.toggle();
        assert_eq!(p.penalize(&s), 60);
        assert_eq!(p.penalize(&s), 120);
        assert_eq!(p.penalize(&s), 20, "cap 200 per block");
        assert_eq!(p.penalize(&s), 0);
        assert_eq!(p.remaining, 260.0);
        p.skip(&s);
        p.skip(&s);
        assert_eq!(p.phase, Phase::Focus);
        assert_eq!(p.penalize(&s), 60);
    }

    #[test]
    fn distracted_hold_stops_focus_clock_but_not_breaks() {
        let s = settings();
        let mut p = Pomodoro::new(&s);
        p.toggle();
        p.distracted_hold = true;
        p.tick(30.0, &s);
        assert_eq!(p.remaining, 60.0);
        assert_eq!(p.earned_secs, 0.0);
        p.distracted_hold = false;
        assert_eq!(p.tick(60.0, &s), vec![Event::PhaseEnded(Phase::Focus)]);
        assert_eq!(p.phase, Phase::ShortBreak);
        assert!(p.running, "auto_start_breaks");
    }

    #[test]
    fn long_break_every_n_focus_blocks() {
        let s = settings();
        let mut p = Pomodoro::new(&s);
        p.skip(&s);
        assert_eq!(p.phase, Phase::ShortBreak);
        p.skip(&s);
        p.skip(&s);
        assert_eq!(p.phase, Phase::LongBreak);
        assert_eq!(p.remaining, 120.0);
        p.skip(&s);
        assert_eq!(p.phase, Phase::Focus);
        assert!(!p.running, "auto_start_focus is off by default");
    }

    #[test]
    fn reset_keeps_earned_focus_growing() {
        // The app books `earned_secs` growth since its last look; a counter that dropped to 0 on
        // reset hid the next block's time until it re-earned everything from before the reset.
        let s = Settings { focus_min: 25, ..settings() };
        let mut p = Pomodoro::new(&s);
        p.toggle();
        p.tick(600.0, &s);
        let before = p.earned_secs;
        p.reset(&s);
        assert!(!p.running);
        assert_eq!(p.remaining, 1500.0);
        p.toggle();
        p.tick(60.0, &s);
        assert_eq!(p.earned_secs - before, 60.0);
    }
}
