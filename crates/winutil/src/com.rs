//! COM on the calling thread, for the Win32 calls that expect it.

use std::marker::PhantomData;
use std::ptr::null;

use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};

/// COM initialised on this thread as a single-threaded apartment, until dropped.
///
/// ShellExecute may hand its work to shell extensions that COM loads, and some of them need an
/// STA, so Microsoft asks for this before calling it; [`crate::shell`] holds one around each
/// call. An STA thread must pump messages while it waits, which a thread with windows or hooks
/// does anyway. A thread that calls ShellExecute often may hold one for its whole life: the
/// last drop closes COM on the thread and unloads the DLLs it loaded, which each call would
/// otherwise repeat.
#[must_use = "COM is uninitialised again when the apartment is dropped"]
#[derive(Debug)]
pub struct Apartment {
    /// CoUninitialize must be called on the thread that initialised COM.
    _this_thread: PhantomData<*const ()>,
}

impl Apartment {
    /// Initialises COM as recommended for ShellExecute. Also succeeds when the thread already
    /// is in an STA. The error is the HRESULT, for the log; RPC_E_CHANGED_MODE means the thread
    /// is in the multithreaded apartment, which stays as it is.
    pub fn sta() -> Result<Self, i32> {
        let mode = (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).cast_unsigned();
        // SAFETY: plain call; the reserved parameter must be null, and the flags are a valid
        // combination.
        let result = unsafe { CoInitializeEx(null(), mode) };
        // S_OK and S_FALSE ("already initialised") both need a matching CoUninitialize; a
        // failure, such as another mode set earlier, must not get one.
        if result >= 0 {
            Ok(Self {
                _this_thread: PhantomData,
            })
        } else {
            Err(result)
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful CoInitializeEx in `sta`, on the same thread, since an
        // `Apartment` can neither be sent nor shared.
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
    use windows_sys::Win32::System::Com::COINIT_MULTITHREADED;

    use super::*;

    /// Enters the multithreaded apartment, as other code on the thread might have done.
    fn enter_mta() -> i32 {
        // SAFETY: plain call; the reserved parameter must be null.
        unsafe { CoInitializeEx(null(), COINIT_MULTITHREADED.cast_unsigned()) }
    }

    #[test]
    fn an_apartment_can_be_entered_twice_on_one_thread() {
        // On a thread of its own, so no other test's COM state is involved.
        std::thread::spawn(|| {
            let outer = Apartment::sta().expect("a new thread can enter an STA");
            let inner = Apartment::sta().expect("the same mode again is fine");
            drop(inner);
            drop(outer);
            // Both were balanced, the S_FALSE one too: COM is closed, so any mode may follow.
            assert_eq!(enter_mta(), S_OK);
            // SAFETY: balances `enter_mta`, on the same thread.
            unsafe { CoUninitialize() };
        })
        .join()
        .unwrap();
    }

    #[test]
    fn a_thread_in_the_multithreaded_apartment_is_left_in_it() {
        std::thread::spawn(|| {
            assert_eq!(enter_mta(), S_OK);
            assert_eq!(Apartment::sta().unwrap_err(), RPC_E_CHANGED_MODE);
            // Still initialised: the failed attempt uninitialised nothing.
            assert_eq!(enter_mta(), S_FALSE);
            // SAFETY: balances both calls to `enter_mta`, on the same thread.
            unsafe {
                CoUninitialize();
                CoUninitialize();
            }
        })
        .join()
        .unwrap();
    }
}
