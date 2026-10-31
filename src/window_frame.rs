//! egui-winit enables Windows' undecorated shadow, including a 1 px non-client strip.
//! Turn off DWM's frame after viewport creation, without changing winit's window styles.

use std::mem::size_of_val;
use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWINDOWATTRIBUTE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMNCRP_DISABLED,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, GetPropW, GetWindowTextW, SetPropW,
};

pub fn remove_overlay_frames() {
    // SAFETY: synchronous enumeration of this UI thread's top-level windows only.
    let _ = unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(remove_frame), LPARAM(0)) };
}

unsafe extern "system" fn remove_frame(hwnd: HWND, _: LPARAM) -> BOOL {
    // SAFETY: hwnd belongs to the current UI thread and is live during enumeration.
    unsafe {
        // winit retains WS_CAPTION even on undecorated top-level windows (it handles
        // WM_NCCALCSIZE itself), so styles cannot distinguish Settings from overlays.
        // These are the titles of our three undecorated viewport builders.
        let mut title = [0u16; 128];
        let len = GetWindowTextW(hwnd, &mut title) as usize;
        let title = String::from_utf16_lossy(&title[..len]);
        if !matches!(title.as_str(), "LockIn" | "LockIn alert" | "Normal Calculator") {
            return BOOL(1);
        }
        // Settings keeps its native decorations. The property is destroyed with the HWND,
        // so a recreated viewport is configured even when Windows reuses the handle.
        let marker = w!("LockIn.OverlayFrameRemoved");
        if !GetPropW(hwnd, marker).is_invalid() {
            return BOOL(1);
        }
        // Border/corner attributes require Windows 11; non-client rendering also works
        // on older Windows. None of these changes alpha, hit testing or window level.
        let _ = set_attribute(hwnd, DWMWA_BORDER_COLOR, &DWMWA_COLOR_NONE);
        let _ = set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_DONOTROUND);
        if set_attribute(hwnd, DWMWA_NCRENDERING_POLICY, &DWMNCRP_DISABLED).is_ok() {
            let _ = SetPropW(hwnd, marker, Some(HANDLE(1usize as *mut _)));
        }
    }
    BOOL(1)
}

/// Caller must supply a live HWND and the attribute's matching Win32 value type.
unsafe fn set_attribute<T>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: &T) -> windows::core::Result<()> {
    // SAFETY: the referenced value remains live for this synchronous call.
    unsafe { DwmSetWindowAttribute(hwnd, attribute, std::ptr::from_ref(value).cast(), size_of_val(value) as u32) }
}
