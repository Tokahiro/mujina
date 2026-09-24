//! Run-time binding to system libraries that may not exist on every Windows build, and keeping
//! a process to the system directory for its libraries and helper programs.

use std::ffi::{CStr, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW, SetDefaultDllDirectories,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

use crate::error::{Win32Result, last_error};
use crate::wide::to_wide;

/// Makes this process look for DLLs loaded by name only in the system directory, never in its
/// own or the current folder, where a planted DLL could wait. Call it first thing in `main`.
/// False if Windows refused.
pub fn search_only_system32() -> bool {
    // SAFETY: plain call with a documented flag.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) != 0 }
}

/// The system directory, e.g. `C:\Windows\System32`: where a program started by its full path
/// is Windows' own and not one planted next to the caller.
pub fn system_directory() -> Option<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        let size = u32::try_from(buffer.len()).ok()?;
        // SAFETY: `buffer` is writable for `size` UTF-16 units.
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), size) } as usize;
        match length {
            0 => return None,
            // Too small: `length` is the size needed, terminator included.
            _ if length >= buffer.len() => buffer.resize(length, 0),
            _ => return Some(OsString::from_wide(&buffer[..length]).into()),
        }
    }
}

/// An exported function whose real signature the caller knows.
pub type RawSymbol = unsafe extern "system" fn() -> isize;

/// A system library that stays loaded for the rest of the process.
///
/// It is never unloaded on purpose: function pointers obtained from it are handed out as plain
/// `fn` values, and those must stay valid without a lifetime tying them to this struct.
#[derive(Debug, Clone, Copy)]
pub struct SystemLibrary(HMODULE);

impl SystemLibrary {
    /// Loads `name` from the system directory (API-set names are resolved by the loader).
    pub fn load(name: &str) -> Win32Result<Self> {
        let name = to_wide(name);
        // SAFETY: `name` is NUL-terminated and outlives the call; the file handle parameter is
        // reserved and must be null.
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return Err(last_error("LoadLibraryExW"));
        }
        Ok(Self(module))
    }

    /// Looks up an export. The caller transmutes it to the documented signature.
    pub fn symbol(self, name: &CStr) -> Win32Result<RawSymbol> {
        // SAFETY: `self.0` is a module handle that is never freed; `name` is NUL-terminated.
        unsafe { GetProcAddress(self.0, name.as_ptr().cast()) }
            .ok_or_else(|| last_error("GetProcAddress"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_export_and_rejects_unknown() {
        use windows_sys::Win32::Foundation::{ERROR_MOD_NOT_FOUND, ERROR_PROC_NOT_FOUND};

        use crate::error::Win32Error;

        let kernel32 = SystemLibrary::load("kernel32.dll").unwrap();
        assert!(kernel32.symbol(c"GetTickCount").is_ok());
        assert_eq!(
            kernel32.symbol(c"MujinaDoesNotExist").unwrap_err(),
            Win32Error {
                call: "GetProcAddress",
                code: ERROR_PROC_NOT_FOUND,
            }
        );
        assert_eq!(
            SystemLibrary::load("mujina-no-such-library.dll").unwrap_err(),
            Win32Error {
                call: "LoadLibraryExW",
                code: ERROR_MOD_NOT_FOUND,
            }
        );
    }

    #[test]
    fn the_system_directory_holds_windows_own_programs() {
        let system = system_directory().unwrap();
        assert!(system.is_absolute());
        assert!(system.join("kernel32.dll").is_file());
    }
}
