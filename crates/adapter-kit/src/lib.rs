//! Code that launcher adapters share and that speaks in ports. Adapters never depend on each
//! other, and `winutil` knows nothing about Mujina, so it lives here.

pub mod launcher;
#[cfg(windows)]
pub mod plugin;
