//! A failed Win32 call: its name, its error code and Windows' text for the code, so that a
//! failure on a device can be diagnosed from its log.

use std::fmt;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GetLastError};
use windows_sys::Win32::System::Diagnostics::Debug::{
    FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS, FormatMessageW,
};

/// A Win32 call that failed, and the error code it reported: a `WIN32_ERROR`, an `LSTATUS` or an
/// `HRESULT` (as its bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Win32Error {
    pub call: &'static str,
    pub code: u32,
}

pub type Win32Result<T> = Result<T, Win32Error>;

/// US English, the language of Mujina's log and technical details.
const ENGLISH: u32 = 0x0409;

/// The last error of this thread, for a call that reports failure that way. Read it right after
/// the call: nearly every other call may change it.
pub fn last_error(call: &'static str) -> Win32Error {
    // SAFETY: plain call.
    let code = unsafe { GetLastError() };
    Win32Error { call, code }
}

/// The status a call returns directly (a `WIN32_ERROR` or an `LSTATUS`), as a result.
pub(crate) fn checked(call: &'static str, status: u32) -> Win32Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(Win32Error { call, code: status })
    }
}

impl Win32Error {
    /// Windows' text for the code: in English, the log's language, else in the user's language.
    /// `None` if Windows has no text for it.
    pub fn message(&self) -> Option<String> {
        let mut buffer = [0u16; 512];
        // A language asked for by its id is the only one looked up; 0 lets Windows choose.
        let length = [ENGLISH, 0]
            .into_iter()
            .map(|language| self.format(language, &mut buffer))
            .find(|&length| length > 0)?;
        let text = String::from_utf16_lossy(buffer.get(..length)?);
        // System texts end in a full stop and a line break, and some span several lines.
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = text.trim_end_matches('.');
        (!text.is_empty()).then(|| text.to_string())
    }

    /// Writes the system's text for the code in `language` to `buffer`; its length, 0 if none.
    fn format(&self, language: u32, buffer: &mut [u16]) -> usize {
        // SAFETY: `buffer` is writable for the length passed; with IGNORE_INSERTS no arguments
        // are read, and with FROM_SYSTEM the source is unused.
        let length = unsafe {
            FormatMessageW(
                FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                std::ptr::null(),
                self.code,
                language,
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(0),
                std::ptr::null(),
            )
        };
        length as usize
    }
}

impl fmt::Display for Win32Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed with error ", self.call)?;
        // An HRESULT reads better in hex, the way Microsoft lists them; its top bit marks failure.
        if self.code & 0x8000_0000 == 0 {
            write!(f, "{}", self.code)?;
        } else {
            write!(f, "{:#010x}", self.code)?;
        }
        match self.message() {
            Some(message) => write!(f, ": {message}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for Win32Error {}

#[cfg(test)]
mod tests {
    use super::*;

    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_INVALID_HANDLE, SetLastError,
    };

    // Windows may lack English texts, so the words are checked only where it has them.

    #[test]
    fn the_text_is_english_where_windows_has_it() {
        let error = Win32Error {
            call: "Test",
            code: ERROR_FILE_NOT_FOUND,
        };
        if error.format(ENGLISH, &mut [0u16; 512]) == 0 {
            return;
        }
        assert_eq!(
            error.message().as_deref(),
            Some("The system cannot find the file specified")
        );
    }

    #[test]
    fn a_status_is_a_result() {
        assert_eq!(checked("RegCloseKey", ERROR_SUCCESS), Ok(()));
        assert_eq!(
            checked("RegCloseKey", ERROR_INVALID_HANDLE),
            Err(Win32Error {
                call: "RegCloseKey",
                code: ERROR_INVALID_HANDLE,
            })
        );
    }

    #[test]
    fn a_known_code_is_named_and_explained() {
        let error = Win32Error {
            call: "RegOpenKeyExW",
            code: ERROR_FILE_NOT_FOUND,
        };
        let text = error.to_string();
        assert!(
            text.starts_with("RegOpenKeyExW failed with error 2: "),
            "{text}"
        );
        let message = error.message().unwrap();
        assert!(!message.ends_with('.'), "{message}");
        assert!(!message.contains(['\r', '\n']), "{message}");
        assert!(text.ends_with(&message), "{text}");
    }

    #[test]
    fn a_code_without_text_is_just_the_number() {
        // Bit 29 marks codes that applications define; Windows has no text for them.
        let error = Win32Error {
            call: "Test",
            code: 0x2000_0001,
        };
        assert_eq!(error.message(), None);
        assert_eq!(error.to_string(), "Test failed with error 536870913");
    }

    #[test]
    fn an_hresult_is_shown_in_hex() {
        let error = Win32Error {
            call: "Test",
            code: 0xA000_0001,
        };
        assert!(
            error
                .to_string()
                .starts_with("Test failed with error 0xa0000001"),
            "{error}"
        );
    }

    #[test]
    fn last_error_reads_the_thread_s_code() {
        // SAFETY: plain call.
        unsafe { SetLastError(ERROR_INVALID_HANDLE) };
        assert_eq!(
            last_error("CloseHandle"),
            Win32Error {
                call: "CloseHandle",
                code: ERROR_INVALID_HANDLE,
            }
        );
    }
}
