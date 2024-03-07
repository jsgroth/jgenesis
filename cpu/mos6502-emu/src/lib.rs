pub mod bus;
mod instructions;

use crate::bus::BusInterface;
use crate::instructions::InstructionState;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum StatusReadContext {
    HardwareInterruptHandler,
    Brk,
    PushStack,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct StatusFlags {
    negative: bool,
    overflow: bool,
    decimal: bool,
    interrupt_disable: bool,
    zero: bool,
    carry: bool,
}

impl StatusFlags {
    #[must_use]
    pub fn new() -> Self {
        // I flag defaults to 1, others default to 0
        Self {
            negative: false,
            overflow: false,
            decimal: false,
            interrupt_disable: true,
            zero: false,
            carry: false,
        }
    }

    pub fn set_negative(&mut self, negative: bool) -> &mut Self {
        self.negative = negative;
        self
    }

    pub fn set_overflow(&mut self, overflow: bool) -> &mut Self {
        self.overflow = overflow;
        self
    }

    pub fn set_zero(&mut self, zero: bool) -> &mut Self {
        self.zero = zero;
        self
    }

    pub fn set_carry(&mut self, carry: bool) -> &mut Self {
        self.carry = carry;
        self
    }

    #[must_use]
    pub fn to_byte(self, read_ctx: StatusReadContext) -> u8 {
        // B flag is set during BRK and PHA/PHP, cleared during NMI & IRQ handlers
        let b_flag = match read_ctx {
            StatusReadContext::Brk | StatusReadContext::PushStack => 0x10,
            StatusReadContext::HardwareInterruptHandler => 0x00,
        };

        // Bit 5 is unused, always reads as 1
        (u8::from(self.negative) << 7)
            | (u8::from(self.overflow) << 6)
            | 0x20
            | b_flag
            | (u8::from(self.decimal) << 3)
            | (u8::from(self.interrupt_disable) << 2)
            | (u8::from(self.zero) << 1)
            | u8::from(self.carry)
    }

    #[must_use]
    pub fn from_byte(byte: u8) -> Self {
        Self {
            negative: byte.bit(7),
            overflow: byte.bit(6),
            decimal: byte.bit(3),
            interrupt_disable: byte.bit(2),
            zero: byte.bit(1),
            carry: byte.bit(0),
        }
    }
}

impl Default for StatusFlags {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CpuRegisters {
    pub accumulator: u8,
    pub x: u8,
    pub y: u8,
    pub status: StatusFlags,
    pub pc: u16,
    pub sp: u8,
}

impl CpuRegisters {
    fn new(reset_vector: u16) -> Self {
        Self { accumulator: 0, x: 0, y: 0, status: StatusFlags::new(), pc: reset_vector, sp: 0xFD }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Mos6502 {
    registers: CpuRegisters,
    state: InstructionState,
    frozen: bool,
}

const NMI_VECTOR: u16 = 0xFFFA;
const RESET_VECTOR: u16 = 0xFFFC;
const IRQ_VECTOR: u16 = 0xFFFE;

impl Mos6502 {
    /// Create a new 6502 with the PC pointing to the RESET vector, read from $FFFC.
    pub fn new<B: BusInterface>(bus: &mut B) -> Self {
        let reset_vector_lsb = bus.read(RESET_VECTOR);
        let reset_vector_msb = bus.read(RESET_VECTOR + 1);
        let reset_vector = u16::from_le_bytes([reset_vector_lsb, reset_vector_msb]);

        Self {
            registers: CpuRegisters::new(reset_vector),
            state: InstructionState::default(),
            frozen: false,
        }
    }

    /// Reset the 6502, which does the following:
    /// * Immediately update PC to point to the RESET vector, abandoning any in-progress instruction
    /// * Subtract 3 from the stack pointer
    /// * Disable IRQs
    /// * If the CPU was frozen by an illegal KIL opcode, unfreeze it
    pub fn reset<B: BusInterface>(&mut self, bus: &mut B) {
        let reset_vector_lsb = bus.read(RESET_VECTOR);
        let reset_vector_msb = bus.read(RESET_VECTOR + 1);
        self.registers.pc = u16::from_le_bytes([reset_vector_lsb, reset_vector_msb]);
        self.state = InstructionState::default();

        self.registers.sp = self.registers.sp.wrapping_sub(3);

        self.registers.status.interrupt_disable = true;

        self.frozen = false;
    }

    /// Run the CPU for 1 cycle.
    #[inline]
    pub fn tick<B: BusInterface>(&mut self, bus: &mut B) {
        if self.frozen {
            // CPU was frozen by an illegal KIL opcode; do nothing
            return;
        }

        if self.state.instruction_complete {
            // Opcode is always read, even if handling an interrupt
            let opcode = bus.read(self.registers.pc);

            if self.state.pending_interrupt {
                self.state.pending_interrupt = false;
                self.state.executing_interrupt = true;
            } else {
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.state.opcode = opcode;
            }

            self.state.instruction_complete = false;
            self.state.cycle = 0;
            return;
        }

        instructions::execute_cycle(self, bus);
    }

    #[inline]
    #[must_use]
    pub fn pc(&self) -> u16 {
        self.registers.pc
    }

    #[must_use]
    pub fn is_mid_instruction(&self) -> bool {
        !self.state.instruction_complete
    }

    #[must_use]
    pub fn registers(&self) -> &CpuRegisters {
        &self.registers
    }

    pub fn set_registers(&mut self, registers: CpuRegisters) {
        self.registers = registers;
    }

    /// Return whether the CPU has frozen from a KIL instruction.
    #[inline]
    #[must_use]
    pub fn frozen(&self) -> bool {
        self.frozen
    }
}
