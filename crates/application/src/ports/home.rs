//! Ports around the console home: activation, the launch screen and the home app setting.

use super::PortResult;

pub trait AgentControl {
    fn ensure_running(&self) -> PortResult<()>;

    /// Tells a running agent to look for the launcher's process again. Must not block.
    fn launcher_started(&self);
}

/// Asks Windows to start the home role, which runs [`HomeActivation`](crate::home::HomeActivation).
pub trait HomeActivator {
    /// As if the user pressed the home button; the agent itself may not take the foreground.
    fn activate_home(&self) -> PortResult<()>;
    /// Brings the running game to the front; in Xbox mode the shell undoes a raise by the agent.
    fn activate_game(&self) -> PortResult<()>;
}

/// What the user looks at while the launcher starts.
pub trait LaunchScreen {
    fn show(&self);
    /// Waits until `ready` (checked when a window appears or comes to the front) or `timeout`;
    /// true if it got ready. The screen stays up.
    fn hold_until(&self, ready: &dyn Fn() -> bool, timeout: std::time::Duration) -> bool;
    /// Puts the screen above every window, the launcher's too; for the hand-over only.
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
