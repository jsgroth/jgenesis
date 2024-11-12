mod disassemble;

use crate::bus::BusInterface;
use crate::{Arm7Tdmi, CpuMode, CpuState, Registers, StatusRegister};
use jgenesis_common::num::{GetBit, SignBit};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
struct ConditionCodes {
    sign: bool,
    zero: bool,
    carry: bool,
    overflow: bool,
}

impl ConditionCodes {
    fn logical(value: u32, shifter_out: bool, overflow: bool) -> Self {
        Self { sign: value.bit(31), zero: value == 0, carry: shifter_out, overflow }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Condition {
    // EQ (Z=1)
    Equal = 0x0,
    // NE (Z=0)
    NotEqual = 0x1,
    // CS (C=1)
    CarrySet = 0x2,
    // CC (C=0)
    CarryClear = 0x3,
    // MI (N=1)
    Minus = 0x4,
    // PL (N=0)
    Plus = 0x5,
    // VS (V=1)
    OverflowSet = 0x6,
    // VC (V=0)
    OverflowClear = 0x7,
    // HI (C=1 and Z=0)
    Higher = 0x8,
    // LS (C=0 or Z=1)
    LowerOrSame = 0x9,
    // GE (N=V)
    GreaterOrEqual = 0xA,
    // LT (N!=V)
    Less = 0xB,
    // GT (Z=0 and N=V)
    Greater = 0xC,
    // LE (Z=1 or N!=V)
    LessOrEqual = 0xD,
    // AL (always true)
    Always = 0xE,
    // Reserved - treat as always false
    Reserved = 0xF,
}

impl Condition {
    fn from_bits(bits: u32) -> Self {
        match bits & 0xF {
            0x0 => Self::Equal,
            0x1 => Self::NotEqual,
            0x2 => Self::CarrySet,
            0x3 => Self::CarryClear,
            0x4 => Self::Minus,
            0x5 => Self::Plus,
            0x6 => Self::OverflowSet,
            0x7 => Self::OverflowClear,
            0x8 => Self::Higher,
            0x9 => Self::LowerOrSame,
            0xA => Self::GreaterOrEqual,
            0xB => Self::Less,
            0xC => Self::Greater,
            0xD => Self::LessOrEqual,
            0xE => Self::Always,
            0xF => Self::Reserved,
            _ => unreachable!("value is always <= 0xF"),
        }
    }

    fn from_arm_opcode(opcode: u32) -> Self {
        Self::from_bits(opcode >> 28)
    }

    fn check(self, cpsr: StatusRegister) -> bool {
        match self {
            Self::Equal => cpsr.zero,
            Self::NotEqual => !cpsr.zero,
            Self::CarrySet => cpsr.carry,
            Self::CarryClear => !cpsr.carry,
            Self::Minus => cpsr.sign,
            Self::Plus => !cpsr.sign,
            Self::OverflowSet => cpsr.overflow,
            Self::OverflowClear => !cpsr.overflow,
            Self::Higher => cpsr.carry && !cpsr.zero,
            Self::LowerOrSame => !cpsr.carry || cpsr.zero,
            Self::GreaterOrEqual => cpsr.sign == cpsr.overflow,
            Self::Less => cpsr.sign != cpsr.overflow,
            Self::Greater => !cpsr.zero && cpsr.sign == cpsr.overflow,
            Self::LessOrEqual => cpsr.zero || cpsr.sign != cpsr.overflow,
            Self::Always => true,
            Self::Reserved => false,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::Equal => "EQ",
            Self::NotEqual => "NE",
            Self::CarrySet => "CS",
            Self::CarryClear => "CC",
            Self::Minus => "MI",
            Self::Plus => "PL",
            Self::OverflowSet => "VS",
            Self::OverflowClear => "VC",
            Self::Higher => "HI",
            Self::LowerOrSame => "LS",
            Self::GreaterOrEqual => "GE",
            Self::Less => "LT",
            Self::Greater => "GT",
            Self::LessOrEqual => "LE",
            Self::Always => "",
            Self::Reserved => "(invalid)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AluOp {
    And = 0x0,
    ExclusiveOr = 0x1,
    Subtract = 0x2,
    ReverseSubtract = 0x3,
    Add = 0x4,
    AddCarry = 0x5,
    SubtractCarry = 0x6,
    ReverseSubtractCarry = 0x7,
    Test = 0x8,
    TestEqual = 0x9,
    Compare = 0xA,
    CompareNegate = 0xB,
    Or = 0xC,
    Move = 0xD,
    BitClear = 0xE,
    MoveNegate = 0xF,
}

impl AluOp {
    fn from_bits(bits: u32) -> Self {
        match bits & 0xF {
            0x0 => Self::And,
            0x1 => Self::ExclusiveOr,
            0x2 => Self::Subtract,
            0x3 => Self::ReverseSubtract,
            0x4 => Self::Add,
            0x5 => Self::AddCarry,
            0x6 => Self::SubtractCarry,
            0x7 => Self::ReverseSubtractCarry,
            0x8 => Self::Test,
            0x9 => Self::TestEqual,
            0xA => Self::Compare,
            0xB => Self::CompareNegate,
            0xC => Self::Or,
            0xD => Self::Move,
            0xE => Self::BitClear,
            0xF => Self::MoveNegate,
            _ => unreachable!("value & 0xF is always <= 0xF"),
        }
    }

    // AND, EOR, TST, TEQ, ORR, MOV, BIC, MVN
    fn is_logical(self) -> bool {
        matches!(
            self,
            Self::And
                | Self::ExclusiveOr
                | Self::Test
                | Self::TestEqual
                | Self::Or
                | Self::Move
                | Self::BitClear
                | Self::MoveNegate
        )
    }

    // TST, TEQ, CMP, CMN
    fn is_test(self) -> bool {
        matches!(self, Self::Test | Self::TestEqual | Self::Compare | Self::CompareNegate)
    }
}

type ArmFn = fn(&mut Arm7Tdmi, u32, &mut dyn BusInterface) -> u32;

struct ArmDecodeEntry {
    mask: u32,
    target: u32,
    op_fn: ArmFn,
}

impl ArmDecodeEntry {
    const fn new(mask: u32, target: u32, op_fn: ArmFn) -> Self {
        Self { mask, target, op_fn }
    }
}

const ARM_DECODE_TABLE: &[ArmDecodeEntry] = &[
    ArmDecodeEntry::new(0b1111_1111_1111, 0b0001_0010_0001, arm_bx),
    ArmDecodeEntry::new(0b1111_1100_1111, 0b0000_0000_1001, arm_multiply),
    ArmDecodeEntry::new(0b1111_1000_1111, 0b0000_1000_1001, arm_multiply_long),
    ArmDecodeEntry::new(0b1111_1011_1111, 0b0001_0000_1001, arm_swap),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0000_1001, arm_load_halfword::<false, false>),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0100_1001, arm_load_halfword::<false, true>),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0001_1001, arm_load_halfword::<true, false>),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0101_1001, arm_load_halfword::<true, true>),
    ArmDecodeEntry::new(0b1110_0100_1001, 0b0000_0100_1001, |_, opcode, _| {
        todo!("LDRH/STRH immediate offset {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1111_1011_1111, 0b0001_0000_0000, arm_mrs),
    ArmDecodeEntry::new(0b1111_1011_0000, 0b0001_0010_0000, arm_msr::<false>),
    ArmDecodeEntry::new(0b1111_1011_0000, 0b0011_0010_0000, arm_msr::<true>),
    ArmDecodeEntry::new(0b1110_0000_0000, 0b0000_0000_0000, arm_alu::<false>),
    ArmDecodeEntry::new(0b1110_0000_0000, 0b0010_0000_0000, arm_alu::<true>),
    ArmDecodeEntry::new(0b1110_0000_0001, 0b0110_0000_0001, |_, opcode, _| {
        todo!("undefined {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1110_0001_0000, 0b0100_0000_0000, arm_load_word::<false, false>),
    ArmDecodeEntry::new(0b1110_0001_0000, 0b0100_0001_0000, arm_load_word::<true, false>),
    ArmDecodeEntry::new(0b1110_0001_0000, 0b0110_0000_0000, arm_load_word::<false, true>),
    ArmDecodeEntry::new(0b1110_0001_0000, 0b0110_0001_0000, arm_load_word::<true, true>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1000_0000_0000, arm_ldm_stm::<false, false, true>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1000_0001_0000, arm_ldm_stm::<true, false, true>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1000_1000_0000, arm_ldm_stm::<false, true, true>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1000_1001_0000, arm_ldm_stm::<true, true, true>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1001_0000_0000, arm_ldm_stm::<false, false, false>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1001_0001_0000, arm_ldm_stm::<true, false, false>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1001_1000_0000, arm_ldm_stm::<false, true, false>),
    ArmDecodeEntry::new(0b1111_1001_0000, 0b1001_1001_0000, arm_ldm_stm::<true, true, false>),
    ArmDecodeEntry::new(0b1111_0000_0000, 0b1010_0000_0000, arm_branch::<false>),
    ArmDecodeEntry::new(0b1111_0000_0000, 0b1011_0000_0000, arm_branch::<true>),
];

type ThumbFn = fn(&mut Arm7Tdmi, u16, &mut dyn BusInterface) -> u32;

struct ThumbDecodeEntry {
    mask: u16,
    target: u16,
    op_fn: ThumbFn,
}

impl ThumbDecodeEntry {
    const fn new(mask: u16, target: u16, op_fn: ThumbFn) -> Self {
        Self { mask, target, op_fn }
    }
}

const THUMB_DECODE_TABLE: &[ThumbDecodeEntry] = &[
    ThumbDecodeEntry::new(0xF8, 0x18, thumb_add_sub),
    ThumbDecodeEntry::new(0xE0, 0x00, thumb_move_shifted_register),
    ThumbDecodeEntry::new(0xE0, 0x20, thumb_alu_immediate),
    ThumbDecodeEntry::new(0xFC, 0x40, thumb_alu),
    ThumbDecodeEntry::new(0xFC, 0x44, thumb_high_register_op),
    ThumbDecodeEntry::new(0xF8, 0x48, thumb_pc_relative_load),
    ThumbDecodeEntry::new(0xF2, 0x50, thumb_load_register_offset),
    ThumbDecodeEntry::new(0xE0, 0x60, thumb_load_immediate_offset),
    ThumbDecodeEntry::new(0xF0, 0x80, thumb_load_halfword),
    ThumbDecodeEntry::new(0xF0, 0x90, thumb_load_sp_relative),
    ThumbDecodeEntry::new(0xF0, 0xA0, thumb_load_address),
    ThumbDecodeEntry::new(0xFF, 0xB0, |_, opcode, _| todo!("Thumb format 13")),
    ThumbDecodeEntry::new(0xF6, 0xB4, thumb_push_pop),
    ThumbDecodeEntry::new(0xF0, 0xC0, thumb_load_multiple),
    ThumbDecodeEntry::new(0xFF, 0xDF, |_, opcode, _| todo!("Thumb format 17")),
    ThumbDecodeEntry::new(0xF0, 0xD0, thumb_conditional_branch),
    ThumbDecodeEntry::new(0xF8, 0xE0, thumb_unconditional_branch),
    ThumbDecodeEntry::new(0xF0, 0xF0, thumb_long_branch),
];

const ARM_OPCODE_LEN: u32 = 4;
const THUMB_OPCODE_LEN: u32 = 2;

impl Arm7Tdmi {
    pub(crate) fn execute_arm_opcode(&mut self, opcode: u32, bus: &mut impl BusInterface) -> u32 {
        // TODO remove
        disassemble::arm(opcode);

        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "Executing opcode {opcode:08X}, PC+8={:08X}, str={}",
                self.registers.r[15],
                disassemble::arm(opcode)
            );
            log::trace!("  R={:08X?}", self.registers.r);
        }

        let condition = Condition::from_arm_opcode(opcode);
        if !condition.check(self.registers.cpsr) {
            // 1S if instruction is skipped
            self.fetch_arm_opcode(bus);
            return 1;
        }

        let opcode_mask = ((opcode >> 16) & 0xFF0) | ((opcode >> 4) & 0xF);
        for &ArmDecodeEntry { mask, target, op_fn } in ARM_DECODE_TABLE {
            if opcode_mask & mask == target {
                return op_fn(self, opcode, bus);
            }
        }

        todo!("decode {opcode:08X}")
    }

    pub(crate) fn execute_thumb_opcode(&mut self, opcode: u16, bus: &mut impl BusInterface) -> u32 {
        // TODO remove
        disassemble::thumb(opcode);

        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "Executing opcode {opcode:04X}, PC+4={:08X}, str={}",
                self.registers.r[15],
                disassemble::thumb(opcode)
            );
            log::trace!("  R={:08X?}", self.registers.r);
        }

        let opcode_mask = opcode >> 8;
        for &ThumbDecodeEntry { mask, target, op_fn } in THUMB_DECODE_TABLE {
            if opcode_mask & mask == target {
                return op_fn(self, opcode, bus);
            }
        }

        todo!("Decode Thumb {opcode:04X}")
    }
}

// B: Branch
// BL: Branch and link
fn arm_branch<const LINK: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    // Offset is a 24-bit signed value, shifted left 2
    let offset = (((opcode & 0xFFFFFF) as i32) << 8) >> 6;
    branch::<LINK, ARM_OPCODE_LEN>(cpu, offset, bus)
}

fn branch<const LINK: bool, const OPCODE_LEN: u32>(
    cpu: &mut Arm7Tdmi,
    offset: i32,
    bus: &mut dyn BusInterface,
) -> u32 {
    if LINK {
        cpu.registers.r[14] = cpu.registers.r[15].wrapping_sub(OPCODE_LEN);
    }

    cpu.registers.r[15] = cpu.registers.r[15].wrapping_add_signed(offset);
    cpu.refill_prefetch(bus);

    // 2S + 1N
    3
}

// BX: Branch and exchange ARM/Thumb state
fn arm_bx(cpu: &mut Arm7Tdmi, opcode: u32, bus: &mut dyn BusInterface) -> u32 {
    branch_exchange(cpu, opcode & 0xF, bus)
}

fn branch_exchange(cpu: &mut Arm7Tdmi, rn: u32, bus: &mut dyn BusInterface) -> u32 {
    let new_pc = cpu.registers.r[rn as usize];
    cpu.registers.cpsr.state = CpuState::from_bit(new_pc.bit(0));

    log::trace!("CPU state is now {:?}", cpu.registers.cpsr.state);

    cpu.registers.r[15] = new_pc;
    cpu.fetch_opcode(bus);
    cpu.fetch_opcode(bus);

    // 2S + 1N
    3
}

// MRS: Transfer PSR contents to register
fn arm_mrs(cpu: &mut Arm7Tdmi, opcode: u32, bus: &mut dyn BusInterface) -> u32 {
    let rd = (opcode >> 12) & 0xF;
    let spsr = opcode.bit(22);

    if spsr {
        todo!("MRS from SPSR")
    } else {
        cpu.registers.r[rd as usize] = cpu.registers.cpsr.into();
    }

    cpu.fetch_opcode(bus);

    // 1S
    1
}

// MSR: Transfer register contents to PSR
fn arm_msr<const IMMEDIATE: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let operand = if IMMEDIATE {
        let (immediate, rotation) = arm_parse_rotated_immediate(opcode);
        immediate.rotate_right(rotation)
    } else {
        cpu.registers.r[(opcode & 0xF) as usize]
    };

    let spsr = opcode.bit(22);
    let flags_only = !opcode.bit(16);

    if spsr {
        todo!("MSR to SPSR")
    } else if flags_only {
        cpu.write_cpsr_flags(operand);
    } else {
        cpu.write_cpsr(operand);
    }

    cpu.fetch_arm_opcode(bus);

    // 1S
    1
}

// Data processing instructions
//   AND: And
//   EOR: Exclusive or
//   SUB: Subtract
//   RSB: Reverse subtract
//   ADD: Add
//   ADC: Add with carry
//   SBC: Subtract with carry
//   RSC: Reverse subtract with carry
//   TST: Test bits
//   TEQ: Test equal
//   CMP: Compare
//   CMN: Compare negated
//   ORR: Or
//   MOV: Move
//   BIC: Bit clear
//   MVN: Move negated
fn arm_alu<const IMMEDIATE: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let alu_op = AluOp::from_bits(opcode >> 21);
    let set_condition_codes = opcode.bit(20);
    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;

    if IMMEDIATE {
        let (immediate, rotation) = arm_parse_rotated_immediate(opcode);
        alu_rotated_immediate(cpu, alu_op, rn, rd, set_condition_codes, immediate, rotation, bus)
    } else if opcode.bit(4) {
        let rm = opcode & 0xF;
        let rs = (opcode >> 8) & 0xF;
        let shift_type = ShiftType::from_bits(opcode >> 5);
        alu_register_shift::<ARM_OPCODE_LEN>(
            cpu,
            alu_op,
            rn,
            rd,
            set_condition_codes,
            rm,
            shift_type,
            rs,
            bus,
        )
    } else {
        let rm = opcode & 0xF;
        let shift_type = ShiftType::from_bits(opcode >> 5);
        let shift = (opcode >> 7) & 0x1F;
        alu_immediate_shift(cpu, alu_op, rn, rd, set_condition_codes, rm, shift_type, shift, bus)
    }
}

#[inline]
fn arm_parse_rotated_immediate(opcode: u32) -> (u32, u32) {
    let immediate = opcode & 0xFF;
    let rotation = ((opcode >> 8) & 0xF) << 1;
    (immediate, rotation)
}

#[inline]
fn alu_rotated_immediate(
    cpu: &mut Arm7Tdmi,
    op: AluOp,
    rn: u32,
    rd: u32,
    set_condition_codes: bool,
    immediate: u32,
    rotation: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let (operand2, shifter_out) = if rotation != 0 {
        (immediate.rotate_right(rotation), immediate.bit((rotation - 1) as u8))
    } else {
        (immediate, cpu.registers.cpsr.carry)
    };

    alu(cpu, op, cpu.registers.r[rn as usize], rd, set_condition_codes, operand2, shifter_out, bus)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShiftType {
    Left = 0,
    LogicalRight = 1,
    ArithmeticRight = 2,
    RotateRight = 3,
}

impl ShiftType {
    fn from_bits(bits: u32) -> Self {
        match bits & 3 {
            0 => Self::Left,
            1 => Self::LogicalRight,
            2 => Self::ArithmeticRight,
            3 => Self::RotateRight,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[inline]
fn alu_immediate_shift(
    cpu: &mut Arm7Tdmi,
    op: AluOp,
    rn: u32,
    rd: u32,
    set_condition_codes: bool,
    rm: u32,
    shift_type: ShiftType,
    shift: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let value = cpu.registers.r[rm as usize];

    let (operand2, shifter_out) =
        apply_immediate_shift(value, shift_type, shift, cpu.registers.cpsr.carry);

    alu(cpu, op, cpu.registers.r[rn as usize], rd, set_condition_codes, operand2, shifter_out, bus)
}

fn apply_immediate_shift(
    value: u32,
    shift_type: ShiftType,
    shift: u32,
    carry_in: bool,
) -> (u32, bool) {
    match (shift_type, shift) {
        (ShiftType::Left, 0) => (value, carry_in),
        (ShiftType::Left, _) => (value << shift, value.bit((32 - shift) as u8)),
        (ShiftType::LogicalRight, 0) => (0, value.bit(31)),
        (ShiftType::LogicalRight, _) => (value >> shift, value.bit((shift - 1) as u8)),
        (ShiftType::ArithmeticRight, 0) => (((value as i32) >> 31) as u32, value.bit(31)),
        (ShiftType::ArithmeticRight, _) => {
            (((value as i32) >> shift) as u32, value.bit((shift - 1) as u8))
        }
        (ShiftType::RotateRight, 0) => {
            // RRX: Rotate right through carry
            let result = (value >> 1) | (u32::from(carry_in) << 31);
            (result, value.bit(0))
        }
        (ShiftType::RotateRight, _) => (value.rotate_right(shift), value.bit((shift - 1) as u8)),
    }
}

#[inline]
fn alu_register_shift<const OPCODE_LEN: u32>(
    cpu: &mut Arm7Tdmi,
    op: AluOp,
    rn: u32,
    rd: u32,
    set_condition_codes: bool,
    rm: u32,
    shift_type: ShiftType,
    rs: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let mut value = cpu.registers.r[rm as usize];
    if rm == 15 {
        value = value.wrapping_add(OPCODE_LEN);
    }

    let shift = cpu.registers.r[rs as usize];

    let (operand2, shifter_out) = if shift == 0 {
        (value, cpu.registers.cpsr.carry)
    } else {
        match shift_type {
            ShiftType::Left => match shift.cmp(&32) {
                Ordering::Less => (value << shift, value.bit((32 - shift) as u8)),
                Ordering::Equal => (0, value.bit(0)),
                Ordering::Greater => (0, false),
            },
            ShiftType::LogicalRight => match shift.cmp(&32) {
                Ordering::Less => (value >> shift, value.bit((shift - 1) as u8)),
                Ordering::Equal => (0, value.bit(31)),
                Ordering::Greater => (0, false),
            },
            ShiftType::ArithmeticRight => {
                if shift < 32 {
                    (((value as i32) >> shift) as u32, value.bit((shift - 1) as u8))
                } else {
                    (((value as i32) >> 31) as u32, value.bit(31))
                }
            }
            ShiftType::RotateRight => {
                let mut shift = shift;
                while shift > 32 {
                    shift -= 32;
                }

                if shift == 32 {
                    (value, value.bit(31))
                } else {
                    (value.rotate_right(shift), value.bit((shift - 1) as u8))
                }
            }
        }
    };

    let operand1 = if rn == 15 {
        cpu.registers.r[15].wrapping_add(OPCODE_LEN)
    } else {
        cpu.registers.r[rn as usize]
    };

    alu(cpu, op, operand1, rd, set_condition_codes, operand2, shifter_out, bus)
}

#[inline]
fn alu(
    cpu: &mut Arm7Tdmi,
    op: AluOp,
    operand1: u32,
    rd: u32,
    set_condition_codes: bool,
    operand2: u32,
    shifter_out: bool,
    bus: &mut dyn BusInterface,
) -> u32 {
    let (result, codes) = match op {
        AluOp::And | AluOp::Test => {
            let result = operand1 & operand2;
            (result, ConditionCodes::logical(result, shifter_out, cpu.registers.cpsr.overflow))
        }
        AluOp::ExclusiveOr | AluOp::TestEqual => {
            let result = operand1 ^ operand2;
            (result, ConditionCodes::logical(result, shifter_out, cpu.registers.cpsr.overflow))
        }
        AluOp::Subtract | AluOp::Compare => alu_add(operand1, !operand2, true),
        AluOp::ReverseSubtract => alu_add(operand2, !operand1, true),
        AluOp::Add | AluOp::CompareNegate => alu_add(operand1, operand2, false),
        AluOp::AddCarry => alu_add(operand1, operand2, cpu.registers.cpsr.carry),
        AluOp::SubtractCarry => alu_add(operand1, !operand2, cpu.registers.cpsr.carry),
        AluOp::ReverseSubtractCarry => alu_add(operand2, !operand1, cpu.registers.cpsr.carry),
        AluOp::Or => {
            let result = operand1 | operand2;
            (result, ConditionCodes::logical(result, shifter_out, cpu.registers.cpsr.overflow))
        }
        AluOp::Move => {
            (operand2, ConditionCodes::logical(operand2, shifter_out, cpu.registers.cpsr.overflow))
        }
        AluOp::BitClear => {
            let result = operand1 & !operand2;
            (result, ConditionCodes::logical(result, shifter_out, cpu.registers.cpsr.overflow))
        }
        AluOp::MoveNegate => (
            !operand2,
            ConditionCodes::logical(!operand2, shifter_out, cpu.registers.cpsr.overflow),
        ),
    };

    // 1S always
    let mut cycles = 1;

    if !op.is_test() {
        cpu.registers.r[rd as usize] = result;

        if rd == 15 {
            cpu.fetch_opcode(bus);

            // +1N +1S if Rd = 15
            cycles += 2;
        }
    }

    cpu.fetch_opcode(bus);

    if set_condition_codes {
        if rd == 15 {
            todo!("SPSR -> CPSR")
        } else {
            cpu.registers.cpsr.sign = codes.sign;
            cpu.registers.cpsr.zero = codes.zero;
            cpu.registers.cpsr.carry = codes.carry;
            cpu.registers.cpsr.overflow = codes.overflow;
        }
    }

    cycles
}

fn alu_add(operand1: u32, operand2: u32, carry_in: bool) -> (u32, ConditionCodes) {
    let carry_in: u32 = carry_in.into();

    let (partial_sum, carry1) = operand1.overflowing_add(operand2);
    let (sum, carry2) = partial_sum.overflowing_add(carry_in);
    let carry = carry1 || carry2;

    let bit_30_carry = (operand1 & 0x7FFFFFFF) + (operand2 & 0x7FFFFFFF) + carry_in >= 0x80000000;
    let overflow = bit_30_carry != carry;

    (sum, ConditionCodes { sign: sum.bit(31), zero: sum == 0, carry, overflow })
}

fn arm_multiply(cpu: &mut Arm7Tdmi, opcode: u32, bus: &mut dyn BusInterface) -> u32 {
    let rm = opcode & 0xF;
    let rs = (opcode >> 8) & 0xF;
    let rn = (opcode >> 12) & 0xF;
    let rd = (opcode >> 16) & 0xF;
    let set_condition_codes = opcode.bit(20);
    let accumulate = opcode.bit(21);

    multiply(cpu, rm, rs, rn, rd, set_condition_codes, accumulate, bus)
}

fn multiply(
    cpu: &mut Arm7Tdmi,
    rm: u32,
    rs: u32,
    rn: u32,
    rd: u32,
    set_condition_codes: bool,
    accumulate: bool,
    bus: &mut dyn BusInterface,
) -> u32 {
    cpu.fetch_opcode(bus);

    let operand = cpu.registers.r[rs as usize];
    let mut product = cpu.registers.r[rm as usize].wrapping_mul(operand);
    if accumulate {
        product = product.wrapping_add(cpu.registers.r[rn as usize]);
    }

    cpu.registers.r[rd as usize] = product;

    if set_condition_codes {
        cpu.registers.cpsr.sign = product.bit(31);
        cpu.registers.cpsr.zero = product == 0;
        // TODO carry bit?
    }

    let m = if operand & 0xFFFFFF00 == 0 || operand & 0xFFFFFF00 == 0xFFFFFF00 {
        1
    } else if operand & 0xFFFF0000 == 0 || operand & 0xFFFF0000 == 0xFFFF0000 {
        2
    } else if operand & 0xFF000000 == 0 || operand & 0xFF000000 == 0xFF000000 {
        3
    } else {
        4
    };

    // 1S + m*I
    // +1I for MLA
    1 + m + u32::from(accumulate)
}

fn arm_multiply_long(cpu: &mut Arm7Tdmi, opcode: u32, bus: &mut dyn BusInterface) -> u32 {
    let rm = opcode & 0xF;
    let rs = (opcode >> 8) & 0xF;
    let rdlo = (opcode >> 12) & 0xF;
    let rdhi = (opcode >> 16) & 0xF;
    let set_condition_codes = opcode.bit(20);
    let accumulate = opcode.bit(21);
    let signed = opcode.bit(22);

    multiply_long(cpu, rm, rs, rdlo, rdhi, set_condition_codes, accumulate, signed, bus)
}

#[inline]
fn multiply_long(
    cpu: &mut Arm7Tdmi,
    rm: u32,
    rs: u32,
    rdlo: u32,
    rdhi: u32,
    set_condition_codes: bool,
    accumulate: bool,
    signed: bool,
    bus: &mut dyn BusInterface,
) -> u32 {
    cpu.fetch_opcode(bus);

    let operand = cpu.registers.r[rs as usize];
    let product = if signed {
        let mut product =
            i64::from(cpu.registers.r[rm as usize] as i32) * i64::from(operand as i32);
        if accumulate {
            let existing = (i64::from(cpu.registers.r[rdhi as usize]) << 32)
                | i64::from(cpu.registers.r[rdlo as usize]);
            product = product.wrapping_add(existing);
        }
        product as u64
    } else {
        let mut product = u64::from(cpu.registers.r[rm as usize]) * u64::from(operand);
        if accumulate {
            let existing = (u64::from(cpu.registers.r[rdhi as usize]) << 32)
                | u64::from(cpu.registers.r[rdlo as usize]);
            product = product.wrapping_add(existing);
        }
        product
    };

    cpu.registers.r[rdlo as usize] = product as u32;
    cpu.registers.r[rdhi as usize] = (product >> 32) as u32;

    if set_condition_codes {
        cpu.registers.cpsr.sign = product.bit(63);
        cpu.registers.cpsr.zero = product == 0;
        // TODO carry and overflow flags?
    }

    let m = if operand & 0xFFFFFF00 == 0 || (signed && operand & 0xFFFFFF00 == 0xFFFFFF00) {
        1
    } else if operand & 0xFFFF0000 == 0 || (signed && operand & 0xFFFF0000 == 0xFFFF0000) {
        2
    } else if operand & 0xFF000000 == 0 || (signed && operand & 0xFF000000 == 0xFF000000) {
        3
    } else {
        4
    };

    // MULL: 1S + (m+1)*I
    // +1I for MLAL
    2 + m + u32::from(accumulate)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadIndexing {
    Post = 0,
    Pre = 1,
}

impl LoadIndexing {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Pre } else { Self::Post }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexOp {
    Subtract = 0,
    Add = 1,
}

impl IndexOp {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Add } else { Self::Subtract }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteBack {
    No = 0,
    Yes = 1,
}

impl WriteBack {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Yes } else { Self::No }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HalfwordLoadType {
    UnsignedHalfword,
    SignedByte,
    SignedHalfword,
}

impl HalfwordLoadType {
    fn from_bits(bits: u32) -> Self {
        match bits & 3 {
            0 => panic!("HalfwordLoadType::from_bits() called for SWP opcode"),
            1 => Self::UnsignedHalfword,
            2 => Self::SignedByte,
            3 => Self::SignedHalfword,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

// LDR: Load word
// STR: Store word
fn arm_load_word<const LOAD: bool, const REGISTER_OFFSET: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let indexing = LoadIndexing::from_bit(opcode.bit(24));
    let index_op = IndexOp::from_bit(opcode.bit(23));
    let size = LoadSize::from_bit(opcode.bit(22));
    let write_back = WriteBack::from_bit(opcode.bit(21));
    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;

    let offset = if REGISTER_OFFSET {
        let rm = opcode & 0xF;
        let value = cpu.registers.r[rm as usize];
        let shift_type = ShiftType::from_bits(opcode >> 5);
        let shift = (opcode >> 7) & 0x1F;

        let (shifted, _) =
            apply_immediate_shift(value, shift_type, shift, cpu.registers.cpsr.carry);
        shifted
    } else {
        opcode & 0xFFF
    };

    load_word::<LOAD>(cpu, rn, rd, offset, size, indexing, index_op, write_back, bus)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadSize {
    Word = 0,
    Byte = 1,
}

impl LoadSize {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Byte } else { Self::Word }
    }
}

#[inline]
fn load_word<const LOAD: bool>(
    cpu: &mut Arm7Tdmi,
    rn: u32,
    rd: u32,
    mut offset: u32,
    size: LoadSize,
    indexing: LoadIndexing,
    index_op: IndexOp,
    write_back: WriteBack,
    bus: &mut dyn BusInterface,
) -> u32 {
    if index_op == IndexOp::Subtract {
        offset = (!offset).wrapping_add(1);
    }

    let mut address = cpu.registers.r[rn as usize];

    if indexing == LoadIndexing::Pre {
        address = address.wrapping_add(offset);
    }

    if LOAD {
        cpu.registers.r[rd as usize] = match size {
            LoadSize::Word => {
                let word = bus.read_word(address);
                word.rotate_right(8 * (address & 3))
            }
            LoadSize::Byte => bus.read_byte(address).into(),
        };

        if rd == 15 {
            cpu.fetch_opcode(bus);
        }
        cpu.fetch_opcode(bus);
    } else {
        cpu.fetch_opcode(bus);

        let value = cpu.registers.r[rd as usize];
        match size {
            LoadSize::Word => bus.write_word(address, value),
            LoadSize::Byte => bus.write_byte(address, value as u8),
        }
    }

    // Write back only applies on loads if the base register and destination register are different
    if !(LOAD && rn == rd) {
        if indexing == LoadIndexing::Post {
            cpu.registers.r[rn as usize] = address.wrapping_add(offset);
        } else if write_back == WriteBack::Yes {
            cpu.registers.r[rn as usize] = address;
        }
    }

    if LOAD {
        // 1S + 1N + 1I always
        // +1S +1N if Rd = 15
        3 + (u32::from(rd == 15) << 1)
    } else {
        // 2N
        2
    }
}

// LDRH: Load halfword
// STRH: Store halfword
// LDRSB: Load sign-extended byte
// LDRSH: Load sign-extended halfword
fn arm_load_halfword<const LOAD: bool, const IMMEDIATE_OFFSET: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let indexing = LoadIndexing::from_bit(opcode.bit(24));
    let index_op = IndexOp::from_bit(opcode.bit(23));
    let write_back = WriteBack::from_bit(opcode.bit(21));
    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;

    let offset = if IMMEDIATE_OFFSET {
        ((opcode >> 4) & 0xF0) | (opcode & 0xF)
    } else {
        let rm = opcode & 0xF;
        cpu.registers.r[rm as usize]
    };

    let load_type = HalfwordLoadType::from_bits(opcode >> 5);

    load_halfword::<LOAD>(cpu, rn, rd, offset, load_type, indexing, index_op, write_back, bus)
}

#[inline]
fn load_halfword<const LOAD: bool>(
    cpu: &mut Arm7Tdmi,
    rn: u32,
    rd: u32,
    mut offset: u32,
    load_type: HalfwordLoadType,
    indexing: LoadIndexing,
    index_op: IndexOp,
    write_back: WriteBack,
    bus: &mut dyn BusInterface,
) -> u32 {
    if index_op == IndexOp::Subtract {
        offset = (!offset).wrapping_add(1);
    }

    let mut address = cpu.registers.r[rn as usize];
    if indexing == LoadIndexing::Pre {
        address = address.wrapping_add(offset);
    }

    if LOAD {
        let value = match load_type {
            HalfwordLoadType::UnsignedHalfword => u32::from(bus.read_halfword(address)),
            HalfwordLoadType::SignedByte => bus.read_byte(address) as i8 as u32,
            HalfwordLoadType::SignedHalfword => bus.read_halfword(address) as i16 as u32,
        };
        cpu.registers.r[rd as usize] = value;

        if rd == 15 {
            cpu.fetch_opcode(bus);
        }
        cpu.fetch_opcode(bus);
    } else {
        cpu.fetch_opcode(bus);
        bus.write_halfword(address, cpu.registers.r[rd as usize] as u16);
    }

    // Write back only applies on loads if the base register and destination register are different
    if !(LOAD && rn == rd) {
        if indexing == LoadIndexing::Post {
            cpu.registers.r[rn as usize] = address.wrapping_add(offset);
        } else if write_back == WriteBack::Yes {
            cpu.registers.r[rn as usize] = address;
        }
    }

    if LOAD {
        // 1S + 1N + 1I always
        // +1S +1N if Rd = 15
        3 + (u32::from(rd == 15) << 1)
    } else {
        // 2N
        2
    }
}

fn arm_ldm_stm<const LOAD: bool, const INCREMENT: bool, const AFTER: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    let register_bits = opcode & 0xFFFF;
    let rn = (opcode >> 16) & 0xF;
    let write_back = WriteBack::from_bit(opcode.bit(21));
    let s_bit = opcode.bit(22);

    load_multiple::<LOAD, INCREMENT, AFTER>(cpu, register_bits, rn, write_back, s_bit, bus)
}

#[inline]
fn load_multiple<const LOAD: bool, const INCREMENT: bool, const AFTER: bool>(
    cpu: &mut Arm7Tdmi,
    mut register_bits: u32,
    rn: u32,
    write_back: WriteBack,
    s_bit: bool,
    bus: &mut dyn BusInterface,
) -> u32 {
    if register_bits == 0 {
        // Hardware quirk: empty list loads/stores only R15
        register_bits = 1 << 15;
    }

    let count = register_bits.count_ones();
    let r15_loaded = register_bits.bit(15);

    let base_addr = cpu.registers.r[rn as usize];
    let final_addr = if INCREMENT {
        base_addr.wrapping_add(4 * count)
    } else {
        base_addr.wrapping_sub(4 * count)
    };

    let mut address = if INCREMENT { base_addr } else { final_addr };

    cpu.fetch_opcode(bus);

    let mut need_write_back = write_back == WriteBack::Yes;
    for r in 0..16 {
        if !register_bits.bit(r) {
            continue;
        }

        if LOAD && need_write_back {
            cpu.registers.r[rn as usize] = final_addr;
            log::trace!("  Wrote back to R{rn}: {:08X}", cpu.registers.r[rn as usize]);
            need_write_back = false;
        }

        if !(AFTER ^ !INCREMENT) {
            address = address.wrapping_add(4);
        }

        if LOAD {
            if r == 15 {
                cpu.registers.r[15] = bus.read_word(address);

                cpu.fetch_opcode(bus);
                cpu.fetch_opcode(bus);
                if s_bit {
                    todo!("SPSR -> CPSR")
                }
            } else if s_bit && !r15_loaded {
                let register = get_user_register(&mut cpu.registers, r.into());
                *register = bus.read_word(address);
            } else {
                cpu.registers.r[r as usize] = bus.read_word(address);
            }
            log::trace!("  LDM: Loaded R{r} from {address:08X}");
        } else {
            let value = if s_bit {
                *get_user_register(&mut cpu.registers, r.into())
            } else {
                cpu.registers.r[r as usize]
            };
            bus.write_word(address, value);
            log::trace!("  STM: Stored R{r} to {address:08X}");
        }

        if AFTER ^ !INCREMENT {
            address = address.wrapping_add(4);
        }

        if !LOAD && need_write_back {
            cpu.registers.r[rn as usize] = final_addr;
            log::trace!("  Wrote back to R{rn}: {:08X}", cpu.registers.r[rn as usize]);
            need_write_back = false;
        }
    }

    if LOAD {
        // LDM: n*S + 1N + 1I
        // +1S +1N if R15 loaded
        2 + count + (u32::from(r15_loaded) << 1)
    } else {
        // STM: (n-1)*S + 2N
        1 + count
    }
}

fn get_user_register(registers: &mut Registers, r: u32) -> &mut u32 {
    match (registers.cpsr.mode, r) {
        (CpuMode::User | CpuMode::System, _) => &mut registers.r[r as usize],
        (CpuMode::Fiq, 8..=12) => &mut registers.other_r_8_12[(r - 8) as usize],
        (_, 13) => &mut registers.r13_usr,
        (_, 14) => &mut registers.r14_usr,
        _ => &mut registers.r[r as usize],
    }
}

fn arm_swap(cpu: &mut Arm7Tdmi, opcode: u32, bus: &mut dyn BusInterface) -> u32 {
    let rm = opcode & 0xF;
    let rd = (opcode >> 12) & 0xF;
    let rn = (opcode >> 16) & 0xF;
    let size = LoadSize::from_bit(opcode.bit(22));

    swap(cpu, rm, rd, rn, size, bus)
}

fn swap(
    cpu: &mut Arm7Tdmi,
    rm: u32,
    rd: u32,
    rn: u32,
    size: LoadSize,
    bus: &mut dyn BusInterface,
) -> u32 {
    let address = cpu.registers.r[rn as usize];

    match size {
        LoadSize::Word => {
            let value = bus.read_word(address);
            bus.write_word(address, cpu.registers.r[rm as usize]);
            cpu.registers.r[rd as usize] = value;
        }
        LoadSize::Byte => {
            let value = bus.read_byte(address);
            bus.write_byte(address, cpu.registers.r[rm as usize] as u8);
            cpu.registers.r[rd as usize] = value.into();
        }
    }

    cpu.fetch_opcode(bus);

    // 1S + 2N + 1I
    4
}

// Format 1: Move shifted register
fn thumb_move_shifted_register(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let rd = opcode & 7;
    let rs = (opcode >> 3) & 7;
    let shift = (opcode >> 6) & 0x1F;
    let shift_type = ShiftType::from_bits((opcode >> 11).into());

    alu_immediate_shift(
        cpu,
        AluOp::Move,
        rd.into(),
        rd.into(),
        true,
        rs.into(),
        shift_type,
        shift.into(),
        bus,
    )
}

// Format 2: Add/subtract
fn thumb_add_sub(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let rd = opcode & 7;
    let rs = (opcode >> 3) & 7;
    let rn = (opcode >> 6) & 7;
    let alu_op = if opcode.bit(9) { AluOp::Subtract } else { AluOp::Add };

    let immediate = opcode.bit(10);
    if immediate {
        alu_rotated_immediate(cpu, alu_op, rs.into(), rd.into(), true, rn.into(), 0, bus)
    } else {
        alu_immediate_shift(
            cpu,
            alu_op,
            rs.into(),
            rd.into(),
            true,
            rn.into(),
            ShiftType::Left,
            0,
            bus,
        )
    }
}

// Format 3: Move/compare/add/subtract immediate
fn thumb_alu_immediate(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let immediate = opcode & 0xFF;
    let rd = (opcode >> 8) & 7;
    let alu_op = match (opcode >> 11) & 3 {
        0 => AluOp::Move,
        1 => AluOp::Compare,
        2 => AluOp::Add,
        3 => AluOp::Subtract,
        _ => unreachable!("value & 3 is always <= 3"),
    };

    alu_rotated_immediate(cpu, alu_op, rd.into(), rd.into(), true, immediate.into(), 0, bus)
}

// Format 4: ALU operations
fn thumb_alu(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let rd = opcode & 7;
    let rs = (opcode >> 3) & 7;

    let basic_op = match (opcode >> 6) & 0xF {
        0x0 => AluOp::And,
        0x1 => AluOp::ExclusiveOr,
        op @ (0x2..=0x4 | 0x7) => {
            // LSL/LSR/ASR/ROR
            let shift_type = match op {
                2 => ShiftType::Left,
                3 => ShiftType::LogicalRight,
                4 => ShiftType::ArithmeticRight,
                7 => ShiftType::RotateRight,
                _ => unreachable!("nested match expressions"),
            };

            return alu_register_shift::<THUMB_OPCODE_LEN>(
                cpu,
                AluOp::Move,
                rd.into(),
                rd.into(),
                true,
                rd.into(),
                shift_type,
                rs.into(),
                bus,
            );
        }
        0x5 => AluOp::AddCarry,
        0x6 => AluOp::SubtractCarry,
        0x8 => AluOp::Test,
        0x9 => {
            // NEG
            return alu_rotated_immediate(
                cpu,
                AluOp::ReverseSubtract,
                rs.into(),
                rd.into(),
                true,
                0,
                0,
                bus,
            );
        }
        0xA => AluOp::Compare,
        0xB => AluOp::CompareNegate,
        0xC => AluOp::Or,
        0xD => {
            // MUL
            return multiply(cpu, rd.into(), rs.into(), rd.into(), rd.into(), true, false, bus);
        }
        0xE => AluOp::BitClear,
        0xF => AluOp::MoveNegate,
        _ => unreachable!("value & 0xF is always <= 0xF"),
    };

    alu_immediate_shift(
        cpu,
        basic_op,
        rd.into(),
        rd.into(),
        true,
        rs.into(),
        ShiftType::Left,
        0,
        bus,
    )
}

// Format 5: Hi register operations / branch exchange
fn thumb_high_register_op(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let mut rd = opcode & 7;
    let mut rs = (opcode >> 3) & 7;

    if opcode.bit(6) {
        rs += 8;
    }

    if opcode.bit(7) {
        rd += 8;
    }

    let alu_op = match (opcode >> 8) & 3 {
        0 => AluOp::Add,
        1 => AluOp::Compare,
        2 => AluOp::Move,
        3 => return branch_exchange(cpu, rs.into(), bus),
        _ => unreachable!("value & 3 is always <= 3"),
    };

    let set_condition_codes = alu_op == AluOp::Compare;

    alu_immediate_shift(
        cpu,
        alu_op,
        rd.into(),
        rd.into(),
        set_condition_codes,
        rs.into(),
        ShiftType::Left,
        0,
        bus,
    )
}

// Format 6: PC-relative load
fn thumb_pc_relative_load(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    // Bit 1 of PC is forcibly cleared for PC-relative loads
    // Fake this by subtracting 2 from the offset if bit 1 of PC is set
    let mut offset: u32 = ((opcode & 0xFF) << 2).into();
    offset = offset.wrapping_sub(cpu.registers.r[15] & 2);

    let rd = (opcode >> 8) & 7;

    load_word::<true>(
        cpu,
        15,
        rd.into(),
        offset,
        LoadSize::Word,
        LoadIndexing::Pre,
        IndexOp::Add,
        WriteBack::No,
        bus,
    )
}

// Format 7: Load/store with register offset
fn thumb_load_register_offset(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let rd = opcode & 7;
    let rb = (opcode >> 3) & 7;
    let ro = (opcode >> 6) & 7;
    let size = LoadSize::from_bit(opcode.bit(10));
    let load = opcode.bit(11);

    let offset = cpu.registers.r[ro as usize];

    if load {
        load_word::<true>(
            cpu,
            rb.into(),
            rd.into(),
            offset,
            size,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    } else {
        load_word::<false>(
            cpu,
            rb.into(),
            rd.into(),
            offset,
            size,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    }
}

// Format 9: Load/store with immediate offset
fn thumb_load_immediate_offset(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let rd = opcode & 7;
    let rb = (opcode >> 3) & 7;
    let mut offset = (opcode >> 6) & 0x1F;
    let load = opcode.bit(11);
    let size = LoadSize::from_bit(opcode.bit(12));

    if size == LoadSize::Word {
        offset <<= 2;
    }

    if load {
        load_word::<true>(
            cpu,
            rb.into(),
            rd.into(),
            offset.into(),
            size,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    } else {
        load_word::<false>(
            cpu,
            rb.into(),
            rd.into(),
            offset.into(),
            size,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    }
}

// Format 10: Load/store halfword
fn thumb_load_halfword(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let rd = opcode & 7;
    let rb = (opcode >> 3) & 7;
    let offset = ((opcode >> 6) & 0x1F) << 1;
    let load = opcode.bit(11);

    if load {
        load_halfword::<true>(
            cpu,
            rb.into(),
            rd.into(),
            offset.into(),
            HalfwordLoadType::UnsignedHalfword,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    } else {
        load_halfword::<false>(
            cpu,
            rb.into(),
            rd.into(),
            offset.into(),
            HalfwordLoadType::UnsignedHalfword,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    }
}

// Format 11: SP-relative load/store
fn thumb_load_sp_relative(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let offset = (opcode & 0xFF) << 2;
    let rd = (opcode >> 8) & 7;
    let load = opcode.bit(11);

    if load {
        load_halfword::<true>(
            cpu,
            13,
            rd.into(),
            offset.into(),
            HalfwordLoadType::UnsignedHalfword,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    } else {
        load_halfword::<false>(
            cpu,
            13,
            rd.into(),
            offset.into(),
            HalfwordLoadType::UnsignedHalfword,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        )
    }
}

// Format 12: Load address
fn thumb_load_address(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let mut immediate: u32 = ((opcode & 0xFF) << 2).into();
    let rd = (opcode >> 8) & 7;

    // Bit 11 selects between SP (R13) and PC (R15)
    let rn = if opcode.bit(11) { 13 } else { 15 };

    // Bit 1 is forced clear when PC is used as Rn
    // Fake by subtracting 2 from immediate if bit 1 is set
    if rn == 15 {
        immediate = immediate.wrapping_sub(cpu.registers.r[15] & 2);
    }

    alu_rotated_immediate(cpu, AluOp::Add, rn, rd.into(), false, immediate, 0, bus)
}

// Format 14: Push/pop registers
fn thumb_push_pop(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let lr_pc_bit = opcode.bit(8);
    let load = opcode.bit(11);

    let mut register_bits = opcode & 0xFF;
    if lr_pc_bit {
        // Store LR, load PC
        register_bits |= 1 << (14 + u32::from(load));
    }

    if load {
        load_multiple::<true, true, true>(cpu, register_bits.into(), 13, WriteBack::Yes, false, bus)
    } else {
        load_multiple::<false, false, false>(
            cpu,
            register_bits.into(),
            13,
            WriteBack::Yes,
            false,
            bus,
        )
    }
}

// Format 15: Multiple load/store
fn thumb_load_multiple(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let register_bits = opcode & 0xFF;
    let rb = (opcode >> 8) & 7;
    let load = opcode.bit(11);

    if load {
        load_multiple::<true, true, true>(
            cpu,
            register_bits.into(),
            rb.into(),
            WriteBack::Yes,
            false,
            bus,
        )
    } else {
        load_multiple::<false, true, true>(
            cpu,
            register_bits.into(),
            rb.into(),
            WriteBack::Yes,
            false,
            bus,
        )
    }
}

// Format 16: Conditional branch
fn thumb_conditional_branch(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let condition = Condition::from_bits((opcode >> 8).into());
    if !condition.check(cpu.registers.cpsr) {
        // 1S when condition fails
        cpu.fetch_thumb_opcode(bus);
        return 1;
    }

    let offset = i32::from(opcode as i8) << 1;
    branch::<false, THUMB_OPCODE_LEN>(cpu, offset, bus)
}

// Format 18: Unconditional branch
fn thumb_unconditional_branch(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    let offset = (i32::from(opcode & 0x7FF) << 21) >> 20;
    branch::<false, THUMB_OPCODE_LEN>(cpu, offset, bus)
}

// Format 19: Long branch with link
// This instruction has no ARM equivalent
fn thumb_long_branch(cpu: &mut Arm7Tdmi, opcode: u16, bus: &mut dyn BusInterface) -> u32 {
    // Offset is a signed 23-bit value split across two opcodes, with 11 bits in each
    let offset_high: u32 = (opcode & 0x7FF).into();
    let offset_low = cpu.prefetch[1] & 0x7FF;
    let unsigned_offset = (offset_high << 11) | offset_low;
    let offset = ((unsigned_offset as i32) << 10) >> 9;

    cpu.registers.r[14] = cpu.registers.r[15] | 1;
    cpu.registers.r[15] = cpu.registers.r[15].wrapping_add_signed(offset);
    cpu.fetch_thumb_opcode(bus);
    cpu.fetch_thumb_opcode(bus);

    // 1N + 3S
    4
}
