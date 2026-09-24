//! Kernel events that wake the main loop. Give an OS callback a pointer to a live [`Event`],
//! never its raw handle: a closed handle's value is reused and could signal another object.

use std::ffi::c_void;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::ptr::null;

use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForSingleObject,
};

use crate::error::{Win32Error, Win32Result, last_error};
use crate::wide::to_wide;

#[derive(Debug)]
pub struct Event(OwnedHandle);

impl Event {
    /// An unnamed auto-reset event, initially unsignalled.
    pub fn new() -> Win32Result<Self> {
        Self::create(false, None)
    }

    /// Shared by every process of this session that uses the name; created if it does not exist.
    pub fn named_auto_reset(name: &str) -> Win32Result<Self> {
        Self::create(false, Some(name))
    }

    /// Like [`Event::named_auto_reset`], but stays signalled until [`Event::reset`].
    pub fn named_manual_reset(name: &str) -> Win32Result<Self> {
        Self::create(true, Some(name))
    }

    fn create(manual_reset: bool, name: Option<&str>) -> Win32Result<Self> {
        let wide = name.map(to_wide);
        let name = wide.as_ref().map_or(null(), Vec::as_ptr);
        // SAFETY: null attributes are valid; `name` is null or NUL-terminated and outlives it.
        let handle = unsafe { CreateEventW(null(), i32::from(manual_reset), 0, name) };
        if handle.is_null() {
            return Err(last_error("CreateEventW"));
        }
        // SAFETY: a new handle of our own, which CloseHandle closes.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    pub fn reset(&self) {
        // SAFETY: valid event handle owned by `self`.
        unsafe { ResetEvent(self.as_raw()) };
    }

    pub fn set(&self) {
        // SAFETY: valid event handle owned by `self`.
        unsafe { SetEvent(self.as_raw()) };
    }

    /// Blocks until the event is signalled; an auto-reset event is unsignalled again after.
    pub fn wait(&self) -> Win32Result<()> {
        // SAFETY: valid event handle owned by `self`.
        match unsafe { WaitForSingleObject(self.as_raw(), INFINITE) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_FAILED => Err(last_error("WaitForSingleObject")),
            other => Err(Win32Error {
                call: "WaitForSingleObject",
                code: other,
            }),
        }
    }

    /// The raw handle, for wait functions that take an array of them. Valid while `self` is.
    pub fn as_raw(&self) -> *mut c_void {
        self.0.as_raw_handle()
    }
}

impl AsHandle for Event {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::thread;

    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;

    /// Does not wait; consumes an auto-reset signal.
    fn is_signalled(event: &Event) -> bool {
        // SAFETY: valid event handle owned by `event`.
        match unsafe { WaitForSingleObject(event.as_raw(), 0) } {
            WAIT_OBJECT_0 => true,
            WAIT_TIMEOUT => false,
            other => panic!("wait failed: {other:#x}"),
        }
    }

    fn unique_name() -> String {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        format!(
            r"Local\MujinaTests.event.{}.{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn one_wait_consumes_one_signal() {
        let event = Event::new().unwrap();
        assert!(!is_signalled(&event));
        event.set();
        event.wait().unwrap();
        assert!(!is_signalled(&event), "auto-reset after the wait");
        event.set();
        event.set();
        assert!(is_signalled(&event));
        assert!(!is_signalled(&event), "two signals are one");
    }

    #[test]
    fn another_thread_wakes_a_waiter() {
        let event = Arc::new(Event::new().unwrap());
        let setter = {
            let event = Arc::clone(&event);
            thread::spawn(move || event.set())
        };
        event.wait().unwrap();
        setter.join().unwrap();
    }

    #[test]
    fn a_named_event_is_shared_by_its_name() {
        let name = unique_name();
        let listener = Event::named_auto_reset(&name).unwrap();
        Event::named_auto_reset(&name).unwrap().set();
        assert!(is_signalled(&listener));
        assert!(!is_signalled(&listener));
    }

    #[test]
    fn a_manual_reset_event_stays_signalled_until_reset() {
        let event = Event::named_manual_reset(&unique_name()).unwrap();
        event.set();
        assert!(is_signalled(&event));
        assert!(is_signalled(&event));
        event.reset();
        assert!(!is_signalled(&event));
    }

    #[test]
    fn the_borrowed_handle_is_the_event_s() {
        let event = Event::new().unwrap();
        assert_eq!(event.as_handle().as_raw_handle(), event.as_raw());
    }
}
