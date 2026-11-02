//! `lockin --demo [marks.json]`: plays every feature on a fixed timeline for the demo video
//! (`scripts/demo.ps1` records the screen meanwhile). It runs on a throwaway data folder seeded
//! with sample tasks and a year of history, ignores what the camera and screen layers decide (the
//! alerts it shows are the scripted ones), moves the mouse where a feature needs it, and writes
//! one caption mark per scene to `marks.json` so the edit can line captions up with the footage.

use super::{App, CalTarget, Level};
use crate::config::{PillTheme, Settings};
use crate::policy::{Action, Kind};
use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Copy)]
enum Step {
    /// Caption for the scene starting now.
    Mark(&'static str),
    StartTimer,
    /// Type into the timer bar's quick add, one letter at a time over the given seconds.
    QuickAdd(&'static str, f32),
    QuickAddCommit,
    Theme(PillTheme),
    /// Fold after one idle second (true), or never (false).
    Fold(bool),
    Cursor(Spot),
    Warn(&'static str),
    Alarm,
    Back,
    StepAway,
    Calc(bool),
    CalcKey(&'static str),
    Camera,
    Dashboard,
    Idea(&'static str),
    Sheet(Option<&'static str>),
    Calibrate,
    End,
}

#[derive(Clone, Copy)]
enum Spot {
    /// Bottom-right corner, clear of every LockIn window.
    Away,
    /// The folded ball (centre of the timer bar).
    Ball,
}

use Step::*;

/// (seconds from start, step). Scenes are a few seconds each; the edit trims and speeds them.
const SCRIPT: &[(f32, Step)] = &[
    (0.0, Cursor(Spot::Away)),
    (0.5, Mark("A focus timer that sits on top of everything")),
    (1.5, StartTimer),
    (3.0, Mark("Quick-add a task without leaving your work")),
    (3.2, QuickAdd("Finish problem set 7", 1.6)),
    (5.4, QuickAddCommit),
    (6.5, Mark("Glass, Slate or Midnight")),
    (7.0, Theme(PillTheme::Slate)),
    (8.0, Theme(PillTheme::Midnight)),
    (9.0, Theme(PillTheme::Glass)),
    (10.0, Mark("Idle, it folds into a live clock")),
    (10.0, Fold(true)),
    (13.5, Mark("Hover to open it again")),
    (13.5, Cursor(Spot::Ball)),
    (15.5, Cursor(Spot::Away)),
    (18.0, Mark("Drift to a blocked site and a nudge grows out of the ball")),
    (18.0, Warn("youtube.com")),
    (23.0, Back),
    (24.0, Mark("Keep drifting and the alarm adds focus time")),
    (24.0, Alarm),
    (29.0, Back),
    (29.5, Cursor(Spot::Ball)),
    (30.0, Fold(false)),
    (30.2, Cursor(Spot::Away)),
    (30.5, Mark("Step away without alarms")),
    (31.0, StepAway),
    (34.0, StepAway),
    (35.0, Mark("A calculator one shortcut away")),
    (35.0, Calc(true)),
    (35.8, CalcKey("2")),
    (36.1, CalcKey("5")),
    (36.4, CalcKey("*")),
    (36.7, CalcKey("4")),
    (37.0, CalcKey("equals")),
    (38.5, Calc(false)),
    (39.0, Mark("Camera off in one click")),
    (39.5, Camera),
    (41.0, Camera),
    (42.5, Mark("Live gaze tracking, without showing your face")),
    (42.5, Dashboard),
    (47.5, Mark("Tasks, side ideas and a year of focus")),
    (48.5, Idea("Ask about the lab report extension")),
    (52.0, Mark("Rebind anything, even mouse buttons")),
    (52.0, Sheet(Some("keys"))),
    (55.0, Mark("Make the timer bar yours")),
    (55.0, Sheet(Some("pill"))),
    (58.0, Sheet(None)),
    (58.5, Mark("Calibrate by following the ball")),
    (58.5, Calibrate),
    (78.0, Mark("LockIn")),
    (81.0, End),
];

pub struct Demo {
    start: Instant,
    next: usize,
    /// Wall clock at the start, so the recording script can line the marks up with its video.
    start_unix_ms: u128,
    marks: Vec<serde_json::Value>,
    /// Where things sit on screen (points, plus the scale to pixels), for the edit's framing.
    layout: serde_json::Value,
    marks_path: Option<PathBuf>,
    typing: Option<(&'static str, Instant, f32)>,
    /// Section the main window's settings sheet should show (`None` = closed).
    pub sheet: Option<&'static str>,
}

/// Point `config::data_dir` at a fresh folder and turn the user's settings into the demo's.
pub fn prepare(real: &Settings) -> Settings {
    let dir = std::env::temp_dir().join("LockIn-demo");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    // SAFETY: called on the main thread before any other thread exists.
    unsafe { std::env::set_var("LOCKIN_DATA_DIR", &dir) };
    let mut s = real.clone();
    s.tour_done = true;
    s.hide_camera_image = true;
    s.camera_enabled = true;
    s.pill_theme = PillTheme::Glass;
    s.pill_fold = false;
    s.pill_clock = true;
    s.overlay_pos = None;
    s.overlay_click_through = false;
    s.launch_at_startup = false;
    s.focus_min = 25;
    s.alarm_loop = false;
    s.volume = s.volume.min(0.3);
    s.keybinds = Default::default();
    s.save();
    s
}

impl Demo {
    pub fn new(marks_path: Option<PathBuf>) -> Demo {
        // The backdrop goes up and the recording starts before the first step.
        const LEAD: std::time::Duration = std::time::Duration::from_millis(2500);
        let start_unix_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0) + LEAD.as_millis();
        Demo { start: Instant::now() + LEAD, next: 0, start_unix_ms, marks: Vec::new(), layout: serde_json::Value::Null, marks_path, typing: None, sheet: None }
    }

    fn write_marks(&self) {
        let Some(path) = &self.marks_path else { return };
        let doc = serde_json::json!({ "start_unix_ms": self.start_unix_ms.to_string(), "layout": self.layout, "marks": self.marks });
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&doc).unwrap_or_default());
    }
}

impl App {
    /// Full-screen plain backdrop behind LockIn's windows, so the recording shows nothing of the
    /// user's desktop. Opened first, it sits above every other app; LockIn's own windows open later
    /// (or are always on top) and stay in front of it.
    pub fn demo_backdrop(&mut self, ctx: &egui::Context) {
        if self.demo.is_none() {
            return;
        }
        let monitor = ctx.input_for(egui::ViewportId::ROOT, |i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
        let builder = egui::ViewportBuilder::default()
            .with_title("LockIn demo backdrop")
            .with_position([0.0, 0.0])
            .with_inner_size(monitor)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("demo-backdrop"), builder, |ui, _| {
            // Diagonal gradient in the app's own slate.
            let r = ui.max_rect();
            let (lit, dark) = (egui::Color32::from_rgb(0x22, 0x2a, 0x3a), egui::Color32::from_rgb(0x0c, 0x0e, 0x13));
            let mut mesh = egui::Mesh::default();
            let mid = lit.lerp_to_gamma(dark, 0.5);
            mesh.colored_vertex(r.left_top(), lit);
            mesh.colored_vertex(r.right_top(), mid);
            mesh.colored_vertex(r.right_bottom(), dark);
            mesh.colored_vertex(r.left_bottom(), mid);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(egui::Shape::mesh(mesh));
        });
    }

    /// Sample tasks, ideas, today's numbers and a year of pomodoros for the heatmap.
    pub fn demo_seed(&mut self) {
        let now = crate::tasks::now_secs();
        for (title, estimate, actual) in [("Read chapter 4: thermodynamics", 3, 1), ("Flashcards: organic reactions", 2, 0), ("Past paper 2023, Q1 to Q5", 4, 0)] {
            if let Some(id) = self.tasks.add_task(title, estimate, now) {
                if let Some(t) = self.tasks.tasks.iter_mut().find(|t| t.id == id) {
                    t.actual = actual;
                }
            }
        }
        self.tasks.add_idea("Find a Carnot cycle animation", now);
        let today = crate::config::days_since_epoch_today();
        self.tasks.history.clear();
        for k in (1..=330i64).rev() {
            // Deterministic, uneven pattern: busier weekdays, the odd day off, a ramp toward today.
            let n = ((k * 7 + k / 5) % 9) as u32 * (330 - k as u32) / 330 + u32::from(k % 6 != 0);
            if k % 11 == 0 || n == 0 {
                continue;
            }
            self.tasks.history.push(crate::tasks::DayHistory {
                date: crate::config::civil_from_days(today - k),
                completed_focus: n,
                earned_secs: n as u64 * 1500,
                tasks_completed: n / 3,
            });
        }
        self.tasks.save();
        let day = self.stats.today();
        day.focus_secs = 95 * 60;
        day.blocks = 3;
        day.distractions = 1;
        self.stats.clean_streak = 4;
        self.stats.save();
    }

    /// Run the steps that are due; called every frame while the demo plays.
    pub fn demo_tick(&mut self, ctx: &egui::Context) {
        let Some(demo) = self.demo.as_mut() else { return };
        let t = demo.start.elapsed().as_secs_f32();
        // Letters appear one by one.
        if let Some((text, since, secs)) = demo.typing {
            let k = (since.elapsed().as_secs_f32() / secs).min(1.0);
            let n = (text.chars().count() as f32 * k).round() as usize;
            self.pill_add_text = text.chars().take(n).collect();
            if k >= 1.0 {
                self.demo.as_mut().unwrap().typing = None;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
        loop {
            let demo = self.demo.as_mut().unwrap();
            let Some(&(at, step)) = SCRIPT.get(demo.next) else { return };
            if t < at {
                return;
            }
            demo.next += 1;
            self.demo_step(step, t, ctx);
        }
    }

    fn demo_step(&mut self, step: Step, t: f32, ctx: &egui::Context) {
        match step {
            Mark(text) => {
                let layout = serde_json::json!({
                    "ppp": ctx.pixels_per_point(),
                    "monitor": ctx.input(|i| i.viewport().monitor_size).map(|m| [m.x, m.y]),
                    "pill": self.s.overlay_pos,
                    "pill_size": [crate::ui::PILL.x, crate::ui::PILL.y],
                    // Physical pixels [left, top, width, height] of the main window, once open.
                    "dash": self.dash.rect(),
                });
                let demo = self.demo.as_mut().unwrap();
                demo.layout = layout;
                demo.marks.push(serde_json::json!({ "t": t, "text": text }));
                demo.write_marks();
            }
            StartTimer => {
                if !self.pomo.running {
                    self.toggle_timer();
                }
            }
            QuickAdd(text, secs) => {
                self.pill_adding = true;
                self.pill_add_idea = false;
                self.pill_add_text.clear();
                self.demo.as_mut().unwrap().typing = Some((text, Instant::now(), secs));
            }
            QuickAddCommit => {
                let text = std::mem::take(&mut self.pill_add_text);
                if let Some(id) = self.tasks.add_task(&text, 2, crate::tasks::now_secs()) {
                    self.tasks.set_active(id);
                }
                self.tasks.save();
                self.pill_adding = false;
            }
            Theme(theme) => self.s.pill_theme = theme,
            Fold(on) => {
                self.s.pill_fold = on;
                self.s.pill_fold_secs = 1;
            }
            Cursor(spot) => {
                let ppp = ctx.pixels_per_point();
                let (x, y) = match spot {
                    Spot::Away => {
                        let m = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
                        (m.x - 40.0, m.y - 120.0)
                    }
                    Spot::Ball => {
                        let home = self.s.overlay_pos.map(|[x, y]| egui::pos2(x, y)).unwrap_or_default();
                        let c = crate::fold::Fold::ball_center(home, crate::ui::PILL);
                        (c.x, c.y)
                    }
                };
                // SAFETY: plain cursor move.
                let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::SetCursorPos((x * ppp) as i32, (y * ppp) as i32) };
            }
            Warn(site) => self.on_action(Action::Warn(Kind::Site(site.into())), ctx),
            Alarm => self.on_action(Action::Alarm(Kind::LookingAway), ctx),
            Back => {
                if self.alarm_active() {
                    self.clear_alarm(true);
                } else if self.alert.as_ref().is_some_and(|a| a.level == Level::Warn) {
                    self.alert = None;
                }
            }
            StepAway => self.brb_or_back(),
            Calc(open) => {
                self.calc_open = open;
                if open {
                    self.calc.open();
                    self.calc_min = false;
                }
            }
            CalcKey(key) => {
                let _ = self.calc.press(key);
            }
            Camera => self.toggle_camera(),
            Dashboard => self.open_dashboard(),
            Idea(text) => {
                self.tasks.add_idea(text, crate::tasks::now_secs());
                self.tasks.save();
            }
            Sheet(section) => self.demo.as_mut().unwrap().sheet = section,
            Calibrate => self.calibrate(CalTarget::Screen),
            End => {
                self.demo.as_ref().unwrap().write_marks();
                self.shutdown();
                std::process::exit(0);
            }
        }
    }
}
