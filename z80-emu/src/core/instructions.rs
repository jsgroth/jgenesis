mod parser;

use crate::core::instructions::parser::{InstructionParser, ParseResult};
use crate::core::{IndexRegister, Register16, Register8, Registers};
use crate::traits::AddressSpace;
use std::mem;

trait GetBit: Copy {
    fn bit(self, i: u8) -> bool;
}

impl GetBit for u8 {
    fn bit(self, i: u8) -> bool {
        assert!(i <= 7);
        self & (1 << i) != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ReadTarget8 {
    Immediate(u8),
    Register(Register8),
    Indirect(Register16),
    Direct(u16),
    Indexed(IndexRegister, i8),
}

impl ReadTarget8 {
    fn read<A: AddressSpace>(self, registers: &Registers, address_space: &mut A) -> u8 {
        match self {
            Self::Immediate(n) => n,
            Self::Register(register) => register.read(registers),
            Self::Indirect(register) => {
                let address = register.read(registers);
                address_space.read(address)
            }
            Self::Direct(address) => address_space.read(address),
            Self::Indexed(index, d) => {
                let address = index_address(index.read(registers), d);
                address_space.read(address)
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Immediate(..) | Self::Register(..) => 0,
            Self::Indirect(..) | Self::Direct(..) => 3,
            Self::Indexed(..) => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTarget8 {
    Register(Register8),
    Indirect(Register16),
    Direct(u16),
    Indexed(IndexRegister, i8),
}

impl WriteTarget8 {
    fn write<A: AddressSpace>(self, registers: &mut Registers, address_space: &mut A, value: u8) {
        match self {
            Self::Register(register) => {
                register.write(registers, value);
            }
            Self::Indirect(register) => {
                let address = register.read(registers);
                address_space.write(address, value);
            }
            Self::Direct(address) => {
                address_space.write(address, value);
            }
            Self::Indexed(index, d) => {
                let address = index_address(index.read(registers), d);
                address_space.write(address, value);
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Register(..) => 0,
            Self::Indirect(..) | Self::Direct(..) => 3,
            Self::Indexed(..) => 8,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ModifyResult {
    original: u8,
    modified: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifyTarget8 {
    Register(Register8),
    Indirect(Register16),
    Indexed(IndexRegister, i8),
}

impl ModifyTarget8 {
    fn read<A: AddressSpace>(self, registers: &Registers, address_space: &mut A) -> u8 {
        match self {
            Self::Register(register) => {
                ReadTarget8::Register(register).read(registers, address_space)
            }
            Self::Indirect(register) => {
                ReadTarget8::Indirect(register).read(registers, address_space)
            }
            Self::Indexed(index, d) => {
                ReadTarget8::Indexed(index, d).read(registers, address_space)
            }
        }
    }

    fn write<A: AddressSpace>(self, registers: &mut Registers, address_space: &mut A, value: u8) {
        match self {
            Self::Register(register) => {
                WriteTarget8::Register(register).write(registers, address_space, value);
            }
            Self::Indirect(register) => {
                WriteTarget8::Indirect(register).write(registers, address_space, value);
            }
            Self::Indexed(index, d) => {
                WriteTarget8::Indexed(index, d).write(registers, address_space, value);
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Register(..) => 0,
            Self::Indirect(..) => 7,
            Self::Indexed(..) => 12,
        }
    }
}

fn index_address(address: u16, d: i8) -> u16 {
    (i32::from(address) + i32::from(d)) as u16
}

#[derive(Debug, Clone, Copy)]
pub enum ReadTarget16 {
    Immediate(u16),
    Register(Register16),
    Direct(u16),
}

impl ReadTarget16 {
    fn read<A: AddressSpace>(self, registers: &Registers, address_space: &mut A) -> u16 {
        match self {
            Self::Immediate(nn) => nn,
            Self::Register(register) => register.read(registers),
            Self::Direct(nn) => {
                let lsb = address_space.read(nn);
                let msb = address_space.read(nn.wrapping_add(1));
                u16::from_le_bytes([lsb, msb])
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Immediate(..) | Self::Register(..) => 0,
            Self::Direct(..) => 6,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum WriteTarget16 {
    Register(Register16),
    Direct(u16),
}

impl WriteTarget16 {
    fn write<A: AddressSpace>(self, registers: &mut Registers, address_space: &mut A, value: u16) {
        match self {
            Self::Register(register) => {
                register.write(registers, value);
            }
            Self::Direct(nn) => {
                let [lsb, msb] = value.to_le_bytes();
                address_space.write(nn, lsb);
                address_space.write(nn.wrapping_add(1), msb);
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Register(..) => 0,
            Self::Direct(..) => 6,
        }
    }
}

pub struct ExecuteResult {
    t_cycles: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum Instruction {
    Load8(WriteTarget8, ReadTarget8),
    Load16(WriteTarget16, ReadTarget16),
    Push(Register16),
    Pop(Register16),
    ExchangeDEHL,
    ExchangeAF,
    ExchangeGeneralPurpose,
    ExchangeStack(Register16),
    Transfer(TransferMode),
    CompareBlock(TransferMode),
    Add {
        read_target: ReadTarget8,
        with_carry: bool,
    },
    Subtract {
        read_target: ReadTarget8,
        with_carry: bool,
    },
    And(ReadTarget8),
    Or(ReadTarget8),
    Xor(ReadTarget8),
    Compare(ReadTarget8),
    Increment(ModifyTarget8),
    Decrement(ModifyTarget8),
    Halt,
}

impl Instruction {
    pub fn execute<A>(self, registers: &mut Registers, address_space: &mut A) -> ExecuteResult
    where
        A: AddressSpace,
    {
        match self {
            Self::Load8(write_target, read_target) => {
                load_8(registers, address_space, write_target, read_target)
            }
            Self::Load16(write_target, read_target) => {
                load_16(registers, address_space, write_target, read_target)
            }
            Self::Push(register) => push(registers, address_space, register),
            Self::Pop(register) => pop(registers, address_space, register),
            Self::ExchangeDEHL => exchange_de_hl(registers),
            Self::ExchangeAF => exchange_af(registers),
            Self::ExchangeGeneralPurpose => exchange_general_purpose(registers),
            Self::ExchangeStack(register) => exchange_stack(registers, address_space, register),
            Self::Transfer(transfer_mode) => transfer(registers, address_space, transfer_mode),
            Self::CompareBlock(transfer_mode) => {
                compare_block(registers, address_space, transfer_mode)
            }
            Self::Add {
                read_target,
                with_carry,
            } => add(registers, address_space, read_target, with_carry),
            Self::Subtract {
                read_target,
                with_carry,
            } => subtract(registers, address_space, read_target, with_carry),
            Self::And(read_target) => and(registers, address_space, read_target),
            Self::Or(read_target) => or(registers, address_space, read_target),
            Self::Xor(read_target) => xor(registers, address_space, read_target),
            Self::Compare(read_target) => compare(registers, address_space, read_target),
            Self::Increment(modify_target) => increment(registers, address_space, modify_target),
            Self::Decrement(modify_target) => decrement(registers, address_space, modify_target),
            Self::Halt => todo!("halt not implemented"),
        }
    }
}

fn load_8<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    write_target: WriteTarget8,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let value = read_target.read(registers, address_space);
    write_target.write(registers, address_space, value);

    if write_target == WriteTarget8::Register(Register8::A)
        && matches!(
            read_target,
            ReadTarget8::Register(Register8::I | Register8::R)
        )
    {
        // TODO set overflow to false if an interrupt has triggered
        registers
            .f
            .set_sign(value.bit(7))
            .set_zero(value == 0)
            .set_half_carry(false)
            .set_overflow(registers.iff2)
            .set_subtract(false);
    }

    let mut t_cycles = read_target.t_cycles_required() + write_target.t_cycles_required();
    if let (WriteTarget8::Indexed(..), ReadTarget8::Immediate(..)) = (write_target, read_target) {
        // TODO comment
        t_cycles -= 3;
    }

    ExecuteResult { t_cycles }
}

fn load_16<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    write_target: WriteTarget16,
    read_target: ReadTarget16,
) -> ExecuteResult {
    let value = read_target.read(registers, address_space);
    write_target.write(registers, address_space, value);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required() + write_target.t_cycles_required(),
    }
}

fn push<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    register: Register16,
) -> ExecuteResult {
    let value = register.read(registers);
    let [lsb, msb] = value.to_le_bytes();

    registers.sp = registers.sp.wrapping_sub(1);
    address_space.write(registers.sp, msb);
    registers.sp = registers.sp.wrapping_sub(1);
    address_space.write(registers.sp, lsb);

    ExecuteResult {
        // Extra cycle for the opcode read + 2 memory writes
        t_cycles: 7,
    }
}

fn pop<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    register: Register16,
) -> ExecuteResult {
    let lsb = address_space.read(registers.sp);
    registers.sp = registers.sp.wrapping_add(1);
    let msb = address_space.read(registers.sp);
    registers.sp = registers.sp.wrapping_add(1);

    let value = u16::from_le_bytes([lsb, msb]);
    register.write(registers, value);

    ExecuteResult {
        // 2 memory reads
        t_cycles: 6,
    }
}

fn exchange_de_hl(registers: &mut Registers) -> ExecuteResult {
    mem::swap(&mut registers.d, &mut registers.h);
    mem::swap(&mut registers.e, &mut registers.l);

    ExecuteResult { t_cycles: 0 }
}

fn exchange_af(registers: &mut Registers) -> ExecuteResult {
    mem::swap(&mut registers.a, &mut registers.ap);
    mem::swap(&mut registers.f, &mut registers.fp);

    ExecuteResult { t_cycles: 0 }
}

fn exchange_general_purpose(registers: &mut Registers) -> ExecuteResult {
    mem::swap(&mut registers.b, &mut registers.bp);
    mem::swap(&mut registers.c, &mut registers.cp);
    mem::swap(&mut registers.d, &mut registers.dp);
    mem::swap(&mut registers.e, &mut registers.ep);
    mem::swap(&mut registers.h, &mut registers.hp);
    mem::swap(&mut registers.l, &mut registers.lp);

    ExecuteResult { t_cycles: 0 }
}

fn exchange_stack<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    register: Register16,
) -> ExecuteResult {
    let stack_lsb = address_space.read(registers.sp);
    let stack_msb = address_space.read(registers.sp.wrapping_add(1));
    let stack_value = u16::from_le_bytes([stack_lsb, stack_msb]);

    let [register_lsb, register_msb] = register.read(registers).to_le_bytes();

    address_space.write(registers.sp, register_lsb);
    address_space.write(registers.sp.wrapping_add(1), register_msb);
    register.write(registers, stack_value);

    ExecuteResult { t_cycles: 15 }
}

#[derive(Debug, Clone, Copy)]
pub enum TransferMode {
    Increment { repeat: bool },
    Decrement { repeat: bool },
}

impl TransferMode {
    fn repeat(self) -> bool {
        match self {
            Self::Increment { repeat } | Self::Decrement { repeat } => repeat,
        }
    }
}

fn transfer<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    mode: TransferMode,
) -> ExecuteResult {
    let hl = Register16::HL.read(registers);
    let de = Register16::DE.read(registers);

    let value = address_space.read(hl);
    address_space.write(de, value);

    let bc = Register16::BC.read(registers);
    Register16::BC.write(registers, bc.wrapping_sub(1));

    match mode {
        TransferMode::Increment { .. } => {
            Register16::HL.write(registers, hl.wrapping_add(1));
            Register16::DE.write(registers, de.wrapping_add(1));
        }
        TransferMode::Decrement { .. } => {
            Register16::HL.write(registers, hl.wrapping_sub(1));
            Register16::DE.write(registers, de.wrapping_sub(1));
        }
    }

    let should_repeat = mode.repeat() && bc != 1;
    if should_repeat {
        registers.pc = registers.pc.wrapping_sub(2);
    }

    registers
        .f
        .set_half_carry(false)
        .set_subtract(false)
        .set_overflow(bc != 1);

    ExecuteResult {
        t_cycles: if should_repeat { 13 } else { 8 },
    }
}

fn compare_block<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    mode: TransferMode,
) -> ExecuteResult {
    let hl = Register16::HL.read(registers);
    let value = address_space.read(hl);

    let bc = Register16::BC.read(registers);
    Register16::BC.write(registers, bc.wrapping_sub(1));

    match mode {
        TransferMode::Increment { .. } => {
            Register16::HL.write(registers, hl.wrapping_add(1));
        }
        TransferMode::Decrement { .. } => {
            Register16::HL.write(registers, hl.wrapping_sub(1));
        }
    }

    let should_repeat = mode.repeat() && bc != 1;
    if should_repeat {
        registers.pc = registers.pc.wrapping_sub(2);
    }

    let a = registers.a;

    registers
        .f
        .set_sign(a.wrapping_sub(value).bit(7))
        .set_zero(a == value)
        .set_half_carry(a & 0x0F < value & 0x0F)
        .set_overflow(bc != 1)
        .set_subtract(true);

    ExecuteResult {
        t_cycles: if should_repeat { 13 } else { 8 },
    }
}

fn add<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    read_target: ReadTarget8,
    with_carry: bool,
) -> ExecuteResult {
    let operand = read_target.read(registers, address_space);
    let carry_operand = if with_carry {
        u8::from(registers.f.carry())
    } else {
        0
    };

    let a = registers.a;
    let half_carry = (a & 0x0F) + (operand & 0x0F) + carry_operand >= 0x10;
    let (value, carry) = match a.overflowing_add(operand) {
        (sum, true) => (sum + carry_operand, true),
        (sum, false) => sum.overflowing_add(carry_operand),
    };

    let bit_6_carry = (a & 0x7F) + (operand & 0x7F) + carry_operand >= 0x80;
    let overflow = bit_6_carry != carry;

    registers.a = value;
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(half_carry)
        .set_overflow(overflow)
        .set_subtract(false)
        .set_carry(carry);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required(),
    }
}

fn subtract<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    read_target: ReadTarget8,
    with_carry: bool,
) -> ExecuteResult {
    let operand = read_target.read(registers, address_space);
    let carry_operand = if with_carry {
        u8::from(registers.f.carry())
    } else {
        0
    };

    let a = registers.a;
    let half_carry = a & 0x0F < (operand & 0x0F) + carry_operand;
    let (value, carry) = match a.overflowing_sub(operand) {
        (difference, true) => (difference - carry_operand, true),
        (difference, false) => difference.overflowing_sub(carry_operand),
    };

    let bit_6_borrow = a & 0x7F < (operand & 0x7F) + carry_operand;
    let overflow = bit_6_borrow != carry;

    registers.a = value;
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(half_carry)
        .set_overflow(overflow)
        .set_subtract(true)
        .set_carry(carry);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required(),
    }
}

fn and<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, address_space);
    let value = registers.a & operand;

    let parity = value.count_ones() % 2 == 0;

    registers.a = value;
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(true)
        .set_overflow(parity)
        .set_subtract(false)
        .set_carry(false);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required(),
    }
}

fn or<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, address_space);
    let value = registers.a | operand;

    let parity = value.count_ones() % 2 == 0;

    registers.a = value;
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(false)
        .set_overflow(parity)
        .set_subtract(false)
        .set_carry(false);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required(),
    }
}

fn xor<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, address_space);
    let value = registers.a ^ operand;

    let parity = value.count_ones() % 2 == 0;

    registers.a = value;
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(false)
        .set_overflow(parity)
        .set_subtract(false)
        .set_carry(false);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required(),
    }
}

fn compare<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, address_space);

    let a = registers.a;
    let half_carry = a & 0x0F < operand & 0x0F;
    let (value, carry) = a.overflowing_sub(operand);

    let bit_6_borrow = a & 0x7F < value & 0x7F;
    let overflow = bit_6_borrow != carry;

    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(half_carry)
        .set_overflow(overflow)
        .set_subtract(true)
        .set_carry(carry);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required(),
    }
}

fn increment<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    modify_target: ModifyTarget8,
) -> ExecuteResult {
    let original = modify_target.read(registers, address_space);
    let value = original.wrapping_add(1);

    modify_target.write(registers, address_space, value);
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(original.trailing_ones() >= 4)
        .set_overflow(value == 0x80)
        .set_subtract(false);

    ExecuteResult {
        t_cycles: modify_target.t_cycles_required(),
    }
}

fn decrement<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    modify_target: ModifyTarget8,
) -> ExecuteResult {
    let original = modify_target.read(registers, address_space);
    let value = original.wrapping_sub(1);

    modify_target.write(registers, address_space, value);
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(original.trailing_zeros() >= 4)
        .set_overflow(value == 0x7F)
        .set_subtract(true);

    ExecuteResult {
        t_cycles: modify_target.t_cycles_required(),
    }
}

pub fn parse_next_instruction<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
) -> ParseResult {
    InstructionParser::new(registers, address_space).parse()
}
