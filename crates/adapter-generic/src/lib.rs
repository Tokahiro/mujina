//! A launcher Mujina knows nothing about, driven entirely by the configuration.
//!
//! Good enough to boot into any full-screen frontend: locate and start an executable, tell
//! whether it is up, and bring its window to the front. Everything a generic launcher cannot
//! know — menus beyond `ESC`, navigation, a network indicator, game detection — stays at its
//! defaults; a launcher that deserves more gets a dedicated adapter (see `docs/new-launcher.md`).
//!
//! [`DESCRIPTOR`] and [`GenericLauncherConfig`] are built everywhere, so they are tested on
//! Linux too; the launcher at work needs Windows.

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
