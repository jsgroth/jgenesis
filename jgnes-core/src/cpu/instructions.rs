#![cfg(test)]

use crate::bus;
use crate::bus::CpuBus;
use crate::cpu::{CpuRegisters, StatusFlags};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressingMode {
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX,
    IndirectY,
    Implied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCondition {
    // BCC
    CarryClear,
    // BCS
    CarrySet,
    // BEQ
    Equal,
    // BMI
    Minus,
    // BNE
    NotEqual,
    // BPL
    Positive,
    // BVC
    OverflowClear,
    // BVS
    OverflowSet,
}

impl BranchCondition {
    fn check(self, status_flags: &StatusFlags<'_>) -> bool {
        match self {
            Self::CarryClear => !status_flags.carry(),
            Self::CarrySet => status_flags.carry(),
            Self::Equal => status_flags.zero(),
            Self::Minus => status_flags.negative(),
            Self::NotEqual => !status_flags.zero(),
            Self::Positive => !status_flags.negative(),
            Self::OverflowClear => !status_flags.overflow(),
            Self::OverflowSet => status_flags.overflow(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuRegister {
    A,
    X,
    Y,
    S,
    P,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionType {
    Read,
    Write,
    ReadModifyWrite,
    RegistersOnly,
    Branch,
    Jump,
    JumpToSubroutine,
    PushStack,
    PullStack,
    ReturnFromInterrupt,
    ReturnFromSubroutine,
    ForceInterrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    // ADC
    AddWithCarry(AddressingMode),
    // AND
    And(AddressingMode),
    // ASL
    ShiftLeft(AddressingMode),
    // Various branch instructions, see BranchCondition
    Branch(BranchCondition),
    // BIT
    BitTest(AddressingMode),
    // BRK
    ForceInterrupt,
    // CLC
    ClearCarryFlag,
    // CLD
    ClearDecimalFlag,
    // CLI
    ClearInterruptDisable,
    // CLV
    ClearOverflowFlag,
    // CMP, CPX, CPY
    Compare(CpuRegister, AddressingMode),
    // DEC
    DecrementMemory(AddressingMode),
    // DEX, DEY
    DecrementRegister(CpuRegister),
    // EOR
    ExclusiveOr(AddressingMode),
    // INC
    IncrementMemory(AddressingMode),
    // INX, INY
    IncrementRegister(CpuRegister),
    // JMP
    Jump(AddressingMode),
    // JSR
    JumpToSubroutine,
    // LDA, LDX, LDY
    LoadRegister(CpuRegister, AddressingMode),
    // LSR
    LogicalShiftRight(AddressingMode),
    // NOP
    NoOp,
    // ORA
    InclusiveOr(AddressingMode),
    // PHA, PHP
    PushStack(CpuRegister),
    // PLA, PLP
    PullStack(CpuRegister),
    // ROL
    RotateLeft(AddressingMode),
    // ROR
    RotateRight(AddressingMode),
    // RTI
    ReturnFromInterrupt,
    // RTS
    ReturnFromSubroutine,
    // SBC
    SubtractWithCarry(AddressingMode),
    // SEC
    SetCarryFlag,
    // SED
    SetDecimalFlag,
    // SEI
    SetInterruptDisable,
    // STA, STX, STY
    StoreRegister(CpuRegister, AddressingMode),
    // TAX, TAY, TSX, TXA, TXS, TYA
    TransferBetweenRegisters { to: CpuRegister, from: CpuRegister },
}

impl Instruction {
    fn from_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            0x00 => Some(Self::ForceInterrupt),
            0x01 => Some(Self::InclusiveOr(AddressingMode::IndirectX)),
            0x05 => Some(Self::InclusiveOr(AddressingMode::ZeroPage)),
            0x06 => Some(Self::ShiftLeft(AddressingMode::ZeroPage)),
            0x08 => Some(Self::PushStack(CpuRegister::P)),
            0x09 => Some(Self::InclusiveOr(AddressingMode::Immediate)),
            0x0A => Some(Self::ShiftLeft(AddressingMode::Accumulator)),
            0x0D => Some(Self::InclusiveOr(AddressingMode::Absolute)),
            0x0E => Some(Self::ShiftLeft(AddressingMode::Absolute)),
            0x10 => Some(Self::Branch(BranchCondition::Positive)),
            0x11 => Some(Self::InclusiveOr(AddressingMode::IndirectY)),
            0x15 => Some(Self::InclusiveOr(AddressingMode::ZeroPageX)),
            0x16 => Some(Self::ShiftLeft(AddressingMode::ZeroPageX)),
            0x18 => Some(Self::ClearCarryFlag),
            0x19 => Some(Self::InclusiveOr(AddressingMode::AbsoluteY)),
            0x1D => Some(Self::InclusiveOr(AddressingMode::AbsoluteX)),
            0x1E => Some(Self::ShiftLeft(AddressingMode::AbsoluteX)),
            0x20 => Some(Self::JumpToSubroutine),
            0x21 => Some(Self::And(AddressingMode::IndirectX)),
            0x24 => Some(Self::BitTest(AddressingMode::ZeroPage)),
            0x25 => Some(Self::And(AddressingMode::ZeroPage)),
            0x26 => Some(Self::RotateLeft(AddressingMode::ZeroPage)),
            0x28 => Some(Self::PullStack(CpuRegister::P)),
            0x29 => Some(Self::And(AddressingMode::Immediate)),
            0x2A => Some(Self::RotateLeft(AddressingMode::Accumulator)),
            0x2C => Some(Self::BitTest(AddressingMode::Absolute)),
            0x2D => Some(Self::And(AddressingMode::Absolute)),
            0x2E => Some(Self::RotateLeft(AddressingMode::Absolute)),
            0x30 => Some(Self::Branch(BranchCondition::Minus)),
            0x31 => Some(Self::And(AddressingMode::IndirectY)),
            0x35 => Some(Self::And(AddressingMode::ZeroPageX)),
            0x36 => Some(Self::RotateLeft(AddressingMode::ZeroPageX)),
            0x38 => Some(Self::SetCarryFlag),
            0x39 => Some(Self::And(AddressingMode::AbsoluteY)),
            0x3D => Some(Self::And(AddressingMode::AbsoluteX)),
            0x3E => Some(Self::RotateLeft(AddressingMode::AbsoluteX)),
            0x40 => Some(Self::ReturnFromInterrupt),
            0x41 => Some(Self::ExclusiveOr(AddressingMode::IndirectX)),
            0x45 => Some(Self::ExclusiveOr(AddressingMode::ZeroPage)),
            0x46 => Some(Self::LogicalShiftRight(AddressingMode::ZeroPage)),
            0x48 => Some(Self::PushStack(CpuRegister::A)),
            0x49 => Some(Self::ExclusiveOr(AddressingMode::Immediate)),
            0x4A => Some(Self::LogicalShiftRight(AddressingMode::Accumulator)),
            0x4C => Some(Self::Jump(AddressingMode::Absolute)),
            0x4D => Some(Self::ExclusiveOr(AddressingMode::Absolute)),
            0x4E => Some(Self::LogicalShiftRight(AddressingMode::Absolute)),
            0x50 => Some(Self::Branch(BranchCondition::OverflowClear)),
            0x51 => Some(Self::ExclusiveOr(AddressingMode::IndirectY)),
            0x55 => Some(Self::ExclusiveOr(AddressingMode::ZeroPageX)),
            0x56 => Some(Self::LogicalShiftRight(AddressingMode::ZeroPageX)),
            0x58 => Some(Self::ClearInterruptDisable),
            0x59 => Some(Self::ExclusiveOr(AddressingMode::AbsoluteY)),
            0x5D => Some(Self::ExclusiveOr(AddressingMode::AbsoluteX)),
            0x5E => Some(Self::LogicalShiftRight(AddressingMode::AbsoluteX)),
            0x60 => Some(Self::ReturnFromSubroutine),
            0x61 => Some(Self::AddWithCarry(AddressingMode::IndirectX)),
            0x65 => Some(Self::AddWithCarry(AddressingMode::ZeroPage)),
            0x66 => Some(Self::RotateRight(AddressingMode::ZeroPage)),
            0x68 => Some(Self::PullStack(CpuRegister::A)),
            0x69 => Some(Self::AddWithCarry(AddressingMode::Immediate)),
            0x6A => Some(Self::RotateRight(AddressingMode::Accumulator)),
            0x6C => Some(Self::Jump(AddressingMode::Indirect)),
            0x6D => Some(Self::AddWithCarry(AddressingMode::Absolute)),
            0x6E => Some(Self::RotateRight(AddressingMode::Absolute)),
            0x70 => Some(Self::Branch(BranchCondition::OverflowSet)),
            0x71 => Some(Self::AddWithCarry(AddressingMode::IndirectY)),
            0x75 => Some(Self::AddWithCarry(AddressingMode::ZeroPageX)),
            0x76 => Some(Self::RotateRight(AddressingMode::ZeroPageX)),
            0x78 => Some(Self::SetInterruptDisable),
            0x79 => Some(Self::AddWithCarry(AddressingMode::AbsoluteY)),
            0x7D => Some(Self::AddWithCarry(AddressingMode::AbsoluteX)),
            0x7E => Some(Self::RotateRight(AddressingMode::AbsoluteX)),
            0x81 => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::IndirectX,
            )),
            0x84 => Some(Self::StoreRegister(
                CpuRegister::Y,
                AddressingMode::ZeroPage,
            )),
            0x85 => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::ZeroPage,
            )),
            0x86 => Some(Self::StoreRegister(
                CpuRegister::X,
                AddressingMode::ZeroPage,
            )),
            0x88 => Some(Self::DecrementRegister(CpuRegister::Y)),
            0x8A => Some(Self::TransferBetweenRegisters {
                to: CpuRegister::A,
                from: CpuRegister::X,
            }),
            0x8C => Some(Self::StoreRegister(
                CpuRegister::Y,
                AddressingMode::Absolute,
            )),
            0x8D => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::Absolute,
            )),
            0x8E => Some(Self::StoreRegister(
                CpuRegister::X,
                AddressingMode::Absolute,
            )),
            0x90 => Some(Self::Branch(BranchCondition::CarryClear)),
            0x91 => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::IndirectY,
            )),
            0x94 => Some(Self::StoreRegister(
                CpuRegister::Y,
                AddressingMode::ZeroPageX,
            )),
            0x95 => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::ZeroPageX,
            )),
            0x96 => Some(Self::StoreRegister(
                CpuRegister::X,
                AddressingMode::ZeroPageY,
            )),
            0x98 => Some(Self::TransferBetweenRegisters {
                to: CpuRegister::A,
                from: CpuRegister::Y,
            }),
            0x99 => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteY,
            )),
            0x9A => Some(Self::TransferBetweenRegisters {
                to: CpuRegister::S,
                from: CpuRegister::X,
            }),
            0x9D => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteX,
            )),
            0xA0 => Some(Self::LoadRegister(
                CpuRegister::Y,
                AddressingMode::Immediate,
            )),
            0xA1 => Some(Self::LoadRegister(
                CpuRegister::A,
                AddressingMode::IndirectX,
            )),
            0xA2 => Some(Self::LoadRegister(
                CpuRegister::X,
                AddressingMode::Immediate,
            )),
            0xA4 => Some(Self::LoadRegister(CpuRegister::Y, AddressingMode::ZeroPage)),
            0xA5 => Some(Self::LoadRegister(CpuRegister::A, AddressingMode::ZeroPage)),
            0xA6 => Some(Self::LoadRegister(CpuRegister::X, AddressingMode::ZeroPage)),
            0xA8 => Some(Self::TransferBetweenRegisters {
                to: CpuRegister::Y,
                from: CpuRegister::A,
            }),
            0xA9 => Some(Self::LoadRegister(
                CpuRegister::A,
                AddressingMode::Immediate,
            )),
            0xAA => Some(Self::TransferBetweenRegisters {
                to: CpuRegister::X,
                from: CpuRegister::A,
            }),
            0xAC => Some(Self::LoadRegister(CpuRegister::Y, AddressingMode::Absolute)),
            0xAD => Some(Self::LoadRegister(CpuRegister::A, AddressingMode::Absolute)),
            0xAE => Some(Self::LoadRegister(CpuRegister::X, AddressingMode::Absolute)),
            0xB0 => Some(Self::Branch(BranchCondition::CarrySet)),
            0xB1 => Some(Self::LoadRegister(
                CpuRegister::A,
                AddressingMode::IndirectY,
            )),
            0xB4 => Some(Self::LoadRegister(
                CpuRegister::Y,
                AddressingMode::ZeroPageX,
            )),
            0xB5 => Some(Self::LoadRegister(
                CpuRegister::A,
                AddressingMode::ZeroPageX,
            )),
            0xB6 => Some(Self::LoadRegister(
                CpuRegister::X,
                AddressingMode::ZeroPageY,
            )),
            0xB8 => Some(Self::ClearOverflowFlag),
            0xB9 => Some(Self::LoadRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteY,
            )),
            0xBA => Some(Self::TransferBetweenRegisters {
                to: CpuRegister::X,
                from: CpuRegister::S,
            }),
            0xBC => Some(Self::LoadRegister(
                CpuRegister::Y,
                AddressingMode::AbsoluteX,
            )),
            0xBD => Some(Self::LoadRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteX,
            )),
            0xBE => Some(Self::LoadRegister(
                CpuRegister::X,
                AddressingMode::AbsoluteY,
            )),
            0xC0 => Some(Self::Compare(CpuRegister::Y, AddressingMode::Immediate)),
            0xC1 => Some(Self::Compare(CpuRegister::A, AddressingMode::IndirectX)),
            0xC4 => Some(Self::Compare(CpuRegister::Y, AddressingMode::ZeroPage)),
            0xC5 => Some(Self::Compare(CpuRegister::A, AddressingMode::ZeroPage)),
            0xC6 => Some(Self::DecrementMemory(AddressingMode::ZeroPage)),
            0xC8 => Some(Self::IncrementRegister(CpuRegister::Y)),
            0xC9 => Some(Self::Compare(CpuRegister::A, AddressingMode::Immediate)),
            0xCA => Some(Self::DecrementRegister(CpuRegister::X)),
            0xCC => Some(Self::Compare(CpuRegister::Y, AddressingMode::Absolute)),
            0xCD => Some(Self::Compare(CpuRegister::A, AddressingMode::Absolute)),
            0xCE => Some(Self::DecrementMemory(AddressingMode::Absolute)),
            0xD0 => Some(Self::Branch(BranchCondition::NotEqual)),
            0xD1 => Some(Self::Compare(CpuRegister::A, AddressingMode::IndirectY)),
            0xD5 => Some(Self::Compare(CpuRegister::A, AddressingMode::ZeroPageX)),
            0xD6 => Some(Self::DecrementMemory(AddressingMode::ZeroPageX)),
            0xD8 => Some(Self::ClearDecimalFlag),
            0xD9 => Some(Self::Compare(CpuRegister::A, AddressingMode::AbsoluteY)),
            0xDD => Some(Self::Compare(CpuRegister::A, AddressingMode::AbsoluteX)),
            0xDE => Some(Self::DecrementMemory(AddressingMode::AbsoluteX)),
            0xE0 => Some(Self::Compare(CpuRegister::X, AddressingMode::Immediate)),
            0xE1 => Some(Self::SubtractWithCarry(AddressingMode::IndirectX)),
            0xE4 => Some(Self::Compare(CpuRegister::X, AddressingMode::ZeroPage)),
            0xE5 => Some(Self::SubtractWithCarry(AddressingMode::ZeroPage)),
            0xE6 => Some(Self::IncrementMemory(AddressingMode::ZeroPage)),
            0xE8 => Some(Self::IncrementRegister(CpuRegister::X)),
            0xE9 => Some(Self::SubtractWithCarry(AddressingMode::Immediate)),
            0xEA => Some(Self::NoOp),
            0xEC => Some(Self::Compare(CpuRegister::X, AddressingMode::Absolute)),
            0xED => Some(Self::SubtractWithCarry(AddressingMode::Absolute)),
            0xEE => Some(Self::IncrementMemory(AddressingMode::Absolute)),
            0xF0 => Some(Self::Branch(BranchCondition::Equal)),
            0xF1 => Some(Self::SubtractWithCarry(AddressingMode::IndirectY)),
            0xF5 => Some(Self::SubtractWithCarry(AddressingMode::ZeroPageX)),
            0xF6 => Some(Self::IncrementMemory(AddressingMode::ZeroPageX)),
            0xF8 => Some(Self::SetDecimalFlag),
            0xF9 => Some(Self::SubtractWithCarry(AddressingMode::AbsoluteY)),
            0xFD => Some(Self::SubtractWithCarry(AddressingMode::AbsoluteX)),
            0xFE => Some(Self::IncrementMemory(AddressingMode::AbsoluteX)),
            _ => {
                // Unused or undocumented opcode
                None
            }
        }
    }

    pub const fn get_type(self) -> InstructionType {
        match self {
            Self::AddWithCarry(..)
            | Self::And(..)
            | Self::BitTest(..)
            | Self::Compare(..)
            | Self::ExclusiveOr(..)
            | Self::LoadRegister(..)
            | Self::InclusiveOr(..)
            | Self::SubtractWithCarry(..) => InstructionType::Read,
            Self::StoreRegister(..) => InstructionType::Write,
            Self::ShiftLeft(..)
            | Self::DecrementMemory(..)
            | Self::IncrementMemory(..)
            | Self::LogicalShiftRight(..)
            | Self::RotateLeft(..)
            | Self::RotateRight(..) => InstructionType::ReadModifyWrite,
            Self::ClearCarryFlag
            | Self::ClearDecimalFlag
            | Self::ClearInterruptDisable
            | Self::ClearOverflowFlag
            | Self::DecrementRegister(..)
            | Self::IncrementRegister(..)
            | Self::NoOp
            | Self::SetCarryFlag
            | Self::SetDecimalFlag
            | Self::SetInterruptDisable
            | Self::TransferBetweenRegisters { .. } => InstructionType::RegistersOnly,
            Self::Branch(..) => InstructionType::Branch,
            Self::Jump(..) => InstructionType::Jump,
            Self::JumpToSubroutine => InstructionType::JumpToSubroutine,
            Self::PushStack(..) => InstructionType::PushStack,
            Self::PullStack(..) => InstructionType::PullStack,
            Self::ReturnFromInterrupt => InstructionType::ReturnFromInterrupt,
            Self::ReturnFromSubroutine => InstructionType::ReturnFromSubroutine,
            Self::ForceInterrupt => InstructionType::ForceInterrupt,
        }
    }

    pub fn get_addressing_mode(self) -> AddressingMode {
        match self {
            Self::AddWithCarry(addressing_mode)
            | Self::And(addressing_mode)
            | Self::ShiftLeft(addressing_mode)
            | Self::BitTest(addressing_mode)
            | Self::Compare(_, addressing_mode)
            | Self::DecrementMemory(addressing_mode)
            | Self::ExclusiveOr(addressing_mode)
            | Self::IncrementMemory(addressing_mode)
            | Self::Jump(addressing_mode)
            | Self::LoadRegister(_, addressing_mode)
            | Self::LogicalShiftRight(addressing_mode)
            | Self::InclusiveOr(addressing_mode)
            | Self::RotateLeft(addressing_mode)
            | Self::RotateRight(addressing_mode)
            | Self::SubtractWithCarry(addressing_mode)
            | Self::StoreRegister(_, addressing_mode) => addressing_mode,
            Self::Branch(..)
            | Self::ForceInterrupt
            | Self::ClearCarryFlag
            | Self::ClearDecimalFlag
            | Self::ClearInterruptDisable
            | Self::ClearOverflowFlag
            | Self::DecrementRegister(..)
            | Self::IncrementRegister(..)
            | Self::JumpToSubroutine
            | Self::NoOp
            | Self::PushStack(..)
            | Self::PullStack(..)
            | Self::ReturnFromInterrupt
            | Self::ReturnFromSubroutine
            | Self::SetCarryFlag
            | Self::SetDecimalFlag
            | Self::SetInterruptDisable
            | Self::TransferBetweenRegisters { .. } => AddressingMode::Implied,
        }
    }
}

trait InstructionState<StateType = Self> {
    // Return next state, or None if the instruction has completed
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<StateType>;
}

#[derive(Debug)]
pub(crate) struct AccumulatorState(Instruction);

impl InstructionState for AccumulatorState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        registers.accumulator = execute_modify_instruction(
            self.0,
            registers.accumulator,
            &mut registers.status_flags(),
        );

        None
    }
}

#[derive(Debug)]
pub(crate) struct ImmediateState(Instruction);

impl InstructionState for ImmediateState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        let value = bus.read_address(registers.pc);
        registers.pc += 1;

        execute_read_instruction(self.0, value, registers);

        None
    }
}

#[derive(Debug)]
pub(crate) enum ZeroPageReadState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address: u8,
    },
}

impl InstructionState for ZeroPageReadState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                address,
            } => {
                let value = bus.read_address(u16::from(address));

                execute_read_instruction(instruction, value, registers);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ZeroPageWriteState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address: u8,
    },
}

impl InstructionState for ZeroPageWriteState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                address,
            } => {
                execute_write_instruction(instruction, registers, u16::from(address), bus);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ZeroPageModifyState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address: u8,
    },
    Cycle3 {
        instruction: Instruction,
        address: u8,
        value: u8,
    },
    Cycle4 {
        instruction: Instruction,
        address: u8,
        value: u8,
    },
}

impl InstructionState for ZeroPageModifyState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                address,
            } => {
                let value = bus.read_address(u16::from(address));
                Some(Self::Cycle3 {
                    instruction,
                    address,
                    value,
                })
            }
            Self::Cycle3 {
                instruction,
                address,
                value,
            } => {
                bus.write_address(u16::from(address), value);

                Some(Self::Cycle4 {
                    instruction,
                    address,
                    value,
                })
            }
            Self::Cycle4 {
                instruction,
                address,
                value,
            } => {
                let new_value =
                    execute_modify_instruction(instruction, value, &mut registers.status_flags());

                bus.write_address(u16::from(address), new_value);

                None
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexType {
    X,
    Y,
}

#[derive(Debug)]
pub(crate) enum ZeroPageIndexedReadState {
    Cycle1(Instruction, IndexType),
    Cycle2 {
        instruction: Instruction,
        index_type: IndexType,
        address: u8,
    },
    Cycle3 {
        instruction: Instruction,
        index_type: IndexType,
        address: u8,
    },
}

impl InstructionState for ZeroPageIndexedReadState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction, index_type) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    index_type,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                index_type,
                address,
            } => {
                bus.read_address(u16::from(address));

                Some(Self::Cycle3 {
                    instruction,
                    index_type,
                    address,
                })
            }
            Self::Cycle3 {
                instruction,
                index_type,
                address,
            } => {
                let index = match index_type {
                    IndexType::X => registers.x,
                    IndexType::Y => registers.y,
                };
                let indexed_address = u16::from(address.wrapping_add(index));
                let value = bus.read_address(indexed_address);

                execute_read_instruction(instruction, value, registers);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ZeroPageIndexedWriteState {
    Cycle1(Instruction, IndexType),
    Cycle2 {
        instruction: Instruction,
        index_type: IndexType,
        address: u8,
    },
    Cycle3 {
        instruction: Instruction,
        index_type: IndexType,
        address: u8,
    },
}

impl InstructionState for ZeroPageIndexedWriteState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction, index_type) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    index_type,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                index_type,
                address,
            } => {
                bus.read_address(u16::from(address));

                Some(Self::Cycle3 {
                    instruction,
                    index_type,
                    address,
                })
            }
            Self::Cycle3 {
                instruction,
                index_type,
                address,
            } => {
                let index = match index_type {
                    IndexType::X => registers.x,
                    IndexType::Y => registers.y,
                };
                let indexed_address = u16::from(address.wrapping_add(index));

                execute_write_instruction(instruction, registers, indexed_address, bus);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ZeroPageIndexedModifyState {
    Cycle1(Instruction, IndexType),
    Cycle2 {
        instruction: Instruction,
        index_type: IndexType,
        address: u8,
    },
    Cycle3 {
        instruction: Instruction,
        index_type: IndexType,
        address: u8,
    },
    Cycle4 {
        instruction: Instruction,
        indexed_address: u16,
        value: u8,
    },
    Cycle5 {
        instruction: Instruction,
        indexed_address: u16,
        value: u8,
    },
}

impl InstructionState for ZeroPageIndexedModifyState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction, index_type) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    index_type,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                index_type,
                address,
            } => {
                bus.read_address(u16::from(address));

                Some(Self::Cycle3 {
                    instruction,
                    index_type,
                    address,
                })
            }
            Self::Cycle3 {
                instruction,
                index_type,
                address,
            } => {
                let index = match index_type {
                    IndexType::X => registers.x,
                    IndexType::Y => registers.y,
                };
                let indexed_address = u16::from(address.wrapping_add(index));
                let value = bus.read_address(indexed_address);

                Some(Self::Cycle4 {
                    instruction,
                    indexed_address,
                    value,
                })
            }
            Self::Cycle4 {
                instruction,
                indexed_address,
                value,
            } => {
                bus.write_address(indexed_address, value);

                Some(Self::Cycle5 {
                    instruction,
                    indexed_address,
                    value,
                })
            }
            Self::Cycle5 {
                instruction,
                indexed_address,
                value,
            } => {
                let new_value =
                    execute_modify_instruction(instruction, value, &mut registers.status_flags());

                bus.write_address(indexed_address, new_value);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum AbsoluteReadState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address_lsb: u8,
    },
    Cycle3 {
        instruction: Instruction,
        address: u16,
    },
}

impl InstructionState for AbsoluteReadState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address_lsb,
                })
            }
            Self::Cycle2 {
                instruction,
                address_lsb,
            } => {
                let address_msb = bus.read_address(registers.pc);
                registers.pc += 1;

                let address = u16::from_le_bytes([address_lsb, address_msb]);
                Some(Self::Cycle3 {
                    instruction,
                    address,
                })
            }
            Self::Cycle3 {
                instruction,
                address,
            } => {
                let value = bus.read_address(address);

                execute_read_instruction(instruction, value, registers);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum AbsoluteWriteState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address_lsb: u8,
    },
    Cycle3 {
        instruction: Instruction,
        address: u16,
    },
}

impl InstructionState for AbsoluteWriteState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address_lsb,
                })
            }
            Self::Cycle2 {
                instruction,
                address_lsb,
            } => {
                let address_msb = bus.read_address(registers.pc);
                registers.pc += 1;

                let address = u16::from_le_bytes([address_lsb, address_msb]);
                Some(Self::Cycle3 {
                    instruction,
                    address,
                })
            }
            Self::Cycle3 {
                instruction,
                address,
            } => {
                execute_write_instruction(instruction, registers, address, bus);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum AbsoluteModifyState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address_lsb: u8,
    },
    Cycle3 {
        instruction: Instruction,
        address: u16,
    },
    Cycle4 {
        instruction: Instruction,
        address: u16,
        value: u8,
    },
    Cycle5 {
        instruction: Instruction,
        address: u16,
        value: u8,
    },
}

impl InstructionState for AbsoluteModifyState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address_lsb,
                })
            }
            Self::Cycle2 {
                instruction,
                address_lsb,
            } => {
                let address_msb = bus.read_address(registers.pc);
                registers.pc += 1;

                let address = u16::from_le_bytes([address_lsb, address_msb]);
                Some(Self::Cycle3 {
                    instruction,
                    address,
                })
            }
            Self::Cycle3 {
                instruction,
                address,
            } => {
                let value = bus.read_address(address);

                Some(Self::Cycle4 {
                    instruction,
                    address,
                    value,
                })
            }
            Self::Cycle4 {
                instruction,
                address,
                value,
            } => {
                bus.write_address(address, value);

                Some(Self::Cycle5 {
                    instruction,
                    address,
                    value,
                })
            }
            Self::Cycle5 {
                instruction,
                address,
                value,
            } => {
                let new_value =
                    execute_modify_instruction(instruction, value, &mut registers.status_flags());

                bus.write_address(address, new_value);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum AbsoluteIndexedReadState {
    Cycle1(Instruction, IndexType),
    Cycle2 {
        instruction: Instruction,
        index_type: IndexType,
        address_lsb: u8,
    },
    Cycle3 {
        instruction: Instruction,
        indexed_address: u16,
        address_msb: u8,
    },
    Cycle4 {
        instruction: Instruction,
        indexed_address: u16,
    },
}

impl InstructionState for AbsoluteIndexedReadState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction, index_type) => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    index_type,
                    address_lsb,
                })
            }
            Self::Cycle2 {
                instruction,
                index_type,
                address_lsb,
            } => {
                let address_msb = bus.read_address(registers.pc);
                registers.pc += 1;

                let address = u16::from_le_bytes([address_lsb, address_msb]);

                let index = match index_type {
                    IndexType::X => registers.x,
                    IndexType::Y => registers.y,
                };
                let indexed_address = address.wrapping_add(u16::from(index));

                Some(Self::Cycle3 {
                    instruction,
                    indexed_address,
                    address_msb,
                })
            }
            Self::Cycle3 {
                instruction,
                indexed_address,
                address_msb,
            } if address_msb != (indexed_address >> 8) as u8 => {
                bus.read_address((u16::from(address_msb) << 8) | (indexed_address & 0x00FF));

                Some(Self::Cycle4 {
                    instruction,
                    indexed_address,
                })
            }
            Self::Cycle3 {
                instruction,
                indexed_address,
                ..
            }
            | Self::Cycle4 {
                instruction,
                indexed_address,
            } => {
                let value = bus.read_address(indexed_address);

                execute_read_instruction(instruction, value, registers);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum AbsoluteIndexedWriteState {
    Cycle1(Instruction, IndexType),
    Cycle2 {
        instruction: Instruction,
        index_type: IndexType,
        address_lsb: u8,
    },
    Cycle3 {
        instruction: Instruction,
        indexed_address: u16,
    },
    Cycle4 {
        instruction: Instruction,
        indexed_address: u16,
    },
}

impl InstructionState for AbsoluteIndexedWriteState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction, index_type) => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    index_type,
                    address_lsb,
                })
            }
            Self::Cycle2 {
                instruction,
                index_type,
                address_lsb,
            } => {
                let address_msb = bus.read_address(registers.pc);
                registers.pc += 1;

                let address = u16::from_le_bytes([address_lsb, address_msb]);

                let index = match index_type {
                    IndexType::X => registers.x,
                    IndexType::Y => registers.y,
                };
                let indexed_address = address.wrapping_add(u16::from(index));

                Some(Self::Cycle3 {
                    instruction,
                    indexed_address,
                })
            }
            Self::Cycle3 {
                instruction,
                indexed_address,
            } => {
                bus.read_address(indexed_address);

                Some(Self::Cycle4 {
                    instruction,
                    indexed_address,
                })
            }
            Self::Cycle4 {
                instruction,
                indexed_address,
            } => {
                execute_write_instruction(instruction, registers, indexed_address, bus);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum AbsoluteIndexedModifyState {
    Cycle1(Instruction, IndexType),
    Cycle2 {
        instruction: Instruction,
        index_type: IndexType,
        address_lsb: u8,
    },
    Cycle3 {
        instruction: Instruction,
        indexed_address: u16,
        address: u16,
    },
    Cycle4 {
        instruction: Instruction,
        indexed_address: u16,
    },
    Cycle5 {
        instruction: Instruction,
        indexed_address: u16,
        value: u8,
    },
    Cycle6 {
        instruction: Instruction,
        indexed_address: u16,
        value: u8,
    },
}

impl InstructionState for AbsoluteIndexedModifyState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction, index_type) => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    index_type,
                    address_lsb,
                })
            }
            Self::Cycle2 {
                instruction,
                index_type,
                address_lsb,
            } => {
                let address_msb = bus.read_address(registers.pc);
                registers.pc += 1;

                let address = u16::from_le_bytes([address_lsb, address_msb]);

                let index = match index_type {
                    IndexType::X => registers.x,
                    IndexType::Y => registers.y,
                };
                let indexed_address = address.wrapping_add(u16::from(index));

                Some(Self::Cycle3 {
                    instruction,
                    indexed_address,
                    address,
                })
            }
            Self::Cycle3 {
                instruction,
                indexed_address,
                address,
            } => {
                bus.read_address(address);

                Some(Self::Cycle4 {
                    instruction,
                    indexed_address,
                })
            }
            Self::Cycle4 {
                instruction,
                indexed_address,
            } => {
                let value = bus.read_address(indexed_address);
                Some(Self::Cycle5 {
                    instruction,
                    indexed_address,
                    value,
                })
            }
            Self::Cycle5 {
                instruction,
                indexed_address,
                value,
            } => {
                bus.write_address(indexed_address, value);

                Some(Self::Cycle6 {
                    instruction,
                    indexed_address,
                    value,
                })
            }
            Self::Cycle6 {
                instruction,
                indexed_address,
                value,
            } => {
                let new_value =
                    execute_modify_instruction(instruction, value, &mut registers.status_flags());

                bus.write_address(indexed_address, new_value);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum IndexedIndirectState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address: u8,
    },
    Cycle3 {
        instruction: Instruction,
        indexed_address: u8,
    },
    Cycle4 {
        instruction: Instruction,
        indexed_address: u8,
        effective_address_lsb: u8,
    },
    Cycle5 {
        instruction: Instruction,
        effective_address: u16,
    },
}

impl InstructionState for IndexedIndirectState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                address,
            } => {
                bus.read_address(u16::from(address));

                let indexed_address = address.wrapping_add(registers.x);
                Some(Self::Cycle3 {
                    instruction,
                    indexed_address,
                })
            }
            Self::Cycle3 {
                instruction,
                indexed_address,
            } => {
                let effective_address_lsb = bus.read_address(u16::from(indexed_address));
                Some(Self::Cycle4 {
                    instruction,
                    indexed_address,
                    effective_address_lsb,
                })
            }
            Self::Cycle4 {
                instruction,
                indexed_address,
                effective_address_lsb,
            } => {
                let effective_address_msb =
                    bus.read_address(u16::from(indexed_address.wrapping_add(1)));
                let effective_address =
                    u16::from_le_bytes([effective_address_lsb, effective_address_msb]);

                Some(Self::Cycle5 {
                    instruction,
                    effective_address,
                })
            }
            Self::Cycle5 {
                instruction,
                effective_address,
            } => {
                match instruction {
                    Instruction::StoreRegister(CpuRegister::A, ..) => {
                        bus.write_address(effective_address, registers.accumulator);
                    }
                    _ => {
                        let value = bus.read_address(effective_address);
                        execute_read_instruction(instruction, value, registers);
                    }
                }

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum IndirectIndexedState {
    Cycle1(Instruction),
    Cycle2 {
        instruction: Instruction,
        address: u8,
    },
    Cycle3 {
        instruction: Instruction,
        address: u8,
        effective_address_lsb: u8,
    },
    Cycle4 {
        instruction: Instruction,
        effective_address: u16,
        indexed_address: u16,
    },
    Cycle5 {
        instruction: Instruction,
        indexed_address: u16,
    },
}

impl InstructionState for IndirectIndexedState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(instruction) => {
                let address = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    instruction,
                    address,
                })
            }
            Self::Cycle2 {
                instruction,
                address,
            } => {
                let effective_address_lsb = bus.read_address(u16::from(address));
                Some(Self::Cycle3 {
                    instruction,
                    address,
                    effective_address_lsb,
                })
            }
            Self::Cycle3 {
                instruction,
                address,
                effective_address_lsb,
            } => {
                let effective_address_msb = bus.read_address(u16::from(address.wrapping_add(1)));
                let effective_address =
                    u16::from_le_bytes([effective_address_lsb, effective_address_msb]);

                let indexed_address = effective_address.wrapping_add(u16::from(registers.y));

                Some(Self::Cycle4 {
                    instruction,
                    effective_address,
                    indexed_address,
                })
            }
            Self::Cycle4 {
                instruction: Instruction::StoreRegister(register, ..),
                indexed_address,
                effective_address,
            } => {
                bus.read_address(effective_address);

                Some(Self::Cycle5 {
                    instruction: Instruction::StoreRegister(register, AddressingMode::IndirectY),
                    indexed_address,
                })
            }
            Self::Cycle4 {
                instruction,
                effective_address,
                indexed_address,
            } if (effective_address & 0xFF00) != (indexed_address & 0xFF00) => {
                bus.read_address(effective_address);

                Some(Self::Cycle5 {
                    instruction,
                    indexed_address,
                })
            }
            Self::Cycle4 {
                instruction,
                indexed_address,
                ..
            }
            | Self::Cycle5 {
                instruction,
                indexed_address,
            } => {
                match instruction {
                    Instruction::StoreRegister(CpuRegister::A, ..) => {
                        bus.write_address(indexed_address, registers.accumulator);
                    }
                    _ => {
                        let value = bus.read_address(indexed_address);
                        execute_read_instruction(instruction, value, registers);
                    }
                }

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct RegistersOnlyState(Instruction);

impl InstructionState for RegistersOnlyState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self.0 {
            Instruction::ClearCarryFlag => {
                registers.status_flags().set_carry(false);
            }
            Instruction::ClearDecimalFlag => {
                registers.status_flags().set_decimal(false);
            }
            Instruction::ClearInterruptDisable => {
                registers.status_flags().set_interrupt_disable(false);
            }
            Instruction::ClearOverflowFlag => {
                registers.status_flags().set_overflow(false);
            }
            Instruction::DecrementRegister(register) => {
                let value = decrement(
                    read_register(registers, register),
                    &mut registers.status_flags(),
                );
                write_register(registers, register, value);
            }
            Instruction::IncrementRegister(register) => {
                let value = increment(
                    read_register(registers, register),
                    &mut registers.status_flags(),
                );
                write_register(registers, register, value);
            }
            Instruction::NoOp => {}
            Instruction::SetCarryFlag => {
                registers.status_flags().set_carry(true);
            }
            Instruction::SetDecimalFlag => {
                registers.status_flags().set_decimal(true);
            }
            Instruction::SetInterruptDisable => {
                registers.status_flags().set_interrupt_disable(true);
            }
            Instruction::TransferBetweenRegisters { to, from } => {
                let value = read_register(registers, from);
                registers
                    .status_flags()
                    .set_negative(value & 0x80 != 0)
                    .set_zero(value == 0);
                write_register(registers, to, value);
            }
            _ => panic!(
                "instruction is not a registers-only instruction: {:?}",
                self.0
            ),
        }

        None
    }
}

#[derive(Debug)]
pub(crate) enum BranchState {
    Cycle1(BranchCondition),
    Cycle2 { offset: i8 },
    Cycle3,
}

impl InstructionState for BranchState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(condition) => {
                let offset = bus.read_address(registers.pc) as i8;
                registers.pc += 1;

                condition
                    .check(&registers.status_flags())
                    .then_some(Self::Cycle2 { offset })
            }
            Self::Cycle2 { offset } => {
                let new_address = (i32::from(registers.pc) + i32::from(offset)) as u16;
                let next_state = if (new_address & 0xFF00) == (registers.pc & 0xFF00) {
                    None
                } else {
                    Some(Self::Cycle3)
                };

                registers.pc = new_address;
                next_state
            }
            Self::Cycle3 => None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum JumpState {
    Cycle1(AddressingMode),
    Cycle2 {
        addressing_mode: AddressingMode,
        address_lsb: u8,
    },
    Cycle3 {
        address: u16,
    },
    Cycle4 {
        address: u16,
        effective_address_lsb: u8,
    },
}

impl InstructionState for JumpState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(addressing_mode) => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 {
                    addressing_mode,
                    address_lsb,
                })
            }
            Self::Cycle2 {
                addressing_mode,
                address_lsb,
            } => {
                let address_msb = bus.read_address(registers.pc);
                registers.pc += 1;

                let address = u16::from_le_bytes([address_lsb, address_msb]);

                match addressing_mode {
                    AddressingMode::Absolute => {
                        registers.pc = address;
                        None
                    }
                    AddressingMode::Indirect => Some(Self::Cycle3 { address }),
                    _ => panic!("invalid jump addressing mode: {addressing_mode:?}"),
                }
            }
            Self::Cycle3 { address } => {
                let effective_address_lsb = bus.read_address(address);
                Some(Self::Cycle4 {
                    address,
                    effective_address_lsb,
                })
            }
            Self::Cycle4 {
                address,
                effective_address_lsb,
            } => {
                let effective_address_msb = bus.read_address(address.wrapping_add(1));
                let effective_address =
                    u16::from_le_bytes([effective_address_lsb, effective_address_msb]);

                registers.pc = effective_address;
                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum JumpSubroutineState {
    Cycle1,
    Cycle2 { address_lsb: u8 },
    Cycle3 { address_lsb: u8 },
    Cycle4 { address_lsb: u8 },
    Cycle5 { address_lsb: u8 },
}

impl InstructionState for JumpSubroutineState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1 => {
                let address_lsb = bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2 { address_lsb })
            }
            Self::Cycle2 { address_lsb } => Some(Self::Cycle3 { address_lsb }),
            Self::Cycle3 { address_lsb } => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                bus.write_address(stack_address, (registers.pc >> 8) as u8);
                registers.sp = registers.sp.wrapping_sub(1);

                Some(Self::Cycle4 { address_lsb })
            }
            Self::Cycle4 { address_lsb } => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                bus.write_address(stack_address, (registers.pc & 0x00FF) as u8);
                registers.sp = registers.sp.wrapping_sub(1);

                Some(Self::Cycle5 { address_lsb })
            }
            Self::Cycle5 { address_lsb } => {
                let address_msb = bus.read_address(registers.pc);
                let address = u16::from_le_bytes([address_lsb, address_msb]);

                registers.pc = address;
                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ReturnSubroutineState {
    Cycle1,
    Cycle2,
    Cycle3,
    Cycle4 { pc_lsb: u8 },
    Cycle5,
}

impl InstructionState for ReturnSubroutineState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1 => {
                bus.read_address(registers.pc.wrapping_add(1));

                Some(Self::Cycle2)
            }
            Self::Cycle2 => {
                registers.sp = registers.sp.wrapping_add(1);
                Some(Self::Cycle3)
            }
            Self::Cycle3 => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                let pc_lsb = bus.read_address(stack_address);
                registers.sp = registers.sp.wrapping_add(1);

                Some(Self::Cycle4 { pc_lsb })
            }
            Self::Cycle4 { pc_lsb } => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                let pc_msb = bus.read_address(stack_address);
                registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

                Some(Self::Cycle5)
            }
            Self::Cycle5 => {
                registers.pc += 1;
                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ReturnInterruptState {
    Cycle1,
    Cycle2,
    Cycle3,
    Cycle4,
    Cycle5 { pc_lsb: u8 },
}

impl InstructionState for ReturnInterruptState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1 => {
                bus.read_address(registers.pc.wrapping_add(1));

                Some(Self::Cycle2)
            }
            Self::Cycle2 => {
                registers.sp = registers.sp.wrapping_add(1);
                Some(Self::Cycle3)
            }
            Self::Cycle3 => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                registers.status = bus.read_address(stack_address) | 0x30;
                registers.sp = registers.sp.wrapping_add(1);

                Some(Self::Cycle4)
            }
            Self::Cycle4 => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                let pc_lsb = bus.read_address(stack_address);
                registers.sp = registers.sp.wrapping_add(1);

                Some(Self::Cycle5 { pc_lsb })
            }
            Self::Cycle5 { pc_lsb } => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                let pc_msb = bus.read_address(stack_address);

                registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);
                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum PushStackState {
    Cycle1(CpuRegister),
    Cycle2(CpuRegister),
}

impl InstructionState for PushStackState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(register) => {
                bus.read_address(registers.pc.wrapping_add(1));

                Some(Self::Cycle2(register))
            }
            Self::Cycle2(register) => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                let value = match register {
                    CpuRegister::P => read_register(registers, register) | 0x30,
                    _ => read_register(registers, register),
                };
                bus.write_address(stack_address, value);
                registers.sp = registers.sp.wrapping_sub(1);

                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum PullStackState {
    Cycle1(CpuRegister),
    Cycle2(CpuRegister),
    Cycle3(CpuRegister),
}

impl InstructionState for PullStackState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1(register) => {
                bus.read_address(registers.pc.wrapping_add(1));

                Some(Self::Cycle2(register))
            }
            Self::Cycle2(register) => {
                registers.sp = registers.sp.wrapping_add(1);
                Some(Self::Cycle3(register))
            }
            Self::Cycle3(register) => {
                let value = bus.read_address(bus::CPU_STACK_START | u16::from(registers.sp));

                let value = match register {
                    CpuRegister::P => value | 0x30,
                    _ => value,
                };

                write_register(registers, register, value);
                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ForceInterruptState {
    Cycle1,
    Cycle2,
    Cycle3,
    Cycle4 { interrupt_vector: u16 },
    Cycle5 { interrupt_vector: u16 },
    Cycle6 { interrupt_vector: u16, pc_lsb: u8 },
}

impl InstructionState for ForceInterruptState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle1 => {
                bus.read_address(registers.pc);
                registers.pc += 1;

                Some(Self::Cycle2)
            }
            Self::Cycle2 => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                bus.write_address(stack_address, (registers.pc >> 8) as u8);
                registers.sp = registers.sp.wrapping_sub(1);

                Some(Self::Cycle3)
            }
            Self::Cycle3 => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                bus.write_address(stack_address, (registers.pc & 0x00FF) as u8);
                registers.sp = registers.sp.wrapping_sub(1);

                // NMI interrupts can hijack IRQ/BRK interrupts
                let interrupt_vector = if bus.interrupt_lines().nmi_triggered() {
                    bus.interrupt_lines().clear_nmi_triggered();
                    bus::CPU_NMI_VECTOR
                } else {
                    bus::CPU_IRQ_VECTOR
                };

                Some(Self::Cycle4 { interrupt_vector })
            }
            Self::Cycle4 { interrupt_vector } => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                bus.write_address(stack_address, registers.status | 0x30);
                registers.sp = registers.sp.wrapping_sub(1);

                Some(Self::Cycle5 { interrupt_vector })
            }
            Self::Cycle5 { interrupt_vector } => {
                let pc_lsb = bus.read_address(interrupt_vector);

                registers.status_flags().set_interrupt_disable(true);

                Some(Self::Cycle6 {
                    interrupt_vector,
                    pc_lsb,
                })
            }
            Self::Cycle6 {
                interrupt_vector,
                pc_lsb,
            } => {
                let pc_msb = bus.read_address(interrupt_vector + 1);
                registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

                None
            }
        }
    }
}

fn read_register(registers: &CpuRegisters, register: CpuRegister) -> u8 {
    match register {
        CpuRegister::A => registers.accumulator,
        CpuRegister::X => registers.x,
        CpuRegister::Y => registers.y,
        CpuRegister::P => registers.status,
        CpuRegister::S => registers.sp,
    }
}

fn write_register(registers: &mut CpuRegisters, register: CpuRegister, value: u8) {
    match register {
        CpuRegister::A => {
            registers.accumulator = value;
        }
        CpuRegister::X => {
            registers.x = value;
        }
        CpuRegister::Y => {
            registers.y = value;
        }
        CpuRegister::P => {
            registers.status = value;
        }
        CpuRegister::S => {
            registers.sp = value;
        }
    }
}

fn execute_read_instruction(instruction: Instruction, value: u8, registers: &mut CpuRegisters) {
    match instruction {
        Instruction::AddWithCarry(..) => {
            registers.accumulator =
                add(registers.accumulator, value, &mut registers.status_flags());
        }
        Instruction::And(..) => {
            registers.accumulator =
                and(registers.accumulator, value, &mut registers.status_flags());
        }
        Instruction::BitTest(..) => {
            bit_test(registers.accumulator, value, &mut registers.status_flags());
        }
        Instruction::Compare(register, ..) => {
            compare(
                read_register(registers, register),
                value,
                &mut registers.status_flags(),
            );
        }
        Instruction::ExclusiveOr(..) => {
            registers.accumulator =
                xor(registers.accumulator, value, &mut registers.status_flags());
        }
        Instruction::LoadRegister(register, ..) => {
            write_register(registers, register, value);
        }
        Instruction::InclusiveOr(..) => {
            registers.accumulator = or(registers.accumulator, value, &mut registers.status_flags());
        }
        Instruction::SubtractWithCarry(..) => {
            registers.accumulator =
                subtract(registers.accumulator, value, &mut registers.status_flags());
        }
        _ => panic!("instruction is not a read instruction: {instruction:?}"),
    }
}

fn execute_write_instruction(
    instruction: Instruction,
    registers: &CpuRegisters,
    address: u16,
    bus: &mut CpuBus<'_>,
) {
    let value = match instruction {
        Instruction::StoreRegister(register, ..) => read_register(registers, register),
        _ => panic!("instruction is not a write instruction: {instruction:?}"),
    };

    bus.write_address(address, value);
}

fn execute_modify_instruction(
    instruction: Instruction,
    value: u8,
    status_flags: &mut StatusFlags<'_>,
) -> u8 {
    match instruction {
        Instruction::ShiftLeft(..) => shift_left(value, status_flags),
        Instruction::DecrementMemory(..) => decrement(value, status_flags),
        Instruction::IncrementMemory(..) => increment(value, status_flags),
        Instruction::LogicalShiftRight(..) => shift_right(value, status_flags),
        Instruction::RotateLeft(..) => rotate_left(value, status_flags),
        Instruction::RotateRight(..) => rotate_right(value, status_flags),
        _ => panic!("instruction is not a read-modify-write instruction: {instruction:?}"),
    }
}

fn add(accumulator: u8, value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let existing_carry = flags.carry();

    let (result, new_carry) = match accumulator.overflowing_add(value) {
        (sum, true) => (sum + u8::from(existing_carry), true),
        (sum, false) => sum.overflowing_add(u8::from(existing_carry)),
    };

    let (_, overflow) = match (accumulator as i8).overflowing_add(value as i8) {
        (sum, true) => (sum, true),
        (sum, false) => sum.overflowing_add(i8::from(existing_carry)),
    };

    flags
        .set_negative(result & 0x80 != 0)
        .set_overflow(overflow)
        .set_zero(result == 0)
        .set_carry(new_carry);

    result
}

fn subtract(accumulator: u8, value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    // Carry flag is inverted in subtraction
    let existing_carry = u8::from(!flags.carry());

    let (result, borrowed) = match accumulator.overflowing_sub(value) {
        (difference, true) => (difference - existing_carry, true),
        (difference, false) => difference.overflowing_sub(existing_carry),
    };

    let (_, overflow) = match (accumulator as i8).overflowing_sub(value as i8) {
        (difference, true) => (difference, true),
        (difference, false) => difference.overflowing_sub(existing_carry as i8),
    };

    flags
        .set_negative(result & 0x80 != 0)
        .set_overflow(overflow)
        .set_zero(result == 0)
        .set_carry(!borrowed);

    result
}

fn and(accumulator: u8, value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let result = accumulator & value;
    flags.set_negative(result & 0x80 != 0).set_zero(result == 0);
    result
}

fn or(accumulator: u8, value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let result = accumulator | value;
    flags.set_negative(result & 0x80 != 0).set_zero(result == 0);
    result
}

fn xor(accumulator: u8, value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let result = accumulator ^ value;
    flags.set_negative(result & 0x80 != 0).set_zero(result == 0);
    result
}

fn compare(register: u8, value: u8, flags: &mut StatusFlags<'_>) {
    flags
        .set_negative(register.wrapping_sub(value) & 0x80 != 0)
        .set_zero(register == value)
        .set_carry(register >= value);
}

fn bit_test(accumulator: u8, value: u8, flags: &mut StatusFlags<'_>) {
    flags
        .set_negative(value & 0x80 != 0)
        .set_overflow(value & 0x40 != 0)
        .set_zero(accumulator & value == 0);
}

fn decrement(value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let result = value.wrapping_sub(1);
    flags.set_negative(result & 0x80 != 0).set_zero(result == 0);
    result
}

fn increment(value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let result = value.wrapping_add(1);
    flags.set_negative(result & 0x80 != 0).set_zero(result == 0);
    result
}

fn shift_left(value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let shifted = value << 1;
    flags
        .set_negative(shifted & 0x80 != 0)
        .set_zero(shifted == 0)
        .set_carry(value & 0x80 != 0);
    shifted
}

fn shift_right(value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let shifted = value >> 1;
    flags
        .set_negative(false)
        .set_zero(shifted == 0)
        .set_carry(value & 0x01 != 0);
    shifted
}

fn rotate_left(value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let rotated = (value << 1) | u8::from(flags.carry());
    flags
        .set_negative(rotated & 0x80 != 0)
        .set_zero(rotated == 0)
        .set_carry(value & 0x80 != 0);
    rotated
}

fn rotate_right(value: u8, flags: &mut StatusFlags<'_>) -> u8 {
    let rotated = (value >> 1) | (u8::from(flags.carry()) << 7);
    flags
        .set_negative(rotated & 0x80 != 0)
        .set_zero(rotated == 0)
        .set_carry(value & 0x01 != 0);
    rotated
}

#[derive(Debug)]
pub(crate) enum ExecutingInstruction {
    Accumulator(AccumulatorState),
    Immediate(ImmediateState),
    ZeroPageRead(ZeroPageReadState),
    ZeroPageWrite(ZeroPageWriteState),
    ZeroPageModify(ZeroPageModifyState),
    ZeroPageIndexedRead(ZeroPageIndexedReadState),
    ZeroPageIndexedWrite(ZeroPageIndexedWriteState),
    ZeroPageIndexedModify(ZeroPageIndexedModifyState),
    AbsoluteRead(AbsoluteReadState),
    AbsoluteWrite(AbsoluteWriteState),
    AbsoluteModify(AbsoluteModifyState),
    AbsoluteIndexedRead(AbsoluteIndexedReadState),
    AbsoluteIndexedWrite(AbsoluteIndexedWriteState),
    AbsoluteIndexedModify(AbsoluteIndexedModifyState),
    IndirectIndexed(IndirectIndexedState),
    IndexedIndirect(IndexedIndirectState),
    RegistersOnly(RegistersOnlyState),
    Branch(BranchState),
    Jump(JumpState),
    JumpSubroutine(JumpSubroutineState),
    ReturnSubroutine(ReturnSubroutineState),
    ReturnInterrupt(ReturnInterruptState),
    PushStack(PushStackState),
    PullStack(PullStackState),
    ForceInterrupt(ForceInterruptState),
}

macro_rules! executing_instruction_next {
    (($executing_instruction:expr, $registers:expr, $bus:expr), $($variant:ident,)+$(,)?) => {
        match $executing_instruction {
            $(
                Self::$variant(state) => state.next($registers, $bus).map(Self::$variant),
            )*
        }
    }
}

impl ExecutingInstruction {
    pub fn fetch(registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Self {
        let opcode = bus.read_address(registers.pc);
        registers.pc += 1;

        let Some(instruction) = Instruction::from_opcode(opcode)
        else {
            panic!("unsupported opcode: {opcode:02X}");
        };

        match (instruction.get_addressing_mode(), instruction.get_type()) {
            (AddressingMode::Accumulator, _) => Self::Accumulator(AccumulatorState(instruction)),
            (AddressingMode::Immediate, _) => Self::Immediate(ImmediateState(instruction)),
            (AddressingMode::ZeroPage, InstructionType::Read) => {
                Self::ZeroPageRead(ZeroPageReadState::Cycle1(instruction))
            }
            (AddressingMode::ZeroPage, InstructionType::Write) => {
                Self::ZeroPageWrite(ZeroPageWriteState::Cycle1(instruction))
            }
            (AddressingMode::ZeroPage, InstructionType::ReadModifyWrite) => {
                Self::ZeroPageModify(ZeroPageModifyState::Cycle1(instruction))
            }
            (AddressingMode::ZeroPageX, InstructionType::Read) => Self::ZeroPageIndexedRead(
                ZeroPageIndexedReadState::Cycle1(instruction, IndexType::X),
            ),
            (AddressingMode::ZeroPageY, InstructionType::Read) => Self::ZeroPageIndexedRead(
                ZeroPageIndexedReadState::Cycle1(instruction, IndexType::Y),
            ),
            (AddressingMode::ZeroPageX, InstructionType::Write) => Self::ZeroPageIndexedWrite(
                ZeroPageIndexedWriteState::Cycle1(instruction, IndexType::X),
            ),
            (AddressingMode::ZeroPageY, InstructionType::Write) => Self::ZeroPageIndexedWrite(
                ZeroPageIndexedWriteState::Cycle1(instruction, IndexType::Y),
            ),
            (AddressingMode::ZeroPageX, InstructionType::ReadModifyWrite) => {
                Self::ZeroPageIndexedModify(ZeroPageIndexedModifyState::Cycle1(
                    instruction,
                    IndexType::X,
                ))
            }
            (AddressingMode::ZeroPageY, InstructionType::ReadModifyWrite) => {
                Self::ZeroPageIndexedModify(ZeroPageIndexedModifyState::Cycle1(
                    instruction,
                    IndexType::Y,
                ))
            }
            (AddressingMode::Absolute, InstructionType::Read) => {
                Self::AbsoluteRead(AbsoluteReadState::Cycle1(instruction))
            }
            (AddressingMode::Absolute, InstructionType::Write) => {
                Self::AbsoluteWrite(AbsoluteWriteState::Cycle1(instruction))
            }
            (AddressingMode::Absolute, InstructionType::ReadModifyWrite) => {
                Self::AbsoluteModify(AbsoluteModifyState::Cycle1(instruction))
            }
            (AddressingMode::AbsoluteX, InstructionType::Read) => Self::AbsoluteIndexedRead(
                AbsoluteIndexedReadState::Cycle1(instruction, IndexType::X),
            ),
            (AddressingMode::AbsoluteY, InstructionType::Read) => Self::AbsoluteIndexedRead(
                AbsoluteIndexedReadState::Cycle1(instruction, IndexType::Y),
            ),
            (AddressingMode::AbsoluteX, InstructionType::Write) => Self::AbsoluteIndexedWrite(
                AbsoluteIndexedWriteState::Cycle1(instruction, IndexType::X),
            ),
            (AddressingMode::AbsoluteY, InstructionType::Write) => Self::AbsoluteIndexedWrite(
                AbsoluteIndexedWriteState::Cycle1(instruction, IndexType::Y),
            ),
            (AddressingMode::AbsoluteX, InstructionType::ReadModifyWrite) => {
                Self::AbsoluteIndexedModify(AbsoluteIndexedModifyState::Cycle1(
                    instruction,
                    IndexType::X,
                ))
            }
            (AddressingMode::AbsoluteY, InstructionType::ReadModifyWrite) => {
                Self::AbsoluteIndexedModify(AbsoluteIndexedModifyState::Cycle1(
                    instruction,
                    IndexType::Y,
                ))
            }
            (AddressingMode::IndirectX, _) => {
                Self::IndexedIndirect(IndexedIndirectState::Cycle1(instruction))
            }
            (AddressingMode::IndirectY, _) => {
                Self::IndirectIndexed(IndirectIndexedState::Cycle1(instruction))
            }
            (_, InstructionType::RegistersOnly) => {
                Self::RegistersOnly(RegistersOnlyState(instruction))
            }
            (_, InstructionType::Branch) => {
                let Instruction::Branch(condition) = instruction
                else {
                    panic!("instruction has type branch but is not a branch instruction: {instruction:?}");
                };
                Self::Branch(BranchState::Cycle1(condition))
            }
            (addressing_mode, InstructionType::Jump) => {
                Self::Jump(JumpState::Cycle1(addressing_mode))
            }
            (_, InstructionType::JumpToSubroutine) => {
                Self::JumpSubroutine(JumpSubroutineState::Cycle1)
            }
            (_, InstructionType::ReturnFromSubroutine) => {
                Self::ReturnSubroutine(ReturnSubroutineState::Cycle1)
            }
            (_, InstructionType::ReturnFromInterrupt) => {
                Self::ReturnInterrupt(ReturnInterruptState::Cycle1)
            }
            (_, InstructionType::PushStack) => {
                let Instruction::PushStack(register) = instruction
                else {
                    panic!("instruction has type PushStack but is not a PushStack instruction: {instruction:?}");
                };
                Self::PushStack(PushStackState::Cycle1(register))
            }
            (_, InstructionType::PullStack) => {
                let Instruction::PullStack(register) = instruction
                else {
                    panic!("instruction has type PullStack but is not a PullStack instruction: {instruction:?}");
                };
                Self::PullStack(PullStackState::Cycle1(register))
            }
            (_, InstructionType::ForceInterrupt) => {
                Self::ForceInterrupt(ForceInterruptState::Cycle1)
            }
            _ => panic!(
                "invalid addressing mode / instruction type combination: mode={:?}, type={:?}",
                instruction.get_addressing_mode(),
                instruction.get_type()
            ),
        }
    }

    pub fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        executing_instruction_next!(
            (self, registers, bus),
            Accumulator,
            Immediate,
            ZeroPageRead,
            ZeroPageWrite,
            ZeroPageModify,
            ZeroPageIndexedRead,
            ZeroPageIndexedWrite,
            ZeroPageIndexedModify,
            AbsoluteRead,
            AbsoluteWrite,
            AbsoluteModify,
            AbsoluteIndexedRead,
            AbsoluteIndexedWrite,
            AbsoluteIndexedModify,
            IndirectIndexed,
            IndexedIndirect,
            RegistersOnly,
            Branch,
            Jump,
            JumpSubroutine,
            ReturnSubroutine,
            ReturnInterrupt,
            PushStack,
            PullStack,
            ForceInterrupt,
        )
    }

    pub fn is_branch(&self) -> bool {
        matches!(self, Self::Branch(..))
    }
}
