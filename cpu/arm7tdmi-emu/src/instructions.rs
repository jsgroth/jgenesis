mod disassemble;

use crate::bus::BusInterface;
use crate::{Arm7Tdmi, CpuState, StatusRegister};
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
    fn from_arm_opcode(opcode: u32) -> Self {
        match opcode >> 28 {
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
            _ => unreachable!("(u32 >> 28) is always <= 0xF"),
        }
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
    ArmDecodeEntry::new(0b1111_1111_1111, 0b0001_0010_0001, |_, opcode, _| {
        todo!("BX {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1111_1100_1111, 0b0000_0000_1001, |_, opcode, _| {
        todo!("MUL/MLA {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1111_1000_1111, 0b0000_1000_1001, |_, opcode, _| {
        todo!("MULL/MLAL {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1111_1011_1111, 0b0001_0000_1001, |_, opcode, _| {
        todo!("SWP {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0000_1001, arm_load_halfword::<false, false>),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0100_1001, arm_load_halfword::<false, true>),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0001_1001, arm_load_halfword::<true, false>),
    ArmDecodeEntry::new(0b1110_0101_1001, 0b0000_0101_1001, arm_load_halfword::<true, true>),
    ArmDecodeEntry::new(0b1110_0100_1001, 0b0000_0100_1001, |_, opcode, _| {
        todo!("LDRH/STRH immediate offset {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1111_1011_1111, 0b0001_0000_0000, |_, opcode, _| {
        todo!("MRS {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1111_1011_0000, 0b0001_0010_0000, |_, opcode, _| {
        todo!("MSR register operand {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1111_1011_0000, 0b0011_0010_0000, |_, opcode, _| {
        todo!("MSR immediate operand {opcode:08X}")
    }),
    ArmDecodeEntry::new(0b1110_0000_0001, 0b0000_0000_0000, arm_alu::<false, false>),
    ArmDecodeEntry::new(0b1110_0000_0001, 0b0000_0000_0001, arm_alu::<false, true>),
    ArmDecodeEntry::new(0b1110_0000_0000, 0b0010_0000_0000, arm_alu::<true, false>),
    ArmDecodeEntry::new(0b1111_0000_0000, 0b1010_0000_0000, arm_branch::<false>),
    ArmDecodeEntry::new(0b1111_0000_0000, 0b1011_0000_0000, arm_branch::<true>),
];

const ARM_OPCODE_LEN: u32 = 4;
const THUMB_OPCODE_LEN: u32 = 2;

impl Arm7Tdmi {
    pub(crate) fn execute_arm_opcode(&mut self, opcode: u32, bus: &mut impl BusInterface) -> u32 {
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "Executing opcode {opcode:08X}, PC+8={:08X}, str={}",
                self.registers.r[15],
                disassemble::arm(opcode)
            );
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
fn arm_alu<const IMMEDIATE: bool, const REGISTER_SHIFT: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    assert!(!(IMMEDIATE && REGISTER_SHIFT));

    let alu_op = AluOp::from_bits(opcode >> 21);
    let set_condition_codes = opcode.bit(20);
    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;

    if IMMEDIATE {
        let (immediate, rotation) = arm_parse_rotated_immediate(opcode);
        alu_rotated_immediate(cpu, alu_op, rn, rd, set_condition_codes, immediate, rotation, bus)
    } else {
        todo!("execute ALU register operand {opcode:08X}")
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

    let cycles = alu(cpu, op, rn, rd, set_condition_codes, operand2, shifter_out, bus);
    cpu.fetch_opcode(bus);
    cycles
}

#[inline]
fn alu(
    cpu: &mut Arm7Tdmi,
    op: AluOp,
    rn: u32,
    rd: u32,
    set_condition_codes: bool,
    operand2: u32,
    shifter_out: bool,
    bus: &mut dyn BusInterface,
) -> u32 {
    let operand1 = cpu.registers.r[rn as usize];

    let (result, codes) = match op {
        AluOp::Add => alu_add(operand1, operand2, false),
        AluOp::Compare => alu_add(operand1, !operand2, true),
        AluOp::Move => {
            (operand2, ConditionCodes::logical(operand2, shifter_out, cpu.registers.cpsr.overflow))
        }
        _ => todo!("ALU op {op:?}"),
    };

    if !op.is_test() {
        cpu.registers.r[rd as usize] = result;

        if rd == 15 {
            cpu.fetch_opcode(bus);
        }
    }

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

    // 1S always
    // +1N +1S if Rd = 15
    1 + (u32::from(rd == 15) << 1)
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

    if indexing == LoadIndexing::Post {
        cpu.registers.r[rn as usize] = address.wrapping_add(offset);
    } else if write_back == WriteBack::Yes {
        cpu.registers.r[rn as usize] = address;
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
