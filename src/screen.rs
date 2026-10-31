//! L0 screen layer: foreground process + window title every second, plus the browser address bar
//! through UI Automation (found once per window, then only its value is read). Never captures pixels.

use crate::policy::Foreground;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindow};

const BROWSERS: [&str; 9] =
    ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "opera_gx.exe", "vivaldi.exe", "arc.exe", "zen.exe"];

pub enum Cmd {
    Enable(bool),
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/']).next().unwrap_or_default().to_string()
    }
}

struct Uia {
    auto: IUIAutomation,
    edit_cond: IUIAutomationCondition,
    /// Address bar element per browser window.
    cache: HashMap<isize, Option<IUIAutomationElement>>,
}

impl Uia {
    fn new() -> Option<Uia> {
        unsafe {
            let auto: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let edit_cond = auto.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_EditControlTypeId.0)).ok()?;
            Some(Uia { auto, edit_cond, cache: HashMap::new() })
        }
    }

    fn url(&mut self, hwnd: HWND) -> Option<String> {
        let key = hwnd.0 as isize;
        if !self.cache.contains_key(&key) {
            // Toolbar comes before page content in tree order, so the first Edit is the address bar.
            let found = unsafe {
                self.auto
                    .ElementFromHandle(hwnd)
                    .and_then(|root| root.FindFirst(TreeScope_Descendants, &self.edit_cond))
                    .ok()
            };
            if self.cache.len() > 64 {
                self.cache.clear();
            }
            self.cache.insert(key, found);
        }
        let el = self.cache.get(&key)?.as_ref()?;
        let value = unsafe {
            el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .and_then(|p| p.CurrentValue())
                .map(|b| b.to_string())
        };
        match value {
            Ok(v) if !v.trim().is_empty() => Some(v),
            Ok(_) => None,
            Err(_) => {
                // Stale element (tab torn off, window rebuilt): search again next time.
                self.cache.remove(&key);
                None
            }
        }
    }
}

/// Describe one top-level window: process, title and (for browsers) the address-bar URL.
fn describe(hwnd: HWND, uia: &mut Option<Uia>) -> Option<Foreground> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
        let title = String::from_utf16_lossy(&buf[..n]);
        let process = process_name(pid);
        let url = if BROWSERS.iter().any(|b| process.eq_ignore_ascii_case(b)) {
            uia.as_mut().and_then(|u| u.url(hwnd))
        } else {
            None
        };
        Some(Foreground { process, title, url })
    }
}

/// The window the user is working in. While LockIn's own window (alert card, settings) has focus,
/// keep watching the last external window live: focusing our alert must not read as "left the
/// site", but closing that window must.
struct Watcher {
    uia: Option<Uia>,
    last: Option<HWND>,
}

impl Watcher {
    fn poll(&mut self) -> Option<Foreground> {
        let hwnd = unsafe { GetForegroundWindow() };
        if !hwnd.0.is_null() {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != std::process::id() {
                self.last = Some(hwnd);
            }
        } else {
            self.last = None;
        }
        let fg = self.last.and_then(|h| describe(h, &mut self.uia));
        if fg.is_none() {
            self.last = None;
        }
        fg
    }
}

pub fn spawn(rx: Receiver<Cmd>, tx: Sender<Option<Foreground>>, wake: Box<dyn Fn() + Send>) {
    std::thread::Builder::new()
        .name("screen".into())
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let mut w = Watcher { uia: None, last: None };
            let mut enabled = false;
            loop {
                let msg = if enabled { rx.recv_timeout(Duration::from_secs(1)) } else { rx.recv().map_err(|_| RecvTimeoutError::Disconnected) };
                match msg {
                    Ok(Cmd::Enable(e)) => {
                        enabled = e;
                        if !e {
                            w.uia = None; // free the UIA client between focus blocks
                            continue;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                if w.uia.is_none() {
                    w.uia = Uia::new();
                }
                if tx.send(w.poll()).is_err() {
                    return;
                }
                wake();
            }
        })
        .expect("spawn screen thread");
}

/// `lockin --diag-screen`: print what the screen layer sees for 15 s.
pub fn diag() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut w = Watcher { uia: Uia::new(), last: None };
    for _ in 0..15 {
        let t = std::time::Instant::now();
        let fg = w.poll();
        println!("{:>5.1} ms  {:?}", t.elapsed().as_secs_f64() * 1e3, fg);
        std::thread::sleep(Duration::from_secs(1));
    }
}
