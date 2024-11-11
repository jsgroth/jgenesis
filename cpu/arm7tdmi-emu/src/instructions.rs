mod disassemble;

use crate::bus::{BusInterface, MemoryCycle};
use crate::{Arm7Tdmi, StatusRegister};
use jgenesis_common::num::{GetBit, SignBit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Condition {
    // EQ (Z=1)
    Equal,
    // NE (Z=0)
    NotEqual,
    // CS (C=1)
    CarrySet,
    // CC (C=0)
    CarryClear,
    // MI (N=1)
    Minus,
    // PL (N=0)
    Plus,
    // VS (V=1)
    OverflowSet,
    // VC (V=0)
    OverflowClear,
    // HI (C=1 and Z=0)
    Higher,
    // LS (C=0 or Z=1)
    LowerOrSame,
    // GE (N=V)
    GreaterOrEqual,
    // LT (N!=V)
    Less,
    // GT (Z=0 and N=V)
    Greater,
    // LE (Z=1 or N!=V)
    LessOrEqual,
    // AL (always true)
    Always,
    // Reserved - treat as always false
    Reserved,
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

impl Arm7Tdmi {
    pub(crate) fn execute_arm_opcode(&mut self, opcode: u32, bus: &mut impl BusInterface) -> u32 {
        let condition = Condition::from_arm_opcode(opcode);

        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "Executing ARM opcode: {opcode:08X}, PC+8={:08X}, str={:?}",
                self.registers.r[15],
                disassemble::arm(opcode)
            );
        }

        if !condition.check(self.registers.cpsr) {
            // 1S for false conditions
            self.fetch_arm_opcode(MemoryCycle::Sequential, bus);
            return 1;
        }

        let opcode_fn = arm_decode(opcode);
        opcode_fn(self, opcode, bus)
    }
}

type ArmFn = fn(&mut Arm7Tdmi, u32, &mut dyn BusInterface) -> u32;

fn arm_decode(opcode: u32) -> ArmFn {
    let mask = ((opcode >> 16) & 0xFF0) | ((opcode >> 4) & 0xF);

    if mask == 0x121 {
        todo!("maybe BX {opcode:08X}")
    }

    if mask & 0xFCF == 0x009 {
        todo!("MUL/MLA {opcode:08X}")
    }

    if mask & 0xFCF == 0x049 {
        todo!("MULL/MLAL {opcode:08X}")
    }

    if mask & 0x1BF == 0x109 {
        todo!("single data swap {opcode:08X}")
    }

    if mask & 0xE49 == 0x009 {
        todo!("halfword data transfer register offset {opcode:08X}")
    }

    if mask & 0xE49 == 0x049 {
        let load = opcode.bit(20);
        return if load {
            arm_load_halfword::<true, true>
        } else {
            arm_load_halfword::<false, true>
        };
    }

    if mask & 0xE00 == 0x200 {
        return arm_alu::<true, false>;
    }

    if mask & 0xE00 == 0xA00 {
        return if opcode.bit(24) { arm_branch::<true> } else { arm_branch::<false> };
    }

    todo!("decode {opcode:08X}")
}

fn arm_branch<const LINK: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    // TODO is this why there's an extra S cycle in branching?
    bus.read_word(cpu.registers.r[15], MemoryCycle::Sequential);

    if LINK {
        cpu.registers.r[14] = cpu.registers.r[15].wrapping_sub(4);
    }

    let offset = (((opcode & 0xFFFFFF) as i32) << 8) >> 6;
    cpu.registers.r[15] = cpu.registers.r[15].wrapping_add_signed(offset);

    cpu.fetch_arm_opcode(MemoryCycle::NonSequential, bus);
    cpu.fetch_arm_opcode(MemoryCycle::Sequential, bus);

    // 2S + 1N
    3
}

fn arm_alu<const IMMEDIATE_OPERAND: bool, const REGISTER_SHIFT: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    if !IMMEDIATE_OPERAND && REGISTER_SHIFT {
        // If shift value will be read from a register, prefetch next opcode before performing the ALU op
        // in case R15 is used as an operand
        cpu.fetch_arm_opcode(MemoryCycle::Sequential, bus);
    }

    let rn = (opcode >> 16) & 0xF;
    let operand1 = cpu.registers.r[rn as usize];

    let (operand2, shifter_out) = if IMMEDIATE_OPERAND {
        let imm = opcode & 0xFF;
        let rotation = ((opcode >> 8) & 0xF) << 1;
        if rotation == 0 {
            (imm, cpu.registers.cpsr.carry)
        } else {
            (imm.rotate_right(rotation), imm.bit((rotation - 1) as u8))
        }
    } else if REGISTER_SHIFT {
        todo!("register shift {opcode:08X}")
    } else {
        todo!("immediate shift {opcode:08X}")
    };

    let operation = (opcode >> 21) & 0xF;
    let (result, flags) = match operation {
        // ADD
        4 => alu_add(operand1, operand2, false),
        // CMP
        10 => alu_sub(operand1, operand2, true),
        // MOV
        13 => (operand2, logical_flags(operand2, shifter_out, cpu.registers.cpsr)),
        _ => todo!("ALU operation {operation} opcode {opcode:08X}"),
    };

    if IMMEDIATE_OPERAND || !REGISTER_SHIFT {
        cpu.fetch_arm_opcode(MemoryCycle::Sequential, bus);
    }

    // All operations other than TST, TEQ, CMP, CMN set Rd
    let rd = (opcode >> 12) & 0xF;
    if !(8..12).contains(&operation) {
        cpu.registers.r[rd as usize] = result;
    }

    if opcode.bit(20) {
        // Set condition codes
        cpu.registers.cpsr.zero = flags.zero;
        cpu.registers.cpsr.sign = flags.sign;
        cpu.registers.cpsr.carry = flags.carry;
        cpu.registers.cpsr.overflow = flags.overflow;
    }

    if rd == 15 {
        cpu.fetch_arm_opcode(MemoryCycle::NonSequential, bus);
        cpu.fetch_arm_opcode(MemoryCycle::Sequential, bus);
    }

    // 1S always
    // +1I if register shift
    // +1N +1S if Rd=R15
    1 + u32::from(REGISTER_SHIFT) + (u32::from(rd == 15) << 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flags {
    zero: bool,
    sign: bool,
    carry: bool,
    overflow: bool,
}

#[inline]
fn logical_flags(result: u32, shifter_out: bool, cpsr: StatusRegister) -> Flags {
    Flags {
        zero: result == 0,
        sign: result.sign_bit(),
        carry: shifter_out,
        overflow: cpsr.overflow,
    }
}

fn alu_add(operand1: u32, operand2: u32, carry_in: bool) -> (u32, Flags) {
    let carry_in: u32 = carry_in.into();

    let (partial_sum, carry1) = operand1.overflowing_add(operand2);
    let (sum, carry2) = partial_sum.overflowing_add(carry_in);
    let carry = carry1 || carry2;

    let bit_30_carry = (operand1 & 0x7FFFFFFF) + (operand2 & 0x7FFFFFFF) + carry_in >= 0x80000000;
    let overflow = bit_30_carry ^ carry;

    let flags = Flags { zero: sum == 0, sign: sum.sign_bit(), carry, overflow };

    (sum, flags)
}

fn alu_sub(operand1: u32, operand2: u32, carry_in: bool) -> (u32, Flags) {
    let borrow_in: u32 = (!carry_in).into();

    let (partial_diff, borrow1) = operand1.overflowing_sub(operand2);
    let (difference, borrow2) = partial_diff.overflowing_sub(borrow_in);
    let borrow = borrow1 || borrow2;

    let bit_30_borrow = operand1 & 0x7FFFFFFF < (operand2 & 0x7FFFFFFF) + borrow_in;
    let overflow = bit_30_borrow != borrow;

    let flags =
        Flags { zero: difference == 0, sign: difference.sign_bit(), carry: !borrow, overflow };

    (difference, flags)
}

fn arm_load_halfword<const LOAD: bool, const IMMEDIATE: bool>(
    cpu: &mut Arm7Tdmi,
    opcode: u32,
    bus: &mut dyn BusInterface,
) -> u32 {
    if !IMMEDIATE {
        todo!("load halfword register offset")
    }

    let mut offset = if IMMEDIATE { ((opcode >> 4) & 0xF0) | (opcode & 0xF) } else { todo!() };

    let write_back = opcode.bit(21);
    let add = opcode.bit(23);
    let pre_indexed = opcode.bit(24);
    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;

    if !add {
        offset = (!offset).wrapping_add(1);
    }

    let address = if pre_indexed {
        cpu.registers.r[rn as usize].wrapping_add(offset)
    } else {
        cpu.registers.r[rn as usize]
    };

    if LOAD {
        cpu.fetch_arm_opcode(MemoryCycle::Sequential, bus);
        // TODO is the next opcode prefetch supposed to be N instead of S?

        match (opcode >> 5) & 3 {
            0 => panic!("LDRH function called for SWP"),
            1 => {
                // LDRH
                let halfword = bus.read_halfword(address, MemoryCycle::NonSequential);
                cpu.registers.r[rd as usize] = halfword.into();
            }
            2 => {
                // LDRSB
                let byte = bus.read_byte(address, MemoryCycle::NonSequential);
                cpu.registers.r[rd as usize] = byte as i8 as u32;
            }
            3 => {
                // LDRSH
                let halfword = bus.read_halfword(address, MemoryCycle::NonSequential);
                cpu.registers.r[rd as usize] = halfword as i16 as u32;
            }
            _ => unreachable!("value & 3 is always <=3 3"),
        };

        if !pre_indexed {
            cpu.registers.r[rn as usize] = address.wrapping_add(offset);
        } else if write_back {
            cpu.registers.r[rn as usize] = address;
        }

        if rd == 15 {
            cpu.fetch_arm_opcode(MemoryCycle::NonSequential, bus);
            cpu.fetch_arm_opcode(MemoryCycle::Sequential, bus);
        }

        // 1S + 1N + 1I always
        // +1S +1N if loaded R15
        3 + (u32::from(rd == 15) << 1)
    } else {
        cpu.fetch_arm_opcode(MemoryCycle::NonSequential, bus);

        bus.write_halfword(
            address,
            cpu.registers.r[rd as usize] as u16,
            MemoryCycle::NonSequential,
        );

        if !pre_indexed {
            cpu.registers.r[rn as usize] = address.wrapping_add(offset);
        } else if write_back {
            cpu.registers.r[rn as usize] = address;
        }

        // 2N
        2
    }
}
