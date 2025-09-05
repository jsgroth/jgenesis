mod disassemble;

use crate::bus::{BusInterface, MemoryCycle};
use crate::{Arm7Tdmi, CpuMode, CpuState, Exception, Registers, StatusRegister};
use jgenesis_common::define_bit_enum;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::array;
use std::cmp::Ordering;
use std::ops::Deref;

const ARM_OPCODE_LEN: u32 = 4;
const THUMB_OPCODE_LEN: u32 = 2;

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
    Equal = 0x0,          // EQ (Z=1)
    NotEqual = 0x1,       // NE (Z=0)
    CarrySet = 0x2,       // CS (C=1)
    CarryClear = 0x3,     // CC (C=0)
    Minus = 0x4,          // MI (N=1)
    Plus = 0x5,           // PL (N=0)
    OverflowSet = 0x6,    // VS (V=1)
    OverflowClear = 0x7,  // VC (V=0)
    Higher = 0x8,         // HI (C=1 and Z=0)
    LowerOrSame = 0x9,    // LS (C=0 or Z=1)
    GreaterOrEqual = 0xA, // GE (N=V)
    Less = 0xB,           // LT (N!=V)
    Greater = 0xC,        // GT (Z=0 and N=V)
    LessOrEqual = 0xD,    // LE (Z=1 or N!=V)
    Always = 0xE,         // AL (always true)
    Reserved = 0xF,       // Reserved - treat as always false
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

    // TST, TEQ, CMP, CMN
    fn is_test(self) -> bool {
        matches!(self, Self::Test | Self::TestEqual | Self::Compare | Self::CompareNegate)
    }
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

define_bit_enum!(LoadIndexing, [Post, Pre]);
define_bit_enum!(IndexOp, [Subtract, Add]);
define_bit_enum!(WriteBack, [No, Yes]);

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

define_bit_enum!(LoadSize, [Word, Byte]);

pub type ArmFn<Bus> = fn(&mut Arm7Tdmi<Bus>, u32, &mut Bus);

struct ArmDecode<Bus: BusInterface> {
    mask: u32,
    target: u32,
    op_fn: ArmFn<Bus>,
}

impl<Bus: BusInterface> ArmDecode<Bus> {
    fn new(mask: u32, target: u32, op_fn: ArmFn<Bus>) -> Self {
        Self { mask, target, op_fn }
    }
}

type ArmFnArray<Bus> = [ArmFn<Bus>; 4096];

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct ArmOpTable<Bus: BusInterface>(Box<ArmFnArray<Bus>>);

impl<Bus: BusInterface> Default for ArmOpTable<Bus> {
    fn default() -> Self {
        Self(Box::new(Arm7Tdmi::<Bus>::new_arm_op_table()))
    }
}

impl<Bus: BusInterface> Deref for ArmOpTable<Bus> {
    type Target = Box<ArmFnArray<Bus>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub type ThumbFn<Bus> = fn(&mut Arm7Tdmi<Bus>, u16, &mut Bus);

struct ThumbDecode<Bus: BusInterface> {
    mask: u16,
    target: u16,
    op_fn: ThumbFn<Bus>,
}

impl<Bus: BusInterface> ThumbDecode<Bus> {
    const fn new(mask: u16, target: u16, op_fn: ThumbFn<Bus>) -> Self {
        Self { mask, target, op_fn }
    }
}

type ThumbFnArray<Bus> = [ThumbFn<Bus>; 256];

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct ThumbOpTable<Bus: BusInterface>(Box<ThumbFnArray<Bus>>);

impl<Bus: BusInterface> Default for ThumbOpTable<Bus> {
    fn default() -> Self {
        Self(Box::new(Arm7Tdmi::<Bus>::new_thumb_op_table()))
    }
}

impl<Bus: BusInterface> Deref for ThumbOpTable<Bus> {
    type Target = Box<ThumbFnArray<Bus>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<Bus: BusInterface> Arm7Tdmi<Bus> {
    fn new_arm_op_table() -> ArmFnArray<Bus> {
        #[rustfmt::skip]
        let decode_table = &[
            ArmDecode::new(0b1111_1111_1111, 0b0001_0010_0001, Self::arm_bx),
            ArmDecode::new(0b1111_1100_1111, 0b0000_0000_1001, Self::arm_multiply),
            ArmDecode::new(0b1111_1000_1111, 0b0000_1000_1001, Self::arm_multiply_long),
            ArmDecode::new(0b1111_1011_1111, 0b0001_0000_1001, Self::arm_swap),
            ArmDecode::new(0b1110_0101_1001, 0b0000_0000_1001, Self::arm_load_halfword::<false, false>),
            ArmDecode::new(0b1110_0101_1001, 0b0000_0100_1001, Self::arm_load_halfword::<false, true>),
            ArmDecode::new(0b1110_0101_1001, 0b0000_0001_1001, Self::arm_load_halfword::<true, false>),
            ArmDecode::new(0b1110_0101_1001, 0b0000_0101_1001, Self::arm_load_halfword::<true, true>),
            ArmDecode::new(0b1111_1011_1111, 0b0001_0000_0000, Self::arm_mrs),
            ArmDecode::new(0b1111_1011_0000, 0b0001_0010_0000, Self::arm_msr::<false>),
            ArmDecode::new(0b1111_1011_0000, 0b0011_0010_0000, Self::arm_msr::<true>),
            ArmDecode::new(0b1110_0000_0000, 0b0000_0000_0000, Self::arm_alu::<false>),
            ArmDecode::new(0b1110_0000_0000, 0b0010_0000_0000, Self::arm_alu::<true>),
            ArmDecode::new(0b1110_0001_0000, 0b0100_0000_0000, Self::arm_load_word::<false, false>),
            ArmDecode::new(0b1110_0001_0000, 0b0100_0001_0000, Self::arm_load_word::<true, false>),
            ArmDecode::new(0b1110_0001_0000, 0b0110_0000_0000, Self::arm_load_word::<false, true>),
            ArmDecode::new(0b1110_0001_0000, 0b0110_0001_0000, Self::arm_load_word::<true, true>),
            ArmDecode::new(0b1111_1001_0000, 0b1000_0000_0000, Self::arm_ldm_stm::<false, false, true>),
            ArmDecode::new(0b1111_1001_0000, 0b1000_0001_0000, Self::arm_ldm_stm::<true, false, true>),
            ArmDecode::new(0b1111_1001_0000, 0b1000_1000_0000, Self::arm_ldm_stm::<false, true, true>),
            ArmDecode::new(0b1111_1001_0000, 0b1000_1001_0000, Self::arm_ldm_stm::<true, true, true>),
            ArmDecode::new(0b1111_1001_0000, 0b1001_0000_0000, Self::arm_ldm_stm::<false, false, false>),
            ArmDecode::new(0b1111_1001_0000, 0b1001_0001_0000, Self::arm_ldm_stm::<true, false, false>),
            ArmDecode::new(0b1111_1001_0000, 0b1001_1000_0000, Self::arm_ldm_stm::<false, true, false>),
            ArmDecode::new(0b1111_1001_0000, 0b1001_1001_0000, Self::arm_ldm_stm::<true, true, false>),
            ArmDecode::new(0b1111_0000_0000, 0b1010_0000_0000, Self::arm_branch::<false>),
            ArmDecode::new(0b1111_0000_0000, 0b1011_0000_0000, Self::arm_branch::<true>),
            ArmDecode::new(0b1111_0000_0000, 0b1111_0000_0000, Self::arm_swi),
        ];

        array::from_fn(|opcode_mask| {
            let opcode_mask = opcode_mask as u32;
            for &ArmDecode { mask, target, op_fn } in decode_table {
                if opcode_mask & mask == target {
                    return op_fn;
                }
            }

            |cpu, opcode, bus| {
                log::error!("Executed undefined ARM opcode: {opcode:08X}");
                cpu.handle_exception(Exception::UndefinedInstruction, bus);
            }
        })
    }

    pub(crate) fn execute_arm_opcode(&mut self, opcode: u32, bus: &mut Bus) {
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
            return;
        }

        let opcode_mask = ((opcode >> 16) & 0xFF0) | ((opcode >> 4) & 0xF);
        self.arm_op_table[opcode_mask as usize](self, opcode, bus);
    }

    fn new_thumb_op_table() -> ThumbFnArray<Bus> {
        #[rustfmt::skip]
        let decode_table = &[
            ThumbDecode::new(0xF8, 0x18, Self::thumb_add_sub),
            ThumbDecode::new(0xE0, 0x00, Self::thumb_move_shifted_register),
            ThumbDecode::new(0xE0, 0x20, Self::thumb_alu_immediate),
            ThumbDecode::new(0xFC, 0x40, Self::thumb_alu),
            ThumbDecode::new(0xFC, 0x44, Self::thumb_high_register_op),
            ThumbDecode::new(0xF8, 0x48, Self::thumb_pc_relative_load),
            ThumbDecode::new(0xF2, 0x50, Self::thumb_load_register_offset),
            ThumbDecode::new(0xF2, 0x52, Self::thumb_load_sign_extended),
            ThumbDecode::new(0xE0, 0x60, Self::thumb_load_immediate_offset),
            ThumbDecode::new(0xF0, 0x80, Self::thumb_load_halfword),
            ThumbDecode::new(0xF0, 0x90, Self::thumb_load_sp_relative),
            ThumbDecode::new(0xF0, 0xA0, Self::thumb_load_address),
            ThumbDecode::new(0xFF, 0xB0, Self::thumb_add_offset_sp),
            ThumbDecode::new(0xF6, 0xB4, Self::thumb_push_pop),
            ThumbDecode::new(0xF0, 0xC0, Self::thumb_load_multiple),
            ThumbDecode::new(0xFF, 0xDF, Self::thumb_software_interrupt),
            ThumbDecode::new(0xF0, 0xD0, Self::thumb_conditional_branch),
            ThumbDecode::new(0xF8, 0xE0, Self::thumb_unconditional_branch),
            ThumbDecode::new(0xF0, 0xF0, Self::thumb_long_branch),
        ];

        array::from_fn(|opcode_mask| {
            let opcode_mask = opcode_mask as u16;

            for &ThumbDecode { mask, target, op_fn } in decode_table {
                if opcode_mask & mask == target {
                    return op_fn;
                }
            }

            |cpu, opcode, bus| {
                log::error!("Executed undefined Thumb opcode: {opcode:04X}");
                cpu.handle_exception(Exception::UndefinedInstruction, bus);
            }
        })
    }

    pub(crate) fn execute_thumb_opcode(&mut self, opcode: u16, bus: &mut Bus) {
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "Executing opcode {opcode:04X}, PC+4={:08X}, str={}",
                self.registers.r[15],
                disassemble::thumb(opcode)
            );
            log::trace!("  R={:08X?}", self.registers.r);
        }

        let opcode_mask = opcode >> 8;
        self.thumb_op_table[opcode_mask as usize](self, opcode, bus);
    }

    // B: Branch
    // BL: Branch and link
    fn arm_branch<const LINK: bool>(&mut self, opcode: u32, bus: &mut Bus) {
        // Offset is a 24-bit signed value, shifted left 2
        let offset = (((opcode & 0xFFFFFF) as i32) << 8) >> 6;
        self.branch::<LINK, ARM_OPCODE_LEN>(offset, bus);
    }

    fn branch<const LINK: bool, const OPCODE_LEN: u32>(&mut self, offset: i32, bus: &mut Bus) {
        if LINK {
            self.registers.r[14] = self.prev_r15.wrapping_sub(OPCODE_LEN);
        }

        self.registers.r[15] = self.prev_r15.wrapping_add_signed(offset);
        self.refill_prefetch(bus);
    }

    // BX: Branch and exchange ARM/Thumb state
    fn arm_bx(&mut self, opcode: u32, bus: &mut Bus) {
        self.branch_exchange(opcode & 0xF, bus);
    }

    fn branch_exchange(&mut self, rn: u32, bus: &mut Bus) {
        let new_pc = self.read_register(rn);
        self.registers.cpsr.state = CpuState::from_bit(new_pc.bit(0));

        log::trace!("CPU state is now {:?}", self.registers.cpsr.state);

        self.registers.r[15] = new_pc;
        self.refill_prefetch(bus);
    }

    // MRS: Transfer PSR contents to register
    fn arm_mrs(&mut self, opcode: u32, _bus: &mut Bus) {
        let rd = (opcode >> 12) & 0xF;
        let spsr = opcode.bit(22);

        if spsr {
            let spsr = match self.registers.cpsr.mode.spsr(&mut self.registers) {
                Some(spsr) => *spsr,
                None => {
                    // SPSR reads in user/system mode access CPSR
                    self.registers.cpsr
                }
            };
            self.registers.r[rd as usize] = spsr.into();
        } else {
            self.registers.r[rd as usize] = self.registers.cpsr.into();
        }
    }

    // MSR: Transfer register contents to PSR
    fn arm_msr<const IMMEDIATE: bool>(&mut self, opcode: u32, _bus: &mut Bus) {
        const CONTROL_MASK: u32 = 0xFF;
        const FLAGS_MASK: u32 = 0xF << 28;

        let mut operand = if IMMEDIATE {
            let (immediate, rotation) = arm_parse_rotated_immediate(opcode);
            immediate.rotate_right(rotation)
        } else {
            self.read_register(opcode & 0xF)
        };

        let spsr = opcode.bit(22);
        let psr = if spsr {
            match self.registers.cpsr.mode.spsr(&mut self.registers) {
                Some(spsr) => spsr,
                None => {
                    // SPSR writes in user/system mode do nothing
                    return;
                }
            }
        } else {
            &mut self.registers.cpsr
        };

        let control = opcode.bit(16);
        let flags = opcode.bit(19);

        if !control {
            operand = (operand & !CONTROL_MASK) | (u32::from(*psr) & CONTROL_MASK);
        }
        if !flags {
            operand = (operand & !FLAGS_MASK) | (u32::from(*psr) & FLAGS_MASK);
        }

        if spsr {
            *psr = operand.into();
        } else {
            self.write_cpsr(operand);
        }
    }

    fn arm_swap(&mut self, opcode: u32, bus: &mut Bus) {
        let rm = opcode & 0xF;
        let rd = (opcode >> 12) & 0xF;
        let rn = (opcode >> 16) & 0xF;
        let size = LoadSize::from_bit(opcode.bit(22));

        self.swap(rm, rd, rn, size, bus);
    }

    fn swap(&mut self, rm: u32, rd: u32, rn: u32, size: LoadSize, bus: &mut Bus) {
        let address = self.registers.r[rn as usize];

        bus.lock();

        match size {
            LoadSize::Word => {
                let value = bus.read_word(address, MemoryCycle::N).rotate_right(8 * (address & 3));
                bus.write_word(address, self.read_register(rm), MemoryCycle::S);
                self.registers.r[rd as usize] = value;
            }
            LoadSize::Byte => {
                let value = bus.read_byte(address, MemoryCycle::N);
                bus.write_byte(address, self.read_register(rm) as u8, MemoryCycle::S);
                self.registers.r[rd as usize] = value.into();
            }
        }

        bus.unlock();

        bus.internal_cycles(1);
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
    fn arm_alu<const IMMEDIATE: bool>(&mut self, opcode: u32, bus: &mut Bus) {
        let alu_op = AluOp::from_bits(opcode >> 21);
        let set_condition_codes = opcode.bit(20);
        let rn = (opcode >> 16) & 0xF;
        let rd = (opcode >> 12) & 0xF;

        if IMMEDIATE {
            let (immediate, rotation) = arm_parse_rotated_immediate(opcode);
            self.alu_rotated_immediate(
                alu_op,
                rn,
                rd,
                set_condition_codes,
                immediate,
                rotation,
                bus,
            );
        } else if opcode.bit(4) {
            let rm = opcode & 0xF;
            let rs = (opcode >> 8) & 0xF;
            let shift_type = ShiftType::from_bits(opcode >> 5);
            self.alu_register_shift::<ARM_OPCODE_LEN>(
                alu_op,
                rn,
                rd,
                set_condition_codes,
                rm,
                shift_type,
                rs,
                bus,
            );
        } else {
            let rm = opcode & 0xF;
            let shift_type = ShiftType::from_bits(opcode >> 5);
            let shift = (opcode >> 7) & 0x1F;
            self.alu_immediate_shift(
                alu_op,
                rn,
                rd,
                set_condition_codes,
                rm,
                shift_type,
                shift,
                bus,
            );
        }
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn alu_rotated_immediate(
        &mut self,
        op: AluOp,
        rn: u32,
        rd: u32,
        set_condition_codes: bool,
        immediate: u32,
        rotation: u32,
        bus: &mut Bus,
    ) {
        let (operand2, shifter_out) = if rotation != 0 {
            (immediate.rotate_right(rotation), immediate.bit((rotation - 1) as u8))
        } else {
            (immediate, self.registers.cpsr.carry)
        };

        self.alu(op, self.read_register(rn), rd, set_condition_codes, operand2, shifter_out, bus);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn alu_immediate_shift(
        &mut self,
        op: AluOp,
        rn: u32,
        rd: u32,
        set_condition_codes: bool,
        rm: u32,
        shift_type: ShiftType,
        shift: u32,
        bus: &mut Bus,
    ) {
        let value = self.read_register(rm);

        let (operand2, shifter_out) =
            apply_immediate_shift(value, shift_type, shift, self.registers.cpsr.carry);

        self.alu(op, self.read_register(rn), rd, set_condition_codes, operand2, shifter_out, bus);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn alu_register_shift<const OPCODE_LEN: u32>(
        &mut self,
        op: AluOp,
        rn: u32,
        rd: u32,
        set_condition_codes: bool,
        rm: u32,
        shift_type: ShiftType,
        rs: u32,
        bus: &mut Bus,
    ) {
        let value = self.registers.r[rm as usize];
        let shift = self.registers.r[rs as usize] & 0xFF;

        let (operand2, shifter_out) = if shift == 0 {
            (value, self.registers.cpsr.carry)
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

        let operand1 = self.registers.r[rn as usize];

        // Register specified shift adds 1I
        bus.internal_cycles(1);

        self.alu(op, operand1, rd, set_condition_codes, operand2, shifter_out, bus);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn alu(
        &mut self,
        op: AluOp,
        operand1: u32,
        rd: u32,
        set_condition_codes: bool,
        operand2: u32,
        shifter_out: bool,
        bus: &mut Bus,
    ) {
        let (result, codes) = match op {
            AluOp::And | AluOp::Test => {
                let result = operand1 & operand2;
                (result, ConditionCodes::logical(result, shifter_out, self.registers.cpsr.overflow))
            }
            AluOp::ExclusiveOr | AluOp::TestEqual => {
                let result = operand1 ^ operand2;
                (result, ConditionCodes::logical(result, shifter_out, self.registers.cpsr.overflow))
            }
            AluOp::Subtract | AluOp::Compare => alu_add(operand1, !operand2, true),
            AluOp::ReverseSubtract => alu_add(operand2, !operand1, true),
            AluOp::Add | AluOp::CompareNegate => alu_add(operand1, operand2, false),
            AluOp::AddCarry => alu_add(operand1, operand2, self.registers.cpsr.carry),
            AluOp::SubtractCarry => alu_add(operand1, !operand2, self.registers.cpsr.carry),
            AluOp::ReverseSubtractCarry => alu_add(operand2, !operand1, self.registers.cpsr.carry),
            AluOp::Or => {
                let result = operand1 | operand2;
                (result, ConditionCodes::logical(result, shifter_out, self.registers.cpsr.overflow))
            }
            AluOp::Move => (
                operand2,
                ConditionCodes::logical(operand2, shifter_out, self.registers.cpsr.overflow),
            ),
            AluOp::BitClear => {
                let result = operand1 & !operand2;
                (result, ConditionCodes::logical(result, shifter_out, self.registers.cpsr.overflow))
            }
            AluOp::MoveNegate => (
                !operand2,
                ConditionCodes::logical(!operand2, shifter_out, self.registers.cpsr.overflow),
            ),
        };

        if set_condition_codes {
            if rd == 15 {
                self.spsr_to_cpsr();
            } else {
                self.registers.cpsr.sign = codes.sign;
                self.registers.cpsr.zero = codes.zero;
                self.registers.cpsr.carry = codes.carry;
                self.registers.cpsr.overflow = codes.overflow;
            }
        }

        if !op.is_test() {
            self.registers.r[rd as usize] = result;

            if rd == 15 {
                self.refill_prefetch(bus);
            }
        }
    }

    fn arm_multiply(&mut self, opcode: u32, bus: &mut Bus) {
        let rm = opcode & 0xF;
        let rs = (opcode >> 8) & 0xF;
        let rn = (opcode >> 12) & 0xF;
        let rd = (opcode >> 16) & 0xF;
        let set_condition_codes = opcode.bit(20);
        let accumulate = opcode.bit(21);

        self.multiply(rm, rs, rn, rd, set_condition_codes, accumulate, bus);
    }

    #[allow(clippy::too_many_arguments)]
    fn multiply(
        &mut self,
        rm: u32,
        rs: u32,
        rn: u32,
        rd: u32,
        set_condition_codes: bool,
        accumulate: bool,
        bus: &mut Bus,
    ) {
        let operand = self.registers.r[rs as usize];
        let mut product = self.registers.r[rm as usize].wrapping_mul(operand);
        if accumulate {
            product = product.wrapping_add(self.registers.r[rn as usize]);
        }

        self.registers.r[rd as usize] = product;

        if set_condition_codes {
            self.registers.cpsr.sign = product.bit(31);
            self.registers.cpsr.zero = product == 0;
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

        let i_cycles = m + u32::from(accumulate);
        bus.internal_cycles(i_cycles);
    }

    fn arm_multiply_long(&mut self, opcode: u32, bus: &mut Bus) {
        let rm = opcode & 0xF;
        let rs = (opcode >> 8) & 0xF;
        let rdlo = (opcode >> 12) & 0xF;
        let rdhi = (opcode >> 16) & 0xF;
        let set_condition_codes = opcode.bit(20);
        let accumulate = opcode.bit(21);
        let signed = opcode.bit(22);

        self.multiply_long(rm, rs, rdlo, rdhi, set_condition_codes, accumulate, signed, bus);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn multiply_long(
        &mut self,
        rm: u32,
        rs: u32,
        rdlo: u32,
        rdhi: u32,
        set_condition_codes: bool,
        accumulate: bool,
        signed: bool,
        bus: &mut Bus,
    ) {
        let operand = self.registers.r[rs as usize];
        let product = if signed {
            let mut product =
                i64::from(self.registers.r[rm as usize] as i32) * i64::from(operand as i32);
            if accumulate {
                let existing = (i64::from(self.registers.r[rdhi as usize]) << 32)
                    | i64::from(self.registers.r[rdlo as usize]);
                product = product.wrapping_add(existing);
            }
            product as u64
        } else {
            let mut product = u64::from(self.registers.r[rm as usize]) * u64::from(operand);
            if accumulate {
                let existing = (u64::from(self.registers.r[rdhi as usize]) << 32)
                    | u64::from(self.registers.r[rdlo as usize]);
                product = product.wrapping_add(existing);
            }
            product
        };

        self.registers.r[rdlo as usize] = product as u32;
        self.registers.r[rdhi as usize] = (product >> 32) as u32;

        if set_condition_codes {
            self.registers.cpsr.sign = product.bit(63);
            self.registers.cpsr.zero = product == 0;
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

        let i_cycles = 1 + m + u32::from(accumulate);
        bus.internal_cycles(i_cycles);
    }

    // LDR: Load word
    // STR: Store word
    fn arm_load_word<const LOAD: bool, const REGISTER_OFFSET: bool>(
        &mut self,
        opcode: u32,
        bus: &mut Bus,
    ) {
        let indexing = LoadIndexing::from_bit(opcode.bit(24));
        let index_op = IndexOp::from_bit(opcode.bit(23));
        let size = LoadSize::from_bit(opcode.bit(22));
        let write_back = WriteBack::from_bit(opcode.bit(21));
        let rn = (opcode >> 16) & 0xF;
        let rd = (opcode >> 12) & 0xF;

        let offset = if REGISTER_OFFSET {
            let rm = opcode & 0xF;
            let value = self.registers.r[rm as usize];
            let shift_type = ShiftType::from_bits(opcode >> 5);
            let shift = (opcode >> 7) & 0x1F;

            let (shifted, _) =
                apply_immediate_shift(value, shift_type, shift, self.registers.cpsr.carry);
            shifted
        } else {
            opcode & 0xFFF
        };

        self.load_word::<LOAD>(rn, rd, offset, size, indexing, index_op, write_back, bus);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn load_word<const LOAD: bool>(
        &mut self,
        rn: u32,
        rd: u32,
        mut offset: u32,
        size: LoadSize,
        indexing: LoadIndexing,
        index_op: IndexOp,
        write_back: WriteBack,
        bus: &mut Bus,
    ) {
        if index_op == IndexOp::Subtract {
            offset = (!offset).wrapping_add(1);
        }

        let mut address = self.read_register(rn);

        if indexing == LoadIndexing::Pre {
            address = address.wrapping_add(offset);
        }

        if LOAD {
            self.registers.r[rd as usize] = match size {
                LoadSize::Word => {
                    let word = bus.read_word(address, MemoryCycle::N);
                    word.rotate_right(8 * (address & 3))
                }
                LoadSize::Byte => bus.read_byte(address, MemoryCycle::N).into(),
            };

            bus.internal_cycles(1);

            if rd == 15 {
                self.refill_prefetch(bus);
            }
        } else {
            let value = self.registers.r[rd as usize];
            match size {
                LoadSize::Word => bus.write_word(address, value, MemoryCycle::N),
                LoadSize::Byte => bus.write_byte(address, value as u8, MemoryCycle::N),
            }

            // Next opcode fetch is N
            self.fetch_cycle = MemoryCycle::N;
        }

        // Write back only applies on loads if the base register and destination register are different
        if !(LOAD && rn == rd) {
            if indexing == LoadIndexing::Post {
                self.registers.r[rn as usize] = address.wrapping_add(offset);
                if rn == 15 {
                    self.refill_prefetch(bus);
                }
            } else if write_back == WriteBack::Yes {
                self.registers.r[rn as usize] = address;
                if rn == 15 {
                    self.refill_prefetch(bus);
                }
            }
        }
    }

    // LDRH: Load halfword
    // STRH: Store halfword
    // LDRSB: Load sign-extended byte
    // LDRSH: Load sign-extended halfword
    fn arm_load_halfword<const LOAD: bool, const IMMEDIATE_OFFSET: bool>(
        &mut self,
        opcode: u32,
        bus: &mut Bus,
    ) {
        let indexing = LoadIndexing::from_bit(opcode.bit(24));
        let index_op = IndexOp::from_bit(opcode.bit(23));
        let write_back = WriteBack::from_bit(opcode.bit(21));
        let rn = (opcode >> 16) & 0xF;
        let rd = (opcode >> 12) & 0xF;

        let offset = if IMMEDIATE_OFFSET {
            ((opcode >> 4) & 0xF0) | (opcode & 0xF)
        } else {
            let rm = opcode & 0xF;
            self.read_register(rm)
        };

        let load_type = HalfwordLoadType::from_bits(opcode >> 5);

        self.load_halfword::<LOAD>(rn, rd, offset, load_type, indexing, index_op, write_back, bus);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn load_halfword<const LOAD: bool>(
        &mut self,
        rn: u32,
        rd: u32,
        mut offset: u32,
        load_type: HalfwordLoadType,
        indexing: LoadIndexing,
        index_op: IndexOp,
        write_back: WriteBack,
        bus: &mut Bus,
    ) {
        if index_op == IndexOp::Subtract {
            offset = (!offset).wrapping_add(1);
        }

        let mut address = self.read_register(rn);
        if indexing == LoadIndexing::Pre {
            address = address.wrapping_add(offset);
        }

        if LOAD {
            let value = match load_type {
                HalfwordLoadType::UnsignedHalfword => {
                    let halfword: u32 = bus.read_halfword(address, MemoryCycle::N).into();
                    halfword.rotate_right(8 * (address & 1))
                }
                HalfwordLoadType::SignedByte => bus.read_byte(address, MemoryCycle::N) as i8 as u32,
                HalfwordLoadType::SignedHalfword => {
                    if !address.bit(0) {
                        bus.read_halfword(address, MemoryCycle::N) as i16 as u32
                    } else {
                        // Unaligned LDRSH seems to behave the same as LDRSB
                        bus.read_byte(address, MemoryCycle::N) as i8 as u32
                    }
                }
            };
            self.registers.r[rd as usize] = value;

            bus.internal_cycles(1);

            if rd == 15 {
                self.refill_prefetch(bus);
            }
        } else {
            let halfword = self.registers.r[rd as usize] as u16;
            bus.write_halfword(address, halfword, MemoryCycle::N);

            // Next opcode fetch is N
            self.fetch_cycle = MemoryCycle::N;
        }

        // Write back only applies on loads if the base register and destination register are different
        if !(LOAD && rn == rd) {
            if indexing == LoadIndexing::Post {
                self.registers.r[rn as usize] = address.wrapping_add(offset);
            } else if write_back == WriteBack::Yes {
                self.registers.r[rn as usize] = address;
            }
        }
    }

    fn arm_ldm_stm<const LOAD: bool, const INCREMENT: bool, const AFTER: bool>(
        &mut self,
        opcode: u32,
        bus: &mut Bus,
    ) {
        let register_bits = opcode & 0xFFFF;
        let rn = (opcode >> 16) & 0xF;
        let write_back = WriteBack::from_bit(opcode.bit(21));
        let s_bit = opcode.bit(22);

        self.load_multiple::<LOAD, INCREMENT, AFTER>(register_bits, rn, write_back, s_bit, bus);
    }

    #[inline]
    fn load_multiple<const LOAD: bool, const INCREMENT: bool, const AFTER: bool>(
        &mut self,
        mut register_bits: u32,
        rn: u32,
        write_back: WriteBack,
        s_bit: bool,
        bus: &mut Bus,
    ) {
        let mut empty_list = false;
        if register_bits == 0 {
            // Hardware quirk: empty list loads/stores only R15, and Rb is adjusted by 4 * 16
            register_bits = 1 << 15;
            empty_list = true;
        }

        let count = register_bits.count_ones();
        let r15_loaded = register_bits.bit(15);

        let base_addr = self.read_register(rn);
        let count_for_final_addr = if empty_list { 16 } else { count };
        let final_addr = if INCREMENT {
            base_addr.wrapping_add(4 * count_for_final_addr)
        } else {
            base_addr.wrapping_sub(4 * count_for_final_addr)
        };

        let mut address = if INCREMENT { base_addr } else { final_addr };

        let mut first = true;
        let mut need_write_back = write_back == WriteBack::Yes;
        for r in 0..16 {
            if !register_bits.bit(r) {
                continue;
            }

            let memory_cycle = if first {
                first = false;
                MemoryCycle::N
            } else {
                MemoryCycle::S
            };

            if LOAD && need_write_back {
                self.registers.r[rn as usize] = final_addr;
                log::trace!("  Wrote back to R{rn}: {:08X}", self.registers.r[rn as usize]);
                need_write_back = false;
            }

            if !(AFTER ^ !INCREMENT) {
                address = address.wrapping_add(4);
            }

            if LOAD {
                if r == 15 {
                    self.registers.r[15] = bus.read_word(address, memory_cycle);

                    if s_bit {
                        self.spsr_to_cpsr();
                    }
                } else if s_bit && !r15_loaded {
                    let register = get_user_register(&mut self.registers, r.into());
                    *register = bus.read_word(address, memory_cycle);
                } else {
                    self.registers.r[r as usize] = bus.read_word(address, memory_cycle);
                }
                log::trace!("  LDM: Loaded R{r} from {address:08X}");
            } else {
                let value = if s_bit {
                    *get_user_register(&mut self.registers, r.into())
                } else {
                    self.registers.r[r as usize]
                };
                bus.write_word(address, value, memory_cycle);
                log::trace!("  STM: Stored R{r} to {address:08X}");
            }

            if AFTER ^ !INCREMENT {
                address = address.wrapping_add(4);
            }

            if !LOAD && need_write_back {
                self.registers.r[rn as usize] = final_addr;
                log::trace!("  Wrote back to R{rn}: {:08X}", self.registers.r[rn as usize]);
                need_write_back = false;
            }
        }

        if LOAD {
            bus.internal_cycles(1);

            if r15_loaded {
                self.refill_prefetch(bus);
            }
        } else {
            // Next opcode fetch is N
            self.fetch_cycle = MemoryCycle::N;
        }
    }

    fn arm_swi(&mut self, _opcode: u32, bus: &mut Bus) {
        self.handle_exception(Exception::SoftwareInterrupt, bus);
    }

    // Format 1: Move shifted register
    fn thumb_move_shifted_register(&mut self, opcode: u16, bus: &mut Bus) {
        let rd = opcode & 7;
        let rs = (opcode >> 3) & 7;
        let shift = (opcode >> 6) & 0x1F;
        let shift_type = ShiftType::from_bits((opcode >> 11).into());

        self.alu_immediate_shift(
            AluOp::Move,
            rd.into(),
            rd.into(),
            true,
            rs.into(),
            shift_type,
            shift.into(),
            bus,
        );
    }

    // Format 2: Add/subtract
    fn thumb_add_sub(&mut self, opcode: u16, bus: &mut Bus) {
        let rd = opcode & 7;
        let rs = (opcode >> 3) & 7;
        let rn = (opcode >> 6) & 7;
        let alu_op = if opcode.bit(9) { AluOp::Subtract } else { AluOp::Add };

        let immediate = opcode.bit(10);
        if immediate {
            self.alu_rotated_immediate(alu_op, rs.into(), rd.into(), true, rn.into(), 0, bus);
        } else {
            self.alu_immediate_shift(
                alu_op,
                rs.into(),
                rd.into(),
                true,
                rn.into(),
                ShiftType::Left,
                0,
                bus,
            );
        }
    }

    // Format 3: Move/compare/add/subtract immediate
    fn thumb_alu_immediate(&mut self, opcode: u16, bus: &mut Bus) {
        let immediate = opcode & 0xFF;
        let rd = (opcode >> 8) & 7;
        let alu_op = match (opcode >> 11) & 3 {
            0 => AluOp::Move,
            1 => AluOp::Compare,
            2 => AluOp::Add,
            3 => AluOp::Subtract,
            _ => unreachable!("value & 3 is always <= 3"),
        };

        self.alu_rotated_immediate(alu_op, rd.into(), rd.into(), true, immediate.into(), 0, bus);
    }

    // Format 4: ALU operations
    fn thumb_alu(&mut self, opcode: u16, bus: &mut Bus) {
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

                return self.alu_register_shift::<THUMB_OPCODE_LEN>(
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
                return self.alu_rotated_immediate(
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
                return self.multiply(rs.into(), rd.into(), rd.into(), rd.into(), true, false, bus);
            }
            0xE => AluOp::BitClear,
            0xF => AluOp::MoveNegate,
            _ => unreachable!("value & 0xF is always <= 0xF"),
        };

        self.alu_immediate_shift(
            basic_op,
            rd.into(),
            rd.into(),
            true,
            rs.into(),
            ShiftType::Left,
            0,
            bus,
        );
    }

    // Format 5: Hi register operations / branch exchange
    fn thumb_high_register_op(&mut self, opcode: u16, bus: &mut Bus) {
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
            3 => return self.branch_exchange(rs.into(), bus),
            _ => unreachable!("value & 3 is always <= 3"),
        };

        let set_condition_codes = alu_op == AluOp::Compare;

        self.alu_immediate_shift(
            alu_op,
            rd.into(),
            rd.into(),
            set_condition_codes,
            rs.into(),
            ShiftType::Left,
            0,
            bus,
        );
    }

    // Format 6: PC-relative load
    fn thumb_pc_relative_load(&mut self, opcode: u16, bus: &mut Bus) {
        // Bit 1 of PC is forcibly cleared for PC-relative loads
        let mut offset: u32 = ((opcode & 0xFF) << 2).into();
        offset = offset.wrapping_sub(self.prev_r15 & 2);

        let rd = (opcode >> 8) & 7;

        self.load_word::<true>(
            15,
            rd.into(),
            offset,
            LoadSize::Word,
            LoadIndexing::Pre,
            IndexOp::Add,
            WriteBack::No,
            bus,
        );
    }

    // Format 7: Load/store with register offset
    fn thumb_load_register_offset(&mut self, opcode: u16, bus: &mut Bus) {
        let rd = opcode & 7;
        let rb = (opcode >> 3) & 7;
        let ro = (opcode >> 6) & 7;
        let size = LoadSize::from_bit(opcode.bit(10));
        let load = opcode.bit(11);

        let offset = self.read_register(ro.into());

        if load {
            self.load_word::<true>(
                rb.into(),
                rd.into(),
                offset,
                size,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        } else {
            self.load_word::<false>(
                rb.into(),
                rd.into(),
                offset,
                size,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        }
    }

    // Format 8: Load/store sign-extended byte/halfword
    fn thumb_load_sign_extended(&mut self, opcode: u16, bus: &mut Bus) {
        let rd = opcode & 7;
        let rb = (opcode >> 3) & 7;
        let ro = (opcode >> 6) & 7;

        let offset = self.read_register(ro.into());

        let sh_bits = (opcode >> 10) & 3;
        if sh_bits == 0 {
            // STRH
            self.load_halfword::<false>(
                rb.into(),
                rd.into(),
                offset,
                HalfwordLoadType::UnsignedHalfword,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        } else {
            let load_type = match sh_bits {
                1 => HalfwordLoadType::SignedByte,
                2 => HalfwordLoadType::UnsignedHalfword,
                3 => HalfwordLoadType::SignedHalfword,
                _ => unreachable!(),
            };

            self.load_halfword::<true>(
                rb.into(),
                rd.into(),
                offset,
                load_type,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        }
    }

    // Format 9: Load/store with immediate offset
    fn thumb_load_immediate_offset(&mut self, opcode: u16, bus: &mut Bus) {
        let rd = opcode & 7;
        let rb = (opcode >> 3) & 7;
        let mut offset = (opcode >> 6) & 0x1F;
        let load = opcode.bit(11);
        let size = LoadSize::from_bit(opcode.bit(12));

        if size == LoadSize::Word {
            offset <<= 2;
        }

        if load {
            self.load_word::<true>(
                rb.into(),
                rd.into(),
                offset.into(),
                size,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        } else {
            self.load_word::<false>(
                rb.into(),
                rd.into(),
                offset.into(),
                size,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        }
    }

    // Format 10: Load/store halfword
    fn thumb_load_halfword(&mut self, opcode: u16, bus: &mut Bus) {
        let rd = opcode & 7;
        let rb = (opcode >> 3) & 7;
        let offset = ((opcode >> 6) & 0x1F) << 1;
        let load = opcode.bit(11);

        if load {
            self.load_halfword::<true>(
                rb.into(),
                rd.into(),
                offset.into(),
                HalfwordLoadType::UnsignedHalfword,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        } else {
            self.load_halfword::<false>(
                rb.into(),
                rd.into(),
                offset.into(),
                HalfwordLoadType::UnsignedHalfword,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        }
    }

    // Format 11: SP-relative load/store
    fn thumb_load_sp_relative(&mut self, opcode: u16, bus: &mut Bus) {
        let offset = (opcode & 0xFF) << 2;
        let rd = (opcode >> 8) & 7;
        let load = opcode.bit(11);

        if load {
            self.load_word::<true>(
                13,
                rd.into(),
                offset.into(),
                LoadSize::Word,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        } else {
            self.load_word::<false>(
                13,
                rd.into(),
                offset.into(),
                LoadSize::Word,
                LoadIndexing::Pre,
                IndexOp::Add,
                WriteBack::No,
                bus,
            );
        }
    }

    // Format 12: Load address
    fn thumb_load_address(&mut self, opcode: u16, bus: &mut Bus) {
        let mut immediate: u32 = ((opcode & 0xFF) << 2).into();
        let rd = (opcode >> 8) & 7;

        // Bit 11 selects between SP (R13) and PC (R15)
        let rn = if opcode.bit(11) { 13 } else { 15 };

        // Bit 1 is forced clear when PC is used as Rn
        if rn == 15 {
            immediate = immediate.wrapping_sub(self.prev_r15 & 2);
        }

        self.alu_rotated_immediate(AluOp::Add, rn, rd.into(), false, immediate, 0, bus);
    }

    // Format 13: Add offset to stack pointer
    fn thumb_add_offset_sp(&mut self, opcode: u16, bus: &mut Bus) {
        let mut offset: u32 = ((opcode & 0x7F) << 2).into();
        if opcode.bit(7) {
            offset = (!offset).wrapping_add(1);
        }

        self.alu_rotated_immediate(AluOp::Add, 13, 13, false, offset, 0, bus);
    }

    // Format 14: Push/pop registers
    fn thumb_push_pop(&mut self, opcode: u16, bus: &mut Bus) {
        let lr_pc_bit = opcode.bit(8);
        let load = opcode.bit(11);

        let mut register_bits = opcode & 0xFF;
        if lr_pc_bit {
            // Store LR, load PC
            register_bits |= 1 << (14 + u32::from(load));
        }

        if load {
            self.load_multiple::<true, true, true>(
                register_bits.into(),
                13,
                WriteBack::Yes,
                false,
                bus,
            );
        } else {
            self.load_multiple::<false, false, false>(
                register_bits.into(),
                13,
                WriteBack::Yes,
                false,
                bus,
            );
        }
    }

    // Format 15: Multiple load/store
    fn thumb_load_multiple(&mut self, opcode: u16, bus: &mut Bus) {
        let register_bits = opcode & 0xFF;
        let rb = (opcode >> 8) & 7;
        let load = opcode.bit(11);

        if load {
            self.load_multiple::<true, true, true>(
                register_bits.into(),
                rb.into(),
                WriteBack::Yes,
                false,
                bus,
            );
        } else {
            self.load_multiple::<false, true, true>(
                register_bits.into(),
                rb.into(),
                WriteBack::Yes,
                false,
                bus,
            );
        }
    }

    // Format 16: Conditional branch
    fn thumb_conditional_branch(&mut self, opcode: u16, bus: &mut Bus) {
        let condition = Condition::from_bits((opcode >> 8).into());
        if !condition.check(self.registers.cpsr) {
            return;
        }

        let offset = i32::from(opcode as i8) << 1;
        self.branch::<false, THUMB_OPCODE_LEN>(offset, bus);
    }

    // Format 17: Software interrupt
    fn thumb_software_interrupt(&mut self, _opcode: u16, bus: &mut Bus) {
        self.handle_exception(Exception::SoftwareInterrupt, bus);
    }

    // Format 18: Unconditional branch
    fn thumb_unconditional_branch(&mut self, opcode: u16, bus: &mut Bus) {
        let offset = (i32::from(opcode & 0x7FF) << 21) >> 20;
        self.branch::<false, THUMB_OPCODE_LEN>(offset, bus);
    }

    // Format 19: Long branch with link
    // This instruction has no ARM equivalent
    fn thumb_long_branch(&mut self, opcode: u16, bus: &mut Bus) {
        // Offset is a signed 23-bit value split across two opcodes, with 11 bits in each
        // First opcode has H=0 and second opcode has H=1
        // It is possible to have an H=1 opcode without an H=0 opcode; Golden Sun: The Lost Age does this
        if !opcode.bit(11) {
            // First opcode: Write highest 11 bits of jump address to LR
            let unsigned_offset = i32::from(opcode & 0x7FF) << 12;

            // Clip to signed 23-bit
            let offset = (unsigned_offset << 9) >> 9;

            self.registers.r[14] = self.prev_r15.wrapping_add_signed(offset);
        } else {
            // Second opcode: Add lowest 11 bits of jump address to LR, jump, and write return address to LR
            let offset_low = u32::from(opcode & 0x7FF) << 1;
            let jump_address = self.registers.r[14].wrapping_add(offset_low);

            let return_address = self.prev_r15.wrapping_sub(2);
            self.registers.r[14] = return_address | 1;

            self.registers.r[15] = jump_address;
            self.refill_prefetch(bus);
        }
    }
}

#[inline]
fn arm_parse_rotated_immediate(opcode: u32) -> (u32, u32) {
    let immediate = opcode & 0xFF;
    let rotation = ((opcode >> 8) & 0xF) << 1;
    (immediate, rotation)
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

fn alu_add(operand1: u32, operand2: u32, carry_in: bool) -> (u32, ConditionCodes) {
    let carry_in: u32 = carry_in.into();

    let (partial_sum, carry1) = operand1.overflowing_add(operand2);
    let (sum, carry2) = partial_sum.overflowing_add(carry_in);
    let carry = carry1 || carry2;

    let bit_30_carry = (operand1 & 0x7FFFFFFF) + (operand2 & 0x7FFFFFFF) + carry_in >= 0x80000000;
    let overflow = bit_30_carry != carry;

    (sum, ConditionCodes { sign: sum.bit(31), zero: sum == 0, carry, overflow })
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
