//! Infrastructure shared by the Windows-facing adapters.
//!
//! This is not an adapter: it implements no port and knows nothing about Mujina's domain. It
//! exists so that adapters never depend on each other just to read a registry value. All
//! `unsafe` in here is a direct Win32 call, or the use of a callback or buffer such a call hands
//! over, with its contract stated next to it.

#![cfg(windows)]

pub mod certstore;
pub mod clipboard;
pub mod com;
pub mod console;
pub mod cost;
pub mod error;
pub mod event;
pub mod gamepad;
pub mod library;
pub mod locale;
pub mod location;
pub mod package;
pub mod process;
pub mod registry;
pub mod resource;
pub mod shell;
pub mod time;
pub mod wait;
pub mod wide;
pub mod window;
pub mod wlan;
