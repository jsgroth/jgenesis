mod arithmetic;
mod bits;
mod load;

use crate::core::{
    AddressRegister, AddressingMode, DataRegister, Exception, ExecuteResult, InstructionExecutor,
    OpSize,
};
use crate::traits::{BusInterface, GetBit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
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
    Clear(OpSize, AddressingMode),
    Compare {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    ExclusiveOr {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    ExclusiveOrToCcr,
    ExclusiveOrToSr,
    LoadEffectiveAddress(AddressingMode, AddressRegister),
    Move {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    MoveFromSr(AddressingMode),
    MoveToCcr(AddressingMode),
    MoveToSr(AddressingMode),
    MoveUsp(UspDirection, AddressRegister),
    MoveQuick(i8, DataRegister),
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
    Subtract {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
        with_extend: bool,
    },
}

impl Instruction {
    pub fn source_addressing_mode(self) -> Option<AddressingMode> {
        match self {
            Self::Add { source, .. }
            | Self::And { source, .. }
            | Self::Compare { source, .. }
            | Self::ExclusiveOr { source, .. }
            | Self::LoadEffectiveAddress(source, ..)
            | Self::Move { source, .. }
            | Self::MoveToCcr(source)
            | Self::MoveToSr(source)
            | Self::Or { source, .. }
            | Self::Subtract { source, .. } => Some(source),
            Self::AndToCcr
            | Self::AndToSr
            | Self::Clear(..)
            | Self::ExclusiveOrToCcr
            | Self::ExclusiveOrToSr
            | Self::MoveQuick(..)
            | Self::MoveFromSr(..)
            | Self::MoveUsp(..)
            | Self::Negate { .. }
            | Self::NoOp
            | Self::Not(..)
            | Self::OrToCcr
            | Self::OrToSr => None,
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
            | Self::ExclusiveOrToCcr
            | Self::ExclusiveOrToSr
            | Self::LoadEffectiveAddress(..)
            | Self::MoveToCcr(..)
            | Self::MoveToSr(..)
            | Self::MoveUsp(..)
            | Self::MoveQuick(..)
            | Self::NoOp
            | Self::OrToCcr
            | Self::OrToSr => None,
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
            Instruction::Clear(size, dest) => self.clr(size, dest),
            Instruction::Compare { size, source, dest } => self.cmp(size, source, dest),
            Instruction::ExclusiveOr { size, source, dest } => self.eor(size, source, dest),
            Instruction::ExclusiveOrToCcr => self.eori_to_ccr(),
            Instruction::ExclusiveOrToSr => self.eori_to_sr(),
            Instruction::LoadEffectiveAddress(source, dest) => self.lea(source, dest),
            Instruction::Move { size, source, dest } => self.move_(size, source, dest),
            Instruction::MoveFromSr(dest) => self.move_from_sr(dest),
            Instruction::MoveToCcr(source) => self.move_to_ccr(source),
            Instruction::MoveToSr(source) => self.move_to_sr(source),
            Instruction::MoveQuick(data, register) => {
                self.moveq(data, register);
                Ok(())
            }
            Instruction::MoveUsp(direction, register) => {
                self.move_usp(direction, register);
                Ok(())
            }
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
            Instruction::Subtract {
                size,
                source,
                dest,
                with_extend,
            } => self.sub(size, source, dest, with_extend),
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
            0b0000_1000_0000_0000 => todo!("BTST / BCHG / BCLR / BSET (immediate)"),
            _ => {
                if opcode.bit(8) {
                    todo!("BTST / BCHG / BCLR / BSET (data register")
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
            | 0b0000_1100_1100_0000 => todo!("EXT / MOVEM"),
            0b0000_1000_0000_0000 => todo!("NBCD"),
            0b0000_1000_0100_0000 => todo!("SWAP / PEA"),
            0b0000_1010_1100_0000 => todo!("ILLEGAL / TAS"),
            0b0000_1010_0000_0000 | 0b0000_1010_0100_0000 | 0b0000_1010_1000_0000 => todo!("TST"),
            0b0000_1110_0100_0000 => match opcode & 0b0000_0000_0011_1111 {
                0b0000_0000_0011_0000 => todo!("RESET"),
                0b0000_0000_0011_0001 => Ok(Instruction::NoOp),
                0b0000_0000_0011_0010 => todo!("STOP"),
                _ => match opcode & 0b0000_0000_0011_1000 {
                    0b0000_0000_0000_0000 | 0b0000_0000_0000_1000 => todo!("TRAP"),
                    0b0000_0000_0001_0000 => todo!("LINK"),
                    0b0000_0000_0001_1000 => todo!("UNLK"),
                    0b0000_0000_0010_0000 | 0b0000_0000_0010_1000 => {
                        load::decode_move_usp(opcode, supervisor_mode)
                    }
                    _ => Err(Exception::IllegalInstruction(opcode)),
                },
            },
            0b0000_1110_1000_0000 => todo!("JSR"),
            0b0000_1110_1100_0000 => todo!("JMP"),
            _ => {
                if opcode.bit(8) {
                    load::decode_lea(opcode)
                } else {
                    todo!("CHK")
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
                0b0000_0000_1100_0000 => todo!("DIVU"),
                0b0000_0001_1100_0000 => todo!("DIVS"),
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
        0xC000 => {
            // AND (TODO: MULU / MULS / ABCD / EXG)
            bits::decode_and(opcode)
        }
        0xD000 => arithmetic::decode_add(opcode),
        0xE000 => todo!("ASd / LSd / ROXd / ROd"),
        _ => Err(Exception::IllegalInstruction(opcode)),
    }
}
