//! What adapters share, in Mujina's terms.
//!
//! Adapters never depend on each other, and `winutil` knows nothing about Mujina, so code that
//! every launcher adapter needs and that speaks in ports lives here instead of being copied into
//! each one. Only what needs Windows is built for Windows; the rest is tested everywhere.

pub mod launcher;
#[cfg(windows)]
pub mod plugin;
