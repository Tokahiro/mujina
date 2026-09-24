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

/// The translations of this crate's texts (the titles of its checks), by the language `lang/`
/// names them in: for Mujina Settings, which shows them.
pub const CATALOGS: &[(&str, &str)] = &[("de", include_str!("../lang/de.po"))];
