#[cfg(any(test, feature = "memorybus"))]
pub mod bus;
mod core;
pub mod traits;

pub use crate::core::{M68000, cycles_if_move_btst_cmp};
pub use traits::BusInterface;
