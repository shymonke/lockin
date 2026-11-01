//! Notification-area (tray) icon on its own message-loop thread. Left click opens the main window;
//! right click shows a menu with the hotkey actions and Quit. Commands reach the app over the same
//! channel as the global hotkeys.

use crate::hotkeys::Hotkey;
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::Sender;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW, GetCursorPos,
    GetMessageW, GetSystemMetrics, LoadImageW, PostMessageW, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, TrackPopupMenu, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, MSG,
    SM_CXSMICON, SM_CYSMICON, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};

const CALLBACK: u32 = WM_APP + 1;
const ICON_ID: u32 = 1;
/// Resource id of the app icon in `assets/brand/app.rc`.
const IDI_LOCKIN: u16 = 1;

const MENU: [(Hotkey, PCWSTR); 7] = [
    (Hotkey::Settings, w!("Open LockIn\tCtrl+Alt+O")),
    (Hotkey::StartPause, w!("Start / pause\tCtrl+Alt+P")),
    (Hotkey::Brb, w!("Step away / I'm back\tCtrl+Alt+B")),
    (Hotkey::Camera, w!("Camera on / off\tCtrl+Alt+K")),
    (Hotkey::Calculator, w!("Calculator\tCtrl+Alt+C")),
    (Hotkey::ClickThrough, w!("Timer bar click-through\tCtrl+Alt+L")),
    (Hotkey::Quit, w!("Quit LockIn")),
];

/// Tray window, kept so the icon can be removed from any thread on quit.
static HWND_RAW: AtomicIsize = AtomicIsize::new(0);

struct Ctx {
    tx: Sender<Hotkey>,
    wake: Box<dyn Fn() + Send>,
    icon: HICON,
    taskbar_created: u32,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

pub fn spawn(tx: Sender<Hotkey>, wake: Box<dyn Fn() + Send>) {
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || unsafe {
            let Ok(module) = GetModuleHandleW(None) else { return };
            let class = w!("LockIn.Tray");
            let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: module.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            // A hidden top-level window (not message-only): the popup menu needs a window that
            // can take the foreground so it closes when the user clicks elsewhere.
            let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("LockIn tray"), WS_POPUP, 0, 0, 0, 0, None, None, Some(module.into()), None)
            else {
                return;
            };
            let icon = LoadImageW(
                Some(module.into()),
                PCWSTR(IDI_LOCKIN as usize as *const u16),
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_DEFAULTCOLOR,
            )
            .map(|h| HICON(h.0))
            .unwrap_or_default();
            CTX.with_borrow_mut(|c| {
                *c = Some(Ctx { tx, wake, icon, taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")) })
            });
            HWND_RAW.store(hwnd.0 as isize, Ordering::Release);
            add(hwnd, icon);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn tray thread");
}

/// Remove the icon now; otherwise it lingers in the tray until the mouse passes over it.
pub fn remove() {
    let raw = HWND_RAW.swap(0, Ordering::AcqRel);
    if raw != 0 {
        let nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: HWND(raw as *mut _),
            uID: ICON_ID,
            ..Default::default()
        };
        // SAFETY: plain struct by reference; the shell ignores unknown windows.
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
    }
}

unsafe fn add(hwnd: HWND, icon: HICON) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: CALLBACK,
        hIcon: icon,
        ..Default::default()
    };
    for (dst, src) in nid.szTip.iter_mut().zip("LockIn".encode_utf16()) {
        *dst = src;
    }
    // SAFETY: nid is fully initialised and outlives the call.
    let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
}

fn send(hk: Hotkey) {
    CTX.with_borrow(|c| {
        if let Some(c) = c {
            if c.tx.send(hk).is_ok() {
                (c.wake)();
            }
        }
    });
}

unsafe fn menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        for (i, (hk, label)) in MENU.iter().enumerate() {
            if *hk == Hotkey::Quit {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            }
            let _ = AppendMenuW(menu, MF_STRING, i + 1, *label);
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu dismisses on an outside click (TrackPopupMenu remarks).
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        if let Some((hk, _)) = (picked.0 as usize).checked_sub(1).and_then(|i| MENU.get(i)) {
            send(*hk);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == CALLBACK {
            match lp.0 as u32 {
                WM_LBUTTONUP => send(Hotkey::Settings),
                WM_RBUTTONUP => menu(hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        // Explorer restarted: the tray is new and empty, so add the icon again.
        let restore = CTX.with_borrow(|c| c.as_ref().filter(|c| c.taskbar_created != 0 && msg == c.taskbar_created).map(|c| c.icon));
        if let Some(icon) = restore {
            add(hwnd, icon);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
