//! Everything Mujina knows about Steam: window class, registry layout, command line, none of them
//! a public API.

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

#[cfg(windows)]
pub static PLUGIN: mujina_adapter_kit::plugin::LauncherPlugin =
    mujina_adapter_kit::plugin::LauncherPlugin {
        descriptor: &DESCRIPTOR,
        runtime: &runtime::RUNTIME,
    };
