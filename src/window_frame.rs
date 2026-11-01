//! egui-winit enables Windows' undecorated shadow on frameless windows. winit implements it by
//! shifting the client area down 1 px in WM_NCCALCSIZE, which leaves a visible line above our
//! transparent overlays. We turn off DWM's frame and subclass those windows so the client area
//! is the whole window again.
//!
//! With DWM non-client rendering off, `DefWindowProc` falls back to painting the classic caption
//! (title, minimise/maximise/close) straight over the client area whenever the window activates,
//! is dragged, or has its title or icon set. winit keeps WS_CAPTION on undecorated windows, so the
//! subclass strips the caption styles (and keeps them off when winit re-applies its styles) and
//! swallows the remaining non-client paints.

use std::mem::size_of_val;
use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWINDOWATTRIBUTE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMNCRP_DISABLED,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumThreadWindows, GetPropW, GetWindowLongPtrW, GetWindowTextW, SetPropW, SetWindowLongPtrW,
    SetWindowPos, GWLP_WNDPROC, GWL_STYLE, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCPAINT, WM_SETICON, WM_SETTEXT, WM_STYLECHANGING, WNDPROC,
    WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_VISIBLE,
};

const OLD_PROC: windows::core::PCWSTR = w!("LockIn.OverlayOldProc");

/// Undocumented uxtheme messages that draw the caption and frame directly.
const WM_NCUAHDRAWCAPTION: u32 = 0xAE;
const WM_NCUAHDRAWFRAME: u32 = 0xAF;
/// Styles that make `DefWindowProc` draw a title bar. Dragging still works without them: egui
/// starts a move with WM_NCLBUTTONDOWN/HTCAPTION, which needs no caption.
const CAPTION_STYLES: u32 = WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0;

/// Subclass proc: client area = window area (no winit 1 px shadow strip), no classic caption
/// painting, everything else to winit.
unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let old: WNDPROC = std::mem::transmute(GetPropW(hwnd, OLD_PROC).0);
        match msg {
            WM_NCCALCSIZE if wp.0 != 0 => LRESULT(0),
            WM_NCPAINT | WM_NCUAHDRAWCAPTION | WM_NCUAHDRAWFRAME => LRESULT(0),
            WM_STYLECHANGING if wp.0 as i32 == GWL_STYLE.0 => {
                let s = lp.0 as *mut STYLESTRUCT;
                (*s).styleNew &= !CAPTION_STYLES;
                CallWindowProcW(old, hwnd, msg, wp, lp)
            }
            // lParam -1: DefWindowProc updates the active state but skips the caption repaint.
            WM_NCACTIVATE => CallWindowProcW(old, hwnd, msg, wp, LPARAM(-1)),
            // DefWindowProc repaints the caption after storing the text or icon unless the
            // window is hidden for the duration of the call.
            WM_SETTEXT | WM_SETICON => {
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                let visible = style & WS_VISIBLE.0 as isize != 0;
                if visible {
                    SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_VISIBLE.0 as isize));
                }
                let r = CallWindowProcW(old, hwnd, msg, wp, lp);
                if visible {
                    SetWindowLongPtrW(hwnd, GWL_STYLE, GetWindowLongPtrW(hwnd, GWL_STYLE) | WS_VISIBLE.0 as isize);
                }
                r
            }
            _ => CallWindowProcW(old, hwnd, msg, wp, lp),
        }
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
        let old = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if old != proc_ptr as isize {
            let _ = SetPropW(hwnd, OLD_PROC, Some(HANDLE(old as *mut _)));
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc_ptr as isize);
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(CAPTION_STYLES as isize));
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
