//! The home role: starting the agent, being activated by Windows, covering the screen while the
//! launcher starts, and the per-user home app setting.

use super::PortResult;

pub trait AgentControl {
    fn ensure_running(&self) -> PortResult<()>;

    /// Tells a running agent that the launcher was started or switched to its console UI, so it
    /// looks for the launcher's process again. Must not block.
    fn launcher_started(&self);
}

/// Asks Windows to start the home role, which runs [`HomeActivation`](crate::home::HomeActivation).
pub trait HomeActivator {
    /// As if the user had pressed the home button. Only a process the system activated may take
    /// the foreground, and the agent is not one.
    fn activate_home(&self) -> PortResult<()>;
    /// Brings the running game back to the front through the home role: in Xbox mode the shell
    /// takes the foreground back from a window a background process such as the agent raised.
    fn activate_game(&self) -> PortResult<()>;
}

/// What the user looks at while the launcher starts.
pub trait LaunchScreen {
    fn show(&self);
    /// Waits until `ready` is true (checked whenever a window appears or comes to the front) or
    /// `timeout` passes; returns whether it got ready. The screen stays up.
    fn hold_until(&self, ready: &dyn Fn() -> bool, timeout: std::time::Duration) -> bool;
    /// Puts the screen above every window, for the hand-over only: the launcher's window exists
    /// before it paints, and until then what lies behind it shows through. Hides the launcher too.
    fn raise(&self) {}
    fn close(&self);
}

pub trait PackageIdentity {
    /// `None` when the process is not packaged.
    fn app_user_model_id(&self) -> Option<String>;
}

/// The per-user "which app is the console home" setting.
pub trait HomeAppRegistry {
    fn current(&self) -> PortResult<Option<String>>;
    fn set(&self, app_user_model_id: &str) -> PortResult<()>;
    fn clear(&self) -> PortResult<()>;
    /// The home app that was configured before Mujina took over.
    fn backup(&self) -> PortResult<Option<String>>;
    fn set_backup(&self, app_user_model_id: Option<&str>) -> PortResult<()>;
}
