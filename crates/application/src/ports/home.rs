//! The home role: starting the agent, being activated by Windows, covering the screen while the
//! launcher starts, and the per-user home app setting.

use super::PortResult;

// --- Agent and home ---------------------------------------------------------------------------

pub trait AgentControl {
    /// Starts the resident agent unless it is already running.
    fn ensure_running(&self) -> PortResult<()>;

    /// Tells a running agent that the launcher was just started, or switched to its console UI,
    /// so that it looks for the launcher's process again. Nothing noticeable happens without an
    /// agent. Must not block.
    fn launcher_started(&self);
}

/// Not the [`HomeActivation`](crate::home::HomeActivation) use case: this asks Windows to start
/// the home role, which then runs that use case.
pub trait HomeActivator {
    /// Has the system activate the home role, as if the user had pressed the home button. Only a
    /// process activated by the system may take the foreground, which the agent is not.
    fn activate_home(&self) -> PortResult<()>;
    /// Has the system activate the home role to bring the running game back to the front. In
    /// the console experience the shell takes the foreground back from a window that a
    /// background process such as the agent brought forward.
    fn activate_game(&self) -> PortResult<()>;
}

/// What the user looks at while the launcher starts.
pub trait LaunchScreen {
    /// Covers the screen. Called before the launcher is started.
    fn show(&self);
    /// Waits until `ready` reports true or `timeout` passes, checking `ready` whenever a window
    /// appears or comes to the front. Returns whether it got ready. The screen stays up.
    fn hold_until(&self, ready: &dyn Fn() -> bool, timeout: std::time::Duration) -> bool;
    /// Puts the screen above every other window. For the hand-over to the launcher only: a
    /// window exists before it has painted, and what lies behind it shows through until then.
    /// Whatever the launcher wants to show is hidden while this is in effect.
    fn raise(&self) {}
    /// Uncovers the screen.
    fn close(&self);
}

// --- Home app registration --------------------------------------------------------------------

/// Identity of the running package, if the process is packaged at all.
pub trait PackageIdentity {
    /// The application user model id Windows uses to activate this app.
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
