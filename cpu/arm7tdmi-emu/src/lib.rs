pub mod bus;

use crate::bus::BusInterface;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum CpuState {
    #[default]
    Arm = 0,
    Thumb = 1,
}

impl CpuState {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Thumb } else { Self::Arm }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum CpuMode {
    #[default]
    User = 0x10,
    Irq = 0x12,
    Supervisor = 0x13,
    // Arbitrary unused mode value
    Illegal = 0x1E,
    System = 0x1F,
}

impl CpuMode {
    fn from_bits(bits: u32) -> Self {
        match bits {
            0x10 => Self::User,
            0x12 => Self::Irq,
            0x13 => Self::Supervisor,
            0x1F => Self::System,
            _ => Self::Illegal,
        }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct StatusRegister(u32);

macro_rules! impl_status_accessor {
    ($name:ident, $bit:ident) => {
        fn $name(self) -> bool {
            self.0 & Self::$bit != 0
        }
    };
}

macro_rules! impl_status_setter {
    ($name:ident, $bit:ident) => {
        fn $name(mut self, value: bool) -> Self {
            if value {
                self.0 |= Self::$bit;
            } else {
                self.0 &= !Self::$bit;
            }
            self
        }
    };
}

impl StatusRegister {
    const SIGN_BIT: u32 = 1 << 31;
    const ZERO_BIT: u32 = 1 << 30;
    const CARRY_BIT: u32 = 1 << 29;
    const OVERFLOW_BIT: u32 = 1 << 28;
    const DISABLE_IRQ_BIT: u32 = 1 << 7;
    const STATE_BIT: u32 = 1 << 5;
    const MODE_MASK: u32 = (1 << 5) - 1;

    impl_status_accessor!(sign, SIGN_BIT);
    impl_status_accessor!(zero, ZERO_BIT);
    impl_status_accessor!(carry, CARRY_BIT);
    impl_status_accessor!(overflow, OVERFLOW_BIT);
    impl_status_accessor!(irq_disabled, DISABLE_IRQ_BIT);

    fn state(self) -> CpuState {
        CpuState::from_bit(self.0 & Self::STATE_BIT != 0)
    }

    fn mode(self) -> CpuMode {
        CpuMode::from_bits(self.0 & Self::MODE_MASK)
    }

    impl_status_setter!(with_sign, SIGN_BIT);
    impl_status_setter!(with_zero, ZERO_BIT);
    impl_status_setter!(with_carry, CARRY_BIT);
    impl_status_setter!(with_overflow, OVERFLOW_BIT);
    impl_status_setter!(with_irq_disabled, DISABLE_IRQ_BIT);
}

impl Default for StatusRegister {
    fn default() -> Self {
        // TODO is this right?
        Self(Self::DISABLE_IRQ_BIT | ((CpuState::Arm as u32) << 5) | (CpuMode::Supervisor as u32))
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct Registers {
    // "General-purpose" registers; R15 is the program counter
    r: [u32; 16],
    // R13 and R14 are banked by mode; these are updated on mode change
    r13_usr: u32,
    r14_usr: u32,
    r13_svc: u32,
    r14_svc: u32,
    r13_irq: u32,
    r14_irq: u32,
    // Condition codes and control register
    cpsr: StatusRegister,
    // Previous condition codes and control registers, banked by CPU mode (excluding usr)
    spsr_svc: StatusRegister,
    spsr_irq: StatusRegister,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Arm7Tdmi {
    registers: Registers,
}

impl Arm7Tdmi {
    pub fn new() -> Self {
        Self { registers: Registers::default() }
    }

    pub fn execute_instruction(&mut self, bus: &mut impl BusInterface) -> u32 {
        todo!()
    }
}
