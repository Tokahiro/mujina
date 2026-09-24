//! Starting the resident agent, and making sure there is only one.

use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::ptr::null;

use mujina_application::ports::{AgentControl, PortError, PortResult};
use mujina_winutil::wide::to_wide;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE,
};

use crate::launcher_signal;

/// Per-session names; what another logged-in user runs is none of our business.
const MUTEX_NAME: &str = r"Local\Mujina.agent";
const HOME_MUTEX_NAME: &str = r"Local\Mujina.home";
const SETTINGS_APP_MUTEX_NAME: &str = r"Local\Mujina.settings-app";

/// Argument that makes `mujina.exe` run as agent.
pub const AGENT_ARGUMENT: &str = "agent";

/// Held by the agent for as long as it runs.
pub struct AgentInstance {
    /// Holding the named mutex open is the claim; dropping it closes the mutex.
    _mutex: OwnedHandle,
}

/// Held by a home activation while it brings the launcher up. Windows activates the home app
/// again when it is not satisfied yet (seen three times during one boot); the extra activations
/// must not each start the launcher.
pub type HomeInstance = AgentInstance;

impl AgentInstance {
    /// Claims the agent role; `None` if another agent already runs in this session.
    pub fn claim() -> Option<Self> {
        Self::claim_named(MUTEX_NAME)
    }

    /// Claims the right to bring the launcher up; `None` while another activation is at it.
    pub fn claim_home() -> Option<Self> {
        Self::claim_named(HOME_MUTEX_NAME)
    }

    /// Claims the settings app's window; `None` if the app is open already.
    pub fn claim_settings_app() -> Option<Self> {
        Self::claim_named(SETTINGS_APP_MUTEX_NAME)
    }

    fn claim_named(name: &str) -> Option<Self> {
        let name = to_wide(name);
        // SAFETY: null security attributes are valid; `name` is NUL-terminated.
        let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return None;
        }
        // SAFETY: plain call, read immediately after the call it refers to.
        let taken = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        // SAFETY: a handle of our own, which CloseHandle closes; dropped at once if the name is
        // taken already.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        (!taken).then_some(Self { _mutex: handle })
    }
}

/// Whether an agent runs in this session. Only opens the agent's mutex, never creates it, so
/// asking cannot keep an agent from starting.
pub fn agent_is_running() -> bool {
    let name = to_wide(MUTEX_NAME);
    // SAFETY: `name` is NUL-terminated; failure is a null handle.
    let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: a handle of our own, which CloseHandle closes; it is closed again at once.
    drop(unsafe { OwnedHandle::from_raw_handle(handle) });
    true
}

/// Starts the agent as another instance of the running executable.
#[derive(Debug, Default)]
pub struct WindowsAgentControl;

impl AgentControl for WindowsAgentControl {
    fn ensure_running(&self) -> PortResult<()> {
        if agent_is_running() {
            return Ok(());
        }
        let executable = std::env::current_exe()
            .map_err(|error| PortError::Failed(format!("own executable: {error}")))?;
        std::process::Command::new(executable)
            .arg(AGENT_ARGUMENT)
            .spawn()
            .map(drop)
            .map_err(|error| PortError::Failed(format!("starting the agent: {error}")))
    }

    fn launcher_started(&self) {
        launcher_signal::notify();
    }
}
