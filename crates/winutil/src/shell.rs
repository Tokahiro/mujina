//! Asking the shell to open or run something, in a COM STA unless the thread is in the MTA.

use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

use crate::com;
use crate::error::{Win32Error, last_error};
use crate::wide::to_wide;
use crate::window::WindowHandle;

/// Why [`run_elevated`] did not run the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationError {
    /// The user declined the prompt or the credential entry (`ERROR_CANCELLED`).
    Declined,
    /// E.g. a policy that forbids elevation, or a missing file.
    Failed(Win32Error),
}

impl ElevationError {
    fn from_code(error: Win32Error) -> Self {
        if error.code == ERROR_CANCELLED {
            Self::Declined
        } else {
            Self::Failed(error)
        }
    }
}

/// Runs `program` as administrator (after Windows' prompt, in front of `owner`) and returns its
/// exit code. Blocks until it ends: call it from a thread that may wait.
pub fn run_elevated(
    program: &Path,
    arguments: &str,
    owner: Option<WindowHandle>,
) -> Result<u32, ElevationError> {
    let verb = to_wide("runas");
    let file = to_wide(&program.to_string_lossy());
    let parameters = to_wide(arguments);
    // SAFETY: SHELLEXECUTEINFOW is plain data for which all-zero is a valid value.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0);
    // NOASYNC: Microsoft asks for it on a thread without a message loop.
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.hwnd = owner.map_or(null_mut(), WindowHandle::raw);
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;
    let apartment = com::Apartment::sta();
    // SAFETY: `info` has its size and live NUL-terminated strings; Win32 validates the owner.
    let started = unsafe { ShellExecuteExW(&raw mut info) } != 0;
    // Read before anything else can change it.
    let failure = (!started).then(|| last_error("ShellExecuteExW"));
    // For the call only: the wait below pumps no messages, which an STA thread must.
    drop(apartment);
    if let Some(error) = failure {
        return Err(ElevationError::from_code(error));
    }
    if info.hProcess.is_null() {
        // Started through something without a process (DDE, say): nothing to wait for.
        return Err(ElevationError::Failed(Win32Error {
            call: "ShellExecuteExW",
            code: 0,
        }));
    }
    // SAFETY: a valid process handle, returned because of SEE_MASK_NOCLOSEPROCESS.
    unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
    let mut code: u32 = 0;
    // SAFETY: as above; `code` is writable.
    let read = unsafe { GetExitCodeProcess(info.hProcess, &raw mut code) } != 0;
    let failure = (!read).then(|| last_error("GetExitCodeProcess"));
    // SAFETY: the handle is ours to close, exactly once.
    unsafe { CloseHandle(info.hProcess) };
    match failure {
        None => Ok(code),
        Some(error) => Err(ElevationError::Failed(error)),
    }
}

/// Opens a URI (or file) the way a double-click would. The shell performs the activation, so
/// the target may take the foreground even when the caller could not.
pub fn open(uri: &str) -> bool {
    let verb = to_wide("open");
    let uri = to_wide(uri);
    let _apartment = com::Apartment::sta();
    // SAFETY: both strings are NUL-terminated and outlive the call; the rest may be null.
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            uri.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    // "If the function succeeds, it returns a value greater than 32."
    result as isize > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_cancelled_prompt_counts_as_declined() {
        let error = |code| Win32Error {
            call: "ShellExecuteExW",
            code,
        };
        assert_eq!(
            ElevationError::from_code(error(ERROR_CANCELLED)),
            ElevationError::Declined
        );
        // ERROR_ACCESS_DENIED, e.g. a policy that forbids elevation.
        assert_eq!(
            ElevationError::from_code(error(5)),
            ElevationError::Failed(error(5))
        );
    }
}
