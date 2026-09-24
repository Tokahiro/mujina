//! Watching the keyboard to find out what a device button sends.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mujina_domain::chord::{KeyEvent, Origin};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WAIT_FAILED, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, KBDLLHOOKSTRUCT, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL,
};

use crate::hook::key_event;

/// Allocated up front; the hook drops further events rather than reallocate.
const CAPACITY: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    pub event: KeyEvent,
    pub scan_code: u32,
}

/// What happens to the keys seen while watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keys {
    PassOn,
    /// Nothing else sees them until watching ends (Mujina's own keystrokes excepted).
    Hold,
}

/// Whether the hook holds keys back; set for the duration of one `observe`.
static HOLD: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SEEN: RefCell<Vec<Observation>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn observe_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == 0 {
        // SAFETY: for HC_ACTION (0) Windows passes a valid KBDLLHOOKSTRUCT in `lparam`.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let observation = Observation {
            event: key_event(info, wparam),
            scan_code: info.scanCode,
        };
        SEEN.with(|seen| {
            if let Ok(mut seen) = seen.try_borrow_mut()
                && seen.len() < seen.capacity()
            {
                seen.push(observation);
            }
        });
        let ours = matches!(observation.event.origin, Origin::Own | Origin::Replayed);
        if !ours && HOLD.load(Ordering::Relaxed) {
            return 1;
        }
    }
    // SAFETY: forwarding the unmodified arguments is the documented contract of a hook.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// Calls `report` for every key event until `duration` passes or it returns `false`. Returns
/// everything seen, or `None` if the hook could not be installed.
pub fn observe(
    duration: Duration,
    keys: Keys,
    report: &mut dyn FnMut(&Observation) -> bool,
) -> Option<Vec<Observation>> {
    SEEN.with(|seen| *seen.borrow_mut() = Vec::with_capacity(CAPACITY));
    HOLD.store(keys == Keys::Hold, Ordering::Relaxed);
    // SAFETY: `observe_proc` matches HOOKPROC and outlives the hook, which is removed below.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(observe_proc),
            GetModuleHandleW(null()),
            0,
        )
    };
    if hook.is_null() {
        HOLD.store(false, Ordering::Relaxed);
        return None;
    }

    let deadline = Instant::now() + duration;
    let mut reported = 0;
    let mut watching = true;
    while watching {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let milliseconds = u32::try_from(left.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: no handles are passed; the call only waits for messages or the timeout.
        let woken = unsafe {
            MsgWaitForMultipleObjectsEx(0, null(), milliseconds, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if woken == WAIT_FAILED {
            break;
        }
        // SAFETY: MSG is plain data for which all-zero is a valid value.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: `message` is writable; a null window means "any message of this thread".
        while unsafe { PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: `message` was filled in by PeekMessageW.
            unsafe { DispatchMessageW(&raw const message) };
        }
        // Copied first, so the buffer is not borrowed while `report` runs.
        let fresh: Vec<Observation> = SEEN.with(|seen| seen.borrow()[reported..].to_vec());
        reported += fresh.len();
        for observation in &fresh {
            if !report(observation) {
                watching = false;
                break;
            }
        }
    }

    // SAFETY: the handle came from SetWindowsHookExW on this thread.
    unsafe { UnhookWindowsHookEx(hook) };
    HOLD.store(false, Ordering::Relaxed);
    Some(SEEN.with(|seen| std::mem::take(&mut *seen.borrow_mut())))
}
