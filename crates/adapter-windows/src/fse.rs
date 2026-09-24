//! The Windows full screen experience ("Xbox mode") API, from the GDK's `gamingexperience.h`
//! (SDK 10.0.26100.3916+). Older Windows builds lack its API set, so it is bound at run time.

use std::ffi::c_void;
use std::os::windows::io::{AsHandle, BorrowedHandle};
use std::ptr::null_mut;
use std::sync::Arc;

use mujina_application::agent::AgentEvent;
use mujina_application::ports::{FseState, FullScreenExperience};
use mujina_winutil::error::{Win32Error, Win32Result};
use mujina_winutil::event::Event;
use mujina_winutil::library::{RawSymbol, SystemLibrary};
use mujina_winutil::wait::WaitSource;

const API_SET: &str = "api-ms-win-gaming-experience-l1-1-0.dll";

/// `BOOL IsGamingFullScreenExperienceActive(void)`
type IsActiveFn = unsafe extern "system" fn() -> i32;
/// `void CALLBACK GAMING_FULL_SCREEN_EXPERIENCE_CHANGE_CALLBACK(void* context)`
type ChangeCallback = unsafe extern "system" fn(context: *mut c_void);
/// `HRESULT RegisterGamingFullScreenExperienceChangeNotification(callback, context, registration*)`
type RegisterFn = unsafe extern "system" fn(ChangeCallback, *mut c_void, *mut *mut c_void) -> i32;
/// `void UnregisterGamingFullScreenExperienceChangeNotification(registration)`
type UnregisterFn = unsafe extern "system" fn(*mut c_void);

pub struct WindowsFse {
    is_active: Option<IsActiveFn>,
    /// Bound as a pair, so a registration is only made where it can be undone.
    notifications: Win32Result<(RegisterFn, UnregisterFn)>,
}

unsafe extern "system" fn on_change(context: *mut c_void) {
    // SAFETY: the context is the watch's event, which is never freed (see `Drop for FseWatch`).
    let changed = unsafe { &*context.cast::<Event>() };
    changed.set();
}

impl WindowsFse {
    pub fn bind() -> Self {
        let library = SystemLibrary::load(API_SET);
        let is_active = library
            .and_then(|library| library.symbol(c"IsGamingFullScreenExperienceActive"))
            .map(|raw| {
                // SAFETY: signature per gamingexperience.h; the library is never unloaded.
                unsafe { std::mem::transmute::<RawSymbol, IsActiveFn>(raw) }
            })
            .ok();
        let notifications = library.and_then(|library| {
            let register =
                library.symbol(c"RegisterGamingFullScreenExperienceChangeNotification")?;
            let unregister =
                library.symbol(c"UnregisterGamingFullScreenExperienceChangeNotification")?;
            // SAFETY: as above, signatures per gamingexperience.h.
            Ok(unsafe {
                (
                    std::mem::transmute::<RawSymbol, RegisterFn>(register),
                    std::mem::transmute::<RawSymbol, UnregisterFn>(unregister),
                )
            })
        });
        Self {
            is_active,
            notifications,
        }
    }

    /// Signals the watch's event on every switch, until the watch is dropped.
    pub fn watch(&self) -> Win32Result<FseWatch> {
        let (register, unregister) = self.notifications?;
        let changed = Arc::new(Event::new()?);
        let mut registration: *mut c_void = null_mut();
        // SAFETY: documented signature; the context is the event, never freed (see `FseWatch`).
        let result = unsafe {
            register(
                on_change,
                Arc::as_ptr(&changed).cast_mut().cast(),
                &raw mut registration,
            )
        };
        if result < 0 {
            // The GDK does not say that a failed registration keeps no pointer to the context.
            std::mem::forget(changed);
            return Err(Win32Error {
                call: "RegisterGamingFullScreenExperienceChangeNotification",
                code: result.cast_unsigned(),
            });
        }
        Ok(FseWatch {
            registration,
            unregister,
            changed,
        })
    }
}

pub struct FseWatch {
    registration: *mut c_void,
    unregister: UnregisterFn,
    /// The callback's context. Behind an `Arc`, so its address stays put while the watch moves;
    /// never freed, not even after the registration is undone (see `drop`).
    changed: Arc<Event>,
}

impl FseWatch {
    /// Signalled on every switch; ask [`FullScreenExperience::state`] what it switched to.
    pub fn changed(&self) -> &Event {
        &self.changed
    }
}

impl Drop for FseWatch {
    fn drop(&mut self) {
        // SAFETY: documented signature; `registration` is a successful one, undone exactly once.
        unsafe { (self.unregister)(self.registration) };
        // The GDK does not say whether unregistering waits for a running callback, and a second
        // switch may be under way. So the event is leaked: one handle, as the agent watches once.
        std::mem::forget(Arc::clone(&self.changed));
    }
}

impl FullScreenExperience for WindowsFse {
    fn state(&self) -> FseState {
        match self.is_active {
            None => FseState::Unavailable,
            // SAFETY: bound in `bind` with the documented signature; takes no arguments.
            Some(is_active) => match unsafe { is_active() } {
                0 => FseState::Inactive,
                _ => FseState::Active,
            },
        }
    }
}

pub struct FseSource<'a> {
    fse: &'a WindowsFse,
    watch: FseWatch,
}

impl<'a> FseSource<'a> {
    pub fn new(fse: &'a WindowsFse, watch: FseWatch) -> Self {
        Self { fse, watch }
    }
}

impl WaitSource<AgentEvent> for FseSource<'_> {
    fn name(&self) -> &'static str {
        "Xbox mode"
    }

    fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
        Some(self.watch.changed().as_handle())
    }

    /// The event is auto-reset, so the wait has taken its signal already.
    fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
        out.push(AgentEvent::FseChanged(self.fse.state()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_never_panics_and_a_test_runner_is_not_in_xbox_mode() {
        assert_ne!(WindowsFse::bind().state(), FseState::Active);
    }

    #[test]
    fn a_watch_is_made_and_undone_where_the_api_exists() {
        let fse = WindowsFse::bind();
        match (fse.watch(), fse.notifications) {
            (Ok(watch), _) => drop(watch),
            (Err(error), Err(missing)) => assert_eq!(error, missing),
            // E.g. a Windows Server without the gaming parts.
            (Err(error), Ok(_)) => eprintln!("the API is there, but: {error}"),
        }
    }

    #[test]
    fn where_changes_can_be_registered_they_can_be_unregistered() {
        let Ok(library) = SystemLibrary::load(API_SET) else {
            return;
        };
        let register = library.symbol(c"RegisterGamingFullScreenExperienceChangeNotification");
        let unregister = library.symbol(c"UnregisterGamingFullScreenExperienceChangeNotification");
        assert_eq!(register.is_ok(), unregister.is_ok());
    }
}
