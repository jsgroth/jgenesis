//! SH-2 bus interface
//!
//! Implementations can assume that all addresses are masked to the lowest 29 bits (`address & 0x1FFFFFFF`)
//! because the highest 3 bits are only used internally

use crate::disassemble;
use bincode::{Decode, Encode};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub enum AccessContext {
    Fetch,
    Data { pc: u32, opcode: u16 },
    InterruptVector,
    Dma { channel: usize },
}

impl Display for AccessContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch => {
                write!(f, "Opcode fetch")
            }
            &Self::Data { pc, opcode } => {
                write!(
                    f,
                    "PC={pc:08X}, opcode={opcode:04X}, instruction='{}'",
                    disassemble::disassemble(opcode)
                )
            }
            Self::InterruptVector => write!(f, "Interrupt vector fetch"),
            Self::Dma { channel } => write!(f, "DMA channel {channel}"),
        }
    }
}

pub trait BusInterface {
    fn read_byte(&mut self, address: u32, ctx: AccessContext) -> u8;

    fn read_word(&mut self, address: u32, ctx: AccessContext) -> u16;

    fn read_longword(&mut self, address: u32, ctx: AccessContext) -> u32;

    fn read_cache_line(&mut self, address: u32, ctx: AccessContext) -> [u32; 4];

    fn write_byte(&mut self, address: u32, value: u8, ctx: AccessContext);

    fn write_word(&mut self, address: u32, value: u16, ctx: AccessContext);

    fn write_longword(&mut self, address: u32, value: u32, ctx: AccessContext);

    /// The CPU will halt while this is `true` and then reset when it changes from `true` to `false`
    fn reset(&self) -> bool;

    /// Current external interrupt level from 0 to 15; 0 indicates no interrupt
    fn interrupt_level(&self) -> u8;

    /// DREQ line for DMA channel 0
    fn dma_request_0(&self) -> bool;

    /// DREQ line for DMA channel 1
    fn dma_request_1(&self) -> bool;

    fn acknowledge_dreq_1(&mut self);

    /// Receive a byte from the serial interface, if any
    fn serial_rx(&mut self) -> Option<u8>;

    /// Transmit a byte to the serial interface
    fn serial_tx(&mut self, value: u8);

    fn increment_cycle_counter(&mut self, cycles: u64);

    fn should_stop_execution(&self) -> bool;
}
