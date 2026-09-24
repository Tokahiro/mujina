//! A launcher Mujina knows nothing about, driven entirely by the configuration: start an
//! executable, tell whether it is up, and bring its window to the front. Menus beyond `ESC`,
//! navigation, a network indicator and game detection need a dedicated adapter
//! (`docs/new-launcher.md`).

mod descriptor;
#[cfg(windows)]
mod runtime;

pub use descriptor::{DESCRIPTOR, GenericDescriptor, GenericLauncherConfig};
#[cfg(windows)]
pub use runtime::GenericLauncher;

/// The generic launcher, as `crates/app/src/registry.rs` lists it.
#[cfg(windows)]
pub static PLUGIN: mujina_adapter_kit::plugin::LauncherPlugin =
    mujina_adapter_kit::plugin::LauncherPlugin {
        descriptor: &DESCRIPTOR,
        runtime: &runtime::RUNTIME,
    };
