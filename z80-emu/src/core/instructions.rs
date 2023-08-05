mod parser;

use crate::core::instructions::parser::{InstructionParser, ParseResult};
use crate::core::{IndexRegister, InterruptMode, Register16, Register8, Registers};
use crate::traits::BusInterface;
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

impl GetBit for u16 {
    fn bit(self, i: u8) -> bool {
        assert!(i <= 15);
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
    fn read<B: BusInterface>(self, registers: &Registers, bus: &mut B) -> u8 {
        match self {
            Self::Immediate(n) => n,
            Self::Register(register) => register.read(registers),
            Self::Indirect(register) => {
                let address = register.read(registers);
                bus.read_memory(address)
            }
            Self::Direct(address) => bus.read_memory(address),
            Self::Indexed(index, d) => {
                let address = index_address(index.read(registers), d);
                bus.read_memory(address)
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
    fn write<B: BusInterface>(self, registers: &mut Registers, bus: &mut B, value: u8) {
        match self {
            Self::Register(register) => {
                register.write(registers, value);
            }
            Self::Indirect(register) => {
                let address = register.read(registers);
                bus.write_memory(address, value);
            }
            Self::Direct(address) => {
                bus.write_memory(address, value);
            }
            Self::Indexed(index, d) => {
                let address = index_address(index.read(registers), d);
                bus.write_memory(address, value);
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
    fn read<B: BusInterface>(self, registers: &Registers, bus: &mut B) -> u8 {
        match self {
            Self::Register(register) => ReadTarget8::Register(register).read(registers, bus),
            Self::Indirect(register) => ReadTarget8::Indirect(register).read(registers, bus),
            Self::Indexed(index, d) => ReadTarget8::Indexed(index, d).read(registers, bus),
        }
    }

    fn write<B: BusInterface>(self, registers: &mut Registers, bus: &mut B, value: u8) {
        match self {
            Self::Register(register) => {
                WriteTarget8::Register(register).write(registers, bus, value);
            }
            Self::Indirect(register) => {
                WriteTarget8::Indirect(register).write(registers, bus, value);
            }
            Self::Indexed(index, d) => {
                WriteTarget8::Indexed(index, d).write(registers, bus, value);
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
    fn read<B: BusInterface>(self, registers: &Registers, bus: &mut B) -> u16 {
        match self {
            Self::Immediate(nn) => nn,
            Self::Register(register) => register.read(registers),
            Self::Direct(nn) => {
                let lsb = bus.read_memory(nn);
                let msb = bus.read_memory(nn.wrapping_add(1));
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
    fn write<B: BusInterface>(self, registers: &mut Registers, bus: &mut B, value: u16) {
        match self {
            Self::Register(register) => {
                register.write(registers, value);
            }
            Self::Direct(nn) => {
                let [lsb, msb] = value.to_le_bytes();
                bus.write_memory(nn, lsb);
                bus.write_memory(nn.wrapping_add(1), msb);
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

#[derive(Debug, Clone, Copy)]
pub struct ExecuteResult {
    t_cycles: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpCondition {
    NonZero,
    Zero,
    NoCarry,
    Carry,
    OddParity,
    EvenParity,
    Positive,
    Negative,
}

impl JumpCondition {
    fn from_bits(bits: u8) -> Self {
        match bits & 0x07 {
            0x00 => Self::NonZero,
            0x01 => Self::Zero,
            0x02 => Self::NoCarry,
            0x03 => Self::Carry,
            0x04 => Self::OddParity,
            0x05 => Self::EvenParity,
            0x06 => Self::Positive,
            0x07 => Self::Negative,
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        }
    }

    fn check(self, registers: &Registers) -> bool {
        match self {
            Self::NonZero => !registers.f.zero(),
            Self::Zero => registers.f.zero(),
            Self::NoCarry => !registers.f.carry(),
            Self::Carry => registers.f.carry(),
            Self::OddParity => !registers.f.overflow(),
            Self::EvenParity => registers.f.overflow(),
            Self::Positive => !registers.f.sign(),
            Self::Negative => registers.f.sign(),
        }
    }
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
    Add16 {
        write: Register16,
        read: Register16,
        with_carry: bool,
    },
    Subtract {
        read_target: ReadTarget8,
        with_carry: bool,
    },
    Subtract16 {
        write: Register16,
        read: Register16,
    },
    And(ReadTarget8),
    Or(ReadTarget8),
    Xor(ReadTarget8),
    Compare(ReadTarget8),
    Increment(ModifyTarget8),
    IncrementRegister16(Register16),
    Decrement(ModifyTarget8),
    DecrementRegister16(Register16),
    RotateLeft {
        modify_target: ModifyTarget8,
        thru_carry: bool,
        side_effect: Option<Register8>,
    },
    RotateRight {
        modify_target: ModifyTarget8,
        thru_carry: bool,
        side_effect: Option<Register8>,
    },
    ShiftLeft {
        modify_target: ModifyTarget8,
        side_effect: Option<Register8>,
    },
    ShiftRightArithmetic {
        modify_target: ModifyTarget8,
        side_effect: Option<Register8>,
    },
    ShiftRightLogical {
        modify_target: ModifyTarget8,
        side_effect: Option<Register8>,
    },
    RotateLeft12(ModifyTarget8),
    RotateRight12(ModifyTarget8),
    TestBit(ModifyTarget8, u8),
    SetBit {
        modify_target: ModifyTarget8,
        bit: u8,
        value: bool,
        side_effect: Option<Register8>,
    },
    DecimalAdjustAccumulator,
    ComplementAccumulator,
    NegateAccumulator,
    ComplementCarry,
    SetCarry,
    NoOp,
    Halt,
    DisableInterrupts,
    EnableInterrupts,
    SetInterruptMode(InterruptMode),
    JumpAbsolute(u16, Option<JumpCondition>),
    JumpRelative(i8, Option<JumpCondition>),
    JumpRelativeDecB(i8),
    JumpRegister(Register16),
    Call(u16, Option<JumpCondition>),
    Return {
        condition: Option<JumpCondition>,
        reti: bool,
    },
    ReturnFromNmi,
    Restart(u16),
}

impl Instruction {
    pub fn execute<B>(self, registers: &mut Registers, bus: &mut B) -> ExecuteResult
    where
        B: BusInterface,
    {
        registers.interrupt_delay = false;

        match self {
            Self::Load8(write_target, read_target) => {
                load_8(registers, bus, write_target, read_target)
            }
            Self::Load16(write_target, read_target) => {
                load_16(registers, bus, write_target, read_target)
            }
            Self::Push(register) => push(registers, bus, register),
            Self::Pop(register) => pop(registers, bus, register),
            Self::ExchangeDEHL => exchange_de_hl(registers),
            Self::ExchangeAF => exchange_af(registers),
            Self::ExchangeGeneralPurpose => exchange_general_purpose(registers),
            Self::ExchangeStack(register) => exchange_stack(registers, bus, register),
            Self::Transfer(transfer_mode) => transfer(registers, bus, transfer_mode),
            Self::CompareBlock(transfer_mode) => compare_block(registers, bus, transfer_mode),
            Self::Add {
                read_target,
                with_carry,
            } => add(registers, bus, read_target, with_carry),
            Self::Add16 {
                write,
                read,
                with_carry,
            } => add_u16(registers, write, read, with_carry),
            Self::Subtract {
                read_target,
                with_carry,
            } => subtract(registers, bus, read_target, with_carry),
            Self::Subtract16 { write, read } => subtract_u16(registers, write, read),
            Self::And(read_target) => and(registers, bus, read_target),
            Self::Or(read_target) => or(registers, bus, read_target),
            Self::Xor(read_target) => xor(registers, bus, read_target),
            Self::Compare(read_target) => compare(registers, bus, read_target),
            Self::Increment(modify_target) => increment(registers, bus, modify_target),
            Self::IncrementRegister16(register) => increment_register_16(registers, register),
            Self::Decrement(modify_target) => decrement(registers, bus, modify_target),
            Self::DecrementRegister16(register) => decrement_register_16(registers, register),
            Self::RotateLeft {
                modify_target,
                thru_carry,
                side_effect,
            } => rotate_left(registers, bus, modify_target, thru_carry, side_effect),
            Self::RotateRight {
                modify_target,
                thru_carry,
                side_effect,
            } => rotate_right(registers, bus, modify_target, thru_carry, side_effect),
            Self::ShiftLeft {
                modify_target,
                side_effect,
            } => shift_left(registers, bus, modify_target, side_effect),
            Self::ShiftRightArithmetic {
                modify_target,
                side_effect,
            } => shift_right_arithmetic(registers, bus, modify_target, side_effect),
            Self::ShiftRightLogical {
                modify_target,
                side_effect,
            } => shift_right_logical(registers, bus, modify_target, side_effect),
            Self::RotateLeft12(modify_target) => rotate_left_12(registers, bus, modify_target),
            Self::RotateRight12(modify_target) => rotate_right_12(registers, bus, modify_target),
            Self::TestBit(modify_target, bit) => bit_test(registers, bus, modify_target, bit),
            Self::SetBit {
                modify_target,
                bit,
                value,
                side_effect,
            } => set_bit(registers, bus, modify_target, bit, value, side_effect),
            Self::DecimalAdjustAccumulator => decimal_adjust_accumulator(registers),
            Self::ComplementAccumulator => complement_accumulator(registers),
            Self::NegateAccumulator => negate_accumulator(registers),
            Self::ComplementCarry => complement_carry(registers),
            Self::SetCarry => set_carry(registers),
            Self::NoOp => ExecuteResult { t_cycles: 0 },
            Self::DisableInterrupts => disable_interrupts(registers),
            Self::EnableInterrupts => enable_interrupts(registers),
            Self::SetInterruptMode(interrupt_mode) => set_interrupt_mode(registers, interrupt_mode),
            Self::JumpAbsolute(address, condition) => jump_absolute(registers, address, condition),
            Self::JumpRelative(offset, condition) => jump_relative(registers, offset, condition),
            Self::JumpRelativeDecB(offset) => jump_relative_dec_b(registers, offset),
            Self::JumpRegister(register) => jump_register(registers, register),
            Self::Call(address, condition) => call(registers, bus, address, condition),
            Self::Return { condition, reti } => ret(registers, bus, condition, reti),
            Self::ReturnFromNmi => retn(registers, bus),
            Self::Restart(address) => restart(registers, bus, address),
            Self::Halt => todo!("halt not implemented"),
        }
    }
}

fn load_8<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    write_target: WriteTarget8,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let value = read_target.read(registers, bus);
    write_target.write(registers, bus, value);

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

fn load_16<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    write_target: WriteTarget16,
    read_target: ReadTarget16,
) -> ExecuteResult {
    let value = read_target.read(registers, bus);
    write_target.write(registers, bus, value);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required() + write_target.t_cycles_required(),
    }
}

fn push<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    register: Register16,
) -> ExecuteResult {
    let value = register.read(registers);
    let [lsb, msb] = value.to_le_bytes();

    registers.sp = registers.sp.wrapping_sub(1);
    bus.write_memory(registers.sp, msb);
    registers.sp = registers.sp.wrapping_sub(1);
    bus.write_memory(registers.sp, lsb);

    ExecuteResult {
        // Extra cycle for the opcode read + 2 memory writes
        t_cycles: 7,
    }
}

fn pop<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    register: Register16,
) -> ExecuteResult {
    let lsb = bus.read_memory(registers.sp);
    registers.sp = registers.sp.wrapping_add(1);
    let msb = bus.read_memory(registers.sp);
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

fn exchange_stack<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    register: Register16,
) -> ExecuteResult {
    let stack_lsb = bus.read_memory(registers.sp);
    let stack_msb = bus.read_memory(registers.sp.wrapping_add(1));
    let stack_value = u16::from_le_bytes([stack_lsb, stack_msb]);

    let [register_lsb, register_msb] = register.read(registers).to_le_bytes();

    bus.write_memory(registers.sp, register_lsb);
    bus.write_memory(registers.sp.wrapping_add(1), register_msb);
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

fn transfer<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    mode: TransferMode,
) -> ExecuteResult {
    let hl = Register16::HL.read(registers);
    let de = Register16::DE.read(registers);

    let value = bus.read_memory(hl);
    bus.write_memory(de, value);

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

fn compare_block<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    mode: TransferMode,
) -> ExecuteResult {
    let hl = Register16::HL.read(registers);
    let value = bus.read_memory(hl);

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

fn add<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    read_target: ReadTarget8,
    with_carry: bool,
) -> ExecuteResult {
    let operand = read_target.read(registers, bus);
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

fn add_u16(
    registers: &mut Registers,
    write_register: Register16,
    read_register: Register16,
    with_carry: bool,
) -> ExecuteResult {
    let operand_l = write_register.read(registers);
    let operand_r = read_register.read(registers);
    let carry_operand = if with_carry {
        registers.f.carry().into()
    } else {
        0
    };

    let half_carry = (operand_l & 0x0FFF) + (operand_r & 0x0FFF) + carry_operand >= 0x1000;
    let (value, carry) = match operand_l.overflowing_add(operand_r) {
        (sum, true) => (sum + carry_operand, true),
        (sum, false) => sum.overflowing_add(carry_operand),
    };

    write_register.write(registers, value);

    registers
        .f
        .set_half_carry(half_carry)
        .set_subtract(false)
        .set_carry(carry);

    if with_carry {
        let bit_14_carry = (operand_l & 0x7FFF) + (operand_r & 0x7FFF) + carry_operand >= 0x8000;
        let overflow = bit_14_carry != carry;

        registers
            .f
            .set_sign(value.bit(15))
            .set_zero(value == 0)
            .set_overflow(overflow);
    }

    ExecuteResult { t_cycles: 7 }
}

fn subtract<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    read_target: ReadTarget8,
    with_carry: bool,
) -> ExecuteResult {
    let operand = read_target.read(registers, bus);
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

fn subtract_u16(
    registers: &mut Registers,
    write_register: Register16,
    read_register: Register16,
) -> ExecuteResult {
    let operand_l = write_register.read(registers);
    let operand_r = read_register.read(registers);
    let carry_operand = u16::from(registers.f.carry());

    let half_carry = operand_l & 0x0FFF < (operand_r & 0x0FFF) + carry_operand;
    let (value, carry) = match operand_l.overflowing_sub(operand_r) {
        (difference, true) => (difference - carry_operand, true),
        (difference, false) => difference.overflowing_sub(carry_operand),
    };

    let bit_14_borrow = operand_l & 0x7FFF < (operand_r & 0x7FFF) + carry_operand;
    let overflow = bit_14_borrow != carry;

    write_register.write(registers, value);
    registers
        .f
        .set_sign(value.bit(15))
        .set_zero(value == 0)
        .set_half_carry(half_carry)
        .set_overflow(overflow)
        .set_subtract(true)
        .set_carry(carry);

    ExecuteResult { t_cycles: 7 }
}

fn and<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, bus);
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

fn or<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, bus);
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

fn xor<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, bus);
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

fn compare<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let operand = read_target.read(registers, bus);

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

fn increment<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let value = original.wrapping_add(1);

    modify_target.write(registers, bus, value);
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

fn increment_register_16(registers: &mut Registers, register: Register16) -> ExecuteResult {
    let value = register.read(registers).wrapping_add(1);
    register.write(registers, value);

    ExecuteResult { t_cycles: 2 }
}

fn decrement<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let value = original.wrapping_sub(1);

    modify_target.write(registers, bus, value);
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

fn decrement_register_16(registers: &mut Registers, register: Register16) -> ExecuteResult {
    let value = register.read(registers).wrapping_sub(1);
    register.write(registers, value);

    ExecuteResult { t_cycles: 2 }
}

fn rotate_left<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
    thru_carry: bool,
    side_effect: Option<Register8>,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let bit_0 = if thru_carry {
        registers.f.carry()
    } else {
        original.bit(7)
    };
    let value = (original << 1) | u8::from(bit_0);

    modify_target.write(registers, bus, value);
    registers
        .f
        .set_half_carry(false)
        .set_subtract(false)
        .set_carry(original.bit(7));

    if let Some(register) = side_effect {
        register.write(registers, value);
    }

    let mut t_cycles = modify_target.t_cycles_required();
    if let ModifyTarget8::Indexed(..) = modify_target {
        // TODO comment
        t_cycles -= 4;
    }

    ExecuteResult { t_cycles }
}

fn rotate_right<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
    thru_carry: bool,
    side_effect: Option<Register8>,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let bit_7 = if thru_carry {
        registers.f.carry()
    } else {
        original.bit(0)
    };
    let value = (original >> 1) | (u8::from(bit_7) << 7);

    modify_target.write(registers, bus, value);
    registers
        .f
        .set_half_carry(false)
        .set_subtract(false)
        .set_carry(original.bit(0));

    if let Some(register) = side_effect {
        register.write(registers, value);
    }

    let mut t_cycles = modify_target.t_cycles_required();
    if let ModifyTarget8::Indexed(..) = modify_target {
        // TODO comment
        t_cycles -= 4;
    }

    ExecuteResult { t_cycles }
}

fn shift_left<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
    side_effect: Option<Register8>,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let value = original << 1;

    let parity = value.count_ones() % 2 == 0;

    modify_target.write(registers, bus, value);
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(false)
        .set_overflow(parity)
        .set_subtract(false)
        .set_carry(original.bit(7));

    if let Some(register) = side_effect {
        register.write(registers, value);
    }

    let mut t_cycles = modify_target.t_cycles_required();
    if let ModifyTarget8::Indexed(..) = modify_target {
        // TODO comment
        t_cycles -= 4;
    }

    ExecuteResult { t_cycles }
}

fn shift_right_arithmetic<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
    side_effect: Option<Register8>,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let value = (original >> 1) | (original & 0x80);

    let parity = value.count_ones() % 2 == 0;

    modify_target.write(registers, bus, value);
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(false)
        .set_overflow(parity)
        .set_subtract(false)
        .set_carry(original.bit(0));

    if let Some(register) = side_effect {
        register.write(registers, value);
    }

    let mut t_cycles = modify_target.t_cycles_required();
    if let ModifyTarget8::Indexed(..) = modify_target {
        // TODO comment
        t_cycles -= 4;
    }

    ExecuteResult { t_cycles }
}

fn shift_right_logical<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
    side_effect: Option<Register8>,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let value = original >> 1;

    let parity = value.count_ones() % 2 == 0;

    modify_target.write(registers, bus, value);
    registers
        .f
        .set_sign(false)
        .set_zero(value == 0)
        .set_half_carry(false)
        .set_overflow(parity)
        .set_subtract(false)
        .set_carry(original.bit(0));

    if let Some(register) = side_effect {
        register.write(registers, value);
    }

    let mut t_cycles = modify_target.t_cycles_required();
    if let ModifyTarget8::Indexed(..) = modify_target {
        // TODO comment
        t_cycles -= 4;
    }

    ExecuteResult { t_cycles }
}

fn rotate_left_12<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
) -> ExecuteResult {
    let a = registers.a;
    let memory_value = modify_target.read(registers, bus);

    let new_a = (a & 0xF0) | ((memory_value & 0xF0) >> 4);
    let new_memory_value = ((memory_value & 0x0F) << 4) | (a & 0x0F);

    let parity = new_a.count_ones() % 2 == 0;

    registers.a = new_a;
    modify_target.write(registers, bus, new_memory_value);
    registers
        .f
        .set_sign(new_a.bit(7))
        .set_zero(new_a == 0)
        .set_half_carry(false)
        .set_overflow(parity)
        .set_subtract(false);

    ExecuteResult { t_cycles: 10 }
}

fn rotate_right_12<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
) -> ExecuteResult {
    let a = registers.a;
    let memory_value = modify_target.read(registers, bus);

    let new_a = (a & 0xF0) | (memory_value & 0x0F);
    let new_memory_value = ((a & 0x0F) << 4) | ((memory_value & 0xF0) >> 4);

    let parity = new_a.count_ones() % 2 == 0;

    registers.a = new_a;
    modify_target.write(registers, bus, new_memory_value);
    registers
        .f
        .set_sign(new_a.bit(7))
        .set_zero(new_a == 0)
        .set_half_carry(false)
        .set_overflow(parity)
        .set_subtract(false);

    ExecuteResult { t_cycles: 10 }
}

fn bit_test<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
    bit: u8,
) -> ExecuteResult {
    let value = modify_target.read(registers, bus);
    let bit_set = value & (1 << bit) != 0;

    registers
        .f
        .set_zero(!bit_set)
        .set_half_carry(true)
        .set_subtract(false);

    let t_cycles = match modify_target {
        ModifyTarget8::Register(..) => 0,
        ModifyTarget8::Indirect(..) => 4,
        ModifyTarget8::Indexed(..) => 5,
    };

    ExecuteResult { t_cycles }
}

fn set_bit<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    modify_target: ModifyTarget8,
    bit: u8,
    set: bool,
    side_effect: Option<Register8>,
) -> ExecuteResult {
    let original = modify_target.read(registers, bus);
    let value = if set {
        original | (1 << bit)
    } else {
        original & !(1 << bit)
    };

    modify_target.write(registers, bus, value);

    if let Some(register) = side_effect {
        register.write(registers, value);
    }

    let mut t_cycles = modify_target.t_cycles_required();
    if let ModifyTarget8::Indexed(..) = modify_target {
        // TODO comment
        t_cycles -= 4;
    }

    ExecuteResult { t_cycles }
}

fn decimal_adjust_accumulator(registers: &mut Registers) -> ExecuteResult {
    if registers.f.subtract() {
        let mut value = registers.a;
        if registers.f.half_carry() {
            value = value.wrapping_sub(0x06);
        }
        if registers.f.carry() {
            value = value.wrapping_sub(0x60);
        }

        registers.a = value;
        registers
            .f
            .set_sign(value.bit(7))
            .set_zero(value == 0)
            .set_half_carry(false);
    } else {
        let mut value = registers.a;
        let mut carry = false;
        if value > 0x99 || registers.f.carry() {
            value = value.wrapping_add(0x60);
            carry = true;
        }
        if value & 0x0F >= 0x0A || registers.f.half_carry() {
            value = value.wrapping_add(0x06);
        }

        registers.a = value;
        registers
            .f
            .set_sign(value.bit(7))
            .set_zero(value == 0)
            .set_half_carry(false)
            .set_carry(carry);
    }

    ExecuteResult { t_cycles: 0 }
}

fn complement_accumulator(registers: &mut Registers) -> ExecuteResult {
    registers.a = !registers.a;
    registers.f.set_half_carry(true).set_subtract(true);

    ExecuteResult { t_cycles: 0 }
}

fn negate_accumulator(registers: &mut Registers) -> ExecuteResult {
    let (value, carry) = 0_u8.overflowing_sub(registers.a);
    let half_carry = registers.a.trailing_zeros() < 4;

    registers.a = value;
    registers
        .f
        .set_sign(value.bit(7))
        .set_zero(value == 0)
        .set_half_carry(half_carry)
        .set_overflow(value == 0x80)
        .set_subtract(true)
        .set_carry(carry);

    ExecuteResult { t_cycles: 0 }
}

pub fn parse_next_instruction<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
) -> ParseResult {
    InstructionParser::new(registers, bus).parse()
}

fn complement_carry(registers: &mut Registers) -> ExecuteResult {
    let previous_carry = registers.f.carry();
    registers
        .f
        .set_half_carry(previous_carry)
        .set_subtract(false)
        .set_carry(!previous_carry);

    ExecuteResult { t_cycles: 0 }
}

fn set_carry(registers: &mut Registers) -> ExecuteResult {
    registers
        .f
        .set_half_carry(false)
        .set_subtract(false)
        .set_carry(true);

    ExecuteResult { t_cycles: 0 }
}

fn disable_interrupts(registers: &mut Registers) -> ExecuteResult {
    registers.iff1 = false;
    registers.iff2 = false;

    ExecuteResult { t_cycles: 0 }
}

fn enable_interrupts(registers: &mut Registers) -> ExecuteResult {
    registers.iff1 = true;
    registers.iff2 = true;
    registers.interrupt_delay = true;

    ExecuteResult { t_cycles: 0 }
}

fn set_interrupt_mode(registers: &mut Registers, mode: InterruptMode) -> ExecuteResult {
    registers.interrupt_mode = mode;

    ExecuteResult { t_cycles: 0 }
}

fn jump_absolute(
    registers: &mut Registers,
    address: u16,
    condition: Option<JumpCondition>,
) -> ExecuteResult {
    if condition.is_some_and(|cc| !cc.check(registers)) {
        return ExecuteResult { t_cycles: 0 };
    }

    registers.pc = address;
    ExecuteResult { t_cycles: 0 }
}

fn jump_relative(
    registers: &mut Registers,
    offset: i8,
    condition: Option<JumpCondition>,
) -> ExecuteResult {
    if condition.is_some_and(|cc| !cc.check(registers)) {
        return ExecuteResult { t_cycles: 0 };
    }

    registers.pc = (i32::from(registers.pc) + i32::from(offset)) as u16;
    ExecuteResult { t_cycles: 5 }
}

fn jump_relative_dec_b(registers: &mut Registers, offset: i8) -> ExecuteResult {
    registers.b = registers.b.wrapping_sub(1);

    if registers.b != 0 {
        registers.pc = (i32::from(registers.pc) + i32::from(offset)) as u16;
        ExecuteResult { t_cycles: 5 }
    } else {
        ExecuteResult { t_cycles: 0 }
    }
}

fn jump_register(registers: &mut Registers, register: Register16) -> ExecuteResult {
    registers.pc = register.read(registers);
    ExecuteResult { t_cycles: 0 }
}

fn call<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    address: u16,
    condition: Option<JumpCondition>,
) -> ExecuteResult {
    if condition.is_some_and(|cc| !cc.check(registers)) {
        return ExecuteResult { t_cycles: 0 };
    }

    registers.sp = registers.sp.wrapping_sub(1);
    bus.write_memory(registers.sp, (registers.pc >> 8) as u8);
    registers.sp = registers.sp.wrapping_sub(1);
    bus.write_memory(registers.sp, registers.pc as u8);

    registers.pc = address;

    ExecuteResult { t_cycles: 6 }
}

fn ret<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    condition: Option<JumpCondition>,
    is_reti: bool,
) -> ExecuteResult {
    if condition.is_some_and(|cc| !cc.check(registers)) {
        return ExecuteResult { t_cycles: 1 };
    }

    let pc_lsb = bus.read_memory(registers.sp);
    registers.sp = registers.sp.wrapping_add(1);
    let pc_msb = bus.read_memory(registers.sp);
    registers.sp = registers.sp.wrapping_add(1);

    registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

    let condition_delay = u32::from(condition.is_some());
    let reti_delay = if is_reti { 4 } else { 0 };
    ExecuteResult {
        t_cycles: 6 + condition_delay + reti_delay,
    }
}

fn retn<B: BusInterface>(registers: &mut Registers, bus: &mut B) -> ExecuteResult {
    ret(registers, bus, None, false);
    registers.iff1 = registers.iff2;

    ExecuteResult { t_cycles: 6 }
}

fn restart<B: BusInterface>(registers: &mut Registers, bus: &mut B, address: u16) -> ExecuteResult {
    call(registers, bus, address, None);

    ExecuteResult { t_cycles: 7 }
}

fn io_in<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    write_register: Register8,
    read_register: Register8,
    set_flags: bool,
) -> ExecuteResult {
    let lsb = read_register.read(registers);
    let msb_register = match read_register {
        Register8::C => Register8::B,
        _ => Register8::A,
    };
    let msb = msb_register.read(registers);

    let value = bus.read_io(u16::from_le_bytes([lsb, msb]));
    write_register.write(registers, value);

    if set_flags {
        let parity = value.count_ones() % 2 == 0;
        registers
            .f
            .set_sign(value.bit(7))
            .set_zero(value == 0)
            .set_half_carry(false)
            .set_overflow(parity)
            .set_subtract(false);
    }

    ExecuteResult { t_cycles: 4 }
}

fn io_in_block<B: BusInterface>(
    registers: &mut Registers,
    bus: &mut B,
    mode: TransferMode,
) -> ExecuteResult {
    let address = u16::from_be_bytes([registers.b, registers.c]);
    let value = bus.read_io(address);

    let hl = Register16::HL.read(registers);
    bus.write_memory(hl, value);
    registers.b = registers.b.wrapping_sub(1);

    registers.f.set_zero(registers.b == 0).set_subtract(true);

    match mode {
        TransferMode::Increment { .. } => {
            Register16::HL.write(registers, hl.wrapping_add(1));
        }
        TransferMode::Decrement { .. } => {
            Register16::HL.write(registers, hl.wrapping_sub(1));
        }
    }

    let should_repeat = mode.repeat() && registers.b == 0;
    if should_repeat {
        registers.pc -= 2;
    }

    let t_cycles = if should_repeat { 13 } else { 8 };
    ExecuteResult { t_cycles }
}
