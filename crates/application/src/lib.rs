//! The application ring: use cases, and the [`ports`] that adapters implement for them. Nothing
//! in here may name an operating-system API.

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

pub use role::Role;
// A text Mujina Settings shows in the user's language.
pub use mujina_i18n::Msg;

/// Translations of this crate's [`Msg`] texts, as (language, `.po` file) pairs.
pub const CATALOGS: &[(&str, &str)] = &[("de", include_str!("../lang/de.po"))];
