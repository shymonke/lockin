//! "Start with Windows": a per-user Run entry pointing at the running exe. While enabled it is
//! rewritten on every start, so sign-in launches whichever build ran last (a dev exe rebuilt in
//! place is picked up as is).

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const NAME: PCWSTR = w!("LockIn");

pub fn apply(on: bool) -> Result<(), String> {
    // SAFETY: registry calls with valid, NUL-terminated strings that outlive them.
    unsafe {
        if !on {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME);
            return if r.is_ok() || r == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(format!("Could not remove the startup entry ({})", r.0)) };
        }
        let exe = std::env::current_exe().map_err(|e| format!("Could not find LockIn's exe: {e}"))?;
        let cmd = HSTRING::from(format!("\"{}\"", exe.display()));
        let bytes = (cmd.len() + 1) * 2;
        RegSetKeyValueW(HKEY_CURRENT_USER, RUN, NAME, REG_SZ.0, Some(cmd.as_ptr() as *const _), bytes as u32)
            .ok()
            .map_err(|e| format!("Could not add the startup entry: {}", e.message()))
    }
}
