//! SH-2 bus interface
//!
//! Implementations can assume that all addresses are masked to the lowest 29 bits (`address & 0x1FFFFFFF`)
//! because the highest 3 bits are only used internally

use crate::debug::Sh2Debugger;
use crate::disassemble;
use crate::disassemble::DisassembledInstruction;
use crate::instructions::OpcodeTable;
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
                let mut instruction = DisassembledInstruction::new();
                disassemble::disassemble_into(pc, opcode, &mut instruction);
                write!(f, "PC={pc:08X}, opcode={opcode:04X}, instruction='{}'", instruction.text)
            }
            Self::InterruptVector => write!(f, "Interrupt vector fetch"),
            Self::Dma { channel } => write!(f, "DMA channel {channel}"),
        }
    }
}

pub struct OpSize;

impl OpSize {
    pub const BYTE: u8 = 0;
    pub const WORD: u8 = 1;
    pub const LONGWORD: u8 = 2;

    /// # Panics
    ///
    /// Panics if `SIZE` is not a valid `OpSize` value
    #[must_use]
    #[inline(always)]
    pub fn display<const SIZE: u8>() -> &'static str {
        match SIZE {
            Self::BYTE => "byte",
            Self::WORD => "word",
            Self::LONGWORD => "longword",
            _ => panic!("invalid size {SIZE}"),
        }
    }

    /// # Panics
    ///
    /// Panics if `SIZE` is not a valid `OpSize` value
    #[must_use]
    #[inline(always)]
    pub fn mask<const SIZE: u8>() -> u32 {
        match SIZE {
            Self::BYTE => 0xFF,
            Self::WORD => 0xFFFF,
            Self::LONGWORD => 0xFFFFFFFF,
            _ => panic!("invalid size {SIZE}"),
        }
    }
}

pub trait BusInterface {
    /// Debug view type; if not implemented, set to [`crate::debug::DummySh2Debugger`]
    type DebugView<'a>: Sh2Debugger
    where
        Self: 'a;

    fn read<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32;

    fn read_byte(&mut self, address: u32, ctx: AccessContext) -> u8 {
        self.read::<{ OpSize::BYTE }>(address, ctx) as u8
    }

    fn read_word(&mut self, address: u32, ctx: AccessContext) -> u16 {
        self.read::<{ OpSize::WORD }>(address, ctx) as u16
    }

    fn read_longword(&mut self, address: u32, ctx: AccessContext) -> u32 {
        self.read::<{ OpSize::LONGWORD }>(address, ctx)
    }

    fn read_cache_line(&mut self, address: u32, ctx: AccessContext) -> [u16; 8];

    fn write<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext);

    fn write_byte(&mut self, address: u32, value: u8, ctx: AccessContext) {
        self.write::<{ OpSize::BYTE }>(address, value.into(), ctx);
    }

    fn write_word(&mut self, address: u32, value: u16, ctx: AccessContext) {
        self.write::<{ OpSize::WORD }>(address, value.into(), ctx);
    }

    fn write_longword(&mut self, address: u32, value: u32, ctx: AccessContext) {
        self.write::<{ OpSize::LONGWORD }>(address, value, ctx);
    }

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

    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        None
    }
}

pub trait Sh2LookupTable<Bus: BusInterface> {
    fn table<'a>() -> &'a OpcodeTable<Bus>;
}

#[macro_export]
macro_rules! impl_sh2_lookup_table {
    ($bus:ident) => {
        impl $crate::bus::Sh2LookupTable<$bus> for $crate::Sh2 {
            fn table<'a>() -> &'a $crate::OpcodeTable<$bus> {
                static TABLE: ::std::sync::LazyLock<$crate::OpcodeTable<$bus>> =
                    ::std::sync::LazyLock::new(|| $crate::OpcodeTable::new());

                &*TABLE
            }
        }
    };
}

pub use impl_sh2_lookup_table;
