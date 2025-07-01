mod instructions;

use crate::traits::BusInterface;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum SizeBits {
    Eight,
    Sixteen,
}

impl SizeBits {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Eight } else { Self::Sixteen }
    }

    fn to_bit(self) -> bool {
        self == Self::Eight
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct StatusRegister {
    carry: bool,
    zero: bool,
    irq_disabled: bool,
    decimal_mode: bool,
    index_size: SizeBits,
    accumulator_size: SizeBits,
    overflow: bool,
    negative: bool,
}

impl Default for StatusRegister {
    fn default() -> Self {
        Self {
            carry: false,
            zero: false,
            irq_disabled: true,
            decimal_mode: false,
            index_size: SizeBits::Eight,
            accumulator_size: SizeBits::Eight,
            overflow: false,
            negative: false,
        }
    }
}

impl From<StatusRegister> for u8 {
    fn from(value: StatusRegister) -> Self {
        (u8::from(value.negative) << 7)
            | (u8::from(value.overflow) << 6)
            | (u8::from(value.accumulator_size.to_bit()) << 5)
            | (u8::from(value.index_size.to_bit()) << 4)
            | (u8::from(value.decimal_mode) << 3)
            | (u8::from(value.irq_disabled) << 2)
            | (u8::from(value.zero) << 1)
            | u8::from(value.carry)
    }
}

impl From<u8> for StatusRegister {
    fn from(value: u8) -> Self {
        Self {
            carry: value.bit(0),
            zero: value.bit(1),
            irq_disabled: value.bit(2),
            decimal_mode: value.bit(3),
            index_size: SizeBits::from_bit(value.bit(4)),
            accumulator_size: SizeBits::from_bit(value.bit(5)),
            overflow: value.bit(6),
            negative: value.bit(7),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Registers {
    // Accumulator
    pub a: u16,
    // Index registers
    pub x: u16,
    pub y: u16,
    // Stack pointer
    pub s: u16,
    // Direct page pointer
    pub d: u16,
    // Program bank register
    pub pbr: u8,
    // Program counter
    pub pc: u16,
    // Data bank register
    pub dbr: u8,
    // Status register
    pub p: StatusRegister,
    // Emulation mode flag
    pub emulation_mode: bool,
}

impl Registers {
    fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            s: 0x01FF,
            d: 0,
            pbr: 0,
            pc: 0,
            dbr: 0,
            p: StatusRegister::default(),
            // 65816 always powers on in emulation mode
            emulation_mode: true,
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct State {
    cycle: u8,
    opcode: u8,
    handling_interrupt: Option<InterruptType>,
    waiting: bool,
    stopped: bool,
    last_nmi: bool,
    nmi_triggered: bool,
    last_irq: bool,
    irq_triggered: bool,
    reset_pending: bool,
    // Temp values to store internal instruction state between cycles
    t0: u8,
    t1: u8,
    t2: u8,
    t3: u8,
    t4: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub(crate) enum InterruptType {
    Breakpoint,
    Coprocessor,
    Nmi,
    Irq,
    Reset,
}

impl InterruptType {
    const fn is_software(self) -> bool {
        matches!(self, Self::Breakpoint | Self::Coprocessor)
    }

    const fn emulation_vector(self) -> u16 {
        match self {
            Self::Coprocessor => 0xFFF4,
            Self::Nmi => 0xFFFA,
            Self::Reset => 0xFFFC,
            Self::Breakpoint | Self::Irq => 0xFFFE,
        }
    }

    const fn native_vector(self) -> u16 {
        match self {
            Self::Coprocessor => 0xFFE4,
            Self::Breakpoint => 0xFFE6,
            Self::Nmi => 0xFFEA,
            Self::Irq => 0xFFEE,
            Self::Reset => 0xFFFC,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Wdc65816 {
    registers: Registers,
    state: State,
}

impl Wdc65816 {
    #[must_use]
    pub fn new() -> Self {
        Self { registers: Registers::new(), state: State::default() }
    }

    pub fn reset<B: BusInterface>(&mut self, bus: &mut B) {
        let reset_vector: u32 = InterruptType::Reset.emulation_vector().into();
        let address_lsb = bus.read(reset_vector);
        let address_msb = bus.read(reset_vector + 1);
        self.registers.pc = u16::from_le_bytes([address_lsb, address_msb]);
        self.registers.pbr = 0;

        self.registers.p.irq_disabled = true;
        self.registers.p.decimal_mode = false;
        self.registers.emulation_mode = true;

        // Emulation mode forces on m and x flags and forces the stack to page 1
        self.registers.p.accumulator_size = SizeBits::Eight;
        self.registers.p.index_size = SizeBits::Eight;
        self.registers.x &= 0x00FF;
        self.registers.y &= 0x00FF;
        self.registers.s = 0x0100 | (self.registers.s & 0x00FF);

        self.state = State::default();
    }

    #[inline]
    pub fn tick<B: BusInterface>(&mut self, bus: &mut B) {
        if bus.reset() {
            self.state.reset_pending = true;
            bus.idle();
            return;
        }

        if self.state.reset_pending {
            // TODO timing? this does 2 bus reads and probably takes more internal cycles
            self.reset(bus);
            self.state.reset_pending = false;
            return;
        }

        if bus.halt() {
            bus.idle();
            return;
        }

        instructions::execute(self, bus);
    }

    #[inline]
    #[must_use]
    pub fn is_mid_instruction(&self) -> bool {
        self.state.cycle != 0
    }

    #[inline]
    #[must_use]
    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    #[inline]
    pub fn set_registers(&mut self, registers: Registers) {
        self.registers = registers;

        if self.registers.emulation_mode {
            // Stack is forced to page 1 in emulation mode
            self.registers.s = 0x0100 | (self.registers.s & 0x00FF);
        }
    }
}

impl Default for Wdc65816 {
    fn default() -> Self {
        Self::new()
    }
}
