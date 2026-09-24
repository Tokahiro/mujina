//! Interfaces through which use cases reach the outside world, cut by who needs them (home role,
//! agent). What a launcher cannot do stays at the trait's default.

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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    /// Something that has to exist does not (launcher not installed, value not set, …).
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Failed(String),
}

pub type PortResult<T> = Result<T, PortError>;
