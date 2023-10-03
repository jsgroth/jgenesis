mod arithmetic;
mod bits;
mod controlflow;
mod load;

use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult,
    InstructionExecutor, OpSize, Registers,
};
use crate::traits::BusInterface;
use jgenesis_traits::num::GetBit;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    RegisterToMemory,
    MemoryToRegister,
}

impl Direction {
    fn parse_from_opcode(opcode: u16) -> Self {
        if opcode.bit(8) { Self::RegisterToMemory } else { Self::MemoryToRegister }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UspDirection {
    RegisterToUsp,
    UspToRegister,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftDirection {
    Left,
    Right,
}

impl ShiftDirection {
    fn parse_from_opcode(opcode: u16) -> Self {
        if opcode.bit(8) { Self::Left } else { Self::Right }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftCount {
    Constant(u8),
    Register(DataRegister),
}

impl ShiftCount {
    fn parse_from_opcode(opcode: u16) -> Self {
        let value = ((opcode >> 9) & 0x07) as u8;
        if opcode.bit(5) {
            Self::Register(value.into())
        } else {
            let shift = if value == 0 { 8 } else { value };
            Self::Constant(shift)
        }
    }

    fn get(self, registers: &Registers) -> u8 {
        match self {
            Self::Constant(count) => count,
            Self::Register(register) => register.read_from(registers) as u8,
        }
    }
}

impl Display for ShiftCount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Constant(n) => write!(f, "#<{n}>"),
            Self::Register(register) => write!(f, "D{}", register.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtendOpMode {
    DataDirect,
    AddressIndirectPredecrement,
}

impl ExtendOpMode {
    fn parse_from_opcode(opcode: u16) -> Self {
        if opcode.bit(3) { Self::AddressIndirectPredecrement } else { Self::DataDirect }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCondition {
    True,
    False,
    Higher,
    LowerOrSame,
    CarryClear,
    CarrySet,
    NotEqual,
    Equal,
    OverflowClear,
    OverflowSet,
    Plus,
    Minus,
    GreaterOrEqual,
    LessThan,
    GreaterThan,
    LessOrEqual,
}

impl BranchCondition {
    fn parse_from_opcode(opcode: u16) -> Self {
        match opcode & 0x0F00 {
            0x0000 => BranchCondition::True,
            0x0100 => BranchCondition::False,
            0x0200 => BranchCondition::Higher,
            0x0300 => BranchCondition::LowerOrSame,
            0x0400 => BranchCondition::CarryClear,
            0x0500 => BranchCondition::CarrySet,
            0x0600 => BranchCondition::NotEqual,
            0x0700 => BranchCondition::Equal,
            0x0800 => BranchCondition::OverflowClear,
            0x0900 => BranchCondition::OverflowSet,
            0x0A00 => BranchCondition::Plus,
            0x0B00 => BranchCondition::Minus,
            0x0C00 => BranchCondition::GreaterOrEqual,
            0x0D00 => BranchCondition::LessThan,
            0x0E00 => BranchCondition::GreaterThan,
            0x0F00 => BranchCondition::LessOrEqual,
            _ => unreachable!("value & 0x0F00 is always one of the above values"),
        }
    }

    fn check(self, ccr: ConditionCodes) -> bool {
        match self {
            Self::True => true,
            Self::False => false,
            Self::Higher => !ccr.carry && !ccr.zero,
            Self::LowerOrSame => ccr.carry || ccr.zero,
            Self::CarryClear => !ccr.carry,
            Self::CarrySet => ccr.carry,
            Self::NotEqual => !ccr.zero,
            Self::Equal => ccr.zero,
            Self::OverflowClear => !ccr.overflow,
            Self::OverflowSet => ccr.overflow,
            Self::Plus => !ccr.negative,
            Self::Minus => ccr.negative,
            Self::GreaterOrEqual => ccr.negative == ccr.overflow,
            Self::LessThan => ccr.negative != ccr.overflow,
            Self::GreaterThan => !ccr.zero && ccr.negative == ccr.overflow,
            Self::LessOrEqual => ccr.zero || ccr.negative != ccr.overflow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    Add { size: OpSize, source: AddressingMode, dest: AddressingMode, with_extend: bool },
    AddDecimal { source: AddressingMode, dest: AddressingMode },
    And { size: OpSize, source: AddressingMode, dest: AddressingMode },
    AndToCcr,
    AndToSr,
    ArithmeticShiftMemory(ShiftDirection, AddressingMode),
    ArithmeticShiftRegister(OpSize, ShiftDirection, DataRegister, ShiftCount),
    BitTest { source: AddressingMode, dest: AddressingMode },
    BitTestAndChange { source: AddressingMode, dest: AddressingMode },
    BitTestAndClear { source: AddressingMode, dest: AddressingMode },
    BitTestAndSet { source: AddressingMode, dest: AddressingMode },
    Branch(BranchCondition, i8),
    BranchDecrement(BranchCondition, DataRegister),
    BranchToSubroutine(i8),
    CheckRegister(DataRegister, AddressingMode),
    Clear(OpSize, AddressingMode),
    Compare { size: OpSize, source: AddressingMode, dest: AddressingMode },
    DivideSigned(DataRegister, AddressingMode),
    DivideUnsigned(DataRegister, AddressingMode),
    ExchangeAddress(AddressRegister, AddressRegister),
    ExchangeData(DataRegister, DataRegister),
    ExchangeDataAddress(DataRegister, AddressRegister),
    ExclusiveOr { size: OpSize, source: AddressingMode, dest: AddressingMode },
    ExclusiveOrToCcr,
    ExclusiveOrToSr,
    Extend(OpSize, DataRegister),
    Jump(AddressingMode),
    JumpToSubroutine(AddressingMode),
    Link(AddressRegister),
    LoadEffectiveAddress(AddressingMode, AddressRegister),
    LogicalShiftMemory(ShiftDirection, AddressingMode),
    LogicalShiftRegister(OpSize, ShiftDirection, DataRegister, ShiftCount),
    Move { size: OpSize, source: AddressingMode, dest: AddressingMode },
    MoveFromSr(AddressingMode),
    MoveMultiple(OpSize, AddressingMode, Direction),
    MovePeripheral(OpSize, DataRegister, AddressRegister, Direction),
    MoveQuick(i8, DataRegister),
    MoveToCcr(AddressingMode),
    MoveToSr(AddressingMode),
    MoveUsp(UspDirection, AddressRegister),
    MultiplySigned(DataRegister, AddressingMode),
    MultiplyUnsigned(DataRegister, AddressingMode),
    Negate { size: OpSize, dest: AddressingMode, with_extend: bool },
    NegateDecimal(AddressingMode),
    NoOp,
    Not(OpSize, AddressingMode),
    Or { size: OpSize, source: AddressingMode, dest: AddressingMode },
    OrToCcr,
    OrToSr,
    PushEffectiveAddress(AddressingMode),
    Reset,
    Return { restore_ccr: bool },
    ReturnFromException,
    RotateMemory(ShiftDirection, AddressingMode),
    RotateRegister(OpSize, ShiftDirection, DataRegister, ShiftCount),
    RotateThruExtendMemory(ShiftDirection, AddressingMode),
    RotateThruExtendRegister(OpSize, ShiftDirection, DataRegister, ShiftCount),
    Set(BranchCondition, AddressingMode),
    Subtract { size: OpSize, source: AddressingMode, dest: AddressingMode, with_extend: bool },
    SubtractDecimal { source: AddressingMode, dest: AddressingMode },
    Swap(DataRegister),
    Stop,
    Test(OpSize, AddressingMode),
    TestAndSet(AddressingMode),
    Trap(u32),
    TrapOnOverflow,
    Unlink(AddressRegister),
}

impl Instruction {
    pub fn source_addressing_mode(self) -> Option<AddressingMode> {
        match self {
            Self::Add { source, .. }
            | Self::AddDecimal { source, .. }
            | Self::And { source, .. }
            | Self::ArithmeticShiftMemory(_, source)
            | Self::BitTest { source, .. }
            | Self::BitTestAndChange { source, .. }
            | Self::BitTestAndClear { source, .. }
            | Self::BitTestAndSet { source, .. }
            | Self::CheckRegister(_, source)
            | Self::Compare { source, .. }
            | Self::DivideSigned(_, source)
            | Self::DivideUnsigned(_, source)
            | Self::ExclusiveOr { source, .. }
            | Self::LoadEffectiveAddress(source, ..)
            | Self::LogicalShiftMemory(_, source)
            | Self::Jump(source)
            | Self::JumpToSubroutine(source)
            | Self::Move { source, .. }
            | Self::MoveToCcr(source)
            | Self::MoveToSr(source)
            | Self::MultiplySigned(_, source)
            | Self::MultiplyUnsigned(_, source)
            | Self::Or { source, .. }
            | Self::PushEffectiveAddress(source)
            | Self::RotateMemory(_, source)
            | Self::RotateThruExtendMemory(_, source)
            | Self::Subtract { source, .. }
            | Self::SubtractDecimal { source, .. }
            | Self::Test(_, source) => Some(source),
            Self::AndToCcr
            | Self::AndToSr
            | Self::ArithmeticShiftRegister(..)
            | Self::Branch(..)
            | Self::BranchDecrement(..)
            | Self::BranchToSubroutine(..)
            | Self::Clear(..)
            | Self::ExchangeAddress(..)
            | Self::ExchangeData(..)
            | Self::ExchangeDataAddress(..)
            | Self::ExclusiveOrToCcr
            | Self::ExclusiveOrToSr
            | Self::Extend(..)
            | Self::Link(..)
            | Self::LogicalShiftRegister(..)
            | Self::MoveFromSr(..)
            | Self::MoveMultiple(..)
            | Self::MovePeripheral(..)
            | Self::MoveQuick(..)
            | Self::MoveUsp(..)
            | Self::Negate { .. }
            | Self::NegateDecimal(..)
            | Self::NoOp
            | Self::Not(..)
            | Self::OrToCcr
            | Self::OrToSr
            | Self::Reset
            | Self::Return { .. }
            | Self::ReturnFromException
            | Self::RotateRegister(..)
            | Self::RotateThruExtendRegister(..)
            | Self::Set(..)
            | Self::Swap(..)
            | Self::Stop
            | Self::TestAndSet(..)
            | Self::Trap(..)
            | Self::TrapOnOverflow
            | Self::Unlink(..) => None,
        }
    }

    pub fn dest_addressing_mode(self) -> Option<AddressingMode> {
        match self {
            Self::Add { dest, .. }
            | Self::AddDecimal { dest, .. }
            | Self::And { dest, .. }
            | Self::Clear(_, dest)
            | Self::Compare { dest, .. }
            | Self::ExclusiveOr { dest, .. }
            | Self::Move { dest, .. }
            | Self::MoveFromSr(dest)
            | Self::Negate { dest, .. }
            | Self::Not(_, dest)
            | Self::Or { dest, .. }
            | Self::Set(_, dest)
            | Self::Subtract { dest, .. }
            | Self::SubtractDecimal { dest, .. }
            | Self::TestAndSet(dest) => Some(dest),
            Self::AndToCcr
            | Self::AndToSr
            | Self::ArithmeticShiftMemory(..)
            | Self::ArithmeticShiftRegister(..)
            | Self::BitTest { .. }
            | Self::BitTestAndChange { .. }
            | Self::BitTestAndClear { .. }
            | Self::BitTestAndSet { .. }
            | Self::Branch(..)
            | Self::BranchDecrement(..)
            | Self::BranchToSubroutine(..)
            | Self::CheckRegister(..)
            | Self::DivideSigned(..)
            | Self::DivideUnsigned(..)
            | Self::ExchangeAddress(..)
            | Self::ExchangeData(..)
            | Self::ExchangeDataAddress(..)
            | Self::ExclusiveOrToCcr
            | Self::ExclusiveOrToSr
            | Self::Extend(..)
            | Self::Jump(..)
            | Self::JumpToSubroutine(..)
            | Self::Link(..)
            | Self::LoadEffectiveAddress(..)
            | Self::LogicalShiftMemory(..)
            | Self::LogicalShiftRegister(..)
            | Self::MoveMultiple(..)
            | Self::MovePeripheral(..)
            | Self::MoveToCcr(..)
            | Self::MoveToSr(..)
            | Self::MoveUsp(..)
            | Self::MoveQuick(..)
            | Self::MultiplySigned(..)
            | Self::MultiplyUnsigned(..)
            | Self::NegateDecimal(..)
            | Self::NoOp
            | Self::OrToCcr
            | Self::OrToSr
            | Self::PushEffectiveAddress(..)
            | Self::Reset
            | Self::Return { .. }
            | Self::ReturnFromException
            | Self::RotateMemory(..)
            | Self::RotateRegister(..)
            | Self::RotateThruExtendMemory(..)
            | Self::RotateThruExtendRegister(..)
            | Self::Swap(..)
            | Self::Stop
            | Self::Test(..)
            | Self::Trap(..)
            | Self::TrapOnOverflow
            | Self::Unlink(..) => None,
        }
    }
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    #[allow(clippy::enum_glob_use)]
    pub(super) fn do_execute(&mut self) -> ExecuteResult<u32> {
        use Instruction::*;

        let initial_pc = self.registers.pc;

        let opcode = self.fetch_operand()?;
        self.opcode = opcode;

        let instruction = decode_opcode(opcode, self.registers.supervisor_mode)?;
        self.instruction = Some(instruction);
        log::trace!(
            "[{}] Decoded opcode {opcode:04X} (PC={initial_pc:06X}): {instruction}",
            self.name
        );

        match instruction {
            Add { size: OpSize::Byte, source, dest, with_extend } => {
                self.add_byte(source, dest, with_extend)
            }
            Add { size: OpSize::Word, source, dest, with_extend } => {
                self.add_word(source, dest, with_extend)
            }
            Add { size: OpSize::LongWord, source, dest, with_extend } => {
                self.add_long_word(source, dest, with_extend)
            }
            AddDecimal { source, dest } => self.abcd(source, dest),
            And { size: OpSize::Byte, source, dest } => self.and_byte(source, dest),
            And { size: OpSize::Word, source, dest } => self.and_word(source, dest),
            And { size: OpSize::LongWord, source, dest } => self.and_long_word(source, dest),
            AndToCcr => self.andi_to_ccr(),
            AndToSr => self.andi_to_sr(),
            ArithmeticShiftMemory(direction, dest) => self.asd_memory(direction, dest),
            ArithmeticShiftRegister(size, direction, register, count) => {
                Ok(self.asd_register(size, direction, register, count))
            }
            BitTest { source, dest } => self.btst(source, dest),
            BitTestAndChange { source, dest } => self.bchg(source, dest),
            BitTestAndClear { source, dest } => self.bclr(source, dest),
            BitTestAndSet { source, dest } => self.bset(source, dest),
            Branch(condition, displacement) => self.branch(condition, displacement),
            BranchDecrement(condition, register) => self.dbcc(condition, register),
            BranchToSubroutine(displacement) => self.bsr(displacement),
            CheckRegister(register, source) => self.chk(register, source),
            Clear(OpSize::Byte, dest) => self.clr_byte(dest),
            Clear(OpSize::Word, dest) => self.clr_word(dest),
            Clear(OpSize::LongWord, dest) => self.clr_long_word(dest),
            Compare { size: OpSize::Byte, source, dest } => self.cmp_byte(source, dest),
            Compare { size: OpSize::Word, source, dest } => self.cmp_word(source, dest),
            Compare { size: OpSize::LongWord, source, dest } => self.cmp_long_word(source, dest),
            DivideSigned(register, source) => self.divs(register, source),
            DivideUnsigned(register, source) => self.divu(register, source),
            ExchangeAddress(rx, ry) => Ok(self.exg_address(rx, ry)),
            ExchangeData(rx, ry) => Ok(self.exg_data(rx, ry)),
            ExchangeDataAddress(rx, ry) => Ok(self.exg_data_address(rx, ry)),
            ExclusiveOr { size: OpSize::Byte, source, dest } => self.eor_byte(source, dest),
            ExclusiveOr { size: OpSize::Word, source, dest } => self.eor_word(source, dest),
            ExclusiveOr { size: OpSize::LongWord, source, dest } => {
                self.eor_long_word(source, dest)
            }
            ExclusiveOrToCcr => self.eori_to_ccr(),
            ExclusiveOrToSr => self.eori_to_sr(),
            Extend(size, register) => Ok(self.ext(size, register)),
            Jump(source) => self.jmp(source),
            JumpToSubroutine(source) => self.jsr(source),
            Link(register) => self.link(register),
            LoadEffectiveAddress(source, dest) => self.lea(source, dest),
            LogicalShiftMemory(direction, dest) => self.lsd_memory(direction, dest),
            LogicalShiftRegister(size, direction, register, count) => {
                Ok(self.lsd_register(size, direction, register, count))
            }
            Move { size: OpSize::Byte, source, dest } => self.move_byte(source, dest),
            Move { size: OpSize::Word, source, dest } => self.move_word(source, dest),
            Move { size: OpSize::LongWord, source, dest } => self.move_long_word(source, dest),
            MoveFromSr(dest) => self.move_from_sr(dest),
            MoveMultiple(size, addressing_mode, direction) => {
                self.movem(size, addressing_mode, direction)
            }
            MovePeripheral(size, d_register, a_register, direction) => {
                self.movep(size, d_register, a_register, direction)
            }
            MoveQuick(data, register) => Ok(self.moveq(data, register)),
            MoveToCcr(source) => self.move_to_ccr(source),
            MoveToSr(source) => self.move_to_sr(source),
            MoveUsp(direction, register) => Ok(self.move_usp(direction, register)),
            MultiplySigned(register, source) => self.muls(register, source),
            MultiplyUnsigned(register, source) => self.mulu(register, source),
            Negate { size: OpSize::Byte, dest, with_extend } => self.neg_byte(dest, with_extend),
            Negate { size: OpSize::Word, dest, with_extend } => self.neg_word(dest, with_extend),
            Negate { size: OpSize::LongWord, dest, with_extend } => {
                self.neg_long_word(dest, with_extend)
            }
            NegateDecimal(dest) => self.nbcd(dest),
            NoOp => Ok(controlflow::nop()),
            Not(OpSize::Byte, dest) => self.not_byte(dest),
            Not(OpSize::Word, dest) => self.not_word(dest),
            Not(OpSize::LongWord, dest) => self.not_long_word(dest),
            Or { size: OpSize::Byte, source, dest } => self.or_byte(source, dest),
            Or { size: OpSize::Word, source, dest } => self.or_word(source, dest),
            Or { size: OpSize::LongWord, source, dest } => self.or_long_word(source, dest),
            OrToCcr => self.ori_to_ccr(),
            OrToSr => self.ori_to_sr(),
            PushEffectiveAddress(source) => self.pea(source),
            Reset => {
                // TODO RESET
                Ok(controlflow::nop())
            }
            Return { restore_ccr } => self.ret(restore_ccr),
            ReturnFromException => self.rte(),
            RotateMemory(direction, dest) => self.rod_memory(direction, dest),
            RotateRegister(size, direction, register, count) => {
                Ok(self.rod_register(size, direction, register, count))
            }
            RotateThruExtendMemory(direction, dest) => self.roxd_memory(direction, dest),
            RotateThruExtendRegister(size, direction, register, count) => {
                Ok(self.roxd_register(size, direction, register, count))
            }
            Set(condition, dest) => self.scc(condition, dest),
            Subtract { size: OpSize::Byte, source, dest, with_extend } => {
                self.sub_byte(source, dest, with_extend)
            }
            Subtract { size: OpSize::Word, source, dest, with_extend } => {
                self.sub_word(source, dest, with_extend)
            }
            Subtract { size: OpSize::LongWord, source, dest, with_extend } => {
                self.sub_long_word(source, dest, with_extend)
            }
            SubtractDecimal { source, dest } => self.sbcd(source, dest),
            Swap(register) => Ok(self.swap(register)),
            Stop => self.stop(),
            Test(OpSize::Byte, source) => self.tst_byte(source),
            Test(OpSize::Word, source) => self.tst_word(source),
            Test(OpSize::LongWord, source) => self.tst_long_word(source),
            TestAndSet(dest) => self.tas(dest),
            Trap(vector) => controlflow::trap(vector),
            TrapOnOverflow => self.trapv(),
            Unlink(register) => self.unlk(register),
        }
    }
}

fn decode_opcode(opcode: u16, supervisor_mode: bool) -> ExecuteResult<Instruction> {
    match opcode & 0xF000 {
        0x0000 => match opcode & 0b0000_1111_0000_0000 {
            0b0000_0000_0000_0000 => bits::decode_ori(opcode, supervisor_mode),
            0b0000_0010_0000_0000 => bits::decode_andi(opcode, supervisor_mode),
            0b0000_0100_0000_0000 => arithmetic::decode_subi(opcode),
            0b0000_0110_0000_0000 => arithmetic::decode_addi(opcode),
            0b0000_1010_0000_0000 => bits::decode_eori(opcode, supervisor_mode),
            0b0000_1100_0000_0000 => arithmetic::decode_cmpi(opcode),
            0b0000_1000_0000_0000 => match opcode & 0b0000_0000_1100_0000 {
                0b0000_0000_0000_0000 => bits::decode_btst_static(opcode),
                0b0000_0000_0100_0000 => bits::decode_bchg_static(opcode),
                0b0000_0000_1000_0000 => bits::decode_bclr_static(opcode),
                0b0000_0000_1100_0000 => bits::decode_bset_static(opcode),
                _ => unreachable!("match after bit mask"),
            },
            _ => {
                if opcode.bit(8) {
                    if opcode & 0b0000_0000_0011_1000 == 0b0000_0000_0000_1000 {
                        Ok(load::decode_movep(opcode))
                    } else {
                        match opcode & 0b0000_0000_1100_0000 {
                            0b0000_0000_0000_0000 => bits::decode_btst_dynamic(opcode),
                            0b0000_0000_0100_0000 => bits::decode_bchg_dynamic(opcode),
                            0b0000_0000_1000_0000 => bits::decode_bclr_dynamic(opcode),
                            0b0000_0000_1100_0000 => bits::decode_bset_dynamic(opcode),
                            _ => unreachable!("match after bit mask"),
                        }
                    }
                } else {
                    Err(Exception::IllegalInstruction(opcode))
                }
            }
        },
        0x1000 | 0x2000 | 0x3000 => load::decode_move(opcode),
        0x4000 => match opcode & 0b0000_1111_1100_0000 {
            0b0000_0000_1100_0000 => load::decode_move_from_sr(opcode),
            0b0000_0100_1100_0000 => load::decode_move_to_ccr(opcode),
            0b0000_0110_1100_0000 => load::decode_move_to_sr(opcode, supervisor_mode),
            0b0000_0000_0000_0000 | 0b0000_0000_0100_0000 | 0b0000_0000_1000_0000 => {
                arithmetic::decode_negx(opcode)
            }
            0b0000_0010_0000_0000 | 0b0000_0010_0100_0000 | 0b0000_0010_1000_0000 => {
                bits::decode_clr(opcode)
            }
            0b0000_0100_0000_0000 | 0b0000_0100_0100_0000 | 0b0000_0100_1000_0000 => {
                arithmetic::decode_neg(opcode)
            }
            0b0000_0110_0000_0000 | 0b0000_0110_0100_0000 | 0b0000_0110_1000_0000 => {
                bits::decode_not(opcode)
            }
            0b0000_1000_1000_0000
            | 0b0000_1000_1100_0000
            | 0b0000_1100_1000_0000
            | 0b0000_1100_1100_0000 => {
                if opcode & 0b0000_0000_0011_1000 == 0 {
                    Ok(bits::decode_ext(opcode))
                } else {
                    load::decode_movem(opcode)
                }
            }
            0b0000_1000_0000_0000 => arithmetic::decode_nbcd(opcode),
            0b0000_1000_0100_0000 => {
                if opcode & 0b0000_0000_0011_1000 == 0 {
                    Ok(bits::decode_swap(opcode))
                } else {
                    controlflow::decode_pea(opcode)
                }
            }
            0b0000_1010_1100_0000 => {
                if opcode & 0b0000_0000_0011_1111 == 0b0000_0000_0011_1100 {
                    Err(Exception::IllegalInstruction(opcode))
                } else {
                    bits::decode_tas(opcode)
                }
            }
            0b0000_1010_0000_0000 | 0b0000_1010_0100_0000 | 0b0000_1010_1000_0000 => {
                bits::decode_tst(opcode)
            }
            0b0000_1110_0100_0000 => match opcode & 0b0000_0000_0011_1111 {
                0b0000_0000_0011_0000 => {
                    if supervisor_mode {
                        Ok(Instruction::Reset)
                    } else {
                        Err(Exception::PrivilegeViolation)
                    }
                }
                0b0000_0000_0011_0001 => Ok(Instruction::NoOp),
                0b0000_0000_0011_0010 => Ok(Instruction::Stop),
                0b0000_0000_0011_0011 => controlflow::decode_rte(opcode, supervisor_mode),
                0b0000_0000_0011_0101 => Ok(Instruction::Return { restore_ccr: false }),
                0b0000_0000_0011_0110 => Ok(Instruction::TrapOnOverflow),
                0b0000_0000_0011_0111 => Ok(Instruction::Return { restore_ccr: true }),
                _ => match opcode & 0b0000_0000_0011_1000 {
                    0b0000_0000_0000_0000 | 0b0000_0000_0000_1000 => {
                        Ok(controlflow::decode_trap(opcode))
                    }
                    0b0000_0000_0001_0000 => Ok(controlflow::decode_link(opcode)),
                    0b0000_0000_0001_1000 => Ok(controlflow::decode_unlk(opcode)),
                    0b0000_0000_0010_0000 | 0b0000_0000_0010_1000 => {
                        load::decode_move_usp(opcode, supervisor_mode)
                    }
                    _ => Err(Exception::IllegalInstruction(opcode)),
                },
            },
            0b0000_1110_1000_0000 => controlflow::decode_jsr(opcode),
            0b0000_1110_1100_0000 => controlflow::decode_jmp(opcode),
            _ => {
                if opcode.bit(6) {
                    controlflow::decode_lea(opcode)
                } else {
                    controlflow::decode_chk(opcode)
                }
            }
        },
        0x5000 => match OpSize::parse_from_opcode(opcode) {
            Ok(size) => arithmetic::decode_addq_subq(opcode, size),
            Err(_) => {
                if opcode & 0b0000_0000_0011_1000 == 0b0000_0000_0000_1000 {
                    Ok(controlflow::decode_dbcc(opcode))
                } else {
                    controlflow::decode_scc(opcode)
                }
            }
        },
        0x6000 => Ok(controlflow::decode_branch(opcode)),
        0x7000 => load::decode_movq(opcode),
        0x8000 => match opcode & 0b0000_0001_1111_0000 {
            0b0000_0001_0000_0000 => Ok(arithmetic::decode_sbcd(opcode)),
            _ => match opcode & 0b0000_0001_1100_0000 {
                0b0000_0000_1100_0000 => arithmetic::decode_divu(opcode),
                0b0000_0001_1100_0000 => arithmetic::decode_divs(opcode),
                _ => bits::decode_or(opcode),
            },
        },
        0x9000 => arithmetic::decode_sub(opcode),
        0xB000 => match opcode & 0b0000_0000_1100_0000 {
            0b0000_0000_1100_0000 => arithmetic::decode_cmpa(opcode),
            _ => {
                if opcode.bit(8) {
                    match opcode & 0b0000_0000_0011_1000 {
                        0b0000_0000_0000_1000 => arithmetic::decode_cmpm(opcode),
                        _ => bits::decode_eor(opcode),
                    }
                } else {
                    arithmetic::decode_cmp(opcode)
                }
            }
        },
        0xC000 => match opcode & 0b0000_0001_1111_0000 {
            0b0000_0001_0000_0000 => Ok(arithmetic::decode_abcd(opcode)),
            0b0000_0001_0100_0000 | 0b0000_0001_1000_0000 => load::decode_exg(opcode),
            _ => match opcode & 0b0000_0001_1100_0000 {
                0b0000_0001_1100_0000 => arithmetic::decode_muls(opcode),
                0b0000_0000_1100_0000 => arithmetic::decode_mulu(opcode),
                _ => bits::decode_and(opcode),
            },
        },
        0xD000 => arithmetic::decode_add(opcode),
        0xE000 => match opcode & 0b0000_0000_1100_0000 {
            0b0000_0000_1100_0000 => match opcode & 0b0000_1110_0000_0000 {
                0b0000_0000_0000_0000 => bits::decode_asd_memory(opcode),
                0b0000_0010_0000_0000 => bits::decode_lsd_memory(opcode),
                0b0000_0100_0000_0000 => bits::decode_roxd_memory(opcode),
                0b0000_0110_0000_0000 => bits::decode_rod_memory(opcode),
                _ => Err(Exception::IllegalInstruction(opcode)),
            },
            _ => match opcode & 0b0000_0000_0001_1000 {
                0b0000_0000_0000_0000 => bits::decode_asd_register(opcode),
                0b0000_0000_0000_1000 => bits::decode_lsd_register(opcode),
                0b0000_0000_0001_0000 => bits::decode_roxd_register(opcode),
                0b0000_0000_0001_1000 => bits::decode_rod_register(opcode),
                _ => unreachable!("match after bit mask"),
            },
        },
        _ => Err(Exception::IllegalInstruction(opcode)),
    }
}

fn unary_op_cycles(size: OpSize, dest: AddressingMode) -> u32 {
    let mut cycles = match size {
        OpSize::Byte | OpSize::Word => 4,
        OpSize::LongWord => 8,
    };

    cycles += dest.address_calculation_cycles(size);

    // 2 cycles are saved in .l ops if operating on a data register
    if size == OpSize::LongWord && dest.is_data_direct() {
        cycles -= 2;
    }

    // Add 4 cycles if need to write back to memory
    if !dest.is_data_direct() {
        cycles += 4;
    }

    cycles
}

#[inline]
fn binary_op_cycles(size: OpSize, source: AddressingMode, dest: AddressingMode) -> u32 {
    let mut cycles = match size {
        OpSize::Byte | OpSize::Word => 4,
        OpSize::LongWord => 8,
    };

    // Word-size operations that write to address registers use the same circuitry as long word
    // operations
    if size == OpSize::Word && dest.is_address_direct() {
        cycles += 4;
    }

    cycles += source.address_calculation_cycles(size);
    cycles += dest.address_calculation_cycles(size);

    // 2 cycles are saved in .l ops if the source is in memory and the destination is a register
    if size == OpSize::LongWord
        && source.is_memory()
        && (dest.is_data_direct() || dest.is_address_direct())
    {
        cycles -= 2;
    }

    // 4 extra cycles are needed if the result is written back to memory
    if dest.is_memory() {
        cycles += 4;
    }

    cycles
}

impl Display for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Add { size, source, dest, with_extend: false } => {
                write!(f, "ADD.{size} {source}, {dest}")
            }
            Self::Add { size, source, dest, with_extend: true } => {
                write!(f, "ADDX.{size} {source}, {dest}")
            }
            Self::AddDecimal { source, dest } => {
                write!(f, "ABCD {source}, {dest}")
            }
            Self::And { size, source, dest } => {
                write!(f, "AND.{size} {source}, {dest}")
            }
            Self::AndToCcr => write!(f, "AND to CCR #<d>"),
            Self::AndToSr => write!(f, "AND to SR #<d>"),
            Self::ArithmeticShiftMemory(ShiftDirection::Left, dest) => {
                write!(f, "ASL.w {dest}")
            }
            Self::ArithmeticShiftMemory(ShiftDirection::Right, dest) => {
                write!(f, "ASR.w {dest}")
            }
            Self::ArithmeticShiftRegister(size, ShiftDirection::Left, register, count) => {
                write!(f, "ASL.{size} {count}, D{}", register.0)
            }
            Self::ArithmeticShiftRegister(size, ShiftDirection::Right, register, count) => {
                write!(f, "ASR.{size} {count}, D{}", register.0)
            }
            Self::BitTest { source, dest } => {
                write!(f, "BTST {source}, {dest}")
            }
            Self::BitTestAndChange { source, dest } => {
                write!(f, "BCHG {source}, {dest}")
            }
            Self::BitTestAndClear { source, dest } => {
                write!(f, "BCLR {source}, {dest}")
            }
            Self::BitTestAndSet { source, dest } => {
                write!(f, "BSET {source}, {dest}")
            }
            Self::Branch(BranchCondition::True, displacement) => {
                write!(f, "BRA {displacement}")
            }
            Self::Branch(condition, displacement) => {
                write!(f, "Bcc {displacement} (cc = {condition:?})")
            }
            Self::BranchDecrement(condition, register) => {
                write!(f, "DBcc D{}, #<d> (cc = {condition:?})", register.0)
            }
            Self::BranchToSubroutine(displacement) => {
                write!(f, "BSR {displacement}")
            }
            Self::CheckRegister(register, addressing_mode) => {
                write!(f, "CHK {addressing_mode}, D{}", register.0)
            }
            Self::Clear(size, dest) => {
                write!(f, "CLR.{size} {dest}")
            }
            Self::Compare { size, source, dest } => {
                write!(f, "CMP.{size} {source}, {dest}")
            }
            Self::DivideSigned(register, addressing_mode) => {
                write!(f, "DIVS {addressing_mode}, D{}", register.0)
            }
            Self::DivideUnsigned(register, addressing_mode) => {
                write!(f, "DIVU {addressing_mode}, D{}", register.0)
            }
            Self::ExchangeAddress(a, b) => {
                write!(f, "EXG A{}, A{}", a.0, b.0)
            }
            Self::ExchangeData(a, b) => {
                write!(f, "EXG D{}, D{}", a.0, b.0)
            }
            Self::ExchangeDataAddress(data, address) => {
                write!(f, "EXG D{}, A{}", data.0, address.0)
            }
            Self::ExclusiveOr { size, source, dest } => {
                write!(f, "EOR.{size} {source}, {dest}")
            }
            Self::ExclusiveOrToCcr => write!(f, "EOR to CCR"),
            Self::ExclusiveOrToSr => write!(f, "EOR to SR"),
            Self::Extend(size, register) => {
                write!(f, "EXT.{size} D{}", register.0)
            }
            Self::Jump(dest) => {
                write!(f, "JMP {dest}")
            }
            Self::JumpToSubroutine(dest) => {
                write!(f, "JSR {dest}")
            }
            Self::Link(register) => {
                write!(f, "LINK A{}, #<d>", register.0)
            }
            Self::LoadEffectiveAddress(addressing_mode, register) => {
                write!(f, "LEA {addressing_mode}, A{}", register.0)
            }
            Self::LogicalShiftMemory(ShiftDirection::Left, dest) => {
                write!(f, "LSL.w {dest}")
            }
            Self::LogicalShiftMemory(ShiftDirection::Right, dest) => {
                write!(f, "LSR.w {dest}")
            }
            Self::LogicalShiftRegister(size, ShiftDirection::Left, register, count) => {
                write!(f, "LSL.{size} {count}, D{}", register.0)
            }
            Self::LogicalShiftRegister(size, ShiftDirection::Right, register, count) => {
                write!(f, "LSR.{size} {count}, D{}", register.0)
            }
            Self::Move { size, source, dest } => {
                write!(f, "MOVE.{size} {source}, {dest}")
            }
            Self::MoveFromSr(dest) => write!(f, "MOVE from SR {dest}"),
            Self::MoveMultiple(size, addressing_mode, Direction::MemoryToRegister) => {
                write!(f, "MOVEM.{size} {addressing_mode}, registers")
            }
            Self::MoveMultiple(size, addressing_mode, Direction::RegisterToMemory) => {
                write!(f, "MOVEM.{size} registers, {addressing_mode}")
            }
            Self::MovePeripheral(size, data, address, Direction::MemoryToRegister) => {
                write!(f, "MOVEP.{size} (d, A{}), D{}", address.0, data.0)
            }
            Self::MovePeripheral(size, data, address, Direction::RegisterToMemory) => {
                write!(f, "MOVEP.{size} D{}, (d, A{})", data.0, address.0)
            }
            Self::MoveQuick(value, register) => {
                write!(f, "MOVEQ #<{value}>, D{}", register.0)
            }
            Self::MoveToCcr(source) => {
                write!(f, "MOVE to CCR {source}")
            }
            Self::MoveToSr(source) => {
                write!(f, "MOVE to SR {source}")
            }
            Self::MoveUsp(UspDirection::RegisterToUsp, register) => {
                write!(f, "MOVE to USP A{}", register.0)
            }
            Self::MoveUsp(UspDirection::UspToRegister, register) => {
                write!(f, "MOVE from USP A{}", register.0)
            }
            Self::MultiplySigned(register, addressing_mode) => {
                write!(f, "MULS {addressing_mode}, D{}", register.0)
            }
            Self::MultiplyUnsigned(register, addressing_mode) => {
                write!(f, "MULU {addressing_mode}, D{}", register.0)
            }
            Self::Negate { size, dest, with_extend: false } => {
                write!(f, "NEG.{size} {dest}")
            }
            Self::Negate { size, dest, with_extend: true } => {
                write!(f, "NEGX.{size} {dest}")
            }
            Self::NegateDecimal(dest) => {
                write!(f, "NBCD {dest}")
            }
            Self::NoOp => write!(f, "NOP"),
            Self::Not(size, dest) => {
                write!(f, "NOT.{size} {dest}")
            }
            Self::Or { size, source, dest } => {
                write!(f, "OR.{size} {source}, {dest}")
            }
            Self::OrToCcr => write!(f, "OR to CCR #<d>"),
            Self::OrToSr => write!(f, "OR to SR #<d>"),
            Self::PushEffectiveAddress(source) => {
                write!(f, "PEA {source}")
            }
            Self::Reset => write!(f, "RESET"),
            Self::Return { restore_ccr: false } => write!(f, "RTS"),
            Self::Return { restore_ccr: true } => write!(f, "RTR"),
            Self::ReturnFromException => write!(f, "RTE"),
            Self::RotateMemory(ShiftDirection::Left, dest) => {
                write!(f, "ROL.w {dest}")
            }
            Self::RotateMemory(ShiftDirection::Right, dest) => {
                write!(f, "ROR.w {dest}")
            }
            Self::RotateRegister(size, ShiftDirection::Left, register, count) => {
                write!(f, "ROL.{size} {count}, D{}", register.0)
            }
            Self::RotateRegister(size, ShiftDirection::Right, register, count) => {
                write!(f, "ROR.{size} {count}, D{}", register.0)
            }
            Self::RotateThruExtendMemory(ShiftDirection::Left, dest) => {
                write!(f, "ROXL.w {dest}")
            }
            Self::RotateThruExtendMemory(ShiftDirection::Right, dest) => {
                write!(f, "ROXR.w {dest}")
            }
            Self::RotateThruExtendRegister(size, ShiftDirection::Left, register, count) => {
                write!(f, "ROXL.{size} {count}, D{}", register.0)
            }
            Self::RotateThruExtendRegister(size, ShiftDirection::Right, register, count) => {
                write!(f, "ROXR.{size} {count}, D{}", register.0)
            }
            Self::Set(condition, dest) => {
                write!(f, "Scc {dest} (cc = {condition:?})")
            }
            Self::Subtract { size, source, dest, with_extend: false } => {
                write!(f, "SUB.{size} {source}, {dest}")
            }
            Self::Subtract { size, source, dest, with_extend: true } => {
                write!(f, "SUBX.{size} {source}, {dest}")
            }
            Self::SubtractDecimal { source, dest } => {
                write!(f, "SBCD {source}, {dest}")
            }
            Self::Swap(register) => {
                write!(f, "SWAP D{}", register.0)
            }
            Self::Stop => write!(f, "STOP"),
            Self::Test(size, source) => {
                write!(f, "TST.{size} {source}")
            }
            Self::TestAndSet(dest) => {
                write!(f, "TAS {dest}")
            }
            Self::Trap(vector) => {
                write!(f, "TRAP #<{vector}>")
            }
            Self::TrapOnOverflow => {
                write!(f, "TRAPV")
            }
            Self::Unlink(register) => {
                write!(f, "UNLK A{}", register.0)
            }
        }
    }
}
