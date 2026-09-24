//! The console of the program that started a windowed one.

use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
};

/// For a Windows-subsystem program: sends output that goes nowhere to the parent's console, if
/// any; redirected output is left alone. Whether output now goes anywhere.
pub fn attach_parent() -> bool {
    // SAFETY: plain call; a null or invalid handle means there is no output yet.
    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if !output.is_null() && output != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return true;
    }
    // SAFETY: plain call; it fails if the parent has no console or this process has one.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}
