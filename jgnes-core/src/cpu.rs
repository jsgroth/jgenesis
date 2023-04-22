use crate::bus;
use crate::bus::{Bus, CpuBus};

mod instructions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusReadContext {
    HardwareInterruptHandler,
    Brk,
    PushStack,
}

#[derive(Debug, Clone, Copy)]
pub struct StatusFlags {
    negative: bool,
    overflow: bool,
    decimal: bool,
    interrupt_disable: bool,
    zero: bool,
    carry: bool,
}

impl StatusFlags {
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

    pub fn set_decimal(&mut self, decimal: bool) -> &mut Self {
        self.decimal = decimal;
        self
    }

    pub fn set_interrupt_disable(&mut self, interrupt_disable: bool) -> &mut Self {
        self.interrupt_disable = interrupt_disable;
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

    pub fn from_byte(byte: u8) -> Self {
        Self {
            negative: byte & 0x80 != 0,
            overflow: byte & 0x40 != 0,
            decimal: byte & 0x08 != 0,
            interrupt_disable: byte & 0x04 != 0,
            zero: byte & 0x02 != 0,
            carry: byte & 0x01 != 0,
        }
    }
}

#[derive(Debug)]
pub struct CpuRegisters {
    pub accumulator: u8,
    pub x: u8,
    pub y: u8,
    pub status: StatusFlags,
    pub pc: u16,
    pub sp: u8,
}

impl CpuRegisters {
    pub fn new(bus: &mut CpuBus<'_>) -> Self {
        let pc_lsb = bus.read_address(bus::CPU_RESET_VECTOR);
        let pc_msb = bus.read_address(bus::CPU_RESET_VECTOR + 1);
        let pc = u16::from_le_bytes([pc_lsb, pc_msb]);

        Self {
            accumulator: 0,
            x: 0,
            y: 0,
            status: StatusFlags::new(),
            pc,
            sp: 0xFD,
        }
    }
}

pub struct CpuState {
    registers: CpuRegisters,
}

impl CpuState {
    pub fn new(registers: CpuRegisters) -> Self {
        Self { registers }
    }
}

pub fn tick(state: &mut CpuState, bus: &mut Bus) {
    todo!()
}
