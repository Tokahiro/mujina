//! Composition root. This is the only crate that knows which adapter fulfils which port.

#![cfg(windows)]

pub mod agent;
pub mod capture;
pub mod compose;
pub mod ctl;
#[cfg(test)]
mod fake_hid;
pub mod home;
pub mod probe;
pub mod registry;
pub mod tool;
