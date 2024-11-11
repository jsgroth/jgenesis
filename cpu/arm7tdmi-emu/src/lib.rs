pub mod bus;
mod instructions;

use crate::bus::BusInterface;
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
    Fiq = 0x11,
    Irq = 0x12,
    Supervisor = 0x13,
    Abort = 0x17,
    Undefined = 0x1B,
    // Arbitrary unused mode value
    Illegal = 0x1E,
    System = 0x1F,
}

impl CpuMode {
    fn from_bits(bits: u32) -> Self {
        match bits {
            0x10 => Self::User,
            0x11 => Self::Fiq,
            0x12 => Self::Irq,
            0x13 => Self::Supervisor,
            0x17 => Self::Abort,
            0x1B => Self::Undefined,
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
    // R13 and R14 are banked by mode
    r13_usr: u32,
    r14_usr: u32,
    r13_svc: u32,
    r14_svc: u32,
    r13_irq: u32,
    r14_irq: u32,
    r13_und: u32,
    r14_und: u32,
    r13_fiq: u32,
    r14_fiq: u32,
    // R8-R12 are banked only for FIQ vs. non-FIQ
    fiq_r_8_12: [u32; 5],
    other_r_8_12: [u32; 5],
    // Condition codes and control register
    cpsr: StatusRegister,
    // Previous condition codes and control registers, banked by CPU mode (excluding usr)
    spsr_svc: StatusRegister,
    spsr_irq: StatusRegister,
    spsr_und: StatusRegister,
    spsr_fiq: StatusRegister,
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

        self.refill_prefetch(bus);
    }

    pub fn execute_instruction(&mut self, bus: &mut impl BusInterface) -> u32 {
        let opcode = self.prefetch[0];

        match self.registers.cpsr.state {
            CpuState::Arm => self.execute_arm_opcode(opcode, bus),
            CpuState::Thumb => todo!("execute Thumb opcode {opcode:04X}"),
        }
    }

    fn refill_prefetch(&mut self, bus: &mut (impl BusInterface + ?Sized)) {
        match self.registers.cpsr.state {
            CpuState::Arm => {
                self.fetch_arm_opcode(bus);
                self.fetch_arm_opcode(bus);
            }
            CpuState::Thumb => todo!("Thumb fetch"),
        }
    }

    fn fetch_opcode(&mut self, bus: &mut (impl BusInterface + ?Sized)) {
        match self.registers.cpsr.state {
            CpuState::Arm => self.fetch_arm_opcode(bus),
            CpuState::Thumb => todo!("Thumb fetch"),
        }
    }

    fn fetch_arm_opcode(&mut self, bus: &mut (impl BusInterface + ?Sized)) {
        self.prefetch[0] = self.prefetch[1];
        self.prefetch[1] = bus.read_word(self.registers.r[15]);
        self.registers.r[15] = self.registers.r[15].wrapping_add(4);

        log::trace!(
            "Fetched ARM opcode {:08X} from {:08X}",
            self.prefetch[1],
            self.registers.r[15].wrapping_sub(4)
        );
    }

    fn write_cpsr(&mut self, value: u32) {
        log::trace!("CPSR write: {value:08X}");

        if self.registers.cpsr.mode != CpuMode::User {
            let new_mode = CpuMode::from_bits(value);
            self.change_mode(new_mode);

            self.registers.cpsr = value.into();
        } else {
            // Only condition codes can be modified in user mode
            self.write_cpsr_flags(value);
        }
    }

    fn write_cpsr_flags(&mut self, value: u32) {
        let new_cpsr: StatusRegister = value.into();
        self.registers.cpsr = StatusRegister {
            sign: new_cpsr.sign,
            zero: new_cpsr.zero,
            carry: new_cpsr.carry,
            overflow: new_cpsr.overflow,
            ..self.registers.cpsr
        };
    }

    fn change_mode(&mut self, new_mode: CpuMode) {
        if new_mode == self.registers.cpsr.mode {
            return;
        }

        log::trace!("Changing CPU mode to {new_mode:?}");

        // Bank R13 and R14, as well as R8-12 if changing out of FIQ mode
        match self.registers.cpsr.mode {
            CpuMode::User | CpuMode::System => {
                self.registers.r13_usr = self.registers.r[13];
                self.registers.r14_usr = self.registers.r[14];
            }
            CpuMode::Supervisor => {
                self.registers.r13_svc = self.registers.r[13];
                self.registers.r14_svc = self.registers.r[14];
            }
            CpuMode::Irq => {
                self.registers.r13_irq = self.registers.r[13];
                self.registers.r14_irq = self.registers.r[14];
            }
            CpuMode::Undefined => {
                self.registers.r13_und = self.registers.r[13];
                self.registers.r14_und = self.registers.r[14];
            }
            CpuMode::Fiq => {
                self.registers.r13_fiq = self.registers.r[13];
                self.registers.r14_fiq = self.registers.r[14];

                // Bank and update R8-12 if switching out of FIQ mode
                self.registers.fiq_r_8_12.copy_from_slice(&self.registers.r[8..13]);
                self.registers.r[8..13].copy_from_slice(&self.registers.other_r_8_12);
            }
            _ => {
                log::error!("Unexpected mode {:?}", self.registers.cpsr.mode);
            }
        }

        // Update R13 and R14 to banked values, as well as R8-12 if switching into FIQ mode
        match new_mode {
            CpuMode::User | CpuMode::System => {
                self.registers.r[13] = self.registers.r13_usr;
                self.registers.r[14] = self.registers.r14_usr;
            }
            CpuMode::Supervisor => {
                self.registers.r[13] = self.registers.r13_svc;
                self.registers.r[14] = self.registers.r14_svc;
            }
            CpuMode::Irq => {
                self.registers.r[13] = self.registers.r13_irq;
                self.registers.r[14] = self.registers.r14_irq;
            }
            CpuMode::Undefined => {
                self.registers.r[13] = self.registers.r13_und;
                self.registers.r[14] = self.registers.r14_und;
            }
            CpuMode::Fiq => {
                // Bank and update R8-12 if switching into FIQ mode
                self.registers.other_r_8_12.copy_from_slice(&self.registers.r[8..13]);
                self.registers.r[8..13].copy_from_slice(&self.registers.fiq_r_8_12);

                self.registers.r[13] = self.registers.r13_fiq;
                self.registers.r[14] = self.registers.r14_fiq;
            }
            _ => {
                log::error!("Unexpected mode {new_mode:?}");
            }
        }

        self.registers.cpsr.mode = new_mode;
    }
}
