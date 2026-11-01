//! Main window: a WebView2 page (assets/) hosted in a plain Win32 window on its own STA thread.
//! The page polls `lockin://localhost/state|preview|frame` and posts actions over `window.ipc`.
//! Closing the window ends the thread and the WebView2 processes, giving the memory back.

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use std::borrow::Cow;
use std::cell::RefCell;
use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::http::{header::CONTENT_TYPE, Request, Response};
use wry::{Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

static INDEX: &str = include_str!("../assets/index.html");
static CSS: &str = include_str!("../assets/app.css");
static JS: &str = include_str!("../assets/app.js");
static ICON_FONT: &[u8] = crate::theme::ICON_FONT;

/// Keep WebView2 small: one renderer, no GPU process (the page is a canvas at ~15 fps, software
/// raster is plenty), no background services. wry's defaults (no mini menu / SmartScreen) kept.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
    --disable-gpu --disable-gpu-compositing --renderer-process-limit=1 --disable-background-networking \
    --disable-component-update --disable-sync --no-first-run --disable-breakpad";

/// Background matches the page so opening never flashes white.
const BG: (u8, u8, u8) = (20, 23, 29);

/// What the page can read; filled by the app thread.
pub struct Shared {
    /// App snapshot as JSON (`/state`).
    pub state: Mutex<String>,
    /// `/preview` JSON and (when asked) the `/frame` JPEG of the latest camera preview.
    pub preview: Box<dyn Fn(bool) -> (String, Option<Vec<u8>>) + Send + Sync>,
    /// Actions posted by the page.
    pub actions: Mutex<Sender<serde_json::Value>>,
    pub wake: Box<dyn Fn() + Send + Sync>,
}

/// Handle to the (possibly closed) dashboard window.
#[derive(Default)]
pub struct Dashboard {
    hwnd: Arc<AtomicIsize>,
}

impl Dashboard {
    pub fn is_open(&self) -> bool {
        self.hwnd.load(Ordering::Acquire) != 0
    }

    /// Open the window, or bring it to the front when it already exists.
    pub fn open(&self, shared: Arc<Shared>) {
        let h = self.hwnd.load(Ordering::Acquire);
        if h != 0 {
            unsafe {
                let hwnd = HWND(h as *mut _);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            return;
        }
        // Reserve the slot so a double open does not create two windows.
        if self.hwnd.compare_exchange(0, -1, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return;
        }
        let slot = self.hwnd.clone();
        std::thread::Builder::new()
            .name("dashboard".into())
            .spawn(move || run(slot, shared))
            .expect("spawn dashboard thread");
    }

    /// Hide the window without closing it (screen calibration), or bring it back.
    pub fn set_hidden(&self, hidden: bool) {
        let h = self.hwnd.load(Ordering::Acquire);
        if h <= 0 {
            return;
        }
        let hwnd = HWND(h as *mut _);
        unsafe {
            let _ = ShowWindowAsync(hwnd, if hidden { SW_HIDE } else { SW_SHOW });
            if !hidden {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

struct Hwnd(isize);
impl HasWindowHandle for Hwnd {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let h = Win32WindowHandle::new(NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?);
        // SAFETY: the HWND outlives the webview (both live on this thread until WM_DESTROY).
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}

thread_local! {
    static VIEW: RefCell<Option<WebView>> = const { RefCell::new(None) };
}

fn client_bounds(hwnd: HWND) -> Rect {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    Rect {
        position: wry::dpi::PhysicalPosition::new(0, 0).into(),
        size: wry::dpi::PhysicalSize::new((r.right - r.left) as u32, (r.bottom - r.top) as u32).into(),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            VIEW.with(|v| {
                if let Some(v) = v.borrow().as_ref() {
                    let _ = v.set_bounds(client_bounds(hwnd));
                }
            });
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            let info = &mut *(lp.0 as *mut MINMAXINFO);
            info.ptMinTrackSize.x = 640 * dpi / 96;
            info.ptMinTrackSize.y = 520 * dpi / 96;
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn respond(mime: &str, body: Cow<'static, [u8]>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .header(CONTENT_TYPE, mime)
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap()
}

/// Embedded asset; debug builds read `assets/` from disk so F5 in the window picks up edits.
fn asset(name: &str, embedded: &'static str) -> Cow<'static, [u8]> {
    if cfg!(debug_assertions) {
        if let Ok(b) = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(name)) {
            return Cow::Owned(b);
        }
    }
    Cow::Borrowed(embedded.as_bytes())
}

fn serve(shared: &Shared, req: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    match req.uri().path() {
        "/" | "/index.html" => respond("text/html; charset=utf-8", asset("index.html", INDEX)),
        "/app.css" => respond("text/css; charset=utf-8", asset("app.css", CSS)),
        "/app.js" => respond("text/javascript; charset=utf-8", asset("app.js", JS)),
        "/fonts/phosphor-lockin.ttf" => respond("font/ttf", Cow::Borrowed(ICON_FONT)),
        "/state" => respond("application/json", Cow::Owned(shared.state.lock().unwrap().clone().into_bytes())),
        "/preview" => respond("application/json", Cow::Owned((shared.preview)(false).0.into_bytes())),
        "/frame" => match (shared.preview)(true).1 {
            Some(jpeg) => respond("image/jpeg", Cow::Owned(jpeg)),
            None => Response::builder().status(404).body(Cow::Borrowed(&[][..])).unwrap(),
        },
        _ => Response::builder().status(404).body(Cow::Borrowed(&[][..])).unwrap(),
    }
}

fn run(slot: Arc<AtomicIsize>, shared: Arc<Shared>) {
    let post = |v: serde_json::Value| {
        let _ = shared.actions.lock().unwrap().send(v);
        (shared.wake)();
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("LockInDashboard");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(CreateSolidBrush(COLORREF(BG.0 as u32 | (BG.1 as u32) << 8 | (BG.2 as u32) << 16)).0),
            hIcon: LoadIconW(Some(hinst.into()), PCWSTR(1 as _)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when already registered by an earlier open
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem().max(96) as i32;
        let (w, h) = (1040 * dpi / 96, 780 * dpi / 96);
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("LockIn"),
            WS_OVERLAPPEDWINDOW,
            ((sw - w) / 2).max(0),
            ((sh - h) / 2).max(0),
            w.min(sw),
            h.min(sh - 40 * dpi / 96),
            None,
            None,
            Some(hinst.into()),
            None,
        ) {
            Ok(h) => h,
            Err(_) => {
                slot.store(0, Ordering::Release);
                return;
            }
        };
        // Dark title bar in the page's colour.
        let dark = BOOL(1);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
        let caption = COLORREF(BG.0 as u32 | (BG.1 as u32) << 8 | (BG.2 as u32) << 16);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &caption as *const _ as _, 4);

        let serve_shared = shared.clone();
        let ipc_shared = shared.clone();
        // Profile under %APPDATA%\LockIn instead of beside the exe.
        let mut context = WebContext::new(Some(crate::config::data_dir().join("webview")));
        let built = WebViewBuilder::new_with_web_context(&mut context)
            .with_additional_browser_args(BROWSER_ARGS)
            .with_browser_accelerator_keys(cfg!(debug_assertions))
            .with_default_context_menus(cfg!(debug_assertions))
            .with_url("lockin://localhost/index.html")
            .with_background_color((BG.0, BG.1, BG.2, 255))
            .with_custom_protocol("lockin".into(), move |_, req| serve(&serve_shared, &req))
            .with_ipc_handler(move |req| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(req.body()) {
                    let _ = ipc_shared.actions.lock().unwrap().send(v);
                    (ipc_shared.wake)();
                }
            })
            .with_bounds(client_bounds(hwnd))
            .build_as_child(&Hwnd(hwnd.0 as isize));
        match built {
            Ok(view) => VIEW.with(|v| *v.borrow_mut() = Some(view)),
            Err(e) => {
                post(serde_json::json!({ "cmd": "dashboard-error", "error": e.to_string() }));
                let _ = DestroyWindow(hwnd);
            }
        }
        slot.store(hwnd.0 as isize, Ordering::Release);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        post(serde_json::json!({ "cmd": "dashboard-open" }));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        VIEW.with(|v| v.borrow_mut().take());
    }
    slot.store(0, Ordering::Release);
    post(serde_json::json!({ "cmd": "dashboard-closed" }));
}
