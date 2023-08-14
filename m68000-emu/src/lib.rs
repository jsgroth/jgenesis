#![allow(dead_code)]

#[cfg(any(test, feature = "memorybus"))]
pub mod bus;
mod core;
pub mod traits;

pub use crate::core::M68000;
