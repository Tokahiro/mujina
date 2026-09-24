//! Windows adapter: implements the OS-facing ports of `mujina-application`.

#![cfg(windows)]

pub mod agent_control;
pub mod agent_loop;
pub mod checks;
pub mod foreground;
pub mod fse;
pub mod home_activator;
pub mod home_registry;
pub mod identity;
pub mod launch_screen;
pub mod launcher_signal;
pub mod log_file;
pub mod paths;
pub mod process_exit;
pub mod session_end;
pub mod settings_signal;
pub mod smbios;

/// Translations of this crate's texts (its checks), by language, for Mujina Settings.
pub const CATALOGS: &[(&str, &str)] = &[("de", include_str!("../lang/de.po"))];
