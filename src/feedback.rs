//! Feedback from the main window, posted to a Discord webhook over WinHTTP (no HTTP crate).
//!
//! The webhook URL is baked in at build time (`LOCKIN_FEEDBACK_WEBHOOK`, see build.rs) so it
//! never sits in the public source; a build without it hides the feedback form.

use serde_json::json;
use std::sync::Mutex;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

pub const WEBHOOK: Option<&str> = option_env!("LOCKIN_FEEDBACK_WEBHOOK");

/// Longest message accepted (Discord's embed description limit is 4096).
pub const MAX_LEN: usize = 3500;

/// Last send for the dashboard: a counter (one per send) and its outcome; `None` while sending.
static STATUS: Mutex<(u32, Option<Result<(), String>>)> = Mutex::new((0, None));

pub fn status() -> (u32, Option<Result<(), String>>) {
    STATUS.lock().unwrap().clone()
}

/// Post in the background; `done` runs when the outcome is in `status()`.
pub fn send(message: &str, contact: &str, done: impl FnOnce() + Send + 'static) {
    let Some(url) = WEBHOOK else { return };
    let message: String = message.trim().chars().take(MAX_LEN).collect();
    if message.is_empty() {
        return;
    }
    let contact: String = contact.trim().chars().take(200).collect();
    {
        let mut s = STATUS.lock().unwrap();
        *s = (s.0.wrapping_add(1), None);
    }
    let mut fields = vec![json!({ "name": "Version", "value": env!("CARGO_PKG_VERSION"), "inline": true })];
    if !contact.is_empty() {
        fields.push(json!({ "name": "Contact", "value": contact, "inline": true }));
    }
    let body = json!({
        "username": "LockIn feedback",
        // Text from users must never ping anyone.
        "allowed_mentions": { "parse": [] },
        "embeds": [{ "title": "Feedback", "description": message, "color": 0x8CC7A5, "fields": fields }],
    })
    .to_string();
    std::thread::Builder::new()
        .name("feedback".into())
        .spawn(move || {
            let result = post_json(url, &body);
            STATUS.lock().unwrap().1 = Some(result);
            done();
        })
        .expect("spawn feedback thread");
}

/// Handles closed on drop.
struct Handle(*mut core::ffi::c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a handle this module opened and has not closed.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn post_json(url: &str, body: &str) -> Result<(), String> {
    let rest = url.strip_prefix("https://").ok_or("The feedback address is not https")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let failed = |what: &str| format!("Could not reach Discord ({what}). Check your connection.");
    // SAFETY: plain WinHTTP calls; every pointer outlives the call that uses it.
    unsafe {
        let session = Handle(WinHttpOpen(&HSTRING::from("LockIn"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if session.0.is_null() {
            return Err(failed("session"));
        }
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 15_000);
        let connect = Handle(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if connect.0.is_null() {
            return Err(failed("connect"));
        }
        let request = Handle(WinHttpOpenRequest(connect.0, &HSTRING::from("POST"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE));
        if request.0.is_null() {
            return Err(failed("request"));
        }
        let headers: Vec<u16> = "Content-Type: application/json\r\n".encode_utf16().collect();
        let len = body.len() as u32;
        WinHttpSendRequest(request.0, Some(&headers), Some(body.as_ptr().cast()), len, len, 0).map_err(|_| failed("send"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|_| failed("response"))?;
        let mut code = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some((&mut code as *mut u32).cast()), &mut size, std::ptr::null_mut())
            .map_err(|_| failed("status"))?;
        match code {
            200..=299 => Ok(()),
            429 => Err("Too much feedback at once. Try again in a minute.".into()),
            _ => Err(format!("Discord refused the message (HTTP {code}).")),
        }
    }
}
