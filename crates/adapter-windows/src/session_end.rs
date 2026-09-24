//! Noticing that the session ends: sign-out, shutdown, or the Restart Manager closing the agent.
//! Windows tells windows, not processes, so the agent owns a hidden window that receives
//! `WM_ENDSESSION`. Once that is answered the process may end at any moment, so what must happen
//! (the last log lines) happens while the message is handled; the exit after it is best effort.

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};

use mujina_winutil::wide::to_wide;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ENDSESSION_CLOSEAPP, ENDSESSION_CRITICAL, ENDSESSION_LOGOFF,
    RegisterClassW, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
};

thread_local! {
    /// Set by the window procedure, which runs on this thread while it pumps messages.
    static ENDING: Cell<bool> = const { Cell::new(false) };
    /// Run by the window procedure when the session ends; see [`watch`].
    static LAST_WORDS: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // "May the session end?" Yes; Mujina never holds up a shutdown.
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wparam != 0 {
                ENDING.with(|flag| flag.set(true));
                // Flushing reopens the log by name (see `log_file`), so the closing lines land
                // in the current file.
                log::logger().flush();
                log::info!("session ending ({})", cause(lparam));
                // Taken out first, so the slot is not borrowed while they run.
                if let Some(last_words) = LAST_WORDS.with(|slot| slot.borrow_mut().take()) {
                    last_words();
                }
            }
            0
        }
        // SAFETY: forwarding the unmodified arguments of a window message.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Creates the hidden window on the calling thread, which must pump messages; it lives as long as
/// the process. Returns whether the session end will be noticed.
///
/// `last_words` runs once, inside `WM_ENDSESSION`, since Windows may end the session as soon as
/// every application has returned from it. It must be quick.
pub fn watch(last_words: Box<dyn FnOnce()>) -> bool {
    LAST_WORDS.with(|slot| *slot.borrow_mut() = Some(last_words));
    let class_name = to_wide("MujinaSessionEnd");
    // SAFETY: plain calls; the class structure and the string outlive the calls that use them;
    // `window_proc` matches WNDPROC; a failed registration makes CreateWindowExW fail.
    let window = unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&raw const class);
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    !window.is_null()
}

/// Whether the session started ending since the last call.
pub fn take_ending() -> bool {
    ENDING.with(|flag| flag.replace(false))
}

/// The `WM_ENDSESSION` flags in words, for the log. No flag means shutdown or restart; the raw
/// value follows for anything not named here.
fn cause(lparam: LPARAM) -> String {
    // The documented flags all lie in the low 32 bits.
    let flags = u32::try_from(lparam.cast_unsigned() & 0xFFFF_FFFF).unwrap_or_default();
    let mut parts = Vec::new();
    if flags == 0 {
        parts.push("shutdown or restart".to_string());
    }
    for (flag, name) in [
        (ENDSESSION_LOGOFF, "sign-out"),
        (ENDSESSION_CLOSEAPP, "closed by the Restart Manager"),
        (ENDSESSION_CRITICAL, "forced"),
    ] {
        if flags & flag != 0 {
            parts.push(name.to_string());
        }
    }
    parts.push(format!("flags {flags:#x}"));
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_says_what_ends_the_session() {
        assert_eq!(cause(0), "shutdown or restart, flags 0x0");
        assert_eq!(cause(0x8000_0000), "sign-out, flags 0x80000000");
        assert_eq!(cause(0xC000_0000), "sign-out, forced, flags 0xc0000000");
        assert_eq!(cause(0x1), "closed by the Restart Manager, flags 0x1");
        assert_eq!(cause(0x4), "flags 0x4");
    }
}
