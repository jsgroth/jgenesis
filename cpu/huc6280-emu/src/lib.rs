pub mod bus;
mod instructions;

use crate::bus::{BusInterface, InterruptLines};
use crate::instructions::InstructionExecutor;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct Flags {
    pub negative: bool,
    pub overflow: bool,
    pub memory_op: bool, // T bit; causes AND/ORA/EOR/ADC to operate on ZeroPage[X] instead of A
    pub decimal: bool,
    pub irq_disable: bool,
    pub zero: bool,
    pub carry: bool,
}

impl Flags {
    #[must_use]
    pub fn to_u8_interrupt(self) -> u8 {
        (u8::from(self.negative) << 7)
            | (u8::from(self.overflow) << 6)
            | (u8::from(self.memory_op) << 5)
            | (u8::from(self.decimal) << 3)
            | (u8::from(self.irq_disable) << 2)
            | (u8::from(self.zero) << 1)
            | u8::from(self.carry)
    }

    #[must_use]
    pub fn to_u8_brk(self) -> u8 {
        self.to_u8_interrupt() | (1 << 4)
    }
}

impl From<u8> for Flags {
    fn from(value: u8) -> Self {
        Self {
            negative: value.bit(7),
            overflow: value.bit(6),
            memory_op: value.bit(5),
            decimal: value.bit(3),
            irq_disable: value.bit(2),
            zero: value.bit(1),
            carry: value.bit(0),
        }
    }
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            negative: false,
            overflow: false,
            memory_op: false,
            decimal: false,
            irq_disable: true,
            zero: false,
            carry: false,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Registers {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub pc: u16,
    pub s: u8,
    pub p: Flags,
    pub mpr: [u8; 8],
}

impl Registers {
    fn new() -> Self {
        // TODO register contents should be randomized at power-on
        Self { a: 0xFF, x: 0xFF, y: 0xFF, pc: 0xFFFF, s: 0xFF, p: Flags::default(), mpr: [0xFF; 8] }
    }

    fn map_address(&self, logical_addr: u16) -> u32 {
        let bank_base_addr = u32::from(self.mpr[(logical_addr >> 13) as usize]) << 13;
        bank_base_addr | u32::from(logical_addr & 0x1FFF)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum BlockTransferStep {
    Increment,
    Decrement,
    Alternate,
    None,
}

impl BlockTransferStep {
    #[must_use]
    fn apply(self, address: u16, count: u8) -> u16 {
        match self {
            Self::Increment => address.wrapping_add(1),
            Self::Decrement => address.wrapping_sub(1),
            Self::Alternate => {
                if count & 1 == 0 {
                    address.wrapping_add(1)
                } else {
                    address.wrapping_sub(1)
                }
            }
            Self::None => address,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct BlockTransferState {
    source: u16,
    destination: u16,
    length: u16,
    source_step: BlockTransferStep,
    destination_step: BlockTransferStep,
    count: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    memory_op_at_fetch: bool, // T bit at the most recent opcode fetch (before it gets cleared)
    mpr_buffer: u8,           // Last MPR read or write in a TAM/TMA instruction
    pending_interrupt: bool,
    latched_interrupt_lines: InterruptLines,
    block_transfer: Option<BlockTransferState>,
}

impl State {
    fn new() -> Self {
        Self {
            memory_op_at_fetch: false,
            mpr_buffer: 0,
            pending_interrupt: false,
            latched_interrupt_lines: InterruptLines::default(),
            block_transfer: None,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Huc6280 {
    registers: Registers,
    state: State,
}

impl Huc6280 {
    #[must_use]
    pub fn new() -> Self {
        Self { registers: Registers::new(), state: State::new() }
    }

    pub fn reset(&mut self, bus: &mut impl BusInterface) {
        self.registers.mpr[7] = 0x00;

        self.registers.p.memory_op = false;
        self.registers.p.decimal = false;
        self.registers.p.irq_disable = true;

        // RESET vector always read from physical address $001FFE ($FFFE with MPR7=$00)
        let pc_lsb = bus.read(0x001FFE);
        let pc_msb = bus.read(0x001FFF);
        self.registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

        self.state = State::new();
    }

    /// Execute a single instruction. Each CPU cycle will make exactly one call to `bus.read()`,
    /// `bus.write()`, or `bus.idle()`.
    ///
    /// When executing a block transfer instruction (TAI/TDD/TIA/TII/TIN), each call to this method
    /// will progress the transfer by one byte.
    pub fn execute_instruction(&mut self, bus: &mut impl BusInterface) {
        InstructionExecutor::new(self, bus).execute_instruction();
    }
}

impl Default for Huc6280 {
    fn default() -> Self {
        Self::new()
    }
}
