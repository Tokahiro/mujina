//! Locating, starting and focusing a launcher, with the foreground rules that are easy to get
//! wrong in every adapter again.

#[cfg(windows)]
mod foreground;
mod install;

#[cfg(windows)]
pub use foreground::{focus_game, focus_ui, launch};
pub use install::install_from_executable;
