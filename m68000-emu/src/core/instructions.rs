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
            | Self::Test(..)
            | Self::Trap(..)
            | Self::TrapOnOverflow
            | Self::Unlink(..) => None,
        }
    }
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn do_execute(&mut self) -> ExecuteResult<u32> {
        log::trace!("Beginning instruction execution, PC={:08X}", self.registers.pc);

        let opcode = self.fetch_operand()?;
        self.opcode = opcode;

        let instruction = decode_opcode(opcode, self.registers.supervisor_mode)?;
        self.instruction = Some(instruction);
        log::trace!("Decoded instruction: {instruction:?}");

        match instruction {
            Instruction::Add { size, source, dest, with_extend } => {
                self.add(size, source, dest, with_extend)
            }
            Instruction::AddDecimal { source, dest } => self.abcd(source, dest),
            Instruction::And { size, source, dest } => self.and(size, source, dest),
            Instruction::AndToCcr => self.andi_to_ccr(),
            Instruction::AndToSr => self.andi_to_sr(),
            Instruction::ArithmeticShiftMemory(direction, dest) => self.asd_memory(direction, dest),
            Instruction::ArithmeticShiftRegister(size, direction, register, count) => {
                Ok(self.asd_register(size, direction, register, count))
            }
            Instruction::BitTest { source, dest } => self.btst(source, dest),
            Instruction::BitTestAndChange { source, dest } => self.bchg(source, dest),
            Instruction::BitTestAndClear { source, dest } => self.bclr(source, dest),
            Instruction::BitTestAndSet { source, dest } => self.bset(source, dest),
            Instruction::Branch(condition, displacement) => self.branch(condition, displacement),
            Instruction::BranchDecrement(condition, register) => self.dbcc(condition, register),
            Instruction::BranchToSubroutine(displacement) => self.bsr(displacement),
            Instruction::CheckRegister(register, source) => self.chk(register, source),
            Instruction::Clear(size, dest) => self.clr(size, dest),
            Instruction::Compare { size, source, dest } => self.cmp(size, source, dest),
            Instruction::DivideSigned(register, source) => self.divs(register, source),
            Instruction::DivideUnsigned(register, source) => self.divu(register, source),
            Instruction::ExchangeAddress(rx, ry) => Ok(self.exg_address(rx, ry)),
            Instruction::ExchangeData(rx, ry) => Ok(self.exg_data(rx, ry)),
            Instruction::ExchangeDataAddress(rx, ry) => Ok(self.exg_data_address(rx, ry)),
            Instruction::ExclusiveOr { size, source, dest } => self.eor(size, source, dest),
            Instruction::ExclusiveOrToCcr => self.eori_to_ccr(),
            Instruction::ExclusiveOrToSr => self.eori_to_sr(),
            Instruction::Extend(size, register) => Ok(self.ext(size, register)),
            Instruction::Jump(source) => self.jmp(source),
            Instruction::JumpToSubroutine(source) => self.jsr(source),
            Instruction::Link(register) => self.link(register),
            Instruction::LoadEffectiveAddress(source, dest) => self.lea(source, dest),
            Instruction::LogicalShiftMemory(direction, dest) => self.lsd_memory(direction, dest),
            Instruction::LogicalShiftRegister(size, direction, register, count) => {
                Ok(self.lsd_register(size, direction, register, count))
            }
            Instruction::Move { size, source, dest } => self.move_(size, source, dest),
            Instruction::MoveFromSr(dest) => self.move_from_sr(dest),
            Instruction::MoveMultiple(size, addressing_mode, direction) => {
                self.movem(size, addressing_mode, direction)
            }
            Instruction::MovePeripheral(size, d_register, a_register, direction) => {
                self.movep(size, d_register, a_register, direction)
            }
            Instruction::MoveQuick(data, register) => Ok(self.moveq(data, register)),
            Instruction::MoveToCcr(source) => self.move_to_ccr(source),
            Instruction::MoveToSr(source) => self.move_to_sr(source),
            Instruction::MoveUsp(direction, register) => Ok(self.move_usp(direction, register)),
            Instruction::MultiplySigned(register, source) => self.muls(register, source),
            Instruction::MultiplyUnsigned(register, source) => self.mulu(register, source),
            Instruction::Negate { size, dest, with_extend } => self.neg(size, dest, with_extend),
            Instruction::NegateDecimal(dest) => self.nbcd(dest),
            Instruction::NoOp => Ok(controlflow::nop()),
            Instruction::Not(size, dest) => self.not(size, dest),
            Instruction::Or { size, source, dest } => self.or(size, source, dest),
            Instruction::OrToCcr => self.ori_to_ccr(),
            Instruction::OrToSr => self.ori_to_sr(),
            Instruction::PushEffectiveAddress(source) => self.pea(source),
            Instruction::Reset => {
                // TODO RESET
                Ok(controlflow::nop())
            }
            Instruction::Return { restore_ccr } => self.ret(restore_ccr),
            Instruction::ReturnFromException => self.rte(),
            Instruction::RotateMemory(direction, dest) => self.rod_memory(direction, dest),
            Instruction::RotateRegister(size, direction, register, count) => {
                Ok(self.rod_register(size, direction, register, count))
            }
            Instruction::RotateThruExtendMemory(direction, dest) => {
                self.roxd_memory(direction, dest)
            }
            Instruction::RotateThruExtendRegister(size, direction, register, count) => {
                Ok(self.roxd_register(size, direction, register, count))
            }
            Instruction::Set(condition, dest) => self.scc(condition, dest),
            Instruction::Subtract { size, source, dest, with_extend } => {
                self.sub(size, source, dest, with_extend)
            }
            Instruction::SubtractDecimal { source, dest } => self.sbcd(source, dest),
            Instruction::Swap(register) => Ok(self.swap(register)),
            Instruction::Test(size, source) => self.tst(size, source),
            Instruction::TestAndSet(dest) => self.tas(dest),
            Instruction::Trap(vector) => controlflow::trap(vector),
            Instruction::TrapOnOverflow => self.trapv(),
            Instruction::Unlink(register) => self.unlk(register),
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
                0b0000_0000_0011_0010 => todo!("STOP"),
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
