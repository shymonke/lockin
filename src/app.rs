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
use eframe::egui;
use std::sync::mpsc::{channel, Receiver, Sender};
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

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tab {
    Timer,
    Penalties,
    Camera,
    Sites,
    Alerts,
    Keys,
    Stats,
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

pub struct App {
    pub s: Settings,
    saved_s: Settings,
    dirty_at: Option<Instant>,
    pub stats: Stats,
    pub pomo: Pomodoro,
    pub policy: Policy,
    pub vision: Sender<vision::Cmd>,
    vision_rx: Receiver<vision::Msg>,
    #[allow(dead_code)] // Read by the WebView2 dashboard integration branch.
    pub preview: PreviewSlot,
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
    pub settings_open: bool,
    pub tab: Tab,
    pub tour: Option<usize>,
    pub tour_scrolled: Option<usize>,
    pub calibrating: Option<CalTarget>,
    pub cal_msg: Option<String>,
    pub test_camera: bool,
    pub click_through: bool,
    pub new_site: [String; 3],
    seed: u64,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, s: Settings) -> App {
        crate::theme::install(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let wake = move || ctx.request_repaint();

        let (vtx, vrx) = channel();
        let (vmsg_tx, vmsg_rx) = channel();
        let preview = std::sync::Arc::new(std::sync::Mutex::new(None));
        vision::spawn(vrx, vmsg_tx, Box::new(wake.clone()), preview.clone());
        let _ = vtx.send(vision::Cmd::SetCalibration(s.calibration.clone()));
        let (stx, srx) = channel();
        let (smsg_tx, smsg_rx) = channel();
        screen::spawn(srx, smsg_tx, Box::new(wake.clone()));
        let (atx, arx) = channel();
        audio::spawn(arx);
        let (ktx, krx) = channel();
        hotkeys::spawn(ktx, Box::new(wake));

        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
        let first_run = !s.tour_done;
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
            preview,
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
            settings_open: first_run,
            tab: Tab::Timer,
            tour: first_run.then_some(0),
            tour_scrolled: None,
            calibrating: None,
            cal_msg: None,
            test_camera: false,
            click_through,
            new_site: Default::default(),
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
        self.calibrating = Some(target);
        self.cal_msg = Some("Hold your gaze on the target for 5 seconds...".into());
        let _ = self.vision.send(vision::Cmd::Calibrate { camera: self.s.camera_index, target });
    }

    pub fn refresh_cameras(&mut self) {
        self.cam.cameras = list_cameras();
    }

    pub fn start_tour(&mut self) {
        self.settings_open = true;
        self.tour = Some(0);
        self.tour_scrolled = None;
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
                Hotkey::Settings => self.settings_open = true,
                Hotkey::ClickThrough => {
                    self.click_through = !self.click_through;
                    self.s.overlay_click_through = self.click_through;
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::MousePassthrough(self.click_through));
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
                    self.cal_msg = Some(format!("{target:?}: {:.0}% ({samples} good samples)", progress * 100.0));
                }
                vision::Msg::Calibrated { target, samples, error } => {
                    self.calibrating = None;
                    if let Some(e) = error {
                        self.cal_msg = Some(e);
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
                        self.cal_msg = Some(format!("Saved {count} {target:?} samples."));
                    }
                }
                vision::Msg::Error(e) => {
                    self.calibrating = None;
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

        // Repaint cadence: animation only while an alert card is up, else once a second for the clock.
        if self.alert.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
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
        let want_cam = (self.s.camera_enabled && (focusing || (self.pomo.brb.is_some() && self.s.brb_auto_resume))) || self.test_camera;
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
        self.s.save();
        self.stats.save();
    }
}

fn list_cameras() -> Vec<String> {
    let _ = crate::camera::startup();
    crate::camera::list()
}
