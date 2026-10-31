//! egui-winit enables Windows' undecorated shadow on frameless windows. winit implements it by
//! shifting the client area down 1 px in WM_NCCALCSIZE, which leaves a visible line above our
//! transparent overlays. We turn off DWM's frame and subclass those windows so the client area
//! is the whole window again.

use std::mem::size_of_val;
use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWINDOWATTRIBUTE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMNCRP_DISABLED,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumThreadWindows, GetPropW, GetWindowTextW, SetPropW, SetWindowLongPtrW, SetWindowPos, GWLP_WNDPROC,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_NCCALCSIZE, WNDPROC,
};

const OLD_PROC: windows::core::PCWSTR = w!("LockIn.OverlayOldProc");

/// Subclass proc: client area = window area (no winit 1 px shadow strip), everything else to winit.
unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCALCSIZE && wp.0 != 0 {
            return LRESULT(0);
        }
        let old: WNDPROC = std::mem::transmute(GetPropW(hwnd, OLD_PROC).0);
        CallWindowProcW(old, hwnd, msg, wp, lp)
    }
}

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
        // Store winit's proc before swapping so overlay_proc can always forward.
        let proc_ptr: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = overlay_proc;
        let old = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if old != proc_ptr as isize {
            let _ = SetPropW(hwnd, OLD_PROC, Some(HANDLE(old as *mut _)));
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc_ptr as isize);
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
    BOOL(1)
}

/// Caller must supply a live HWND and the attribute's matching Win32 value type.
unsafe fn set_attribute<T>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: &T) -> windows::core::Result<()> {
    // SAFETY: the referenced value remains live for this synchronous call.
    unsafe { DwmSetWindowAttribute(hwnd, attribute, std::ptr::from_ref(value).cast(), size_of_val(value) as u32) }
}
