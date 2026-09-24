//! The application ring: use cases, and the ports they need the outside world to implement.
//!
//! Use cases are written purely against the traits in [`ports`]. Adapters in the outer ring
//! implement those traits for Windows, Steam and so on; the composition root in `mujina-app`
//! plugs them together. Nothing in here may name an operating-system API.

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
// A text Mujina Settings shows in the user's language: a doctor's title, a setting's words.
pub use mujina_i18n::Msg;

/// The translations of this crate's texts ([`Msg`]), by the language `lang/` names them in: for
/// Mujina Settings, which shows them.
pub const CATALOGS: &[(&str, &str)] = &[("de", include_str!("../lang/de.po"))];
