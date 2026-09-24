//! Use cases and the [`ports`] adapters implement for them. Names no operating-system API.

#![forbid(unsafe_code)]

pub mod agent;
pub mod device;
pub mod doctor;
pub mod home;
pub mod launcher;
pub mod ports;
pub mod register;
mod role;
pub mod settings;

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

pub use mujina_i18n::Msg;
pub use role::Role;

/// Translations of this crate's [`Msg`] texts, as (language, `.po` file) pairs.
pub const CATALOGS: &[(&str, &str)] = &[("de", include_str!("../lang/de.po"))];
