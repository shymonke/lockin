//! Settings window (tabs) and the replayable guided tour that highlights each setting.

use crate::app::{App, Tab};
use crate::audio;
use crate::config::{CameraMode, Meme, Sound};
use crate::hotkeys;
use crate::memes;
use crate::policy::Gaze;
use crate::theme::{self, heavy};
use crate::vision::CalTarget;
use eframe::egui::{self, vec2, Align, DragValue, RichText, Stroke, StrokeKind, Ui, ViewportBuilder, ViewportId};

struct Step {
    tab: Tab,
    anchor: &'static str,
    title: &'static str,
    text: &'static str,
}

const TOUR: [Step; 13] = [
    Step { tab: Tab::Timer, anchor: "timer.lengths", title: "Welcome to LockIn", text: "Quick tour of where everything lives. The pill at the top of your screen is your timer: drag it anywhere, right-click it (or Ctrl+Alt+O) to open these settings." },
    Step { tab: Tab::Timer, anchor: "timer.lengths", title: "Pomodoro", text: "Focus and break lengths. Start or pause with the play button or Ctrl+Alt+P. Monitoring only runs during focus blocks." },
    Step { tab: Tab::Timer, anchor: "timer.brb", title: "BRB pause", text: "Getting water? Ctrl+Alt+B (or BRB on the pill) pauses everything for this long without alerts. Press it again / I'm back in time, otherwise the alarm goes off. The timer resumes exactly where it stopped." },
    Step { tab: Tab::Penalties, anchor: "pen.time", title: "Penalties", text: "Every confirmed distraction can add focus time, hold the timer until you're back, and cost score and your clean streak. Each one is a separate toggle." },
    Step { tab: Tab::Camera, anchor: "cam.calibrate", title: "Calibrate the camera", text: "Look at your screen and press the first button, then look at your book and press the second. Works with any camera position because LockIn learns your poses." },
    Step { tab: Tab::Camera, anchor: "cam.rules", title: "How the camera checks", text: "Layer 1: a 230 KB face model looks every few seconds. If you look away or vanish it speeds up, and layer 2 (an object detector) checks for a phone or whether you left the desk." },
    Step { tab: Tab::Sites, anchor: "sites.blocked", title: "Blocked sites", text: "Opening one shows a warning. The second time, or staying past the limit, sets off the alarm. App names like discord.exe work too." },
    Step { tab: Tab::Sites, anchor: "sites.ask", title: "Ask sites", text: "YouTube can be a lecture or brainrot. LockIn asks once; tick Remember to save the answer." },
    Step { tab: Tab::Alerts, anchor: "alerts.sound", title: "Alarm sound", text: "Pick a built-in sound or put your own MP3/WAV in the memes folder and pick it here." },
    Step { tab: Tab::Alerts, anchor: "alerts.meme", title: "Memes", text: "Painted cats (scuba cat included) on alerts, or your own GIFs from the memes folder." },
    Step { tab: Tab::Alerts, anchor: "alerts.brainrot", title: "Brainrot mode", text: "Switch the alert text to unhinged mode. Off = normal wording." },
    Step { tab: Tab::Keys, anchor: "keys.list", title: "Shortcuts", text: "Ctrl+Alt+C opens the CAT exam calculator anywhere. Ctrl+Alt+L makes the pill click-through so it never blocks clicks." },
    Step { tab: Tab::Stats, anchor: "stats.today", title: "Stats", text: "Focus time, score and streaks live here. Replay this tour any time from the button at the top." },
];

const TABS: [(Tab, &str); 7] = [
    (Tab::Timer, "Timer"),
    (Tab::Penalties, "Penalties"),
    (Tab::Camera, "Camera"),
    (Tab::Sites, "Sites"),
    (Tab::Alerts, "Alerts"),
    (Tab::Keys, "Keys"),
    (Tab::Stats, "Stats"),
];

fn num(ui: &mut Ui, label: &str, v: &mut u32, range: std::ops::RangeInclusive<u32>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(DragValue::new(v).range(range).suffix(suffix));
    });
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(theme::MUTED));
}

impl App {
    pub fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        // Fit small/high-DPI screens (e.g. 1080p at 150% = 720 pt tall).
        let monitor = ctx.input_for(ViewportId::ROOT, |i| i.viewport().monitor_size).unwrap_or(vec2(1280.0, 720.0));
        let h = (monitor.y - 110.0).clamp(420.0, 760.0);
        let b = ViewportBuilder::default()
            .with_title("LockIn settings")
            .with_inner_size(vec2(640.0, h))
            .with_position(egui::pos2(((monitor.x - 640.0) / 2.0).max(0.0), 40.0))
            .with_min_inner_size(vec2(520.0, 400.0));
        ctx.show_viewport_immediate(ViewportId::from_hash_of("settings"), b, |ui, _| {
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                self.settings_open = false;
                if self.tour.take().is_some() {
                    self.s.tour_done = true;
                }
                self.test_camera = false;
            }
            egui::Frame::new().fill(theme::INK).inner_margin(18).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.settings_ui(ui);
            });
        });
    }

    /// Wraps a settings block; records its rect for the tour and highlights/scrolls to it.
    fn section(&mut self, ui: &mut Ui, id: &'static str, title: &str, add: impl FnOnce(&mut App, &mut Ui)) {
        let r = egui::Frame::new()
            .fill(theme::CARD)
            .corner_radius(14)
            .inner_margin(14)
            .stroke(Stroke::new(1.0, theme::LINE))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).font(heavy(16.0)));
                ui.add_space(2.0);
                add(self, ui);
            })
            .response;
        ui.add_space(10.0);
        if let Some(step) = self.tour {
            if TOUR[step].anchor == id {
                if self.tour_scrolled != Some(step) {
                    ui.scroll_to_rect(r.rect, Some(Align::Min));
                    self.tour_scrolled = Some(step);
                }
                let t = ui.ctx().input(|i| i.time) as f32;
                let glow = theme::LIME.linear_multiply(0.55 + 0.45 * (t * 3.0).sin().abs());
                // Clip to the scroll viewport so the ring never paints over the tabs or tour card.
                ui.painter().rect_stroke(r.rect, 14.0, Stroke::new(3.0, glow), StrokeKind::Inside);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("LOCKIN").font(heavy(28.0)).color(theme::LIME));
            ui.label(RichText::new(if self.s.brainrot { "lock in or get cooked" } else { "study focus tracker" }).color(theme::MUTED));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Quit").clicked() {
                    self.shutdown();
                    std::process::exit(0);
                }
                if ui.button(RichText::new("Take the tour").color(theme::LIME)).clicked() {
                    self.start_tour();
                }
            });
        });
        ui.add_space(6.0);

        if let Some(step) = self.tour {
            self.tab = TOUR[step].tab;
        }
        ui.horizontal_wrapped(|ui| {
            for (tab, name) in TABS {
                let on = self.tab == tab;
                let text = RichText::new(name).font(heavy(14.0)).color(if on { theme::INK } else { theme::TEXT });
                let b = egui::Button::new(text).fill(if on { theme::LIME } else { theme::CARD_HI }).corner_radius(16);
                if ui.add(b).clicked() && self.tour.is_none() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(8.0);

        let tour_h = if self.tour.is_some() { 150.0 } else { 0.0 };
        let scroll = egui::ScrollArea::vertical().id_salt(self.tab).max_height(ui.available_height() - tour_h).auto_shrink([false, false]);
        scroll.show(ui, |ui| match self.tab {
            Tab::Timer => self.tab_timer(ui),
            Tab::Penalties => self.tab_penalties(ui),
            Tab::Camera => self.tab_camera(ui),
            Tab::Sites => self.tab_sites(ui),
            Tab::Alerts => self.tab_alerts(ui),
            Tab::Keys => self.tab_keys(ui),
            Tab::Stats => self.tab_stats(ui),
        });

        if let Some(step) = self.tour {
            self.tour_card(ui, step);
        }
    }

    fn tour_card(&mut self, ui: &mut Ui, step: usize) {
        let s = &TOUR[step];
        egui::Frame::new().fill(theme::CARD_HI).corner_radius(16).inner_margin(14).stroke(Stroke::new(2.0, theme::LIME)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("TOUR {}/{}", step + 1, TOUR.len())).small().color(theme::LIME));
                ui.label(RichText::new(s.title).font(heavy(17.0)));
            });
            ui.label(s.text);
            ui.horizontal(|ui| {
                let last = step + 1 == TOUR.len();
                let next = egui::Button::new(RichText::new(if last { "Finish" } else { "Next" }).font(heavy(14.0)).color(theme::INK)).fill(theme::LIME);
                if ui.add(next).clicked() {
                    if last {
                        self.tour = None;
                        self.s.tour_done = true;
                    } else {
                        self.tour = Some(step + 1);
                    }
                }
                if step > 0 && ui.button("Back").clicked() {
                    self.tour = Some(step - 1);
                }
                if !last && ui.button("Skip tour").clicked() {
                    self.tour = None;
                    self.s.tour_done = true;
                }
            });
        });
    }

    fn tab_timer(&mut self, ui: &mut Ui) {
        self.section(ui, "timer.lengths", "Pomodoro", |a, ui| {
            num(ui, "Focus", &mut a.s.focus_min, 1..=240, " min");
            num(ui, "Short break", &mut a.s.short_break_min, 1..=60, " min");
            num(ui, "Long break", &mut a.s.long_break_min, 1..=120, " min");
            num(ui, "Long break after every", &mut a.s.long_break_every, 1..=12, " blocks");
            ui.checkbox(&mut a.s.auto_start_breaks, "Start breaks automatically");
            ui.checkbox(&mut a.s.auto_start_focus, "Start focus blocks automatically after breaks");
            num(ui, "Daily focus goal", &mut a.s.daily_goal_min, 10..=900, " min");
            hint(ui, "Length changes apply from the next block (or press Reset).");
            ui.horizontal(|ui| {
                if ui.button(if a.pomo.running { "Pause" } else { "Start" }).clicked() {
                    a.toggle_timer();
                }
                if ui.button("Skip phase").clicked() {
                    a.skip_phase();
                }
                if ui.button("Reset").clicked() {
                    a.reset_timer();
                }
            });
        });
        self.section(ui, "timer.brb", "BRB pause", |a, ui| {
            num(ui, "Allowed time away", &mut a.s.brb_secs, 15..=1800, " s");
            ui.checkbox(&mut a.s.brb_auto_resume, "Resume automatically when the camera sees me back at work");
            hint(ui, "Ctrl+Alt+B starts it; press again (or I'm back) to resume. No alerts while it runs; the alarm fires only when time is up.");
        });
    }

    fn tab_penalties(&mut self, ui: &mut Ui) {
        self.section(ui, "pen.time", "Add focus time", |a, ui| {
            ui.checkbox(&mut a.s.penalty_add_time, "Add time to the current focus block on each alarm");
            ui.add_enabled_ui(a.s.penalty_add_time, |ui| {
                num(ui, "Penalty", &mut a.s.penalty_add_secs, 10..=900, " s");
                ui.checkbox(&mut a.s.penalty_escalate, "Double it for each further alarm in the same block");
                num(ui, "Max added per block", &mut a.s.penalty_max_add_secs, 10..=3600, " s");
            });
        });
        self.section(ui, "pen.pause", "Hold the timer", |a, ui| {
            ui.checkbox(&mut a.s.penalty_pause_timer, "Freeze the focus timer while distracted (until you're back)");
        });
        self.section(ui, "pen.score", "Score and streak", |a, ui| {
            ui.checkbox(&mut a.s.penalty_score, "Lose score points per alarm");
            ui.add_enabled_ui(a.s.penalty_score, |ui| num(ui, "Points", &mut a.s.penalty_score_points, 1..=50, ""));
            ui.checkbox(&mut a.s.penalty_reset_streak, "Reset the clean-block streak");
            ui.checkbox(&mut a.s.penalty_brb_overdue, "Overrunning a BRB pause counts as a distraction");
        });
    }

    fn tab_camera(&mut self, ui: &mut Ui) {
        self.section(ui, "cam.enable", "Camera", |a, ui| {
            ui.checkbox(&mut a.s.camera_enabled, "Use the camera during focus blocks");
            ui.horizontal(|ui| {
                ui.label("Device");
                let name = a.cam.cameras.get(a.s.camera_index as usize).cloned().unwrap_or_else(|| "No camera found".into());
                egui::ComboBox::from_id_salt("cam-dev").selected_text(name).show_ui(ui, |ui| {
                    for (i, c) in a.cam.cameras.clone().iter().enumerate() {
                        ui.selectable_value(&mut a.s.camera_index, i as u32, c);
                    }
                });
                if ui.button("Refresh").clicked() {
                    a.refresh_cameras();
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut a.s.camera_mode, CameraMode::Interval, "Check on an interval (low power)");
                ui.radio_value(&mut a.s.camera_mode, CameraMode::Always, "Always (2 fps)");
            });
            ui.add_enabled_ui(a.s.camera_mode == CameraMode::Interval, |ui| {
                num(ui, "Check every", &mut a.s.check_interval_secs, 1..=30, " s");
            });
            hint(ui, "Frames are analysed in memory and never saved. The camera is released during breaks and pauses.");
        });
        self.section(ui, "cam.calibrate", "Calibrate (recommended)", |a, ui| {
            ui.horizontal(|ui| {
                let busy = a.calibrating.is_some();
                if ui.add_enabled(!busy, egui::Button::new("1. Look at the SCREEN, then click")).clicked() {
                    a.calibrate(CalTarget::Screen);
                }
                if ui.add_enabled(!busy, egui::Button::new("2. Look at the BOOK, then click")).clicked() {
                    a.calibrate(CalTarget::Book);
                }
                if ui.add_enabled(!busy, egui::Button::new("3. Look AWAY, then click")).clicked() {
                    a.calibrate(CalTarget::Away);
                }
            });
            if let Some(m) = &a.cal_msg {
                ui.label(RichText::new(m).color(theme::LIME));
            }
            let show = |p: Option<crate::config::Pose>| match p {
                Some(p) => format!("pitch {:.2}  yaw {:.2}  size {:.3}", p.pitch, p.yaw, p.area),
                None => "default".into(),
            };
            hint(ui, &format!("Screen pose: {}", show(a.s.calibration.screen)));
            hint(ui, &format!("Book pose: {}", show(a.s.calibration.book)));
            hint(ui, &format!("Eye samples: screen {}  book {}  away {}", a.s.calibration.screen_samples.len(), a.s.calibration.book_samples.len(), a.s.calibration.away_samples.len()));
            if ui.button("Reset calibration").clicked() {
                a.s.calibration = Default::default();
                let _ = a.vision.send(crate::vision::Cmd::SetCalibration(a.s.calibration.clone()));
            }
            ui.separator();
            ui.checkbox(&mut a.test_camera, "Live test (runs the camera now)");
            if a.test_camera {
                let gaze = match a.cam.gaze {
                    Some(Gaze::Screen) => ("looking at SCREEN", theme::LIME),
                    Some(Gaze::Book) => ("looking at BOOK", theme::LIME),
                    Some(Gaze::Away) => ("looking AWAY", theme::AMBER),
                    Some(Gaze::Absent) => ("no face", theme::RED),
                    None => ("starting...", theme::MUTED),
                };
                ui.label(RichText::new(gaze.0).font(heavy(18.0)).color(gaze.1));
                if let Some(f) = a.cam.face {
                    let p = f.pose();
                    hint(ui, &format!("face {:.2}  pitch {:.2}  yaw {:.2}  size {:.3}  |  L1 {:.0} ms", f.score, p.pitch, p.yaw, p.area, a.cam.l1_ms));
                }
                if let Some((o, ms)) = a.cam.l2 {
                    hint(ui, &format!("L2: person {:.2}  phone {:.2}  ({ms:.0} ms)", o.person, o.phone));
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            }
            if let Some(e) = &a.cam.error {
                ui.label(RichText::new(e).color(theme::RED));
            }
        });
        self.section(ui, "cam.rules", "Rules", |a, ui| {
            num(ui, "Alarm when away from the desk for", &mut a.s.absent_secs, 5..=600, " s");
            num(ui, "Alarm when looking away for", &mut a.s.away_secs, 5..=600, " s");
            num(ui, "Check the screen after looking at it for", &mut a.s.screen_secs, 10..=3600, " s");
            hint(ui, "Screen check: if the screen shows a blocked site you get an alarm; an ask-site gets a question.");
            ui.checkbox(&mut a.s.phone_detection, "Detect phones (layer 2, runs only when your head is down)");
            ui.add_enabled_ui(a.s.phone_detection, |ui| num(ui, "Phone check every", &mut a.s.phone_check_secs, 10..=600, " s"));
            ui.checkbox(&mut a.s.posture_hints, "Posture hints when leaning into the screen");
            ui.add_enabled_ui(a.s.posture_hints, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Lean threshold");
                    ui.add(egui::Slider::new(&mut a.s.lean_ratio, 1.2..=2.5).suffix("x face size"));
                });
            });
            hint(ui, "Posture needs the screen calibration (it compares face size).");
        });
    }

    fn site_list(&mut self, ui: &mut Ui, which: usize) {
        let list = match which {
            0 => &mut self.s.blocked_sites,
            1 => &mut self.s.ask_sites,
            _ => &mut self.s.allowed_sites,
        };
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, d) in list.iter().enumerate() {
                let chip = egui::Button::new(RichText::new(format!("{d}  ×")).color(theme::TEXT)).fill(theme::CARD_HI).corner_radius(12);
                if ui.add(chip).on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
            }
        });
        if let Some(i) = remove {
            list.remove(i);
        }
        ui.horizontal(|ui| {
            let input = &mut self.new_site[which];
            let r = ui.add(egui::TextEdit::singleline(input).hint_text("domain.com or app.exe").desired_width(220.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Add").clicked() || enter) && !input.trim().is_empty() {
                let v = crate::policy::host_of(input.trim()).unwrap_or_else(|| input.trim().to_ascii_lowercase());
                let list = match which {
                    0 => &mut self.s.blocked_sites,
                    1 => &mut self.s.ask_sites,
                    _ => &mut self.s.allowed_sites,
                };
                if !list.contains(&v) {
                    list.push(v);
                }
                self.new_site[which].clear();
            }
        });
    }

    fn tab_sites(&mut self, ui: &mut Ui) {
        self.section(ui, "sites.blocked", "Blocked", |a, ui| {
            ui.checkbox(&mut a.s.screen_enabled, "Watch the active app / browser tab during focus blocks");
            a.site_list(ui, 0);
            num(ui, "Alarm on visit number", &mut a.s.strikes_before_alarm, 1..=10, "");
            num(ui, "Alarm anyway after staying", &mut a.s.blocked_dwell_secs, 5..=600, " s");
            hint(ui, "Strikes reset daily. Only the window title, app name and address-bar URL are read; nothing is captured.");
        });
        self.section(ui, "sites.ask", "Ask first", |a, ui| a.site_list(ui, 1));
        self.section(ui, "sites.allowed", "Always allowed", |a, ui| a.site_list(ui, 2));
        if let Some(fg) = &self.fg {
            hint(ui, &format!("Now: {}  |  {}  |  {}", fg.process, fg.title, fg.url.as_deref().unwrap_or("-")));
        }
    }

    fn tab_alerts(&mut self, ui: &mut Ui) {
        self.section(ui, "alerts.sound", "Alarm sound", |a, ui| {
            let files = memes::media_files(&a.media_dir(), &["mp3", "wav"]);
            let label = match &a.s.sound {
                Sound::Builtin(n) => n.clone(),
                Sound::File(p) => p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
            };
            egui::ComboBox::from_id_salt("sound").selected_text(label).width(260.0).show_ui(ui, |ui| {
                for b in audio::BUILTINS {
                    ui.selectable_value(&mut a.s.sound, Sound::Builtin(b.into()), b);
                }
                for f in files {
                    let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    ui.selectable_value(&mut a.s.sound, Sound::File(f), name);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Volume");
                ui.add(egui::Slider::new(&mut a.s.volume, 0.0..=1.0).show_value(false));
                if ui.button("Test").clicked() {
                    let _ = a.audio.send(audio::Cmd::Alarm { sound: a.s.sound.clone(), volume: a.s.volume, looping: false });
                }
                if ui.button("Stop").clicked() {
                    let _ = a.audio.send(audio::Cmd::Stop);
                }
            });
            ui.checkbox(&mut a.s.alarm_loop, "Repeat the alarm until I'm back");
            ui.checkbox(&mut a.s.warn_sound, "Soft chime on warnings and questions");
        });
        self.section(ui, "alerts.meme", "Memes", |a, ui| {
            let label = match &a.s.meme {
                Meme::Random => "Random cat (fits the distraction)".to_string(),
                Meme::Builtin(n) => n.clone(),
                Meme::UserFolder => "From my memes folder".into(),
                Meme::Off => "Off".into(),
            };
            egui::ComboBox::from_id_salt("meme").selected_text(label).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut a.s.meme, Meme::Random, "Random cat (fits the distraction)");
                for b in memes::BUILTINS {
                    ui.selectable_value(&mut a.s.meme, Meme::Builtin(b.into()), b);
                }
                ui.selectable_value(&mut a.s.meme, Meme::UserFolder, "From my memes folder");
                ui.selectable_value(&mut a.s.meme, Meme::Off, "Off");
            });
            let dir = a.media_dir();
            ui.horizontal(|ui| {
                if ui.button("Preview alert").clicked() {
                    let ctx = ui.ctx().clone();
                    a.preview_alert(&ctx);
                }
                if ui.button("Open memes folder").clicked() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                }
            });
            hint(ui, &format!("Folder: {}  (GIF/PNG/JPG memes, MP3/WAV sounds)", dir.display()));
        });
        self.section(ui, "alerts.brainrot", "Brainrot mode", |a, ui| {
            ui.checkbox(&mut a.s.brainrot, "Unhinged alert text");
        });
    }

    fn tab_keys(&mut self, ui: &mut Ui) {
        self.section(ui, "keys.list", "Global shortcuts", |a, ui| {
            egui::Grid::new("keys").num_columns(2).spacing(vec2(18.0, 8.0)).show(ui, |ui| {
                for (_, key, what) in hotkeys::ALL {
                    ui.label(RichText::new(format!("Ctrl + Alt + {key}")).font(heavy(14.0)).color(theme::LIME));
                    ui.label(what);
                    ui.end_row();
                }
            });
            ui.separator();
            if ui.checkbox(&mut a.click_through, "Pill is click-through (unlock with Ctrl+Alt+L)").changed() {
                a.s.overlay_click_through = a.click_through;
                ui.ctx().send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::MousePassthrough(a.click_through));
            }
            if ui.button("Move pill back to top centre").clicked() {
                a.s.overlay_pos = None;
            }
        });
    }

    fn tab_stats(&mut self, ui: &mut Ui) {
        let goal = self.s.daily_goal_min;
        let score = self.stats.score(goal);
        let streak = self.stats.goal_streak(goal);
        self.section(ui, "stats.today", "Today", |a, ui| {
            let d = a.stats.today().clone();
            egui::Grid::new("today").num_columns(2).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                let row = |ui: &mut Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).color(theme::MUTED));
                    ui.label(RichText::new(v).font(heavy(16.0)));
                    ui.end_row();
                };
                row(ui, "Focus time", format!("{} / {} min", d.focus_secs / 60, goal));
                row(ui, "Score", format!("{score} / 100"));
                row(ui, "Focus blocks", d.blocks.to_string());
                row(ui, "Warnings", d.warnings.to_string());
                row(ui, "Distractions", d.distractions.to_string());
                row(ui, "Clean-block streak", format!("{} (best {})", a.stats.clean_streak, a.stats.best_clean_streak));
                row(ui, "Daily-goal streak", format!("{streak} days"));
            });
        });
        self.section(ui, "stats.week", "Last 14 days (focus minutes)", |a, ui| {
            let days: Vec<_> = a.stats.days.iter().rev().take(14).rev().cloned().collect();
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 140.0), egui::Sense::hover());
            let p = ui.painter();
            let max = days.iter().map(|d| d.focus_secs).max().unwrap_or(0).max(goal as u64 * 60).max(60) as f32;
            let w = rect.width() / 14.0;
            let goal_y = rect.bottom() - 18.0 - (goal as f32 * 60.0 / max) * (rect.height() - 30.0);
            p.line_segment([egui::pos2(rect.left(), goal_y), egui::pos2(rect.right(), goal_y)], Stroke::new(1.0, theme::LIME.linear_multiply(0.5)));
            for (i, d) in days.iter().enumerate() {
                let h = (d.focus_secs as f32 / max) * (rect.height() - 30.0);
                let x = rect.left() + i as f32 * w;
                let bar = egui::Rect::from_min_max(egui::pos2(x + 4.0, rect.bottom() - 18.0 - h), egui::pos2(x + w - 4.0, rect.bottom() - 18.0));
                let col = if d.focus_secs >= goal as u64 * 60 { theme::LIME } else { theme::BLUE };
                p.rect_filled(bar, 4.0, col);
                p.text(egui::pos2(x + w / 2.0, rect.bottom()), egui::Align2::CENTER_BOTTOM, &d.date[8..], egui::FontId::proportional(10.0), theme::MUTED);
            }
        });
    }
}
