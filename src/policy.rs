//! Focus policy: turns cheap observations into warnings, alarms and escalation requests.
//!
//! Layers, cheapest first:
//! - L0 screen: foreground app/site, polled every second (no screenshots).
//! - L1 camera: face + iris/head features, one frame every few seconds; switches to fast sampling when off.
//! - L2 camera: object detector (person / phone), only on demand (face lost, long head-down).
//! A long "looking at screen" streak triggers a screen check against the site lists.

use crate::config::{Calibration, FeatureSample, Pose, Settings, GAZE_FEATURES};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gaze {
    Screen,
    Book,
    Away,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Absent,
    LookingAway,
    Phone,
    /// Blocked site/app opened (warning first, alarm on repeat or dwell).
    Site(String),
    /// Looked at the screen for long and the screen shows a blocked site.
    ScreenSite(String),
    BrbOverdue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Warn(Kind),
    Alarm(Kind),
    /// Unknown-purpose site (e.g. YouTube): ask "is this for studying?".
    Ask(String),
    PostureHint,
    /// Condition behind the active alarm cleared.
    Recovered,
    /// Camera cadence: fast while something looks off.
    CameraFast(bool),
    /// Run the L2 detector on the next frame.
    RunL2,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CamObs {
    pub face: Option<Pose>,
    /// Per-user eye/head classification from L1, when eye landmarks were usable.
    pub gaze: Option<(Gaze, f32)>,
    /// L2 results, present only when L2 ran on this frame.
    pub person: Option<bool>,
    pub phone: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Foreground {
    pub process: String,
    pub title: String,
    pub url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteClass {
    Blocked,
    Ask,
    Allowed,
}

const BROWSERS: [&str; 9] =
    ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "opera_gx.exe", "vivaldi.exe", "arc.exe", "zen.exe"];
/// Tab-title fingerprints for popular sites (used when the URL is unavailable).
const TITLE_HINTS: [(&str, &[&str]); 8] = [
    ("x.com", &[" / X", " on X: ", "X. It’s what’s happening"]),
    ("twitter.com", &[" / Twitter", "Twitter"]),
    ("instagram.com", &["Instagram"]),
    ("youtube.com", &["YouTube"]),
    ("reddit.com", &["Reddit", "reddit"]),
    ("tiktok.com", &["TikTok"]),
    ("facebook.com", &["Facebook"]),
    ("netflix.com", &["Netflix"]),
];

pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?;
    let host = host.split(':').next()?.trim().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    (!host.is_empty() && host.contains('.')).then_some(host)
}

fn entry_matches(entry: &str, fg: &Foreground, host: Option<&str>) -> bool {
    let e = entry.trim().to_ascii_lowercase();
    if e.is_empty() {
        return false;
    }
    if e.ends_with(".exe") {
        return fg.process.eq_ignore_ascii_case(&e);
    }
    if let Some(h) = host {
        return h == e || h.ends_with(&format!(".{e}"));
    }
    if !BROWSERS.iter().any(|b| fg.process.eq_ignore_ascii_case(b)) {
        return false;
    }
    let title = fg.title.as_str();
    if let Some((_, hints)) = TITLE_HINTS.iter().find(|(d, _)| *d == e) {
        if hints.iter().any(|h| title.contains(h)) {
            return true;
        }
    }
    title.to_ascii_lowercase().contains(&e)
}

/// Classify the foreground against the lists. Allowed wins, then Blocked, then Ask.
pub fn classify_site(fg: &Foreground, s: &Settings, extra_blocked: &HashSet<String>) -> Option<(String, SiteClass)> {
    let host = fg.url.as_deref().and_then(host_of);
    let host = host.as_deref();
    let find = |list: &mut dyn Iterator<Item = &String>| {
        for e in list {
            if entry_matches(e, fg, host) {
                return Some(e.clone());
            }
        }
        None
    };
    if let Some(e) = find(&mut s.allowed_sites.iter()) {
        return Some((e, SiteClass::Allowed));
    }
    if let Some(e) = find(&mut s.blocked_sites.iter().chain(extra_blocked.iter())) {
        return Some((e, SiteClass::Blocked));
    }
    find(&mut s.ask_sites.iter()).map(|e| (e, SiteClass::Ask))
}

/// Default poses when the user has not calibrated (frontal laptop webcam measured ~0.6-0.65 pitch).
const DEFAULT_SCREEN: Pose = Pose { yaw: 0.0, pitch: 0.62, area: 0.0, roll: 0.0, center: [0.5, 0.5] };
const AWAY_DIST: f32 = 0.28;
const BOOK_PITCH_DROP: f32 = 0.22;

pub fn classify_gaze(p: Pose, cal: &Calibration) -> Gaze {
    let screen = cal.screen.unwrap_or(DEFAULT_SCREEN);
    let book = cal.book.unwrap_or(Pose { pitch: screen.pitch + BOOK_PITCH_DROP, ..screen });
    // Yaw is noisier than pitch on a 5-point face; weight it down.
    let dist = |a: Pose| (((p.yaw - a.yaw) * 0.8).powi(2) + (p.pitch - a.pitch).powi(2)).sqrt();
    let (ds, db) = (dist(screen), dist(book));
    let turned = ((p.yaw - screen.yaw) * 0.8).abs() > AWAY_DIST;
    if !turned && p.pitch >= book.pitch {
        // Even further down than the book pose: still the desk.
        return Gaze::Book;
    }
    if ds.min(db) > AWAY_DIST {
        return Gaze::Away;
    }
    if ds <= db { Gaze::Screen } else { Gaze::Book }
}

/// Classify a complete eye/head feature vector with a per-user diagonal Gaussian model.
/// Returns `None` until all three guided targets have enough samples, preserving the legacy
/// pose-only behaviour for old and partially calibrated settings.
pub fn classify_gaze_features(sample: &FeatureSample, cal: &Calibration) -> Option<(Gaze, f32)> {
    const MIN_SAMPLES: usize = 5;
    const VAR_FLOOR: f32 = 0.16;
    const OUTLIER_D2: f32 = 16.0;
    // Avoid giving effectively infinite weight to a feature that did not happen to move during calibration.
    const SCALE_FLOOR: [f32; GAZE_FEATURES] =
        [5.0, 5.0, 5.0, 0.05, 0.05, 0.05, 0.05, 0.04, 0.04, 0.01, 0.03, 0.03, 0.05, 0.05, 5.0];
    // Eyes first, head light: head angles and face placement shift when you tip your head toward
    // the keyboard while still reading the screen, so they only nudge the decision. A large head
    // turn still stands out as an outlier and reads as Away.
    const WEIGHT: [f32; GAZE_FEATURES] =
        [0.35, 0.3, 0.2, 1.0, 1.0, 1.0, 1.0, 0.8, 0.8, 0.25, 0.25, 0.25, 1.0, 1.0, 1.5];
    let weight_sum: f32 = WEIGHT.iter().sum();
    let classes: [(&[FeatureSample], Gaze); 3] = [
        (&cal.screen_samples, Gaze::Screen),
        (&cal.book_samples, Gaze::Book),
        (&cal.away_samples, Gaze::Away),
    ];
    let stale = |s: &[FeatureSample]| s.len() < MIN_SAMPLES || s.iter().any(|x| x.values.len() != GAZE_FEATURES);
    if sample.values.len() != GAZE_FEATURES || classes.iter().any(|(s, _)| stale(s)) {
        return None;
    }

    let total = classes.iter().map(|(s, _)| s.len()).sum::<usize>() as f32;
    let mut mean = [0.0f32; GAZE_FEATURES];
    for (samples, _) in &classes {
        for s in *samples {
            for (m, x) in mean.iter_mut().zip(&s.values) {
                *m += x / total;
            }
        }
    }
    let mut scale = [0.0f32; GAZE_FEATURES];
    for (samples, _) in &classes {
        for s in *samples {
            for i in 0..GAZE_FEATURES {
                scale[i] += (s.values[i] - mean[i]).powi(2) / total;
            }
        }
    }
    for (x, floor) in scale.iter_mut().zip(SCALE_FLOOR) {
        *x = x.sqrt().max(floor);
    }

    let z: [f32; GAZE_FEATURES] = std::array::from_fn(|i| (sample.values[i] - mean[i]) / scale[i]);
    let mut scored = [(Gaze::Away, f32::INFINITY, f32::INFINITY); 3];
    for (ci, (samples, gaze)) in classes.iter().enumerate() {
        let n = samples.len() as f32;
        let mut cm = [0.0f32; GAZE_FEATURES];
        for s in *samples {
            for i in 0..GAZE_FEATURES {
                cm[i] += ((s.values[i] - mean[i]) / scale[i]) / n;
            }
        }
        let mut cv = [0.0f32; GAZE_FEATURES];
        for s in *samples {
            for i in 0..GAZE_FEATURES {
                let x = (s.values[i] - mean[i]) / scale[i];
                cv[i] += (x - cm[i]).powi(2) / n;
            }
        }
        let mut d2 = 0.0;
        let mut nll = 0.0;
        for i in 0..GAZE_FEATURES {
            let v = cv[i].max(VAR_FLOOR);
            let q = (z[i] - cm[i]).powi(2) / v;
            d2 += WEIGHT[i] * q / weight_sum;
            nll += WEIGHT[i] * (q + v.ln()) / weight_sum;
        }
        scored[ci] = (*gaze, d2, nll);
    }
    scored.sort_by(|a, b| a.2.total_cmp(&b.2));
    if scored[0].1 > OUTLIER_D2 {
        let confidence = (0.5 + (1.0 - (-(scored[0].1 - OUTLIER_D2) * 0.08).exp()) * 0.5).clamp(0.0, 1.0);
        return Some((Gaze::Away, confidence));
    }
    let best = scored[0].2;
    let denom = scored.iter().map(|x| (-0.5 * (x.2 - best)).exp()).sum::<f32>();
    Some((scored[0].0, (1.0 / denom).clamp(0.0, 1.0)))
}

const FACE_LOST_L2_SECS: f64 = 3.0;
const PERSON_VALID_SECS: f64 = 20.0;
const POSTURE_SECS: f64 = 30.0;
const POSTURE_COOLDOWN: f64 = 600.0;
const REARM_SECS: f64 = 10.0;

#[derive(Default)]
pub struct Policy {
    pub gaze: Option<Gaze>,
    gaze_since: f64,
    screen_since: Option<f64>,
    lean_since: Option<f64>,
    last_posture: Option<f64>,
    face_lost_since: Option<f64>,
    person_until: f64,
    last_l2: f64,
    last_phone_check: f64,
    phone_hits: u32,
    fast: bool,
    pub strikes: HashMap<String, u32>,
    strikes_date: String,
    site: Option<(String, SiteClass, f64)>,
    site_alarmed: bool,
    pub session_allowed: HashSet<String>,
    pub session_blocked: HashSet<String>,
    pending_ask: Option<String>,
    pub active: Option<Kind>,
    rearm_at: f64,
}

impl Policy {
    /// Monitoring (re)starts: forget timers, keep strikes and per-session choices.
    pub fn restart(&mut self, now: f64) {
        let keep = (
            std::mem::take(&mut self.strikes),
            std::mem::take(&mut self.strikes_date),
            std::mem::take(&mut self.session_allowed),
            std::mem::take(&mut self.session_blocked),
        );
        *self = Policy::default();
        (self.strikes, self.strikes_date, self.session_allowed, self.session_blocked) = keep;
        self.gaze_since = now;
        self.last_phone_check = now;
    }

    /// User pressed "I'm back" (or answered the card).
    pub fn dismiss(&mut self, now: f64) {
        self.active = None;
        self.rearm_at = now + REARM_SECS;
        self.gaze_since = now;
        self.screen_since = self.screen_since.map(|_| now);
        self.face_lost_since = self.face_lost_since.map(|_| now);
        if let Some(site) = self.site.as_mut() {
            site.2 = now;
        }
        // Still on the blocked site after "I'm back"? The dwell timer restarts, so scrolling on alarms again.
        self.site_alarmed = false;
    }

    pub fn answer_ask(&mut self, domain: &str, study: bool) {
        if study {
            self.session_allowed.insert(domain.to_string());
        } else {
            self.session_blocked.insert(domain.to_string());
            // Re-evaluate as a blocked visit on the next screen poll.
            self.site = None;
        }
        self.pending_ask = None;
    }

    fn alarm(&mut self, kind: Kind, now: f64, out: &mut Vec<Action>) {
        if self.active.is_some() || now < self.rearm_at {
            return;
        }
        self.active = Some(kind.clone());
        out.push(Action::Alarm(kind));
    }

    fn set_fast(&mut self, fast: bool, out: &mut Vec<Action>) {
        if self.fast != fast {
            self.fast = fast;
            out.push(Action::CameraFast(fast));
        }
    }

    fn recover_if(&mut self, pred: impl Fn(&Kind) -> bool, out: &mut Vec<Action>) {
        if self.active.as_ref().is_some_and(pred) {
            self.active = None;
            out.push(Action::Recovered);
        }
    }

    pub fn on_camera(&mut self, obs: CamObs, now: f64, s: &Settings) -> Vec<Action> {
        let mut out = Vec::new();

        // L2 results.
        if obs.person == Some(true) {
            self.person_until = now + PERSON_VALID_SECS;
        }
        match obs.phone {
            Some(true) => self.phone_hits += 1,
            Some(false) => {
                self.phone_hits = 0;
                self.recover_if(|k| *k == Kind::Phone, &mut out);
            }
            None => {}
        }
        if s.phone_detection && self.phone_hits >= 2 {
            self.alarm(Kind::Phone, now, &mut out);
        }

        // L1: gaze from the face, or person-but-no-face = head down at the desk.
        let gaze = match obs.face {
            Some(p) => {
                self.face_lost_since = None;
                obs.gaze.map(|g| g.0).unwrap_or_else(|| classify_gaze(p, &s.calibration))
            }
            None => {
                let since = *self.face_lost_since.get_or_insert(now);
                if now < self.person_until {
                    Gaze::Book
                } else {
                    if now - since >= FACE_LOST_L2_SECS && now - self.last_l2 >= FACE_LOST_L2_SECS {
                        self.last_l2 = now;
                        out.push(Action::RunL2);
                    }
                    Gaze::Absent
                }
            }
        };
        if self.gaze != Some(gaze) {
            self.gaze = Some(gaze);
            self.gaze_since = now;
        }
        let held = now - self.gaze_since;
        self.set_fast(matches!(gaze, Gaze::Away | Gaze::Absent) || self.phone_hits > 0, &mut out);

        match gaze {
            Gaze::Absent if held >= s.absent_secs as f64 => self.alarm(Kind::Absent, now, &mut out),
            Gaze::Away if held >= s.away_secs as f64 => self.alarm(Kind::LookingAway, now, &mut out),
            Gaze::Screen | Gaze::Book => {
                self.recover_if(|k| matches!(k, Kind::Absent | Kind::LookingAway), &mut out);
            }
            _ => {}
        }

        // Screen streak -> screen check.
        if gaze == Gaze::Screen {
            let since = *self.screen_since.get_or_insert(now);
            if now - since >= s.screen_secs as f64 {
                match self.site.clone() {
                    Some((d, SiteClass::Blocked, _)) => self.alarm(Kind::ScreenSite(d), now, &mut out),
                    Some((d, SiteClass::Ask, _)) if !self.session_allowed.contains(&d) && self.pending_ask.is_none() => {
                        self.pending_ask = Some(d.clone());
                        out.push(Action::Ask(d));
                    }
                    _ => {}
                }
                self.screen_since = Some(now);
            }
            // Posture: leaning in towards the screen.
            if let (true, Some(cal), Some(p)) = (s.posture_hints, s.calibration.screen, obs.face) {
                if cal.area > 0.0 && p.area >= cal.area * s.lean_ratio {
                    let since = *self.lean_since.get_or_insert(now);
                    let cooled = self.last_posture.map_or(true, |t| now - t >= POSTURE_COOLDOWN);
                    if now - since >= POSTURE_SECS && cooled {
                        self.last_posture = Some(now);
                        out.push(Action::PostureHint);
                    }
                } else {
                    self.lean_since = None;
                }
            }
        } else {
            self.screen_since = None;
            self.lean_since = None;
        }

        // Long head-down: a phone in the lap looks like reading. Check with L2 now and then.
        if s.phone_detection && gaze == Gaze::Book {
            let every = if self.phone_hits > 0 { 5.0 } else { s.phone_check_secs as f64 };
            if now - self.last_phone_check >= every && now - self.last_l2 >= 2.0 {
                self.last_phone_check = now;
                self.last_l2 = now;
                out.push(Action::RunL2);
            }
        }
        out
    }

    pub fn on_screen(&mut self, fg: Option<&Foreground>, now: f64, s: &Settings) -> Vec<Action> {
        let mut out = Vec::new();
        let today = crate::config::today_string();
        if self.strikes_date != today {
            self.strikes.clear();
            self.strikes_date = today;
        }
        let hit = fg.and_then(|fg| classify_site(fg, s, &self.session_blocked));
        let same = matches!((&hit, &self.site), (Some((d, c)), Some((sd, sc, _))) if d == sd && c == sc);
        if !same {
            // Leaving a site clears its alarm.
            if self.active.as_ref().is_some_and(|k| matches!(k, Kind::Site(_) | Kind::ScreenSite(_))) {
                self.recover_if(|_| true, &mut out);
            }
            self.site_alarmed = false;
            self.site = hit.as_ref().map(|(d, c)| (d.clone(), *c, now));
            match &hit {
                Some((d, SiteClass::Blocked)) if !self.session_allowed.contains(d) => {
                    let n = self.strikes.entry(d.clone()).or_default();
                    *n += 1;
                    if *n >= s.strikes_before_alarm.max(1) {
                        self.site_alarmed = true;
                        self.alarm(Kind::Site(d.clone()), now, &mut out);
                    } else {
                        out.push(Action::Warn(Kind::Site(d.clone())));
                    }
                }
                Some((d, SiteClass::Ask)) if !self.session_allowed.contains(d) && self.pending_ask.is_none() => {
                    self.pending_ask = Some(d.clone());
                    out.push(Action::Ask(d.clone()));
                }
                _ => {}
            }
        } else if let Some((d, SiteClass::Blocked, since)) = self.site.clone() {
            // Ignored the warning and stayed.
            if !self.site_alarmed && !self.session_allowed.contains(&d) && now - since >= s.blocked_dwell_secs as f64 {
                self.site_alarmed = true;
                self.alarm(Kind::Site(d), now, &mut out);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg(process: &str, title: &str, url: Option<&str>) -> Foreground {
        Foreground { process: process.into(), title: title.into(), url: url.map(Into::into) }
    }
    fn face(pitch: f32) -> CamObs {
        CamObs { face: Some(Pose { yaw: 0.0, pitch, area: 0.05, ..Default::default() }), ..Default::default() }
    }
    fn alarms(a: &[Action]) -> Vec<&Action> {
        a.iter().filter(|x| matches!(x, Action::Alarm(_))).collect()
    }

    #[test]
    fn site_matching_by_url_title_and_process() {
        let s = Settings::default();
        let none = HashSet::new();
        let c = |f: Foreground| classify_site(&f, &s, &none);
        assert_eq!(c(fg("chrome.exe", "", Some("https://www.instagram.com/reel/1"))), Some(("instagram.com".into(), SiteClass::Blocked)));
        assert_eq!(c(fg("msedge.exe", "", Some("https://m.youtube.com/watch"))), Some(("youtube.com".into(), SiteClass::Ask)));
        assert_eq!(c(fg("chrome.exe", "Home / X", None)), Some(("x.com".into(), SiteClass::Blocked)));
        assert_eq!(c(fg("Code.exe", "instagram-clone - Visual Studio Code", None)), None, "titles only count in browsers");
        assert_eq!(c(fg("chrome.exe", "", Some("https://notx.com.evil.io/"))), None);
        let s2 = Settings { blocked_sites: vec!["steam.exe".into()], allowed_sites: vec!["youtube.com".into()], ..Default::default() };
        assert_eq!(classify_site(&fg("Steam.exe", "Steam", None), &s2, &none).map(|x| x.1), Some(SiteClass::Blocked));
        assert_eq!(classify_site(&fg("chrome.exe", "", Some("https://youtube.com")), &s2, &none).map(|x| x.1), Some(SiteClass::Allowed));
    }

    #[test]
    fn first_blocked_visit_warns_second_alarms() {
        let s = Settings::default();
        let mut p = Policy::default();
        let x = fg("chrome.exe", "", Some("https://x.com/home"));
        let notes = fg("chrome.exe", "", Some("https://notion.so"));
        assert_eq!(p.on_screen(Some(&x), 0.0, &s), vec![Action::Warn(Kind::Site("x.com".into()))]);
        assert!(p.on_screen(Some(&x), 1.0, &s).is_empty());
        assert!(p.on_screen(Some(&notes), 2.0, &s).is_empty());
        assert_eq!(p.on_screen(Some(&x), 3.0, &s), vec![Action::Alarm(Kind::Site("x.com".into()))]);
        assert_eq!(p.on_screen(Some(&notes), 4.0, &s), vec![Action::Recovered]);
    }

    #[test]
    fn staying_on_blocked_site_after_warning_alarms() {
        let s = Settings { blocked_dwell_secs: 20, ..Default::default() };
        let mut p = Policy::default();
        let insta = fg("chrome.exe", "Instagram", None);
        p.on_screen(Some(&insta), 0.0, &s);
        assert!(alarms(&p.on_screen(Some(&insta), 19.0, &s)).is_empty());
        assert_eq!(alarms(&p.on_screen(Some(&insta), 20.0, &s)).len(), 1);
        // "I'm back" but still scrolling: the dwell restarts and alarms again.
        p.dismiss(21.0);
        assert!(alarms(&p.on_screen(Some(&insta), 40.0, &s)).is_empty());
        assert_eq!(alarms(&p.on_screen(Some(&insta), 41.0, &s)).len(), 1);
    }

    #[test]
    fn ask_site_asks_once_and_answer_is_remembered() {
        let s = Settings::default();
        let mut p = Policy::default();
        let yt = fg("chrome.exe", "Lecture 4 - YouTube", None);
        assert_eq!(p.on_screen(Some(&yt), 0.0, &s), vec![Action::Ask("youtube.com".into())]);
        p.answer_ask("youtube.com", true);
        p.on_screen(None, 1.0, &s);
        assert!(p.on_screen(Some(&yt), 2.0, &s).is_empty());
    }

    #[test]
    fn ask_answered_distraction_becomes_blocked() {
        let s = Settings::default();
        let mut p = Policy::default();
        let yt = fg("chrome.exe", "Shorts - YouTube", None);
        p.on_screen(Some(&yt), 0.0, &s);
        p.answer_ask("youtube.com", false);
        assert_eq!(p.on_screen(Some(&yt), 1.0, &s), vec![Action::Warn(Kind::Site("youtube.com".into()))]);
    }

    #[test]
    fn absence_escalates_to_l2_and_alarms_only_without_a_person() {
        let s = Settings { absent_secs: 30, ..Default::default() };
        let mut p = Policy::default();
        p.restart(0.0);
        p.on_camera(face(0.55), 0.0, &s);
        let a = p.on_camera(CamObs::default(), 1.0, &s);
        assert!(a.contains(&Action::CameraFast(true)));
        assert!(p.on_camera(CamObs::default(), 4.0, &s).contains(&Action::RunL2));
        // L2 sees a person with the head down: counts as the desk, no alarm.
        p.on_camera(CamObs { person: Some(true), phone: Some(false), ..Default::default() }, 5.0, &s);
        assert_eq!(p.gaze, Some(Gaze::Book));
        assert!(alarms(&p.on_camera(CamObs::default(), 20.0, &s)).is_empty());
        // Person gone for good.
        p.on_camera(CamObs { person: Some(false), phone: Some(false), ..Default::default() }, 26.0, &s);
        assert!(alarms(&p.on_camera(CamObs::default(), 50.0, &s)).is_empty());
        assert_eq!(alarms(&p.on_camera(CamObs::default(), 56.0, &s)), vec![&Action::Alarm(Kind::Absent)]);
        let back = p.on_camera(face(0.55), 57.0, &s);
        assert!(back.contains(&Action::Recovered) && back.contains(&Action::CameraFast(false)));
    }

    #[test]
    fn long_screen_streak_checks_the_screen() {
        let s = Settings { screen_secs: 60, ..Default::default() };
        let mut p = Policy::default();
        p.restart(0.0);
        p.on_screen(Some(&fg("chrome.exe", "", Some("https://x.com"))), 0.0, &s);
        p.dismiss(0.0); // ignore the site warning flow here
        p.on_camera(face(0.55), 0.0, &s);
        assert!(alarms(&p.on_camera(face(0.55), 59.0, &s)).is_empty());
        assert_eq!(alarms(&p.on_camera(face(0.55), 61.0, &s)), vec![&Action::Alarm(Kind::ScreenSite("x.com".into()))]);
    }

    #[test]
    fn phone_needs_two_hits() {
        let s = Settings::default();
        let mut p = Policy::default();
        p.restart(0.0);
        let phone = |v| CamObs {
            face: Some(Pose { yaw: 0.0, pitch: 0.8, area: 0.05, ..Default::default() }),
            person: Some(true),
            phone: Some(v),
            ..Default::default()
        };
        assert!(alarms(&p.on_camera(phone(true), 1.0, &s)).is_empty());
        assert_eq!(alarms(&p.on_camera(phone(true), 6.0, &s)), vec![&Action::Alarm(Kind::Phone)]);
        assert!(p.on_camera(phone(false), 11.0, &s).contains(&Action::Recovered));
    }

    #[test]
    fn gaze_classification_uses_calibration() {
        let cal = Calibration {
            screen: Some(Pose { yaw: 0.05, pitch: 0.5, area: 0.06, ..Default::default() }),
            book: Some(Pose { yaw: 0.0, pitch: 0.85, area: 0.05, ..Default::default() }),
            ..Default::default()
        };
        let g = |yaw, pitch| classify_gaze(Pose { yaw, pitch, area: 0.05, ..Default::default() }, &cal);
        assert_eq!(g(0.05, 0.52), Gaze::Screen);
        assert_eq!(g(0.0, 0.8), Gaze::Book);
        assert_eq!(g(0.0, 1.1), Gaze::Book, "further down than the book is still the desk");
        assert_eq!(g(0.6, 0.5), Gaze::Away);
        assert_eq!(g(0.0, 0.1), Gaze::Away, "looking up");
    }

    fn cluster(base: [f32; GAZE_FEATURES]) -> Vec<FeatureSample> {
        (0..8)
            .map(|n| {
                let offset = (n as f32 - 3.5) * 0.002;
                FeatureSample { values: (0..GAZE_FEATURES).map(|i| base[i] + offset * (1.0 + i as f32 * 0.05)).collect() }
            })
            .collect()
    }

    fn trained_calibration() -> Calibration {
        let screen = [0.0, 0.0, 0.0, 0.50, 0.35, 0.50, 0.35, 0.28, 0.28, 0.08, 0.50, 0.48, 0.0, 0.05, -7.0];
        let book = [1.0, 14.0, 0.5, 0.51, 0.73, 0.49, 0.72, 0.24, 0.24, 0.07, 0.50, 0.56, 0.0, -0.45, 25.0];
        let away = [25.0, -4.0, 5.0, 0.78, 0.52, 0.76, 0.51, 0.25, 0.25, 0.06, 0.62, 0.48, 0.5, 0.0, 0.0];
        Calibration {
            screen_samples: cluster(screen),
            book_samples: cluster(book),
            away_samples: cluster(away),
            ..Default::default()
        }
    }

    #[test]
    fn feature_classifier_separates_calibrated_targets() {
        let cal = trained_calibration();
        for (samples, expected) in [
            (&cal.screen_samples, Gaze::Screen),
            (&cal.book_samples, Gaze::Book),
            (&cal.away_samples, Gaze::Away),
        ] {
            let (actual, confidence) = classify_gaze_features(&samples[3], &cal).unwrap();
            assert_eq!(actual, expected);
            assert!(confidence > 0.5, "confidence {confidence}");
        }
    }

    #[test]
    fn head_tipped_down_with_eyes_on_screen_stays_screen() {
        let cal = trained_calibration();
        // Head pitched and placed like the book pose, but the eyes rolled up to the screen, so the
        // combined gaze pitch matches the screen pose.
        let sample = FeatureSample {
            values: vec![1.0, 14.0, 0.5, 0.50, 0.10, 0.50, 0.10, 0.30, 0.30, 0.07, 0.50, 0.56, 0.0, 0.30, -5.5],
        };
        let (gaze, _) = classify_gaze_features(&sample, &cal).unwrap();
        assert_eq!(gaze, Gaze::Screen);
    }

    #[test]
    fn feature_classifier_falls_back_to_away_for_outliers() {
        let cal = trained_calibration();
        let sample = FeatureSample { values: vec![200.0; GAZE_FEATURES] };
        let (gaze, confidence) = classify_gaze_features(&sample, &cal).unwrap();
        assert_eq!(gaze, Gaze::Away);
        assert!(confidence >= 0.5);
    }

    #[test]
    fn uncalibrated_feature_classifier_keeps_pose_default() {
        let cal = Calibration::default();
        assert_eq!(classify_gaze_features(&FeatureSample::default(), &cal), None);
        assert_eq!(
            classify_gaze(Pose { yaw: 0.0, pitch: 0.62, area: 0.05, ..Default::default() }, &cal),
            Gaze::Screen
        );
    }

    #[test]
    fn old_calibration_json_still_deserializes() {
        let old = r#"{"screen":{"yaw":0.1,"pitch":0.6,"area":0.08},"book":null}"#;
        let cal: Calibration = serde_json::from_str(old).unwrap();
        assert_eq!(cal.screen.unwrap().roll, 0.0);
        assert!(cal.screen_samples.is_empty() && cal.book_samples.is_empty() && cal.away_samples.is_empty());
    }

    #[test]
    fn samples_from_an_older_feature_set_are_ignored() {
        let mut cal = trained_calibration();
        cal.screen_samples.iter_mut().for_each(|s| s.values.truncate(12));
        assert_eq!(classify_gaze_features(&cal.book_samples[0], &cal), None);
    }
}
