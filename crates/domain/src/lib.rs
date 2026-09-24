//! The innermost ring of Mujina.
//!
//! Everything in here is plain data and pure decision logic. The crate is `no_std`, forbids
//! `unsafe` and has no dependencies, so it is mechanically impossible for it to perform I/O or to
//! reach an operating-system API. Outer rings depend on this crate, never the other way round.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod activation;
pub mod button;
pub mod chord;
pub mod keys;
pub mod supervision;
