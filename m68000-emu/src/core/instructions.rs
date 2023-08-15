mod arithmetic;
mod bits;
mod controlflow;
mod load;

use crate::core::{
    AddressRegister, AddressingMode, DataRegister, Exception, ExecuteResult, InstructionExecutor,
    OpSize,
};
use crate::traits::{BusInterface, GetBit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    RegisterToMemory,
    MemoryToRegister,
}

impl Direction {
    fn parse_from_opcode(opcode: u16) -> Self {
        if opcode.bit(8) {
            Self::RegisterToMemory
        } else {
            Self::MemoryToRegister
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UspDirection {
    RegisterToUsp,
    UspToRegister,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtendOpMode {
    DataDirect,
    AddressIndirectPredecrement,
}

impl ExtendOpMode {
    fn parse_from_opcode(opcode: u16) -> Self {
        if opcode.bit(3) {
            Self::AddressIndirectPredecrement
        } else {
            Self::DataDirect
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    Add {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
        with_extend: bool,
    },
    And {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    AndToCcr,
    AndToSr,
    BitTest {
        source: AddressingMode,
        dest: AddressingMode,
    },
    BitTestAndChange {
        source: AddressingMode,
        dest: AddressingMode,
    },
    BitTestAndClear {
        source: AddressingMode,
        dest: AddressingMode,
    },
    BitTestAndSet {
        source: AddressingMode,
        dest: AddressingMode,
    },
    CheckRegister(DataRegister, AddressingMode),
    Clear(OpSize, AddressingMode),
    Compare {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    DivideSigned(DataRegister, AddressingMode),
    DivideUnsigned(DataRegister, AddressingMode),
    ExchangeAddress(AddressRegister, AddressRegister),
    ExchangeData(DataRegister, DataRegister),
    ExchangeDataAddress(DataRegister, AddressRegister),
    ExclusiveOr {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    ExclusiveOrToCcr,
    ExclusiveOrToSr,
    Extend(OpSize, DataRegister),
    Jump(AddressingMode),
    JumpToSubroutine(AddressingMode),
    LoadEffectiveAddress(AddressingMode, AddressRegister),
    Link(AddressRegister),
    Move {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    MoveFromSr(AddressingMode),
    MoveMultiple(OpSize, AddressingMode, Direction),
    MovePeripheral(OpSize, DataRegister, AddressRegister, Direction),
    MoveQuick(i8, DataRegister),
    MoveToCcr(AddressingMode),
    MoveToSr(AddressingMode),
    MoveUsp(UspDirection, AddressRegister),
    MultiplySigned(DataRegister, AddressingMode),
    MultiplyUnsigned(DataRegister, AddressingMode),
    Negate {
        size: OpSize,
        dest: AddressingMode,
        with_extend: bool,
    },
    NoOp,
    Not(OpSize, AddressingMode),
    Or {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    OrToCcr,
    OrToSr,
    PushEffectiveAddress(AddressingMode),
    Return {
        restore_ccr: bool,
    },
    ReturnFromException,
    Subtract {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
        with_extend: bool,
    },
    Swap(DataRegister),
    Trap(u32),
    TrapOnOverflow,
    Unlink(AddressRegister),
}

impl Instruction {
    pub fn source_addressing_mode(self) -> Option<AddressingMode> {
        match self {
            Self::Add { source, .. }
            | Self::And { source, .. }
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
            | Self::Jump(source)
            | Self::JumpToSubroutine(source)
            | Self::Move { source, .. }
            | Self::MoveToCcr(source)
            | Self::MoveToSr(source)
            | Self::MultiplySigned(_, source)
            | Self::MultiplyUnsigned(_, source)
            | Self::Or { source, .. }
            | Self::PushEffectiveAddress(source)
            | Self::Subtract { source, .. } => Some(source),
            Self::AndToCcr
            | Self::AndToSr
            | Self::Clear(..)
            | Self::ExchangeAddress(..)
            | Self::ExchangeData(..)
            | Self::ExchangeDataAddress(..)
            | Self::ExclusiveOrToCcr
            | Self::ExclusiveOrToSr
            | Self::Extend(..)
            | Self::Link(..)
            | Self::MoveFromSr(..)
            | Self::MoveMultiple(..)
            | Self::MovePeripheral(..)
            | Self::MoveQuick(..)
            | Self::MoveUsp(..)
            | Self::Negate { .. }
            | Self::NoOp
            | Self::Not(..)
            | Self::OrToCcr
            | Self::OrToSr
            | Self::Return { .. }
            | Self::ReturnFromException
            | Self::Swap(..)
            | Self::Trap(..)
            | Self::TrapOnOverflow
            | Self::Unlink(..) => None,
        }
    }

    pub fn dest_addressing_mode(self) -> Option<AddressingMode> {
        match self {
            Self::Add { dest, .. }
            | Self::And { dest, .. }
            | Self::Clear(_, dest)
            | Self::Compare { dest, .. }
            | Self::ExclusiveOr { dest, .. }
            | Self::Move { dest, .. }
            | Self::MoveFromSr(dest)
            | Self::Negate { dest, .. }
            | Self::Not(_, dest)
            | Self::Or { dest, .. }
            | Self::Subtract { dest, .. } => Some(dest),
            Self::AndToCcr
            | Self::AndToSr
            | Self::BitTest { .. }
            | Self::BitTestAndChange { .. }
            | Self::BitTestAndClear { .. }
            | Self::BitTestAndSet { .. }
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
            | Self::LoadEffectiveAddress(..)
            | Self::Link(..)
            | Self::MoveMultiple(..)
            | Self::MovePeripheral(..)
            | Self::MoveToCcr(..)
            | Self::MoveToSr(..)
            | Self::MoveUsp(..)
            | Self::MoveQuick(..)
            | Self::MultiplySigned(..)
            | Self::MultiplyUnsigned(..)
            | Self::NoOp
            | Self::OrToCcr
            | Self::OrToSr
            | Self::PushEffectiveAddress(..)
            | Self::Return { .. }
            | Self::ReturnFromException
            | Self::Swap(..)
            | Self::Trap(..)
            | Self::TrapOnOverflow
            | Self::Unlink(..) => None,
        }
    }
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn do_execute(&mut self) -> ExecuteResult<()> {
        let opcode = self.fetch_operand()?;
        self.opcode = opcode;

        let instruction = decode_opcode(opcode, self.registers.supervisor_mode)?;
        self.instruction = Some(instruction);
        log::trace!("Decoded instruction: {instruction:?}");

        match instruction {
            Instruction::Add {
                size,
                source,
                dest,
                with_extend,
            } => self.add(size, source, dest, with_extend),
            Instruction::And { size, source, dest } => self.and(size, source, dest),
            Instruction::AndToCcr => self.andi_to_ccr(),
            Instruction::AndToSr => self.andi_to_sr(),
            Instruction::BitTest { source, dest } => self.btst(source, dest),
            Instruction::BitTestAndChange { source, dest } => self.bchg(source, dest),
            Instruction::BitTestAndClear { source, dest } => self.bclr(source, dest),
            Instruction::BitTestAndSet { source, dest } => self.bset(source, dest),
            Instruction::CheckRegister(register, source) => self.chk(register, source),
            Instruction::Clear(size, dest) => self.clr(size, dest),
            Instruction::Compare { size, source, dest } => self.cmp(size, source, dest),
            Instruction::DivideSigned(register, source) => self.divs(register, source),
            Instruction::DivideUnsigned(register, source) => self.divu(register, source),
            Instruction::ExchangeAddress(rx, ry) => {
                self.exg_address(rx, ry);
                Ok(())
            }
            Instruction::ExchangeData(rx, ry) => {
                self.exg_data(rx, ry);
                Ok(())
            }
            Instruction::ExchangeDataAddress(rx, ry) => {
                self.exg_data_address(rx, ry);
                Ok(())
            }
            Instruction::ExclusiveOr { size, source, dest } => self.eor(size, source, dest),
            Instruction::ExclusiveOrToCcr => self.eori_to_ccr(),
            Instruction::ExclusiveOrToSr => self.eori_to_sr(),
            Instruction::Extend(size, register) => {
                self.ext(size, register);
                Ok(())
            }
            Instruction::Jump(source) => self.jmp(source),
            Instruction::JumpToSubroutine(source) => self.jsr(source),
            Instruction::LoadEffectiveAddress(source, dest) => self.lea(source, dest),
            Instruction::Link(register) => self.link(register),
            Instruction::Move { size, source, dest } => self.move_(size, source, dest),
            Instruction::MoveFromSr(dest) => self.move_from_sr(dest),
            Instruction::MoveMultiple(size, addressing_mode, direction) => {
                self.movem(size, addressing_mode, direction)
            }
            Instruction::MovePeripheral(size, d_register, a_register, direction) => {
                self.movep(size, d_register, a_register, direction)
            }
            Instruction::MoveQuick(data, register) => {
                self.moveq(data, register);
                Ok(())
            }
            Instruction::MoveToCcr(source) => self.move_to_ccr(source),
            Instruction::MoveToSr(source) => self.move_to_sr(source),
            Instruction::MoveUsp(direction, register) => {
                self.move_usp(direction, register);
                Ok(())
            }
            Instruction::MultiplySigned(register, source) => self.muls(register, source),
            Instruction::MultiplyUnsigned(register, source) => self.mulu(register, source),
            Instruction::Negate {
                size,
                dest,
                with_extend,
            } => self.neg(size, dest, with_extend),
            Instruction::NoOp => Ok(()),
            Instruction::Not(size, dest) => self.not(size, dest),
            Instruction::Or { size, source, dest } => self.or(size, source, dest),
            Instruction::OrToCcr => self.ori_to_ccr(),
            Instruction::OrToSr => self.ori_to_sr(),
            Instruction::PushEffectiveAddress(source) => self.pea(source),
            Instruction::Subtract {
                size,
                source,
                dest,
                with_extend,
            } => self.sub(size, source, dest, with_extend),
            Instruction::Swap(register) => {
                self.swap(register);
                Ok(())
            }
            Instruction::Return { restore_ccr } => self.ret(restore_ccr),
            Instruction::ReturnFromException => self.rte(),
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
            0b0000_1000_0000_0000 => todo!("NBCD"),
            0b0000_1000_0100_0000 => {
                if opcode & 0b0000_0000_0011_1000 == 0 {
                    Ok(bits::decode_swap(opcode))
                } else {
                    controlflow::decode_pea(opcode)
                }
            }
            0b0000_1010_1100_0000 => todo!("ILLEGAL / TAS"),
            0b0000_1010_0000_0000 | 0b0000_1010_0100_0000 | 0b0000_1010_1000_0000 => todo!("TST"),
            0b0000_1110_0100_0000 => match opcode & 0b0000_0000_0011_1111 {
                0b0000_0000_0011_0000 => todo!("RESET"),
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
                todo!("Scc / DBcc")
            }
        },
        0x6000 => todo!("BRA / BSR / Bcc"),
        0x7000 => load::decode_movq(opcode),
        0x8000 => match opcode & 0b0000_0001_1111_0000 {
            0b0000_0001_0000_0000 => todo!("SBCD"),
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
            0b0000_0001_0000_0000 => todo!("ABCD"),
            0b0000_0001_0100_0000 | 0b0000_0001_1000_0000 => load::decode_exg(opcode),
            _ => match opcode & 0b0000_0001_1100_0000 {
                0b0000_0001_1100_0000 => arithmetic::decode_muls(opcode),
                0b0000_0000_1100_0000 => arithmetic::decode_mulu(opcode),
                _ => bits::decode_and(opcode),
            },
        },
        0xD000 => arithmetic::decode_add(opcode),
        0xE000 => todo!("ASd / LSd / ROXd / ROd"),
        _ => Err(Exception::IllegalInstruction(opcode)),
    }
}
