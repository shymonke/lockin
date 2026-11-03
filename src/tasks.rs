//! Pure Pomodoro task tracking and its small, independent `tasks.json` store.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub estimate: u32,
    pub actual: u32,
    pub distractions: u32,
    pub created_at: u64,
    pub done_at: Option<u64>,
}

impl Default for Task {
    fn default() -> Self {
        Self { id: 0, title: String::new(), estimate: 1, actual: 0, distractions: 0, created_at: 0, done_at: None }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Idea {
    pub id: u64,
    pub title: String,
    pub created_at: u64,
    pub done: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DayHistory {
    pub date: String,
    pub completed_focus: u32,
    pub earned_secs: u64,
    pub tasks_completed: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TaskStore {
    pub tasks: Vec<Task>,
    pub ideas: Vec<Idea>,
    pub history: Vec<DayHistory>,
    pub active_task: Option<u64>,
    next_id: u64,
    /// `tasks.json` existed but could not be read, and no copy of it could be kept: saving would
    /// overwrite the only copy, so saves are skipped this run.
    #[serde(skip)]
    read_only: bool,
    /// Set when loading had to fall back to an empty store; the app shows it once.
    #[serde(skip)]
    pub load_note: Option<String>,
}

impl Default for TaskStore {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            ideas: Vec::new(),
            history: Vec::new(),
            active_task: None,
            next_id: 1,
            read_only: false,
            load_note: None,
        }
    }
}

impl TaskStore {
    pub fn load() -> Self {
        Self::load_from(&crate::config::data_dir().join("tasks.json"))
    }

    pub fn load_from(path: &Path) -> Self {
        let mut store: Self = match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(store) => store,
                Err(_) => {
                    // Corrupt, or written by a build whose format this one cannot read: keep the
                    // bytes under a new name before the empty store is saved over the file.
                    match keep_unreadable(path, &bytes) {
                        Ok(backup) => Self {
                            load_note: Some(format!(
                                "tasks.json could not be read; it was kept as {} and LockIn started a new list.",
                                backup.file_name().and_then(|n| n.to_str()).unwrap_or("a backup")
                            )),
                            ..Self::default()
                        },
                        Err(_) => Self::unsaved("tasks.json could not be read or backed up; changes this session won't be saved."),
                    }
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            // Locked or unreadable right now (antivirus, sync tool): the file may be fine, so never
            // replace it with an empty store.
            Err(_) => Self::unsaved("tasks.json could not be opened; changes this session won't be saved. Restart LockIn."),
        };
        let max_id = store.tasks.iter().map(|t| t.id).chain(store.ideas.iter().map(|i| i.id)).max().unwrap_or(0);
        store.next_id = store.next_id.max(max_id.saturating_add(1)).max(1);
        store.normalize_active();
        store.history.sort_by(|a, b| a.date.cmp(&b.date));
        store
    }

    fn unsaved(note: &str) -> Self {
        Self { read_only: true, load_note: Some(note.into()), ..Self::default() }
    }

    pub fn save(&self) {
        if self.read_only {
            return;
        }
        let _ = self.save_to(&crate::config::data_dir().join("tasks.json"));
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let Some(dir) = path.parent() else { return Ok(()) };
        std::fs::create_dir_all(dir)?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tasks.json");
        let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, bytes)?;
        atomic_replace(&tmp, path)
    }

    pub fn active(&self) -> Option<&Task> {
        let id = self.active_task?;
        self.tasks.iter().find(|t| t.id == id && t.done_at.is_none())
    }

    pub fn normalize_active(&mut self) {
        let valid = self.active_task.is_some_and(|id| self.tasks.iter().any(|t| t.id == id && t.done_at.is_none()));
        if !valid {
            self.active_task = self.tasks.iter().find(|t| t.done_at.is_none()).map(|t| t.id);
        }
    }

    pub fn add_task(&mut self, title: &str, estimate: u32, now: u64) -> Option<u64> {
        let title = title.trim();
        if title.is_empty() {
            return None;
        }
        let id = self.take_id();
        self.tasks.push(Task { id, title: title.into(), estimate: estimate.max(1), created_at: now, ..Default::default() });
        self.normalize_active();
        Some(id)
    }

    pub fn add_idea(&mut self, title: &str, now: u64) -> Option<u64> {
        let title = title.trim();
        if title.is_empty() {
            return None;
        }
        let id = self.take_id();
        self.ideas.push(Idea { id, title: title.into(), created_at: now, done: false });
        Some(id)
    }

    pub fn set_active(&mut self, id: u64) -> bool {
        if self.tasks.iter().any(|t| t.id == id && t.done_at.is_none()) {
            self.active_task = Some(id);
            true
        } else {
            false
        }
    }

    pub fn rename_task(&mut self, id: u64, title: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else { return false };
        task.title = title.into();
        true
    }

    pub fn set_estimate(&mut self, id: u64, estimate: u32) -> bool {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else { return false };
        task.estimate = estimate.clamp(1, 99);
        true
    }

    pub fn move_task(&mut self, id: u64, delta: i32) -> bool {
        let Some(from) = self.tasks.iter().position(|t| t.id == id) else { return false };
        let to = if delta < 0 {
            self.tasks[..from].iter().rposition(|t| t.done_at.is_none())
        } else if delta > 0 {
            self.tasks[from + 1..].iter().position(|t| t.done_at.is_none()).map(|i| from + 1 + i)
        } else {
            None
        };
        let Some(to) = to else { return false };
        self.tasks.swap(from, to);
        true
    }

    pub fn delete_task(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        if self.tasks.len() == before {
            return false;
        }
        self.normalize_active();
        true
    }

    pub fn complete_task(&mut self, id: u64, date: &str, now: u64) -> bool {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id && t.done_at.is_none()) else { return false };
        task.done_at = Some(now);
        let day = self.day_mut(date);
        day.tasks_completed = day.tasks_completed.saturating_add(1);
        self.normalize_active();
        true
    }

    /// Account a finished focus block and return the task to ask about.
    pub fn focus_completed(&mut self, date: &str) -> Option<(u64, String)> {
        let day = self.day_mut(date);
        day.completed_focus = day.completed_focus.saturating_add(1);
        let id = self.active_task?;
        let task = self.tasks.iter_mut().find(|t| t.id == id && t.done_at.is_none())?;
        task.actual = task.actual.saturating_add(1);
        Some((task.id, task.title.clone()))
    }

    pub fn add_earned_secs(&mut self, date: &str, seconds: u64) {
        if seconds == 0 {
            return;
        }
        let day = self.day_mut(date);
        day.earned_secs = day.earned_secs.saturating_add(seconds);
    }

    pub fn add_distraction(&mut self) {
        if let Some(id) = self.active_task {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id && t.done_at.is_none()) {
                task.distractions = task.distractions.saturating_add(1);
            }
        }
    }

    pub fn toggle_idea(&mut self, id: u64) -> bool {
        let Some(idea) = self.ideas.iter_mut().find(|i| i.id == id) else { return false };
        idea.done = !idea.done;
        true
    }

    pub fn delete_idea(&mut self, id: u64) -> bool {
        let before = self.ideas.len();
        self.ideas.retain(|i| i.id != id);
        self.ideas.len() != before
    }

    pub fn current_streak(&self, today: &str) -> u32 {
        let Some(mut expect) = parse_day(today) else { return 0 };
        if self.day(today).map_or(true, |d| d.completed_focus == 0) {
            expect -= 1;
        }
        let mut count = 0;
        loop {
            let active = self.history.iter().any(|d| parse_day(&d.date) == Some(expect) && d.completed_focus > 0);
            if !active {
                break;
            }
            count += 1;
            expect -= 1;
        }
        count
    }

    pub fn day(&self, date: &str) -> Option<&DayHistory> {
        self.history.iter().find(|d| d.date == date)
    }

    fn day_mut(&mut self, date: &str) -> &mut DayHistory {
        if let Some(i) = self.history.iter().position(|d| d.date == date) {
            return &mut self.history[i];
        }
        let i = self.history.partition_point(|d| d.date.as_str() < date);
        self.history.insert(i, DayHistory { date: date.into(), ..Default::default() });
        &mut self.history[i]
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        id
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Copy an unreadable store to `<name>.unreadable-<secs>` beside it, never over an earlier copy:
/// a name already taken (same second, clock moved back) gets `-1`, `-2`… appended.
fn keep_unreadable(path: &Path, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tasks.json");
    let stem = format!("{name}.unreadable-{}", now_secs());
    for n in 0..100u32 {
        let backup = path.with_file_name(if n == 0 { stem.clone() } else { format!("{stem}-{n}") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&backup) {
            Ok(mut f) => {
                f.write_all(bytes)?;
                f.sync_all()?;
                return Ok(backup);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "no free backup name"))
}

fn parse_day(s: &str) -> Option<i64> {
    let mut it = s.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || it.next().is_some() {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn atomic_replace(tmp: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};
        let from: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe { MoveFileExW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr()), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) }
            .map_err(std::io::Error::other)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(tmp, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_selection_and_early_completion_choose_next() {
        let mut s = TaskStore::default();
        let a = s.add_task("First", 1, 10).unwrap();
        let b = s.add_task("Second", 2, 11).unwrap();
        assert_eq!(s.active_task, Some(a));
        assert!(s.complete_task(a, "2026-10-31", 20));
        assert_eq!(s.active_task, Some(b));
        assert_eq!(s.day("2026-10-31").unwrap().tasks_completed, 1);
    }

    #[test]
    fn focus_end_accounts_to_active_task_and_day() {
        let mut s = TaskStore::default();
        let id = s.add_task("Read", 1, 1).unwrap();
        assert_eq!(s.focus_completed("2026-10-31"), Some((id, "Read".into())));
        assert_eq!(s.tasks[0].actual, 1);
        assert_eq!(s.day("2026-10-31").unwrap().completed_focus, 1);
    }

    #[test]
    fn unfinished_tasks_carry_across_days() {
        let mut s = TaskStore::default();
        let id = s.add_task("Carry me", 3, 1).unwrap();
        s.focus_completed("2026-10-31");
        s.add_earned_secs("2026-10-31", 1500);
        s.add_earned_secs("2026-11-01", 60);
        assert_eq!(s.active_task, Some(id));
        assert!(s.tasks[0].done_at.is_none());
        assert_eq!(s.history.len(), 2);
        assert_eq!(s.day("2026-11-01").unwrap().earned_secs, 60);
    }

    #[test]
    fn history_buckets_and_streak_are_calendar_based() {
        let mut s = TaskStore::default();
        s.focus_completed("2026-10-29");
        s.focus_completed("2026-10-30");
        s.focus_completed("2026-10-31");
        assert_eq!(s.current_streak("2026-11-01"), 3, "empty current day preserves yesterday's streak");
        s.focus_completed("2026-11-01");
        assert_eq!(s.current_streak("2026-11-01"), 4);
        let mut gap = TaskStore::default();
        gap.focus_completed("2026-10-30");
        gap.focus_completed("2026-11-01");
        assert_eq!(gap.current_streak("2026-11-01"), 1);
    }

    #[test]
    fn corrupt_files_are_kept_without_overwriting_earlier_copies() {
        let dir = std::env::temp_dir().join(format!("lockin-task-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.json");
        // Two bad loads in the same second must not share a backup name.
        for bad in [&b"{ first bad"[..], &b"{ second bad"[..]] {
            std::fs::write(&path, bad).unwrap();
            let s = TaskStore::load_from(&path);
            assert!(s.tasks.is_empty());
            assert!(!s.read_only, "a kept copy makes it safe to save over the file");
            assert!(s.load_note.is_some());
        }
        let mut kept: Vec<Vec<u8>> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("tasks.json.unreadable-"))
            .map(|e| std::fs::read(e.path()).unwrap())
            .collect();
        kept.sort();
        assert_eq!(kept, vec![b"{ first bad".to_vec(), b"{ second bad".to_vec()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_file_starts_empty_and_saves() {
        let dir = std::env::temp_dir().join(format!("lockin-task-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tasks.json");
        let mut s = TaskStore::load_from(&path);
        assert!(!s.read_only);
        assert!(s.load_note.is_none());
        s.add_task("Chapter 3", 2, 10).unwrap();
        s.save_to(&path).unwrap();
        let back = TaskStore::load_from(&path);
        assert_eq!(back.tasks.len(), 1);
        assert_eq!(back.tasks[0].title, "Chapter 3");
        let _ = std::fs::remove_dir_all(dir);
    }
}
