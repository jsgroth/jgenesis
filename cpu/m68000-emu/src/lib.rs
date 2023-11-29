#[cfg(any(test, feature = "memorybus"))]
pub mod bus;
mod core;
pub mod traits;

pub use crate::core::M68000;
pub use traits::BusInterface;
