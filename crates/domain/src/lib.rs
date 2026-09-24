//! The innermost ring of Mujina: plain data and pure decision logic, no I/O, no dependencies.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod activation;
pub mod button;
pub mod chord;
pub mod keys;
pub mod supervision;
