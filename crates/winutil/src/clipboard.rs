//! Putting text on the clipboard.

use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

/// Replaces what the clipboard holds with `text`; `false` if the clipboard could not be had.
///
/// Call it on the thread of the active window, which becomes the clipboard's owner; without one
/// nothing is copied.
pub fn set_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // Opened without an owner window, SetClipboardData fails after EmptyClipboard.
    // SAFETY: no arguments; null when this thread has no active window.
    let owner = unsafe { GetActiveWindow() };
    if owner.is_null() {
        return false;
    }
    // SAFETY: `owner` is a window of this thread; the clipboard is closed again below.
    if unsafe { OpenClipboard(owner) } == 0 {
        return false;
    }
    let stored = put(&wide);
    // SAFETY: opened above, closed exactly once.
    unsafe { CloseClipboard() };
    stored
}

/// Empties the open clipboard and hands it `wide` (NUL-terminated UTF-16).
fn put(wide: &[u16]) -> bool {
    let bytes = size_of_val(wide);
    // SAFETY: the caller has the clipboard open with an owner window, which this makes the
    // clipboard's owner.
    if unsafe { EmptyClipboard() } == 0 {
        return false;
    }
    // SAFETY: a plain allocation, checked for null below.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if memory.is_null() {
        return false;
    }
    // SAFETY: `memory` is the movable block just allocated.
    let target = unsafe { GlobalLock(memory) };
    if target.is_null() {
        // SAFETY: still ours; freed exactly once.
        unsafe { GlobalFree(memory) };
        return false;
    }
    // SAFETY: the block holds `bytes` bytes, as many as `wide` has; the two do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), target.cast::<u8>(), bytes);
    }
    // SAFETY: locked above, unlocked exactly once.
    unsafe { GlobalUnlock(memory) };
    // SAFETY: the clipboard is open and emptied by its owner; on success it owns `memory`.
    if unsafe { SetClipboardData(u32::from(CF_UNICODETEXT), memory) }.is_null() {
        // SAFETY: not taken by the clipboard, so still ours; freed exactly once.
        unsafe { GlobalFree(memory) };
        return false;
    }
    true
}
