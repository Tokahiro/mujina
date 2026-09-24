//! Safe Win32 wrappers shared by the Windows adapters; no ports, nothing of Mujina's domain.

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
