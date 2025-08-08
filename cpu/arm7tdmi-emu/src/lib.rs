pub mod bus;
mod instructions;

use crate::bus::{BusInterface, MemoryCycle};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum CpuState {
    // 32-bit opcodes, full instruction set
    #[default]
    Arm = 0,
    // 16-bit opcodes, limited instruction set
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
        match bits & 0x1F {
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

    fn spsr(self, registers: &mut Registers) -> Option<&mut StatusRegister> {
        match self {
            Self::Irq => Some(&mut registers.spsr_irq),
            Self::Fiq => Some(&mut registers.spsr_fiq),
            Self::Supervisor => Some(&mut registers.spsr_svc),
            Self::Undefined => Some(&mut registers.spsr_und),
            Self::User | Self::System | Self::Illegal | Self::Abort => None,
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
    fiq_disabled: bool,
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
            | (u32::from(value.fiq_disabled) << 6)
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
            fiq_disabled: value.bit(6),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exception {
    Reset,
    UndefinedInstruction,
    SoftwareInterrupt,
    Irq,
}

impl Exception {
    const fn vector_address(self) -> u32 {
        match self {
            Self::Reset => 0x00,
            Self::UndefinedInstruction => 0x04,
            Self::SoftwareInterrupt => 0x08,
            Self::Irq => 0x18,
        }
    }

    const fn new_mode(self) -> CpuMode {
        match self {
            Self::Reset | Self::SoftwareInterrupt => CpuMode::Supervisor,
            Self::UndefinedInstruction => CpuMode::Undefined,
            Self::Irq => CpuMode::Irq,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn return_address(self, state: CpuState, r15: u32) -> u32 {
        // R15 is PC+8 for ARM and PC+4 for Thumb
        match (state, self) {
            // ARM R14 is always PC+4 except for Data abort, which is not emulated
            (CpuState::Arm, _) => r15.wrapping_sub(4),
            // Thumb R14 is PC+2 for SWI and Undefined
            (CpuState::Thumb, Self::UndefinedInstruction | Self::SoftwareInterrupt) => {
                r15.wrapping_sub(2)
            }
            // Thumb R14 is PC+4 for IRQ (and FIQ and Data abort, which are not emulated)
            (CpuState::Thumb, Self::Irq) => r15,
            // Reset value doesn't matter because return address is not written when handling Reset
            (CpuState::Thumb, Self::Reset) => r15,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Arm7Tdmi {
    registers: Registers,
    prefetch: [u32; 2],
    fetch_cycle: MemoryCycle,
    prev_r15: u32,
}

impl Default for Arm7Tdmi {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Arm7TdmiResetArgs {
    pub pc: u32,
    pub sp_usr: u32,
    pub sp_svc: u32,
    pub sp_irq: u32,
    pub sp_fiq: u32,
    pub mode: CpuMode,
}

impl Arm7Tdmi {
    #[must_use]
    pub fn new() -> Self {
        Self {
            registers: Registers::default(),
            prefetch: [0, 0],
            fetch_cycle: MemoryCycle::N,
            prev_r15: 0,
        }
    }

    pub fn reset(&mut self, bus: &mut impl BusInterface) {
        self.handle_exception(Exception::Reset, bus);
    }

    pub fn manual_reset(&mut self, args: Arm7TdmiResetArgs, bus: &mut impl BusInterface) {
        self.registers = Registers::default();
        self.registers.r[..13].fill(0);
        self.registers.r[14] = args.pc;
        self.registers.r[15] = args.pc;
        self.registers.r[13] = args.sp_usr;
        self.registers.r13_svc = args.sp_svc;
        self.registers.r13_irq = args.sp_irq;
        self.registers.r13_fiq = args.sp_fiq;
        self.registers.cpsr = StatusRegister {
            sign: false,
            zero: false,
            carry: false,
            overflow: false,
            irq_disabled: false,
            fiq_disabled: false,
            state: CpuState::Arm,
            mode: args.mode,
        };
        self.registers.spsr_svc = 0_u32.into();
        self.registers.spsr_irq = 0_u32.into();

        self.refill_prefetch(bus);
    }

    #[inline]
    pub fn execute_instruction(&mut self, bus: &mut impl BusInterface) {
        let irq = !self.registers.cpsr.irq_disabled && bus.irq();
        let opcode = self.prefetch[0];
        self.fetch_opcode(bus);

        if irq {
            return self.handle_exception(Exception::Irq, bus);
        }

        match self.registers.cpsr.state {
            CpuState::Arm => self.execute_arm_opcode(opcode, bus),
            CpuState::Thumb => self.execute_thumb_opcode(opcode as u16, bus),
        }
    }

    fn refill_prefetch(&mut self, bus: &mut (impl BusInterface + ?Sized)) {
        self.fetch_cycle = MemoryCycle::N;

        match self.registers.cpsr.state {
            CpuState::Arm => {
                self.registers.r[15] &= !3;

                self.fetch_opcode_state::<true>(bus);
                self.fetch_opcode_state::<true>(bus);
            }
            CpuState::Thumb => {
                self.registers.r[15] &= !1;

                self.fetch_opcode_state::<false>(bus);
                self.fetch_opcode_state::<false>(bus);
            }
        }
    }

    fn fetch_opcode(&mut self, bus: &mut (impl BusInterface + ?Sized)) {
        match self.registers.cpsr.state {
            CpuState::Arm => self.fetch_opcode_state::<true>(bus),
            CpuState::Thumb => self.fetch_opcode_state::<false>(bus),
        }
    }

    fn fetch_opcode_state<const ARM: bool>(&mut self, bus: &mut (impl BusInterface + ?Sized)) {
        let fetch_cycle = self.fetch_cycle;
        self.fetch_cycle = MemoryCycle::S;
        self.prev_r15 = self.registers.r[15];

        self.prefetch[0] = self.prefetch[1];
        if ARM {
            self.prefetch[1] = bus.fetch_opcode_word(self.registers.r[15], fetch_cycle);
            self.registers.r[15] = self.registers.r[15].wrapping_add(4);

            log::trace!(
                "Fetched ARM opcode {:08X} from {:08X}",
                self.prefetch[1],
                self.registers.r[15].wrapping_sub(4)
            );
        } else {
            // Thumb
            self.prefetch[1] = bus.fetch_opcode_halfword(self.registers.r[15], fetch_cycle).into();
            self.registers.r[15] = self.registers.r[15].wrapping_add(2);

            log::trace!(
                "Fetched Thumb opcode {:04X} from {:08X}",
                self.prefetch[1],
                self.registers.r[15].wrapping_sub(2)
            );
        }
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

    fn spsr_to_cpsr(&mut self) {
        let spsr = match self.registers.cpsr.mode.spsr(&mut self.registers) {
            Some(spsr) => *spsr,
            None => {
                log::error!("Attempted to read SPSR in mode {:?}", self.registers.cpsr.mode);
                return;
            }
        };

        self.change_mode(spsr.mode);
        self.registers.cpsr = spsr;
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

    fn handle_exception(&mut self, exception: Exception, bus: &mut (impl BusInterface + ?Sized)) {
        log::trace!("Handling exception of type {exception:?}");

        let old_cpsr = self.registers.cpsr;

        let mode = exception.new_mode();
        self.change_mode(mode);
        self.registers.cpsr.state = CpuState::Arm;

        if exception != Exception::Reset {
            self.registers.r[14] = exception.return_address(old_cpsr.state, self.prev_r15);
            if let Some(spsr) = mode.spsr(&mut self.registers) {
                *spsr = old_cpsr;
            }
        }

        self.registers.cpsr.irq_disabled = true;
        if exception == Exception::Reset {
            self.registers.cpsr.fiq_disabled = true;
        }

        self.registers.r[15] = exception.vector_address();
        self.refill_prefetch(bus);
    }

    fn read_register(&self, r: u32) -> u32 {
        if r == 15 {
            // Return R15 from before the opcode fetch
            self.prev_r15
        } else {
            self.registers.r[r as usize]
        }
    }
}
