//! System-wide shortcuts, rebindable in Settings → Shortcuts. Keyboard combos use `RegisterHotKey`
//! (it also fires over elevated windows); mouse binds (wheel, middle and side buttons) use a
//! low-level mouse hook that is installed only while at least one mouse bind exists, so plain
//! mouse input never detours through LockIn otherwise. Both live on one message-loop thread.

use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, MapVirtualKeyW, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MAPVK_VK_TO_VSC,
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MSG, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_APP, WM_HOTKEY, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEWHEEL, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
    Calculator,
    StartPause,
    Brb,
    Settings,
    ClickThrough,
    Camera,
    /// Tray menu only; no shortcut.
    Autostart,
    Restart,
    Quit,
}

/// Bindable actions: settings key (also the dashboard's id) and description.
pub const ALL: [(Hotkey, &str, &str); 6] = [
    (Hotkey::StartPause, "start_pause", "Start or pause the timer"),
    (Hotkey::Brb, "brb", "Step away, or I'm back"),
    (Hotkey::Settings, "open", "Open LockIn"),
    (Hotkey::Camera, "camera", "Camera on or off"),
    (Hotkey::Calculator, "calculator", "Calculator"),
    (Hotkey::ClickThrough, "click_through", "Timer bar click-through on or off"),
];

/// Pseudo key codes for the wheel (no virtual-key code exists). Mouse buttons use their real
/// virtual-key codes: `VK_MBUTTON` (4), `VK_XBUTTON1` (5, "Mouse 4", usually Back) and
/// `VK_XBUTTON2` (6, "Mouse 5", usually Forward).
pub const WHEEL_UP: u32 = 0x1_0000;
pub const WHEEL_DOWN: u32 = 0x1_0001;
const VK_MBUTTON: u32 = 4;
const VK_XBUTTON1: u32 = 5;
const VK_XBUTTON2: u32 = 6;

/// One shortcut: modifiers that must be held exactly, plus a key or mouse input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Virtual-key code, a mouse button's virtual-key code, or `WHEEL_UP` / `WHEEL_DOWN`.
    pub key: u32,
}

impl Binding {
    const fn ctrl_alt(key: char) -> Option<Binding> {
        Some(Binding { ctrl: true, alt: true, shift: false, win: false, key: key as u32 })
    }

    fn is_mouse(&self) -> bool {
        matches!(self.key, VK_MBUTTON | VK_XBUTTON1 | VK_XBUTTON2 | WHEEL_UP | WHEEL_DOWN)
    }

    /// Left/right buttons and bare modifiers cannot be bound.
    fn valid(&self) -> bool {
        self.key != 0 && !matches!(self.key, 1 | 2 | 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
    }

    /// "Ctrl+Alt+O", "Mouse 4", "Ctrl+Wheel down".
    pub fn label(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+"), (self.win, "Win+")] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(&key_name(self.key));
        s
    }
}

fn key_name(key: u32) -> String {
    match key {
        WHEEL_UP => return "Wheel up".into(),
        WHEEL_DOWN => return "Wheel down".into(),
        VK_MBUTTON => return "Middle click".into(),
        VK_XBUTTON1 => return "Mouse 4".into(),
        VK_XBUTTON2 => return "Mouse 5".into(),
        0x30..=0x39 | 0x41..=0x5A => return char::from_u32(key).unwrap_or('?').to_string(),
        0x70..=0x87 => return format!("F{}", key - 0x6F),
        _ => {}
    }
    // SAFETY: plain conversions with a stack buffer.
    unsafe {
        let mut sc = MapVirtualKeyW(key, MAPVK_VK_TO_VSC) as i32;
        // Navigation keys share scan codes with the numpad; the extended bit picks the real ones.
        if matches!(key, 0x21..=0x2E | 0x5B..=0x5D | 0x6F | 0x90) {
            sc |= 0x100;
        }
        let mut buf = [0u16; 64];
        let n = GetKeyNameTextW(sc << 16, &mut buf);
        if n > 0 {
            return String::from_utf16_lossy(&buf[..n as usize]);
        }
    }
    format!("Key {key}")
}

/// Shortcuts per action, stored in the settings. `None` = unbound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keybinds {
    pub start_pause: Option<Binding>,
    pub brb: Option<Binding>,
    pub open: Option<Binding>,
    pub camera: Option<Binding>,
    pub calculator: Option<Binding>,
    pub click_through: Option<Binding>,
}

impl Default for Keybinds {
    fn default() -> Self {
        Keybinds {
            start_pause: Binding::ctrl_alt('P'),
            brb: Binding::ctrl_alt('B'),
            open: Binding::ctrl_alt('O'),
            camera: Binding::ctrl_alt('K'),
            calculator: Binding::ctrl_alt('C'),
            // Unbound: pressed by accident it leaves a timer bar that ignores every click.
            click_through: None,
        }
    }
}

impl Keybinds {
    pub fn get(&self, hk: Hotkey) -> Option<Binding> {
        match hk {
            Hotkey::StartPause => self.start_pause,
            Hotkey::Brb => self.brb,
            Hotkey::Settings => self.open,
            Hotkey::Camera => self.camera,
            Hotkey::Calculator => self.calculator,
            Hotkey::ClickThrough => self.click_through,
            Hotkey::Autostart | Hotkey::Restart | Hotkey::Quit => None,
        }
        .filter(Binding::valid)
    }

    /// " (Ctrl+Alt+O)" for tooltips, empty when unbound.
    pub fn hint(&self, hk: Hotkey) -> String {
        self.get(hk).map(|b| format!(" ({})", b.label())).unwrap_or_default()
    }

    fn active(&self) -> Vec<(Hotkey, Binding)> {
        ALL.iter().filter_map(|(hk, _, _)| self.get(*hk).map(|b| (*hk, b))).collect()
    }
}

/// Bindings the hook thread applies, and the dashboard's "recording a shortcut" pause.
static BINDS: Mutex<Vec<(Hotkey, Binding)>> = Mutex::new(Vec::new());
static PAUSED: AtomicBool = AtomicBool::new(false);
/// Actions whose keyboard combo another app already owns (`RegisterHotKey` failed).
static TAKEN: Mutex<Vec<Hotkey>> = Mutex::new(Vec::new());
/// Middle/side button pressed while the dashboard records, with a counter so the page can tell
/// a new press from the last one. Mouse 4 (Back) never reached the page's recorder in the
/// WebView, so the hook captures these buttons for it, wherever the cursor is.
static CAPTURED: Mutex<(u32, Option<Binding>)> = Mutex::new((0, None));
static THREAD: AtomicU32 = AtomicU32::new(0);
const WM_REBIND: u32 = WM_APP + 7;

/// Apply new bindings (from the settings) on the hook thread.
pub fn set(binds: &Keybinds) {
    *BINDS.lock().unwrap() = binds.active();
    rebind();
}

/// While the dashboard records a shortcut every binding is released, so the combo being
/// pressed reaches the page instead of firing.
pub fn pause(on: bool) {
    PAUSED.store(on, Ordering::Release);
    rebind();
}

/// Label of the binding in force for `hk` (tray menu).
pub fn current_label(hk: Hotkey) -> Option<String> {
    BINDS.lock().unwrap().iter().find(|(h, _)| *h == hk).map(|(_, b)| b.label())
}

/// Actions whose keyboard shortcut is held by another app.
pub fn taken() -> Vec<Hotkey> {
    TAKEN.lock().unwrap().clone()
}

/// Latest middle/side button captured while recording: (counter, binding).
pub fn captured() -> (u32, Option<Binding>) {
    *CAPTURED.lock().unwrap()
}

fn rebind() {
    let t = THREAD.load(Ordering::Acquire);
    if t != 0 {
        // SAFETY: posting to our own message-loop thread.
        let _ = unsafe { PostThreadMessageW(t, WM_REBIND, WPARAM(0), LPARAM(0)) };
    }
}

struct Ctx {
    tx: Sender<Hotkey>,
    wake: Box<dyn Fn() + Send>,
    hook: Cell<Option<HHOOK>>,
    /// Mouse binds in force (the hook reads them on every event, so no lock there).
    mouse: RefCell<Vec<(Hotkey, Binding)>>,
    /// A bound button's down was swallowed; swallow its up too so apps never see half a click.
    eaten_up: Cell<Option<u32>>,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

fn fire(hk: Hotkey) {
    CTX.with_borrow(|c| {
        if let Some(c) = c {
            if c.tx.send(hk).is_ok() {
                (c.wake)();
            }
        }
    });
}

/// (Re)register everything from `BINDS` on this thread.
unsafe fn apply(registered: &mut Vec<i32>) {
    for id in registered.drain(..) {
        let _ = UnregisterHotKey(None, id);
    }
    let paused = PAUSED.load(Ordering::Acquire);
    let binds = if paused { Vec::new() } else { BINDS.lock().unwrap().clone() };
    let mut taken = Vec::new();
    for (i, (hk, b)) in binds.iter().enumerate() {
        if b.is_mouse() {
            continue;
        }
        let mut mods = MOD_NOREPEAT;
        for (on, m) in [(b.ctrl, MOD_CONTROL), (b.alt, MOD_ALT), (b.shift, MOD_SHIFT), (b.win, MOD_WIN)] {
            if on {
                mods |= m;
            }
        }
        let id = i as i32 + 1;
        if RegisterHotKey(None, id, HOT_KEY_MODIFIERS(mods.0), b.key).is_ok() {
            registered.push(id);
        } else {
            taken.push(*hk);
        }
    }
    *TAKEN.lock().unwrap() = taken;
    let mouse: Vec<_> = binds.into_iter().filter(|(_, b)| b.is_mouse()).collect();
    CTX.with_borrow(|c| {
        let Some(c) = c else { return };
        // Recording needs the hook too: it captures the buttons the page cannot see.
        match (mouse.is_empty() && !paused, c.hook.get()) {
            (false, None) => c.hook.set(SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0).ok()),
            (true, Some(h)) => {
                let _ = UnhookWindowsHookEx(h);
                c.hook.set(None);
            }
            _ => {}
        }
        *c.mouse.borrow_mut() = mouse;
    });
}

fn held(vk: u16) -> bool {
    // SAFETY: plain state query.
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 {
            let info = &*(lp.0 as *const MSLLHOOKSTRUCT);
            let hi = (info.mouseData >> 16) as u16;
            let (key, down) = match wp.0 as u32 {
                WM_MOUSEWHEEL => (if (hi as i16) < 0 { WHEEL_DOWN } else { WHEEL_UP }, true),
                WM_MBUTTONDOWN => (VK_MBUTTON, true),
                WM_MBUTTONUP => (VK_MBUTTON, false),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let key = if hi == 2 { VK_XBUTTON2 } else { VK_XBUTTON1 };
                    (key, wp.0 as u32 == WM_XBUTTONDOWN)
                }
                _ => (0, false),
            };
            if key != 0 {
                let eat = CTX.with_borrow(|c| {
                    let Some(c) = c else { return false };
                    if !down {
                        return c.eaten_up.get() == Some(key) && { c.eaten_up.set(None); true };
                    }
                    let now = Binding {
                        ctrl: held(VK_CONTROL.0),
                        alt: held(VK_MENU.0),
                        shift: held(VK_SHIFT.0),
                        win: held(VK_LWIN.0) || held(VK_RWIN.0),
                        key,
                    };
                    if PAUSED.load(Ordering::Acquire) {
                        // Recording: the wheel reaches the page fine; buttons are captured here.
                        if matches!(key, WHEEL_UP | WHEEL_DOWN) {
                            return false;
                        }
                        {
                            let mut cap = CAPTURED.lock().unwrap();
                            *cap = (cap.0.wrapping_add(1), Some(now));
                        }
                        c.eaten_up.set(Some(key));
                        (c.wake)();
                        return true;
                    }
                    let hit = c.mouse.borrow().iter().find(|(_, b)| *b == now).map(|(hk, _)| *hk);
                    if let Some(hk) = hit {
                        if !matches!(key, WHEEL_UP | WHEEL_DOWN) {
                            c.eaten_up.set(Some(key));
                        }
                        // The hook must return quickly: the app thread does the work.
                        if c.tx.send(hk).is_ok() {
                            (c.wake)();
                        }
                    }
                    hit.is_some()
                });
                if eat {
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wp, lp)
    }
}

pub fn spawn(tx: Sender<Hotkey>, wake: Box<dyn Fn() + Send>, binds: &Keybinds) {
    *BINDS.lock().unwrap() = binds.active();
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || unsafe {
            CTX.with_borrow_mut(|c| {
                *c = Some(Ctx { tx, wake, hook: Cell::new(None), mouse: RefCell::new(Vec::new()), eaten_up: Cell::new(None) })
            });
            let mut registered = Vec::new();
            let mut msg = MSG::default();
            // Make sure the queue exists before anyone posts to it.
            let _ = windows::Win32::UI::WindowsAndMessaging::PeekMessageW(&mut msg, None, 0, 0, windows::Win32::UI::WindowsAndMessaging::PM_NOREMOVE);
            THREAD.store(GetCurrentThreadId(), Ordering::Release);
            apply(&mut registered);
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                match msg.message {
                    WM_HOTKEY => {
                        let hk = BINDS.lock().unwrap().get(msg.wParam.0.wrapping_sub(1)).map(|(hk, _)| *hk);
                        if let Some(hk) = hk {
                            fire(hk);
                        }
                    }
                    WM_REBIND => apply(&mut registered),
                    _ => {}
                }
            }
        })
        .expect("spawn hotkey thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_like_shortcuts() {
        assert_eq!(Keybinds::default().open.unwrap().label(), "Ctrl+Alt+O");
        assert_eq!(Binding { key: VK_XBUTTON1, ..Default::default() }.label(), "Mouse 4");
        assert_eq!(Binding { ctrl: true, key: WHEEL_DOWN, ..Default::default() }.label(), "Ctrl+Wheel down");
    }

    #[test]
    fn unbindable_inputs_are_ignored() {
        let mut k = Keybinds::default();
        k.open = Some(Binding { key: 1, ..Default::default() });
        k.brb = Some(Binding { ctrl: true, key: 0x11, ..Default::default() });
        assert_eq!(k.get(Hotkey::Settings), None);
        assert_eq!(k.get(Hotkey::Brb), None);
        assert!(k.get(Hotkey::Camera).is_some());
    }
}
