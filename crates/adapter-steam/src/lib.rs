//! Steam adapter: everything Mujina knows about Steam lives in this crate.
//!
//! Steam's window class, registry layout and command line are not a public API. When Valve
//! changes one of them, this crate is the only place that has to follow.
//!
//! [`DESCRIPTOR`] says what Steam is to the configuration and [`library`] where Steam installed a
//! game; both are built everywhere, so their rules are tested on Linux too. The rest needs
//! Windows.

mod descriptor;
pub mod library;
pub mod wifi;

#[cfg(windows)]
mod big_picture;
#[cfg(windows)]
pub mod cdp;
#[cfg(windows)]
pub mod checks;
#[cfg(windows)]
pub mod games;
#[cfg(windows)]
pub mod indicator;
#[cfg(windows)]
pub mod marker;
#[cfg(windows)]
pub mod navigation;
#[cfg(windows)]
pub mod registry_keys;
#[cfg(windows)]
mod runtime;
#[cfg(windows)]
pub mod shortcuts;
#[cfg(windows)]
mod state;
#[cfg(windows)]
pub mod window_rule;
#[cfg(windows)]
mod wlan;

#[cfg(windows)]
pub use big_picture::SteamBigPicture;
pub use descriptor::{DESCRIPTOR, SteamDescriptor, SteamOptions};

/// Steam Big Picture, as `crates/app/src/registry.rs` lists it.
#[cfg(windows)]
pub static PLUGIN: mujina_adapter_kit::plugin::LauncherPlugin =
    mujina_adapter_kit::plugin::LauncherPlugin {
        descriptor: &DESCRIPTOR,
        runtime: &runtime::RUNTIME,
    };
