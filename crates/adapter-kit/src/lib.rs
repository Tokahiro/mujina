//! Code that launcher adapters share and that needs ports, so it cannot live in `winutil`.

pub mod launcher;
#[cfg(windows)]
pub mod plugin;
