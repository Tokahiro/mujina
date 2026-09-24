//! A client of the WLAN service: its handle, change notifications and interface queries.
//!
//! The service calls back on its own thread with a context the client owns. The client frees it
//! only after the service let go: unregistering and closing the handle each wait for a running
//! callback (see WlanRegisterNotification and WlanCloseHandle).

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{ERROR_INVALID_DATA, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::NetworkManagement::WiFi::{
    L2_NOTIFICATION_DATA, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO,
    WLAN_INTERFACE_INFO_LIST, WLAN_NOTIFICATION_SOURCE_NONE, WlanCloseHandle, WlanEnumInterfaces,
    WlanFreeMemory, WlanOpenHandle, WlanQueryInterface, WlanRegisterNotification,
    wlan_interface_state_connected, wlan_intf_opcode_current_connection,
};
use windows_sys::core::GUID;

use crate::error::{Win32Error, Win32Result, checked};

/// A notification from the WLAN service, valid for the callback that receives it.
#[derive(Debug, Clone, Copy)]
pub struct Notification<'a> {
    /// One of the `WLAN_NOTIFICATION_SOURCE_*` values.
    pub source: u32,
    /// What happened; its meaning depends on the source.
    pub code: u32,
    /// The data that comes with it, if any; its layout depends on source and code.
    pub data: &'a [u8],
}

#[derive(Clone, Copy)]
pub struct Interface {
    pub guid: GUID,
    pub connected: bool,
}

/// The connection of an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The network name as the access point sends it: up to 32 bytes, not necessarily UTF-8.
    pub ssid: Vec<u8>,
    /// 0 to 100.
    pub signal_quality: u32,
}

/// What the service calls, behind one thin pointer that can travel as the callback's context.
struct Handler(Box<dyn Fn(&Notification<'_>) + Send + Sync>);

unsafe extern "system" fn on_notification(data: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    // SAFETY: the service passes a valid notification for the duration of the callback (null is
    // tolerated for robustness).
    let Some(data) = (unsafe { data.as_ref() }) else {
        return;
    };
    let bytes = if data.pData.is_null() {
        &[][..]
    } else {
        // SAFETY: `pData` points to the `dwDataSize` bytes of the notification's data, valid for
        // the duration of the callback.
        unsafe { std::slice::from_raw_parts(data.pData.cast::<u8>(), data.dwDataSize as usize) }
    };
    // SAFETY: the context is the handler of the client that registered this callback, which
    // keeps it until the service has let go of it (see `Drop for WlanClient`).
    let handler = unsafe { &*context.cast::<Handler>() };
    (handler.0)(&Notification {
        source: data.NotificationSource,
        code: data.NotificationCode,
        data: bytes,
    });
}

/// A session with the WLAN service, closed when dropped.
pub struct WlanClient {
    handle: HANDLE,
    /// The context handed to the service with the last registration, whether that went through
    /// or not, until the service let go of it. Behind an `Arc`, so its address stays put while
    /// the client moves.
    handler: Option<Arc<Handler>>,
}

impl WlanClient {
    /// Opens a session (client version 2, Vista and later).
    pub fn open() -> Win32Result<Self> {
        let mut negotiated: u32 = 0;
        let mut handle: HANDLE = null_mut();
        // SAFETY: the reserved pointer is null; both out parameters are writable.
        let status = unsafe { WlanOpenHandle(2, null(), &raw mut negotiated, &raw mut handle) };
        checked("WlanOpenHandle", status)?;
        Ok(Self {
            handle,
            handler: None,
        })
    }

    /// Has `handler` called for every notification from `sources` (`WLAN_NOTIFICATION_SOURCE_*`
    /// flags), duplicates left out, until the client is dropped. It runs on a service thread and
    /// must be quick; it must not use this client, whose drop waits for it. Replaces any earlier
    /// handler, also a refused one: after a refusal, call again with fewer sources.
    pub fn notify(
        &mut self,
        sources: u32,
        handler: impl Fn(&Notification<'_>) + Send + Sync + 'static,
    ) -> Win32Result<()> {
        // Also after a refusal: a refused registration leaves the one before it in place, with
        // its context. Unregistering with nothing registered succeeds (both seen on build 26200).
        self.unregister()?;
        // Kept even if registering fails: Microsoft does not say that a failed registration
        // holds no context, so the handler is freed only once the service confirms it let go.
        let context = Arc::as_ptr(self.handler.insert(Arc::new(Handler(Box::new(handler)))));
        // SAFETY: valid client handle; the callback matches WLAN_NOTIFICATION_CALLBACK; the
        // context is the handler stored in `self` until the service let go of it (see
        // `unregister` and `Drop`); the reserved and previous-sources pointers are null.
        let status = unsafe {
            WlanRegisterNotification(
                self.handle,
                sources,
                1,
                Some(on_notification),
                context.cast(),
                null(),
                null_mut(),
            )
        };
        checked("WlanRegisterNotification", status)
    }

    /// Ends the notifications, if any. The handler is let go only once the service confirmed it.
    fn unregister(&mut self) -> Win32Result<()> {
        if self.handler.is_none() {
            return Ok(());
        }
        // SAFETY: valid client handle; no callback is needed to unregister; the reserved pointer
        // is null. It waits for a callback that is running.
        let status = unsafe {
            WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_NONE,
                0,
                None,
                null(),
                null(),
                null_mut(),
            )
        };
        // A name of its own, so that the log tells which step of `notify` failed.
        checked("WlanRegisterNotification (unregister)", status)?;
        self.handler = None;
        Ok(())
    }

    pub fn interfaces(&self) -> Win32Result<Vec<Interface>> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        // SAFETY: valid client handle; the reserved pointer is null; `list` receives memory the
        // WLAN API owns.
        let status = unsafe { WlanEnumInterfaces(self.handle, null(), &raw mut list) };
        checked("WlanEnumInterfaces", status)?;
        let list = WlanMemory::new(list, "WlanEnumInterfaces")?;
        // SAFETY: on success `list` points to a list allocated by the WLAN API whose
        // `InterfaceInfo` holds `dwNumberOfItems` contiguous entries; it is freed only after the
        // last use below, when `list` is dropped.
        let entries = unsafe { interfaces(list.0) };
        Ok(entries
            .iter()
            .map(|entry| Interface {
                guid: entry.InterfaceGuid,
                connected: entry.isState == wlan_interface_state_connected,
            })
            .collect())
    }

    /// The current connection of an interface. Since Windows 11 24H2 this needs the location
    /// permission: without it the code is ERROR_ACCESS_DENIED, and the first call may show the
    /// consent prompt and block until the user answers.
    pub fn current_connection(&self, interface: &GUID) -> Win32Result<Connection> {
        const CALL: &str = "WlanQueryInterface";
        // Written, but not needed: the opcode says what the data is.
        let mut size: u32 = 0;
        let mut data: *mut c_void = null_mut();
        // SAFETY: valid client handle and interface GUID; the reserved pointer is null; `data`
        // receives memory the WLAN API owns.
        let status = unsafe {
            WlanQueryInterface(
                self.handle,
                interface,
                wlan_intf_opcode_current_connection,
                null(),
                &raw mut size,
                &raw mut data,
                null_mut(),
            )
        };
        checked(CALL, status)?;
        let data = WlanMemory::new(data.cast::<WLAN_CONNECTION_ATTRIBUTES>(), CALL)?;
        // SAFETY: for this opcode `data` points to a WLAN_CONNECTION_ATTRIBUTES, valid until
        // `data` is dropped.
        let attributes = unsafe { &*data.0 };
        let association = &attributes.wlanAssociationAttributes;
        let length = (association.dot11Ssid.uSSIDLength as usize).min(32);
        Ok(Connection {
            ssid: association.dot11Ssid.ucSSID[..length].to_vec(),
            signal_quality: association.wlanSignalQuality,
        })
    }
}

impl Drop for WlanClient {
    fn drop(&mut self) {
        let unregistered = self.unregister().is_ok();
        // SAFETY: the handle came from WlanOpenHandle and is closed exactly once; the reserved
        // pointer is null. Closing undoes a registration too, and waits for a callback that is
        // running.
        let closed = unsafe { WlanCloseHandle(self.handle, null()) } == ERROR_SUCCESS;
        // If neither went through, the service may still call back: the handler stays for good.
        if !unregistered && !closed {
            std::mem::forget(self.handler.take());
        }
    }
}

/// Memory the WLAN API allocated for an answer, freed when dropped.
struct WlanMemory<T>(*mut T);

impl<T> WlanMemory<T> {
    /// A call that succeeded without an answer is treated as one whose answer is invalid.
    fn new(pointer: *mut T, call: &'static str) -> Win32Result<Self> {
        if pointer.is_null() {
            return Err(Win32Error {
                call,
                code: ERROR_INVALID_DATA,
            });
        }
        Ok(Self(pointer))
    }
}

impl<T> Drop for WlanMemory<T> {
    fn drop(&mut self) {
        // SAFETY: the memory was allocated by the WLAN API and is released exactly once.
        unsafe { WlanFreeMemory(self.0.cast()) };
    }
}

/// The entries of an interface list. `InterfaceInfo` is declared with one element but holds
/// `dwNumberOfItems`, so the pointer is taken from the field's place: going through a reference
/// to the declared one-element array (`.as_ptr()`) would only grant access to the first entry.
///
/// # Safety
/// `list` points to a list whose `InterfaceInfo` holds `dwNumberOfItems` initialised entries,
/// all in one allocation that stays valid and unchanged for `'a`.
unsafe fn interfaces<'a>(list: *const WLAN_INTERFACE_INFO_LIST) -> &'a [WLAN_INTERFACE_INFO] {
    // SAFETY: `list` is valid for reads (see above); `&raw const` creates no reference.
    let (first, count) = unsafe {
        (
            (&raw const (*list).InterfaceInfo).cast::<WLAN_INTERFACE_INFO>(),
            (*list).dwNumberOfItems as usize,
        )
    };
    // SAFETY: the caller guarantees `count` entries from `first` within one allocation.
    unsafe { std::slice::from_raw_parts(first, count) }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use windows_sys::Win32::NetworkManagement::WiFi::WLAN_NOTIFICATION_SOURCE_ACM;

    /// A list as the WLAN API lays it out, with a second entry right behind the declared one.
    #[repr(C)]
    struct TwoInterfaces {
        list: WLAN_INTERFACE_INFO_LIST,
        second: WLAN_INTERFACE_INFO,
    }

    /// Checks layout and count only: it would also pass with `.as_ptr()`, whose aliasing fault
    /// only Miri finds (`cargo +nightly miri test -p mujina-winutil wlan::tests`).
    #[test]
    fn every_entry_of_an_interface_list_is_read() {
        assert_eq!(
            std::mem::offset_of!(TwoInterfaces, second),
            std::mem::offset_of!(WLAN_INTERFACE_INFO_LIST, InterfaceInfo)
                + size_of::<WLAN_INTERFACE_INFO>(),
            "the fake must be laid out like the real list"
        );
        // SAFETY: plain data for which all-zero is a valid value.
        let mut buffer: TwoInterfaces = unsafe { std::mem::zeroed() };
        buffer.list.dwNumberOfItems = 2;
        buffer.list.InterfaceInfo[0].isState = 7;
        buffer.second.isState = wlan_interface_state_connected;
        buffer.second.InterfaceGuid.data1 = 42;

        // SAFETY: `buffer` holds two initialised entries in one allocation and outlives the slice.
        let entries = unsafe { interfaces((&raw const buffer).cast()) };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].isState, 7);
        assert_eq!(entries[1].isState, wlan_interface_state_connected);
        assert_eq!(entries[1].InterfaceGuid.data1, 42);
    }

    /// Calls the callback the way the service does, with a handler as a client registers it.
    fn deliver(handler: &Arc<Handler>, source: u32, code: u32, data: &mut [u8]) {
        let mut notification = L2_NOTIFICATION_DATA {
            NotificationSource: source,
            NotificationCode: code,
            InterfaceGuid: GUID::from_u128(0),
            dwDataSize: u32::try_from(data.len()).unwrap(),
            pData: if data.is_empty() {
                null_mut()
            } else {
                data.as_mut_ptr().cast()
            },
        };
        // SAFETY: a valid notification whose data outlives the call, and a live handler as the
        // context.
        unsafe {
            on_notification(
                &raw mut notification,
                Arc::as_ptr(handler).cast_mut().cast(),
            );
        }
    }

    /// Only calls that need no location permission: no consent prompt on a developer's machine.
    #[test]
    fn a_client_keeps_its_handler_until_it_lets_go_of_it() {
        // CI runs on Windows Server, usually without the WLAN service.
        let Ok(mut client) = WlanClient::open() else {
            return;
        };
        client.interfaces().unwrap();
        let first = Arc::new(());
        let second = Arc::new(());
        let held = Arc::clone(&first);
        client
            .notify(WLAN_NOTIFICATION_SOURCE_ACM, move |_| {
                let _ = &held;
            })
            .unwrap();
        assert_eq!(Arc::strong_count(&first), 2);
        let held = Arc::clone(&second);
        client
            .notify(WLAN_NOTIFICATION_SOURCE_ACM, move |_| {
                let _ = &held;
            })
            .unwrap();
        assert_eq!(Arc::strong_count(&first), 1, "replaced, so let go");
        assert_eq!(Arc::strong_count(&second), 2);
        drop(client);
        assert_eq!(Arc::strong_count(&second), 1, "dropped, so let go");
    }

    /// A null handle, which every WLAN call refuses with ERROR_INVALID_PARAMETER: nothing ever
    /// says the service let go of the handler, so it is never freed.
    #[test]
    fn a_handler_is_kept_while_the_service_may_hold_it() {
        let mut client = WlanClient {
            handle: null_mut(),
            handler: None,
        };
        let held = Arc::new(());
        let captured = Arc::clone(&held);
        let error = client
            .notify(WLAN_NOTIFICATION_SOURCE_ACM, move |_| {
                let _ = &captured;
            })
            .unwrap_err();
        assert_eq!(error.call, "WlanRegisterNotification");
        assert_eq!(
            Arc::strong_count(&held),
            2,
            "kept although registering failed"
        );
        // Now there is a handler to unregister, which fails too, and says so by its name.
        let error = client
            .notify(WLAN_NOTIFICATION_SOURCE_ACM, |_| {})
            .unwrap_err();
        assert_eq!(error.call, "WlanRegisterNotification (unregister)");
        assert_eq!(Arc::strong_count(&held), 2, "still kept");
        drop(client);
        assert_eq!(
            Arc::strong_count(&held),
            2,
            "kept for good, as neither unregistering nor closing went through"
        );
    }

    /// Refused as documented (no callback for a source other than NONE); the handler `notify`
    /// would leave behind is put in place by hand.
    #[test]
    fn a_refused_registration_does_not_stand_in_the_way_of_the_next() {
        let Ok(mut client) = WlanClient::open() else {
            return;
        };
        let refused = Arc::new(());
        let held = Arc::clone(&refused);
        let context = Arc::as_ptr(client.handler.insert(Arc::new(Handler(Box::new(move |_| {
            let _ = &held;
        })))));
        // SAFETY: valid client handle; the context is the handler stored in the client, as
        // `notify` stores it; the reserved pointer is null.
        let status = unsafe {
            WlanRegisterNotification(
                client.handle,
                WLAN_NOTIFICATION_SOURCE_ACM,
                1,
                None,
                context.cast(),
                null(),
                null_mut(),
            )
        };
        assert_ne!(status, ERROR_SUCCESS, "refused without a callback");
        assert_eq!(Arc::strong_count(&refused), 2);

        let next = Arc::new(());
        let held = Arc::clone(&next);
        client
            .notify(WLAN_NOTIFICATION_SOURCE_ACM, move |_| {
                let _ = &held;
            })
            .unwrap();
        assert_eq!(Arc::strong_count(&refused), 1, "unregistered, so let go");
        assert_eq!(Arc::strong_count(&next), 2);
    }

    #[test]
    fn the_callback_hands_the_handler_the_notification_and_its_data() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handler = {
            let seen = Arc::clone(&seen);
            Arc::new(Handler(Box::new(move |notification| {
                seen.lock().unwrap().push((
                    notification.source,
                    notification.code,
                    notification.data.to_vec(),
                ));
            })))
        };
        deliver(&handler, 16, 8, &mut 77u32.to_ne_bytes());
        deliver(&handler, 8, 21, &mut []);
        assert_eq!(
            *seen.lock().unwrap(),
            [(16, 8, 77u32.to_ne_bytes().to_vec()), (8, 21, Vec::new())]
        );
    }
}
