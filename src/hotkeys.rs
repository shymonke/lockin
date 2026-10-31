//! System-wide Ctrl+Alt hotkeys via `RegisterHotKey` on a dedicated message-loop thread.

use std::sync::mpsc::Sender;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
    Calculator,
    StartPause,
    Brb,
    Settings,
    ClickThrough,
}

pub const ALL: [(Hotkey, char, &str); 5] = [
    (Hotkey::Calculator, 'C', "Toggle the CAT calculator"),
    (Hotkey::StartPause, 'P', "Start / pause the pomodoro"),
    (Hotkey::Brb, 'B', "BRB pause / I'm back"),
    (Hotkey::Settings, 'O', "Open settings"),
    (Hotkey::ClickThrough, 'L', "Lock overlay (click-through) on/off"),
];

pub fn spawn(tx: Sender<Hotkey>, wake: Box<dyn Fn() + Send>) {
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || unsafe {
            for (i, (_, key, _)) in ALL.iter().enumerate() {
                // Failure means another app owns the combo; the menu buttons still work.
                let _ = RegisterHotKey(None, i as i32 + 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, *key as u32);
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_HOTKEY {
                    if let Some((hk, _, _)) = ALL.get(msg.wParam.0.wrapping_sub(1)) {
                        if tx.send(*hk).is_err() {
                            return;
                        }
                        wake();
                    }
                }
            }
        })
        .expect("spawn hotkey thread");
}
