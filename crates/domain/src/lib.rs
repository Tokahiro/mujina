//! The innermost ring of Mujina: plain data and pure decision logic. `no_std`, no `unsafe` and no
//! dependencies, so it cannot do I/O or call the OS. Outer rings depend on it, never the reverse.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod activation;
pub mod button;
pub mod chord;
pub mod keys;
pub mod supervision;
