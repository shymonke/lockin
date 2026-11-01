//! App state and the per-frame logic: drains worker channels, runs the pomodoro, applies policy
//! actions (alerts, sounds, penalties) and starts/stops the camera and screen layers on demand.

use crate::audio;
use crate::calc::Calc;
use crate::config::{CameraMode, Meme, Settings, Stats};
use crate::copy;
use crate::hotkeys::{self, Hotkey};
use crate::memes::{self, CatStyle, UserMeme};
use crate::policy::{Action, Foreground, Gaze, Kind, Policy};
use crate::pomodoro::{Event, Phase, Pomodoro};
use crate::screen;
use crate::vision::{self, CalTarget, EyeOverlay, Face, Objects, PreviewSlot};
use crate::web;
use eframe::egui;
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Warn,
    Alarm,
    Ask,
}

pub struct Alert {
    pub level: Level,
    pub kind: Option<Kind>,
    pub ask_domain: Option<String>,
    pub line: copy::Line,
    pub opened: Instant,
    pub auto_close: Option<f32>,
    pub cat: Option<CatStyle>,
    pub added_secs: u32,
    pub remember: bool,
}

#[derive(Default)]
pub struct CamView {
    pub gaze: Option<Gaze>,
    pub face: Option<Face>,
    pub eyes: Option<Vec<EyeOverlay>>,
    pub confidence: f32,
    pub l1_ms: f32,
    pub l2: Option<(Objects, f32)>,
    pub error: Option<String>,
    pub cameras: Vec<String>,
}

/// Guided calibration step in progress (shown in the dashboard).
#[derive(Default)]
pub struct CalState {
    pub target: Option<CalTarget>,
    pub progress: f32,
    /// Usable frames so far (face and both eyes found).
    pub samples: usize,
    pub error: Option<String>,
    /// Screen step: waiting for the pill to fold into the ball (needs the egui context).
    pub ball_pending: bool,
    pub ball: Option<crate::ball::Ball>,
}

pub struct App {
    pub s: Settings,
    saved_s: Settings,
    dirty_at: Option<Instant>,
    pub stats: Stats,
    pub pomo: Pomodoro,
    pub policy: Policy,
    pub vision: Sender<vision::Cmd>,
    vision_rx: Receiver<vision::Msg>,
    screen: Sender<screen::Cmd>,
    screen_rx: Receiver<Option<Foreground>>,
    pub audio: audio::Tx,
    keys: Receiver<Hotkey>,
    t0: Instant,
    last_tick: Instant,
    last_stats_save: Instant,
    earned_seen: f64,
    block_clean: bool,
    cam_running: Option<(u32, f64)>,
    screen_running: bool,
    policy_live: bool,
    pub cam: CamView,
    pub fg: Option<Foreground>,
    pub alert: Option<Alert>,
    pub user_meme: Option<UserMeme>,
    pub calc: Calc,
    pub calc_open: bool,
    pub calc_min: bool,
    pub calc_spawn: Option<[f32; 2]>,
    pub cal: CalState,
    pub click_through: bool,
    dash: web::Dashboard,
    dash_shared: Arc<web::Shared>,
    dash_rx: Receiver<Value>,
    dash_was_open: bool,
    last_publish: Instant,
    first_tick: bool,
    preview_cam: Option<u32>,
    seed: u64,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, s: Settings) -> App {
        crate::theme::install(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let wake = move || ctx.request_repaint();

        let (vtx, vrx) = channel();
        let (vmsg_tx, vmsg_rx) = channel();
        let preview: PreviewSlot = Arc::new(Mutex::new(None));
        vision::spawn(vrx, vmsg_tx, Box::new(wake.clone()), preview.clone());
        let _ = vtx.send(vision::Cmd::SetCalibration(s.calibration.clone()));
        let (stx, srx) = channel();
        let (smsg_tx, smsg_rx) = channel();
        screen::spawn(srx, smsg_tx, Box::new(wake.clone()));
        let (atx, arx) = channel();
        audio::spawn(arx);
        let (ktx, krx) = channel();
        hotkeys::spawn(ktx.clone(), Box::new(wake.clone()));
        crate::tray::spawn(ktx, Box::new(wake.clone()));
        let (dtx, dash_rx) = channel();
        let dash_shared = Arc::new(web::Shared {
            state: Mutex::new("{}".into()),
            preview: Box::new(move |with_jpeg| {
                let slot = preview.lock().unwrap();
                match slot.as_ref() {
                    Some(f) => (
                        json!({ "seq": f.seq, "w": f.width, "h": f.height, "overlay": f.overlay }).to_string(),
                        with_jpeg.then(|| f.jpeg.clone()),
                    ),
                    None => (json!({ "seq": null }).to_string(), None),
                }
            }),
            actions: Mutex::new(dtx),
            wake: Box::new(wake),
        });

        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
        let click_through = s.overlay_click_through;
        App {
            pomo: Pomodoro::new(&s),
            saved_s: s.clone(),
            s,
            dirty_at: None,
            stats: Stats::load(),
            policy: Policy::default(),
            vision: vtx,
            vision_rx: vmsg_rx,
            screen: stx,
            screen_rx: smsg_rx,
            audio: atx,
            keys: krx,
            t0: Instant::now(),
            last_tick: Instant::now(),
            last_stats_save: Instant::now(),
            earned_seen: 0.0,
            block_clean: true,
            cam_running: None,
            screen_running: false,
            policy_live: false,
            cam: CamView { cameras: list_cameras(), ..Default::default() },
            fg: None,
            alert: None,
            user_meme: None,
            calc: {
                let mut c = Calc::default();
                c.open();
                c
            },
            calc_open: false,
            calc_min: false,
            calc_spawn: None,
            cal: CalState::default(),
            click_through,
            dash: web::Dashboard::default(),
            dash_shared,
            dash_rx,
            dash_was_open: false,
            last_publish: Instant::now(),
            first_tick: true,
            preview_cam: None,
            seed,
        }
    }

    pub fn now(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    fn rand(&mut self) -> u64 {
        // xorshift: only used to vary memes.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    pub fn alarm_active(&self) -> bool {
        self.alert.as_ref().is_some_and(|a| a.level == Level::Alarm)
    }

    // ---------------------------------------------------------------- user intents

    pub fn toggle_timer(&mut self) {
        if self.pomo.running && self.alarm_active() {
            self.clear_alarm(false);
        }
        self.pomo.toggle();
    }

    pub fn skip_phase(&mut self) {
        self.clear_alarm(false);
        self.pomo.skip(&self.s);
        self.block_clean = true;
    }

    pub fn reset_timer(&mut self) {
        self.clear_alarm(false);
        self.pomo.reset(&self.s);
        self.block_clean = true;
    }

    /// Ctrl+Alt+B / pill button: start a BRB pause, or end one / acknowledge an alarm.
    pub fn brb_or_back(&mut self) {
        if self.pomo.brb.is_some() || self.alarm_active() {
            self.im_back();
        } else if self.pomo.brb_start(&self.s) {
            self.alert = None;
        }
    }

    pub fn im_back(&mut self) {
        let was_brb = self.pomo.brb.is_some();
        self.pomo.back();
        if self.alarm_active() {
            self.clear_alarm(true);
        } else if was_brb {
            self.alert = None;
        }
    }

    fn clear_alarm(&mut self, greet: bool) {
        let now = self.now();
        self.policy.dismiss(now);
        let _ = self.audio.send(audio::Cmd::Stop);
        self.pomo.distracted_hold = false;
        self.user_meme = None;
        self.alert = None;
        if greet {
            self.info(copy::welcome_back(self.s.brainrot), 2.5);
        }
    }

    pub fn answer_ask(&mut self, study: bool) {
        let Some(alert) = self.alert.take() else { return };
        let Some(domain) = alert.ask_domain else { return };
        self.policy.answer_ask(&domain, study);
        if study && alert.remember && !self.s.allowed_sites.contains(&domain) {
            self.s.allowed_sites.push(domain.clone());
            self.s.ask_sites.retain(|d| d != &domain);
        }
        if !study && alert.remember && !self.s.blocked_sites.contains(&domain) {
            self.s.blocked_sites.push(domain.clone());
            self.s.ask_sites.retain(|d| d != &domain);
        }
    }

    pub fn calibrate(&mut self, target: CalTarget) {
        if self.cal.ball.is_some() {
            return;
        }
        self.cal = CalState { target: Some(target), ..Default::default() };
        if target == CalTarget::Screen {
            // The pill folds into the ball first; sampling starts when it reaches the first spot.
            self.cal.ball_pending = true;
        } else {
            let _ = self.vision.send(vision::Cmd::Calibrate { camera: self.s.camera_index, target });
        }
    }

    /// Called by the pill each frame while the screen-calibration ball runs; false once it has
    /// handed the window back.
    pub fn ball_frame(&mut self, ui: &mut egui::Ui) -> bool {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.cal.ball_pending) {
            let info = ctx.input(|i| i.viewport().clone());
            let home = info.outer_rect.map(|r| r.min).unwrap_or(egui::Pos2::ZERO);
            let monitor = info.monitor_size.unwrap_or(egui::vec2(1920.0, 1080.0));
            self.cal.ball = Some(crate::ball::Ball::new(home, monitor));
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
            self.dash.set_hidden(true);
            let _ = self.audio.send(audio::Cmd::Note { freq: 0.0, volume: 0.0 });
        }
        let Some(ball) = self.cal.ball.as_mut() else { return false };
        for step in ball.frame(ui, crate::ui::PILL) {
            match step {
                crate::ball::Step::StartSampling => {
                    let _ = self.vision.send(vision::Cmd::Calibrate { camera: self.s.camera_index, target: CalTarget::Screen });
                }
                crate::ball::Step::Note(freq) => {
                    let _ = self.audio.send(audio::Cmd::Note { freq, volume: self.s.volume });
                }
                crate::ball::Step::Finished => {
                    self.cal.ball = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(self.click_through));
                    self.dash.set_hidden(false);
                    return false;
                }
            }
        }
        true
    }

    pub fn open_dashboard(&mut self) {
        self.dash.open(self.dash_shared.clone());
    }

    /// The current phase has been running (or was penalised) rather than freshly set up.
    pub fn pomo_started(&self) -> bool {
        let mins = match self.pomo.phase {
            Phase::Focus => self.s.focus_min,
            Phase::ShortBreak => self.s.short_break_min,
            Phase::LongBreak => self.s.long_break_min,
        };
        self.pomo.remaining < mins as f64 * 60.0 - 0.5 || self.pomo.block_added_secs > 0
    }

    /// Pill button / Ctrl+Alt+K. Persists like the settings switch; screen checks keep running.
    pub fn toggle_camera(&mut self) {
        self.s.camera_enabled = !self.s.camera_enabled;
    }

    fn set_click_through(&mut self, on: bool, ctx: &egui::Context) {
        self.click_through = on;
        self.s.overlay_click_through = on;
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::MousePassthrough(on));
    }

    /// Merge a partial settings object from the dashboard; invalid values are ignored.
    fn apply_patch(&mut self, patch: &Value, ctx: &egui::Context) {
        let Value::Object(patch) = patch else { return };
        let Ok(mut cur) = serde_json::to_value(&self.s) else { return };
        for (k, v) in patch {
            cur[k] = v.clone();
        }
        if let Ok(new) = serde_json::from_value::<Settings>(cur) {
            let click = new.overlay_click_through;
            let cam_changed = new.camera_index != self.s.camera_index;
            self.s = new;
            if click != self.click_through {
                self.set_click_through(click, ctx);
            }
            if cam_changed {
                self.cam.error = None;
            }
        }
    }

    fn dash_action(&mut self, v: Value, ctx: &egui::Context) {
        match v["cmd"].as_str().unwrap_or_default() {
            "toggle" => self.toggle_timer(),
            "brb" => self.brb_or_back(),
            "skip" => self.skip_phase(),
            "reset" => self.reset_timer(),
            "settings" => self.apply_patch(&v["patch"], ctx),
            "calibrate" => {
                if let Some(t) = cal_target(v["target"].as_str().unwrap_or_default()) {
                    self.calibrate(t);
                }
            }
            // The ball must fly home and hand the window back; it ends with the worker's result.
            "cal-cancel" if self.cal.ball.is_none() && !self.cal.ball_pending => self.cal = CalState::default(),
            "cal-cancel" => {}
            "cal-reset" => {
                self.s.calibration = Default::default();
                let _ = self.vision.send(vision::Cmd::SetCalibration(self.s.calibration.clone()));
            }
            "test-sound" => {
                let _ = self.audio.send(audio::Cmd::Alarm { sound: self.s.sound.clone(), volume: self.s.volume, looping: false });
            }
            "stop-sound" => {
                let _ = self.audio.send(audio::Cmd::Stop);
            }
            "preview-alert" => self.preview_alert(ctx),
            "open-memes" => {
                let dir = self.media_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
            "pill-home" => self.s.overlay_pos = None,
            "calc" => {
                self.calc_open = true;
                self.calc_min = false;
                self.calc.open();
            }
            "quit" => {
                self.shutdown();
                std::process::exit(0);
            }
            "tour-done" => self.s.tour_done = true,
            "dashboard-error" => self.cam.error = v["error"].as_str().map(|e| format!("Window failed: {e}")),
            _ => {}
        }
    }

    /// Snapshot for the dashboard (`/state`).
    fn publish_state(&mut self) {
        let goal = self.s.daily_goal_min;
        let score = self.stats.score(goal);
        let goal_streak = self.stats.goal_streak(goal);
        let clean = self.stats.clean_streak;
        let day = self.stats.today().clone();
        let every = self.s.long_break_every.max(1);
        let sounds: Vec<[String; 2]> = audio::BUILTINS
            .iter()
            .map(|b| [format!("b:{b}"), b.to_string()])
            .chain(memes::media_files(&self.media_dir(), &["mp3", "wav"]).into_iter().map(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                [format!("f:{}", p.display()), name]
            }))
            .collect();
        let mut meme_opts = vec![["Random".to_string(), "A cat that fits the moment".to_string()]];
        meme_opts.extend(memes::BUILTINS.iter().map(|b| [format!("b:{b}"), b.to_string()]));
        meme_opts.push(["UserFolder".into(), "From my memes folder".into()]);
        meme_opts.push(["Off".into(), "No picture".into()]);
        let state = json!({
            "timer": {
                "phase": match self.pomo.phase { Phase::Focus => "focus", Phase::ShortBreak => "short", Phase::LongBreak => "long" },
                "remaining": self.pomo.remaining,
                "running": self.pomo.running,
                "started": self.pomo_started(),
                "brb": self.pomo.brb,
                "held": self.pomo.distracted_hold,
                "added": self.pomo.block_added_secs,
                "round": self.pomo.completed_focus % every + 1,
                "every": every,
            },
            "alarm": self.alarm_active(),
            "cam": {
                "enabled": self.s.camera_enabled,
                "error": self.cam.error,
                "cameras": self.cam.cameras,
                "calibrated": calibration_counts(&self.s.calibration),
            },
            "cal": {
                "target": self.cal.target.map(cal_name),
                "progress": self.cal.progress,
                "samples": self.cal.samples,
                "error": self.cal.error,
            },
            "today": {
                "focus_secs": day.focus_secs,
                "goal_min": goal,
                "score": score,
                "blocks": day.blocks,
                "distractions": day.distractions,
                "clean_streak": clean,
                "goal_streak": goal_streak,
            },
            "settings": self.s,
            "tour_done": self.s.tour_done,
            "hotkeys": hotkeys::ALL.iter().map(|(_, k, d)| [k.to_string(), d.to_string()]).collect::<Vec<_>>(),
            "sounds": sounds,
            "memes": meme_opts,
        });
        *self.dash_shared.state.lock().unwrap() = state.to_string();
    }

    // ---------------------------------------------------------------- alerts

    fn info(&mut self, line: copy::Line, secs: f32) {
        self.alert = Some(Alert {
            level: Level::Info,
            kind: None,
            ask_domain: None,
            line,
            opened: Instant::now(),
            auto_close: Some(secs),
            cat: None,
            added_secs: 0,
            remember: false,
        });
    }

    /// Settings "Preview alert": show a warning card with the current meme choice.
    pub fn preview_alert(&mut self, ctx: &egui::Context) {
        let kind = Kind::Site("x.com".into());
        let cat = self.pick_cat(&kind, ctx);
        self.alert = Some(Alert {
            level: Level::Warn,
            line: copy::warn(&kind, self.s.brainrot),
            kind: Some(kind),
            ask_domain: None,
            opened: Instant::now(),
            auto_close: Some(10.0),
            cat,
            added_secs: 0,
            remember: false,
        });
    }

    fn pick_cat(&mut self, kind: &Kind, ctx: &egui::Context) -> Option<CatStyle> {
        self.user_meme = None;
        let r = self.rand();
        match &self.s.meme {
            Meme::Off => None,
            Meme::Builtin(name) => Some(CatStyle::from_name(name)),
            Meme::Random => Some(CatStyle::for_kind(kind, r)),
            Meme::UserFolder => {
                let dir = self.media_dir();
                let files = memes::media_files(&dir, &["gif", "png", "jpg", "jpeg"]);
                if !files.is_empty() {
                    self.user_meme = UserMeme::load(ctx, &files[(r % files.len() as u64) as usize]);
                }
                // Folder empty or file broken: fall back to a painted cat.
                self.user_meme.is_none().then(|| CatStyle::for_kind(kind, r))
            }
        }
    }

    pub fn media_dir(&self) -> std::path::PathBuf {
        self.s.media_dir.clone().unwrap_or_else(|| crate::config::data_dir().join("memes"))
    }

    fn raise_alarm(&mut self, kind: Kind, ctx: &egui::Context) {
        let penalize = kind != Kind::BrbOverdue || self.s.penalty_brb_overdue;
        let mut added = 0;
        if penalize {
            added = self.pomo.penalize(&self.s);
            let day = self.stats.today();
            day.distractions += 1;
            if self.s.penalty_score {
                day.score_lost += self.s.penalty_score_points;
            }
            if self.s.penalty_reset_streak {
                self.stats.clean_streak = 0;
            }
            self.block_clean = false;
        }
        if self.s.penalty_pause_timer && kind != Kind::BrbOverdue {
            self.pomo.distracted_hold = true;
        }
        let _ = self.audio.send(audio::Cmd::Alarm { sound: self.s.sound.clone(), volume: self.s.volume, looping: self.s.alarm_loop });
        let cat = self.pick_cat(&kind, ctx);
        self.alert = Some(Alert {
            level: Level::Alarm,
            line: copy::alarm(&kind, self.s.brainrot),
            kind: Some(kind),
            ask_domain: None,
            opened: Instant::now(),
            auto_close: None,
            cat,
            added_secs: added,
            remember: false,
        });
    }

    fn on_action(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            Action::Warn(kind) => {
                self.stats.today().warnings += 1;
                if !self.alarm_active() {
                    let cat = self.pick_cat(&kind, ctx);
                    self.alert = Some(Alert {
                        level: Level::Warn,
                        line: copy::warn(&kind, self.s.brainrot),
                        kind: Some(kind),
                        ask_domain: None,
                        opened: Instant::now(),
                        auto_close: Some(8.0),
                        cat,
                        added_secs: 0,
                        remember: false,
                    });
                    if self.s.warn_sound {
                        let _ = self.audio.send(audio::Cmd::Warn { volume: self.s.volume });
                    }
                }
            }
            Action::Alarm(kind) => self.raise_alarm(kind, ctx),
            Action::Ask(domain) => {
                if !self.alarm_active() {
                    self.alert = Some(Alert {
                        level: Level::Ask,
                        line: copy::ask(&domain, self.s.brainrot),
                        kind: None,
                        ask_domain: Some(domain),
                        opened: Instant::now(),
                        auto_close: None,
                        cat: None,
                        added_secs: 0,
                        remember: false,
                    });
                    if self.s.warn_sound {
                        let _ = self.audio.send(audio::Cmd::Warn { volume: self.s.volume });
                    }
                }
            }
            Action::PostureHint => {
                if self.alert.is_none() {
                    self.info(copy::posture(self.s.brainrot), 6.0);
                }
            }
            Action::Recovered => {
                if self.alarm_active() {
                    self.clear_alarm(true);
                }
            }
            Action::CameraFast(f) => {
                let _ = self.vision.send(vision::Cmd::Fast(f));
            }
            Action::RunL2 => {
                let _ = self.vision.send(vision::Cmd::RunL2);
            }
        }
    }

    // ---------------------------------------------------------------- per-frame logic

    pub fn tick(&mut self, ctx: &egui::Context) {
        let now = self.now();
        if std::mem::take(&mut self.first_tick) && !self.s.tour_done {
            // First run: open the main window, which starts the guided tour.
            self.open_dashboard();
        }
        while let Ok(v) = self.dash_rx.try_recv() {
            self.dash_action(v, ctx);
        }

        while let Ok(k) = self.keys.try_recv() {
            match k {
                Hotkey::Calculator => {
                    self.calc_open = !self.calc_open;
                    if self.calc_open {
                        self.calc.open();
                        self.calc_min = false;
                    }
                }
                Hotkey::StartPause => self.toggle_timer(),
                Hotkey::Brb => self.brb_or_back(),
                Hotkey::Settings => self.open_dashboard(),
                Hotkey::ClickThrough => self.set_click_through(!self.click_through, ctx),
                Hotkey::Camera => self.toggle_camera(),
                Hotkey::Quit => {
                    self.shutdown();
                    std::process::exit(0);
                }
            }
        }

        let mut actions = Vec::new();
        while let Ok(m) = self.vision_rx.try_recv() {
            match m {
                vision::Msg::Obs { obs, face, eyes, gaze, l1_ms, l2 } => {
                    self.cam.error = None;
                    self.cam.face = face;
                    self.cam.eyes = eyes;
                    self.cam.l1_ms = l1_ms;
                    if l2.is_some() {
                        self.cam.l2 = l2;
                    }
                    self.cam.gaze = gaze.map(|g| g.0);
                    self.cam.confidence = gaze.map_or(0.0, |g| g.1);
                    if self.policy_live && self.s.camera_enabled {
                        actions.extend(self.policy.on_camera(obs, now, &self.s));
                        self.cam.gaze = self.policy.gaze;
                    }
                    // BRB auto-resume: back at the desk, looking at work.
                    if let Some(left) = self.pomo.brb {
                        let away = self.s.brb_secs as f64 - left;
                        if self.s.brb_auto_resume && away >= 5.0 && matches!(self.cam.gaze, Some(Gaze::Screen | Gaze::Book)) {
                            self.im_back();
                        }
                    }
                }
                vision::Msg::CalProgress { target, progress, samples } => {
                    if self.cal.target == Some(target) {
                        self.cal.progress = progress;
                        self.cal.samples = samples;
                    }
                }
                vision::Msg::Calibrated { target, samples, error } => {
                    // A cancelled step's late result is dropped.
                    if self.cal.target != Some(target) {
                        continue;
                    }
                    let mut ball = self.cal.ball.take();
                    if let Some(b) = ball.as_mut() {
                        b.sampling_done();
                    }
                    self.cal = CalState { ball, ..Default::default() };
                    if let Some(e) = error {
                        self.cal.error = Some(e);
                    } else {
                        let count = samples.len();
                        if matches!(target, CalTarget::Screen | CalTarget::Book) && count > 0 {
                            let n = count as f32;
                            let avg = |i: usize| samples.iter().map(|s| s.values[i]).sum::<f32>() / n;
                            let p = crate::config::Pose {
                                yaw: avg(0) / 70.0,
                                pitch: avg(1) / 75.0 + 0.62,
                                roll: avg(2),
                                area: avg(9),
                                center: [avg(10), avg(11)],
                            };
                            match target {
                                CalTarget::Screen => self.s.calibration.screen = Some(p),
                                CalTarget::Book => self.s.calibration.book = Some(p),
                                CalTarget::Away => {}
                            }
                        }
                        match target {
                            CalTarget::Screen => self.s.calibration.screen_samples = samples,
                            CalTarget::Book => self.s.calibration.book_samples = samples,
                            CalTarget::Away => self.s.calibration.away_samples = samples,
                        }
                        let _ = self.vision.send(vision::Cmd::SetCalibration(self.s.calibration.clone()));
                    }
                }
                vision::Msg::Error(e) => {
                    if self.cal.target.is_some() {
                        let mut ball = self.cal.ball.take();
                        if let Some(b) = ball.as_mut() {
                            b.sampling_done();
                        }
                        self.cal = CalState { error: Some(e.clone()), ball, ..Default::default() };
                    }
                    self.cam.error = Some(e);
                }
            }
        }
        while let Ok(fg) = self.screen_rx.try_recv() {
            if self.policy_live && self.s.screen_enabled {
                actions.extend(self.policy.on_screen(fg.as_ref(), now, &self.s));
            }
            self.fg = fg;
        }
        for a in actions {
            self.on_action(a, ctx);
        }

        // Pomodoro.
        let dt = self.last_tick.elapsed().as_secs_f64().min(5.0);
        self.last_tick = Instant::now();
        for ev in self.pomo.tick(dt, &self.s) {
            match ev {
                Event::PhaseEnded(Phase::Focus) => {
                    let day = self.stats.today();
                    day.blocks += 1;
                    if self.block_clean {
                        self.stats.clean_streak += 1;
                        self.stats.best_clean_streak = self.stats.best_clean_streak.max(self.stats.clean_streak);
                    }
                    self.block_clean = true;
                    self.stats.save();
                    if self.alarm_active() {
                        self.clear_alarm(false);
                    }
                    self.info(copy::phase_done(true, self.s.brainrot), 8.0);
                    let _ = self.audio.send(audio::Cmd::Warn { volume: self.s.volume });
                }
                Event::PhaseEnded(_) => {
                    self.block_clean = true;
                    self.info(copy::phase_done(false, self.s.brainrot), 8.0);
                    let _ = self.audio.send(audio::Cmd::Warn { volume: self.s.volume });
                }
                Event::BrbOverdue => self.raise_alarm(Kind::BrbOverdue, ctx),
            }
        }
        let earned = self.pomo.earned_secs - self.earned_seen;
        if earned >= 1.0 {
            self.stats.today().focus_secs += earned as u64;
            self.earned_seen += earned.floor();
        }

        self.sync_layers(now);

        if let Some(a) = &self.alert {
            if a.auto_close.is_some_and(|s| a.opened.elapsed().as_secs_f32() >= s) {
                self.alert = None;
                self.user_meme = None;
            }
        }

        // Persist settings (debounced; drags change positions every frame).
        if self.s != self.saved_s {
            let at = *self.dirty_at.get_or_insert_with(Instant::now);
            if at.elapsed().as_secs_f32() > 1.5 {
                self.s.save();
                self.saved_s = self.s.clone();
                self.dirty_at = None;
            }
        }
        if self.last_stats_save.elapsed().as_secs() >= 60 {
            self.stats.save();
            self.last_stats_save = Instant::now();
        }

        // Dashboard: live camera preview while it is open, and a state snapshot a few times a second.
        let dash_open = self.dash.is_open();
        let want_preview = dash_open.then_some(self.s.camera_index);
        if want_preview != self.preview_cam {
            let _ = self.vision.send(vision::Cmd::Preview(want_preview));
            self.preview_cam = want_preview;
        }
        if dash_open && (self.last_publish.elapsed().as_millis() >= 200 || !self.dash_was_open) {
            self.publish_state();
            self.last_publish = Instant::now();
        }
        self.dash_was_open = dash_open;

        // Repaint cadence: animation only while an alert card is up, else often enough for the clocks.
        if self.alert.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        } else if dash_open {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        } else if self.pomo.running || self.dirty_at.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    /// Start/stop the camera and screen layers to match what is needed right now.
    fn sync_layers(&mut self, now: f64) {
        let focusing = self.pomo.focusing();
        if focusing != self.policy_live {
            self.policy_live = focusing;
            if focusing {
                self.policy.restart(now);
            }
        }
        let want_cam = self.s.camera_enabled && (focusing || (self.pomo.brb.is_some() && self.s.brb_auto_resume));
        let interval = match self.s.camera_mode {
            CameraMode::Always => 0.5,
            CameraMode::Interval => self.s.check_interval_secs.max(1) as f64,
        };
        let want = want_cam.then_some((self.s.camera_index, interval));
        if want != self.cam_running {
            let _ = self.vision.send(match want {
                Some((camera, interval)) => vision::Cmd::Start { camera, interval },
                None => vision::Cmd::Stop,
            });
            if want.is_none() {
                self.cam.gaze = None;
                self.cam.face = None;
                self.cam.eyes = None;
                self.cam.confidence = 0.0;
            }
            self.cam_running = want;
        }
        let want_screen = self.s.screen_enabled && focusing;
        if want_screen != self.screen_running {
            let _ = self.screen.send(screen::Cmd::Enable(want_screen));
            self.screen_running = want_screen;
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.vision.send(vision::Cmd::Quit);
        crate::tray::remove();
        self.s.save();
        self.stats.save();
    }
}

fn list_cameras() -> Vec<String> {
    let _ = crate::camera::startup();
    crate::camera::list()
}

fn cal_target(name: &str) -> Option<CalTarget> {
    match name {
        "screen" => Some(CalTarget::Screen),
        "book" => Some(CalTarget::Book),
        "away" => Some(CalTarget::Away),
        _ => None,
    }
}

fn cal_name(t: CalTarget) -> &'static str {
    match t {
        CalTarget::Screen => "screen",
        CalTarget::Book => "book",
        CalTarget::Away => "away",
    }
}

/// Samples collected per target, for the dashboard summary.
fn calibration_counts(c: &crate::config::Calibration) -> Value {
    json!({ "screen": c.screen_samples.len(), "book": c.book_samples.len(), "away": c.away_samples.len() })
}
