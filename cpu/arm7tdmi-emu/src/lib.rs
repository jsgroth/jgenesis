pub mod bus;
mod instructions;

use crate::bus::{BusInterface, MemoryCycle};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum CpuState {
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
pub enum CpuMode {
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

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct StatusRegister {
    sign: bool,
    zero: bool,
    carry: bool,
    overflow: bool,
    irq_disabled: bool,
    state: CpuState,
    mode: CpuMode,
}

impl From<StatusRegister> for u32 {
    fn from(value: StatusRegister) -> Self {
        (u32::from(value.sign) << 31)
            | (u32::from(value.zero) << 30)
            | (u32::from(value.carry) << 29)
            | (u32::from(value.overflow) << 28)
            | (u32::from(value.irq_disabled) << 7)
            | ((value.state as u32) << 5)
            | (value.mode as u32)
    }
}

impl From<u32> for StatusRegister {
    fn from(value: u32) -> Self {
        Self {
            sign: value.bit(31),
            zero: value.bit(30),
            carry: value.bit(29),
            overflow: value.bit(28),
            irq_disabled: value.bit(7),
            state: CpuState::from_bit(value.bit(5)),
            mode: CpuMode::from_bits(value & 0x1F),
        }
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
    prefetch: [u32; 2],
}

pub struct Arm7TdmiResetArgs {
    pub pc: u32,
    pub sp_usr: u32,
    pub sp_svc: u32,
    pub sp_irq: u32,
    pub mode: CpuMode,
}

impl Arm7Tdmi {
    pub fn new() -> Self {
        Self { registers: Registers::default(), prefetch: [0, 0] }
    }

    pub fn reset(&mut self, args: Arm7TdmiResetArgs, bus: &mut impl BusInterface) {
        self.registers = Registers::default();
        self.registers.r[15] = args.pc;
        self.registers.r[13] = args.sp_usr;
        self.registers.r13_svc = args.sp_svc;
        self.registers.r13_irq = args.sp_irq;
        self.registers.cpsr.mode = args.mode;
        self.registers.spsr_svc = 0_u32.into();
        self.registers.spsr_irq = 0_u32.into();

        self.refill_arm_prefetch(bus);
    }

    pub fn execute_instruction(&mut self, bus: &mut impl BusInterface) -> u32 {
        let opcode = self.prefetch[0];

        let cycles = match self.registers.cpsr.state {
            CpuState::Arm => self.execute_arm_opcode(opcode, bus),
            CpuState::Thumb => todo!("execute Thumb opcode"),
        };

        cycles + bus.access_cycles()
    }

    fn refill_arm_prefetch(&mut self, bus: &mut impl BusInterface) {
        self.fetch_arm_opcode(MemoryCycle::NonSequential, bus);
        self.fetch_arm_opcode(MemoryCycle::Sequential, bus);
    }

    fn fetch_arm_opcode(&mut self, cycle: MemoryCycle, bus: &mut (impl BusInterface + ?Sized)) {
        self.prefetch[0] = self.prefetch[1];
        self.prefetch[1] = bus.read_word(self.registers.r[15], cycle);
        self.registers.r[15] = self.registers.r[15].wrapping_add(4);

        log::trace!(
            "Fetched opcode {:08X} from {:08X}",
            self.prefetch[1],
            self.registers.r[15].wrapping_sub(4)
        );
    }
}
