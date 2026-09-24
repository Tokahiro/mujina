//! A launcher Mujina knows nothing about, driven by the configuration. Anything beyond an `ESC`
//! menu needs a dedicated adapter (`docs/new-launcher.md`).

mod descriptor;
#[cfg(windows)]
mod runtime;

pub use descriptor::{DESCRIPTOR, GenericDescriptor, GenericLauncherConfig};
#[cfg(windows)]
pub use runtime::GenericLauncher;

#[cfg(windows)]
pub static PLUGIN: mujina_adapter_kit::plugin::LauncherPlugin =
    mujina_adapter_kit::plugin::LauncherPlugin {
        descriptor: &DESCRIPTOR,
        runtime: &runtime::RUNTIME,
    };
