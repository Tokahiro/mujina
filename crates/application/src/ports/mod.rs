//! Ports: the interfaces through which use cases reach the outside world.
//!
//! They are small and cut by who needs them: the home role asks a [`HomeLauncher`], the agent a
//! [`SessionLauncher`] and the device's [`DeviceButtons`], and what a launcher cannot do it
//! leaves at the trait's default.
//!
//! One file per topic; everything is used from here, as `ports::HomeLauncher` and so on, so
//! moving a port between files changes no adapter.

pub mod absent;
mod device;
mod home;
mod input;
mod launcher;
mod system;

pub use device::DeviceButtons;
pub use home::{AgentControl, HomeActivator, HomeAppRegistry, LaunchScreen, PackageIdentity};
pub use input::KeySender;
pub use launcher::{
    Direct, GameWhereabouts, HomeLauncher, LauncherInstall, LauncherState, SessionLauncher,
};
pub use system::{ForegroundProbe, FseState, FullScreenExperience};

/// Failure of an operation behind a port.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    /// Something that has to exist does not (launcher not installed, value not set, …).
    #[error("not found: {0}")]
    NotFound(String),
    /// The operation was attempted and failed.
    #[error("{0}")]
    Failed(String),
}

pub type PortResult<T> = Result<T, PortError>;
