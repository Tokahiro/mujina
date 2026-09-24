//! Locating, starting and focusing a launcher.

#[cfg(windows)]
mod foreground;
mod install;

#[cfg(windows)]
pub use foreground::{focus_game, focus_ui, launch};
pub use install::install_from_executable;
