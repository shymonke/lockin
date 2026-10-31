//! User settings and stats, stored as JSON in `%APPDATA%\LockIn`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Pose {
    /// Nose offset from the eye midpoint, in eye-distance units (left/right turn).
    pub yaw: f32,
    /// Nose drop between eyes and mouth (0 = at eyes, 1 = at mouth); grows when looking down.
    pub pitch: f32,
    /// Face box area as a fraction of the frame (grows when leaning in).
    pub area: f32,
    /// In-plane head roll in degrees. Older settings omit this field.
    pub roll: f32,
    /// Face-box centre in normalised frame coordinates. Older settings omit this field.
    pub center: [f32; 2],
}

pub const GAZE_FEATURES: usize = 12;

/// One eye/head observation used by the per-user gaze classifier.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FeatureSample {
    /// yaw/pitch/roll degrees, two iris x/y ratios, two openness ratios,
    /// face area and face centre x/y.
    pub values: [f32; GAZE_FEATURES],
}

impl Default for FeatureSample {
    fn default() -> Self {
        Self { values: [0.0; GAZE_FEATURES] }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Calibration {
    /// Legacy pose averages retained for old settings and uncalibrated fallback/posture size.
    pub screen: Option<Pose>,
    pub book: Option<Pose>,
    pub screen_samples: Vec<FeatureSample>,
    pub book_samples: Vec<FeatureSample>,
    pub away_samples: Vec<FeatureSample>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CameraMode {
    /// One frame every `check_interval_secs`; speeds up only when something looks off.
    Interval,
    /// Two frames per second all the time.
    Always,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Sound {
    Builtin(String),
    File(PathBuf),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Meme {
    Random,
    Builtin(String),
    /// Pick from the user media folder (GIF/PNG/JPG).
    UserFolder,
    Off,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    // Pomodoro
    pub focus_min: u32,
    pub short_break_min: u32,
    pub long_break_min: u32,
    pub long_break_every: u32,
    pub auto_start_breaks: bool,
    pub auto_start_focus: bool,
    pub brb_secs: u32,
    pub brb_auto_resume: bool,
    pub daily_goal_min: u32,

    // Penalties (each toggleable)
    pub penalty_add_time: bool,
    pub penalty_add_secs: u32,
    pub penalty_escalate: bool,
    pub penalty_max_add_secs: u32,
    pub penalty_pause_timer: bool,
    pub penalty_score: bool,
    pub penalty_score_points: u32,
    pub penalty_reset_streak: bool,
    pub penalty_brb_overdue: bool,

    // Camera layer
    pub camera_enabled: bool,
    pub camera_index: u32,
    pub camera_mode: CameraMode,
    pub check_interval_secs: u32,
    pub absent_secs: u32,
    pub away_secs: u32,
    pub screen_secs: u32,
    pub posture_hints: bool,
    pub lean_ratio: f32,
    pub phone_detection: bool,
    pub phone_check_secs: u32,
    pub calibration: Calibration,

    // Screen layer
    pub screen_enabled: bool,
    pub blocked_sites: Vec<String>,
    pub ask_sites: Vec<String>,
    pub allowed_sites: Vec<String>,
    pub strikes_before_alarm: u32,
    pub blocked_dwell_secs: u32,

    // Alerts
    pub sound: Sound,
    pub warn_sound: bool,
    pub volume: f32,
    pub alarm_loop: bool,
    pub meme: Meme,
    pub media_dir: Option<PathBuf>,
    pub brainrot: bool,

    // Windows
    pub overlay_pos: Option<[f32; 2]>,
    pub overlay_click_through: bool,
    pub calc_pos: Option<[f32; 2]>,
    pub tour_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
        Settings {
            focus_min: 25,
            short_break_min: 5,
            long_break_min: 15,
            long_break_every: 4,
            auto_start_breaks: true,
            auto_start_focus: false,
            brb_secs: 120,
            brb_auto_resume: false,
            daily_goal_min: 120,

            penalty_add_time: true,
            penalty_add_secs: 60,
            penalty_escalate: true,
            penalty_max_add_secs: 300,
            penalty_pause_timer: true,
            penalty_score: true,
            penalty_score_points: 5,
            penalty_reset_streak: true,
            penalty_brb_overdue: true,

            camera_enabled: true,
            camera_index: 0,
            camera_mode: CameraMode::Interval,
            check_interval_secs: 3,
            absent_secs: 30,
            away_secs: 20,
            screen_secs: 300,
            posture_hints: true,
            lean_ratio: 1.5,
            phone_detection: true,
            phone_check_secs: 45,
            calibration: Calibration::default(),

            screen_enabled: true,
            blocked_sites: s(&["x.com", "twitter.com", "instagram.com", "tiktok.com", "facebook.com"]),
            ask_sites: s(&["youtube.com", "reddit.com"]),
            allowed_sites: Vec::new(),
            strikes_before_alarm: 2,
            blocked_dwell_secs: 20,

            sound: Sound::Builtin("Siren".into()),
            warn_sound: true,
            volume: 0.8,
            alarm_loop: true,
            meme: Meme::Random,
            media_dir: None,
            brainrot: false,

            overlay_pos: None,
            overlay_click_through: false,
            calc_pos: None,
            tour_done: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Day {
    pub date: String,
    pub focus_secs: u64,
    pub blocks: u32,
    pub warnings: u32,
    pub distractions: u32,
    pub score_lost: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub days: Vec<Day>,
    /// Focus blocks finished in a row without a distraction alarm.
    pub clean_streak: u32,
    pub best_clean_streak: u32,
}

impl Stats {
    pub fn today(&mut self) -> &mut Day {
        let date = today_string();
        if self.days.last().map(|d| d.date != date).unwrap_or(true) {
            self.days.push(Day { date, ..Default::default() });
            // Keep a year of history.
            if self.days.len() > 366 {
                self.days.remove(0);
            }
        }
        self.days.last_mut().unwrap()
    }
    /// Today's score out of 100: focus progress towards the goal minus penalties.
    pub fn score(&mut self, goal_min: u32) -> u32 {
        let d = self.today();
        let progress = (d.focus_secs as f64 / (goal_min.max(1) as f64 * 60.0) * 100.0).min(100.0) as i64;
        (progress - d.score_lost as i64).clamp(0, 100) as u32
    }
    /// Consecutive days (ending today or yesterday) that hit the daily goal.
    pub fn goal_streak(&self, goal_min: u32) -> u32 {
        let goal = goal_min as u64 * 60;
        let mut n = 0;
        let mut expect = days_since_epoch_today();
        for d in self.days.iter().rev() {
            let Some(day) = parse_day(&d.date) else { break };
            if day == expect && d.focus_secs >= goal {
                n += 1;
                expect -= 1;
            } else if day == expect && n == 0 && day == days_since_epoch_today() {
                // Today not finished yet: streak may still count from yesterday.
                expect -= 1;
            } else {
                break;
            }
        }
        n
    }
}

fn days_since_epoch_today() -> i64 {
    local_now_secs().div_euclid(86_400)
}

/// Local wall-clock seconds since the Unix epoch (UTC + local offset).
fn local_now_secs() -> i64 {
    let utc = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    utc + local_offset_secs()
}

fn local_offset_secs() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    // SAFETY: plain out-parameter call.
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = match id {
        2 => tz.Bias + tz.DaylightBias, // TIME_ZONE_ID_DAYLIGHT
        1 => tz.Bias + tz.StandardBias,
        _ => tz.Bias,
    };
    -(bias as i64) * 60
}

pub fn today_string() -> String {
    civil_from_days(days_since_epoch_today())
}

fn civil_from_days(z: i64) -> String {
    // Howard Hinnant's days -> civil date.
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn parse_day(s: &str) -> Option<i64> {
    let mut it = s.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("LockIn")
}

fn load<T: for<'de> Deserialize<'de> + Default>(name: &str) -> T {
    std::fs::read(data_dir().join(name))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save<T: Serialize>(name: &str, value: &T) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_vec_pretty(value) {
        // Write-then-rename so a crash never leaves a half-written file.
        let tmp = dir.join(format!("{name}.tmp"));
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(name));
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        load("settings.json")
    }
    pub fn save(&self) {
        save("settings.json", self);
    }
}

impl Stats {
    pub fn load() -> Self {
        load("stats.json")
    }
    pub fn save(&self) {
        save("stats.json", self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_round_trip() {
        for z in [-1000, 0, 19_000, 20_000, 20_757] {
            assert_eq!(parse_day(&civil_from_days(z)), Some(z));
        }
        assert_eq!(civil_from_days(0), "1970-01-01");
        assert_eq!(parse_day("2026-10-31"), Some(20_757));
    }

    #[test]
    fn goal_streak_counts_consecutive_goal_days_and_tolerates_unfinished_today() {
        let today = days_since_epoch_today();
        let day = |off: i64, mins: u64| Day { date: civil_from_days(today - off), focus_secs: mins * 60, ..Default::default() };
        let mut st = Stats { days: vec![day(3, 200), day(2, 130), day(1, 125), day(0, 10)], ..Default::default() };
        assert_eq!(st.goal_streak(120), 3);
        st.days[1].focus_secs = 0;
        assert_eq!(st.goal_streak(120), 1);
        st.days.pop();
        st.days.pop();
        assert_eq!(st.goal_streak(120), 0);
    }
}
