//! Reading and writing single registry values.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS,
    ERROR_SUCCESS,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY, KEY_QUERY_VALUE, REG_DWORD,
    REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_THREAD_AGNOSTIC, REG_SZ,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegDeleteKeyValueW, RegDeleteKeyW, RegEnumValueW,
    RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, RegSetKeyValueW,
};

use crate::error::{Win32Error, Win32Result, checked};
use crate::event::Event;

use crate::wide::{from_wide, to_wide};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    fn handle(self) -> HKEY {
        match self {
            Self::CurrentUser => HKEY_CURRENT_USER,
            Self::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

/// The error of a registry call: the call and the code it returned.
pub type RegistryError = Win32Error;

/// Reads a `REG_SZ` value. A missing key or value is `Ok(None)`.
pub fn read_string(hive: Hive, key: &str, value: &str) -> Result<Option<String>, RegistryError> {
    let key = to_wide(key);
    let value = to_wide(value);
    let mut bytes: u32 = 0;
    // SAFETY: both strings are NUL-terminated and outlive the call; a null data pointer with a
    // valid size pointer asks only for the required size.
    let status = unsafe {
        RegGetValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &raw mut bytes,
        )
    };
    match status {
        ERROR_FILE_NOT_FOUND => return Ok(None),
        other => checked("RegGetValueW", other)?,
    }

    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
    let mut bytes = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
    // SAFETY: as above; `buffer` is writable for `bytes` bytes.
    let status = unsafe {
        RegGetValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut bytes,
        )
    };
    match status {
        ERROR_FILE_NOT_FOUND => Ok(None),
        other => checked("RegGetValueW", other).map(|()| Some(from_wide(&buffer))),
    }
}

/// Reads a `REG_DWORD` value. A missing key or value is `Ok(None)`.
pub fn read_u32(hive: Hive, key: &str, value: &str) -> Result<Option<u32>, RegistryError> {
    let key = to_wide(key);
    let value = to_wide(value);
    let mut data: u32 = 0;
    let mut bytes: u32 = 4;
    // SAFETY: both strings are NUL-terminated and outlive the call; `data` is writable for the
    // four bytes announced in `bytes`.
    let status = unsafe {
        RegGetValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut data).cast(),
            &raw mut bytes,
        )
    };
    match status {
        ERROR_FILE_NOT_FOUND => Ok(None),
        other => checked("RegGetValueW", other).map(|()| Some(data)),
    }
}

/// Writes a `REG_SZ` value, creating the key if necessary.
pub fn write_string(hive: Hive, key: &str, value: &str, data: &str) -> Result<(), RegistryError> {
    let key = to_wide(key);
    let value = to_wide(value);
    let data = to_wide(data);
    let bytes = u32::try_from(data.len() * 2).map_err(|_| Win32Error {
        call: "RegSetKeyValueW",
        code: ERROR_INVALID_PARAMETER,
    })?;
    // SAFETY: all strings are NUL-terminated and outlive the call; `data` is readable for
    // `bytes` bytes, terminator included as REG_SZ requires.
    let status = unsafe {
        RegSetKeyValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            bytes,
        )
    };
    checked("RegSetKeyValueW", status)
}

/// Writes a `REG_DWORD` value, creating the key if necessary.
pub fn write_u32(hive: Hive, key: &str, value: &str, data: u32) -> Result<(), RegistryError> {
    let key = to_wide(key);
    let value = to_wide(value);
    // SAFETY: both strings are NUL-terminated and outlive the call; `data` is readable for the
    // four bytes announced.
    let status = unsafe {
        RegSetKeyValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            REG_DWORD,
            (&raw const data).cast(),
            4,
        )
    };
    checked("RegSetKeyValueW", status)
}

/// Deletes a key that has no subkeys, with its values. Deleting what does not exist is not an
/// error.
pub fn delete_key(hive: Hive, key: &str) -> Result<(), RegistryError> {
    let key = to_wide(key);
    // SAFETY: the string is NUL-terminated and outlives the call.
    let status = unsafe { RegDeleteKeyW(hive.handle(), key.as_ptr()) };
    match status {
        ERROR_FILE_NOT_FOUND => Ok(()),
        other => checked("RegDeleteKeyW", other),
    }
}

/// Deletes a value. Deleting what does not exist is not an error.
pub fn delete_value(hive: Hive, key: &str, value: &str) -> Result<(), RegistryError> {
    let key = to_wide(key);
    let value = to_wide(value);
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let status = unsafe { RegDeleteKeyValueW(hive.handle(), key.as_ptr(), value.as_ptr()) };
    match status {
        ERROR_FILE_NOT_FOUND => Ok(()),
        other => checked("RegDeleteKeyValueW", other),
    }
}

/// Every `REG_SZ` value of a key, as name and data; values of other types are left out. A
/// missing key has none.
pub fn string_values(hive: Hive, key: &str) -> Result<Vec<(String, String)>, RegistryError> {
    let key = match OpenKey::open(hive, key, KEY_QUERY_VALUE) {
        Ok(key) => key,
        Err(error) if error.code == ERROR_FILE_NOT_FOUND => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut values = Vec::new();
    // Value names are at most 16,383 characters; the data buffer grows when a value needs more.
    let mut name = vec![0u16; 16_384];
    let mut data = vec![0u16; 261];
    let mut index: u32 = 0;
    loop {
        let mut name_length = u32::try_from(name.len()).unwrap_or(u32::MAX);
        let mut bytes = u32::try_from(data.len() * 2).unwrap_or(u32::MAX);
        let mut kind: u32 = 0;
        // SAFETY: the key is open with KEY_QUERY_VALUE; `name` is writable for `name_length`
        // characters and `data` for `bytes` bytes; the reserved pointer is null as required.
        let status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                name.as_mut_ptr(),
                &raw mut name_length,
                null(),
                &raw mut kind,
                data.as_mut_ptr().cast(),
                &raw mut bytes,
            )
        };
        match status {
            ERROR_SUCCESS => {
                if kind == REG_SZ {
                    // The data need not end in a NUL, so it is cut at the length reported.
                    let text = data.get(..bytes as usize / 2).unwrap_or_default();
                    let name = name.get(..name_length as usize).unwrap_or_default();
                    values.push((from_wide(name), from_wide(text)));
                }
                index += 1;
            }
            // `bytes` now says what this value needs; the same index again.
            ERROR_MORE_DATA => data.resize((bytes as usize).div_ceil(2) + 1, 0),
            ERROR_NO_MORE_ITEMS => return Ok(values),
            other => {
                return Err(Win32Error {
                    call: "RegEnumValueW",
                    code: other,
                });
            }
        }
    }
}

/// An open registry key, closed when dropped. Not a kernel handle: it is closed with
/// RegCloseKey, so `OwnedHandle` does not fit.
#[derive(Debug)]
struct OpenKey(HKEY);

impl OpenKey {
    /// A missing key is an error with the code ERROR_FILE_NOT_FOUND.
    fn open(hive: Hive, key: &str, access: u32) -> Win32Result<Self> {
        let path = to_wide(key);
        let mut handle: HKEY = null_mut();
        // SAFETY: `path` is NUL-terminated and outlives the call; `handle` is writable.
        let status =
            unsafe { RegOpenKeyExW(hive.handle(), path.as_ptr(), 0, access, &raw mut handle) };
        checked("RegOpenKeyExW", status).map(|()| Self(handle))
    }
}

impl Drop for OpenKey {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW and is closed exactly once.
        unsafe { RegCloseKey(self.0) };
    }
}

/// Signals an event when anything below a key changes. One-shot by nature: call
/// [`RegistryWatch::rearm`] after every signal.
#[derive(Debug)]
pub struct RegistryWatch {
    // Closed before the event, which it may still signal as it goes.
    key: OpenKey,
    event: Event,
}

impl RegistryWatch {
    /// Opens `key` for notification and arms the watch. A missing key is an error with the code
    /// ERROR_FILE_NOT_FOUND.
    pub fn open(hive: Hive, key: &str) -> Win32Result<Self> {
        let event = Event::new()?;
        let key = OpenKey::open(hive, key, KEY_NOTIFY)?;
        let watch = Self { key, event };
        watch.rearm()?;
        Ok(watch)
    }

    /// [`RegistryWatch::open`] for a caller that does not report why a key cannot be watched.
    pub fn new(hive: Hive, key: &str) -> Option<Self> {
        Self::open(hive, key).ok()
    }

    /// Arms the notification again. Thread-agnostic, so it survives the arming thread. A watch
    /// that could not be armed never signals again.
    pub fn rearm(&self) -> Win32Result<()> {
        let filter =
            REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC;
        // SAFETY: the key is open with KEY_NOTIFY and the event handle is valid; both live as
        // long as `self`.
        let status =
            unsafe { RegNotifyChangeKeyValue(self.key.0, 1, filter, self.event.as_raw(), 1) };
        checked("RegNotifyChangeKeyValue", status)
    }

    pub fn event(&self) -> &Event {
        &self.event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU32, Ordering};

    const KEY: &str = r"Software\MujinaTests\registry";

    /// A key of this test's own. Test runs of several checkouts may run at once, and on a shared
    /// key one run's writes and deletes would reach the other's reads and watches.
    fn unique_key(purpose: &str) -> String {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        format!(
            r"Software\MujinaTests\{purpose}.{}.{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn string_round_trip_and_delete() {
        let key = &unique_key("registry");
        assert_eq!(read_string(Hive::CurrentUser, key, "missing"), Ok(None));
        write_string(Hive::CurrentUser, key, "name", "狢 mujina").unwrap();
        assert_eq!(
            read_string(Hive::CurrentUser, key, "name"),
            Ok(Some("狢 mujina".to_string()))
        );
        delete_value(Hive::CurrentUser, key, "name").unwrap();
        delete_value(Hive::CurrentUser, key, "name").unwrap();
        assert_eq!(read_string(Hive::CurrentUser, key, "name"), Ok(None));
        delete_key(Hive::CurrentUser, key).unwrap();
    }

    #[test]
    fn a_watch_signals_a_change_and_can_be_armed_again() {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;

        let key = &unique_key("registry-watch");
        write_u32(Hive::CurrentUser, key, "count", 1).unwrap();
        let watch = RegistryWatch::open(Hive::CurrentUser, key).unwrap();
        for count in 2..4 {
            write_u32(Hive::CurrentUser, key, "count", count).unwrap();
            // SAFETY: valid event handle owned by `watch`; bounded, so a broken watch fails the
            // test instead of hanging it.
            let woken = unsafe { WaitForSingleObject(watch.event().as_raw(), 10_000) };
            assert_eq!(woken, WAIT_OBJECT_0);
            watch.rearm().unwrap();
        }
        drop(watch);
        delete_key(Hive::CurrentUser, key).unwrap();
    }

    #[test]
    fn a_missing_key_cannot_be_watched_and_says_so() {
        let error = RegistryWatch::open(Hive::CurrentUser, r"Software\MujinaTests\no-such-key")
            .unwrap_err();
        assert_eq!(
            error,
            Win32Error {
                call: "RegOpenKeyExW",
                code: ERROR_FILE_NOT_FOUND,
            }
        );
        assert!(
            RegistryWatch::new(Hive::CurrentUser, r"Software\MujinaTests\no-such-key").is_none()
        );
    }

    #[test]
    fn missing_dword_is_none() {
        assert_eq!(read_u32(Hive::CurrentUser, KEY, "no-such-dword"), Ok(None));
    }

    /// Read-only: Windows' own version key, which has both strings and numbers.
    #[test]
    fn string_values_lists_strings_only() {
        let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let values = string_values(Hive::LocalMachine, key).unwrap();
        let build = read_string(Hive::LocalMachine, key, "CurrentBuild").unwrap();
        assert!(build.is_some());
        assert!(values.contains(&("CurrentBuild".to_string(), build.unwrap_or_default())));
        assert!(
            read_u32(Hive::LocalMachine, key, "CurrentMajorVersionNumber")
                .is_ok_and(|n| n.is_some())
        );
        assert!(
            !values
                .iter()
                .any(|(name, _)| name == "CurrentMajorVersionNumber")
        );
        assert_eq!(
            string_values(Hive::CurrentUser, r"Software\MujinaTests\no-such-key"),
            Ok(Vec::new())
        );
    }
}
