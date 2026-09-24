//! Wall-clock time for log lines.

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// Local time as `YYYY-MM-DD HH:MM:SS.mmm`.
pub fn local_timestamp() -> String {
    let mut now = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: `now` is a valid, writable SYSTEMTIME.
    unsafe { GetLocalTime(&raw mut now) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn has_the_documented_shape() {
        let stamp = super::local_timestamp();
        assert_eq!(stamp.len(), 23, "{stamp}");
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], " ");
    }
}
