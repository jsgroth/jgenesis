#[cfg(any(test, feature = "memorybus"))]
pub mod bus;
mod core;
pub mod disassembler;
pub mod traits;

pub use crate::core::{M68000, cycles_if_move_or_btst};
pub use traits::BusInterface;
