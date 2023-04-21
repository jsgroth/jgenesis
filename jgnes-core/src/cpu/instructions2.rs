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
pub enum CpuRegister {
    A,
    X,
    Y,
    S,
    P,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadInstruction {
    // ADC
    AddWithCarry(AddressingMode),
    // AND
    And(AddressingMode),
    // BIT
    BitTest(AddressingMode),
    // CMP / CPX / CPY
    Compare(CpuRegister, AddressingMode),
    // EOR
    ExclusiveOr(AddressingMode),
    // LDA / LDX / LDY
    LoadRegister(CpuRegister, AddressingMode),
    // ORA
    InclusiveOr(AddressingMode),
    // SBC
    SubtractWithCarry(AddressingMode),
}

impl ReadInstruction {
    fn addressing_mode(self) -> AddressingMode {
        match self {
            Self::AddWithCarry(addressing_mode)
            | Self::And(addressing_mode)
            | Self::BitTest(addressing_mode)
            | Self::Compare(_, addressing_mode)
            | Self::ExclusiveOr(addressing_mode)
            | Self::LoadRegister(_, addressing_mode)
            | Self::InclusiveOr(addressing_mode)
            | Self::SubtractWithCarry(addressing_mode) => addressing_mode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModifyInstruction {
    // ASL
    ShiftLeft(AddressingMode),
    // DEC
    DecrementMemory(AddressingMode),
    // INC
    IncrementMemory(AddressingMode),
    // LSR
    LogicalShiftRight(AddressingMode),
    // ROL
    RotateLeft(AddressingMode),
    // ROR
    RotateRight(AddressingMode),
}

impl ModifyInstruction {
    fn addressing_mode(self) -> AddressingMode {
        match self {
            Self::ShiftLeft(addressing_mode)
            | Self::DecrementMemory(addressing_mode)
            | Self::IncrementMemory(addressing_mode)
            | Self::LogicalShiftRight(addressing_mode)
            | Self::RotateLeft(addressing_mode)
            | Self::RotateRight(addressing_mode) => addressing_mode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistersInstruction {
    // CLC
    ClearCarryFlag,
    // CLD
    ClearDecimalFlag,
    // CLI
    ClearInterruptDisable,
    // CLV
    ClearOverflowFlag,
    // DEX / DEY
    DecrementRegister(CpuRegister),
    // INX / INY
    IncrementRegister(CpuRegister),
    // NOP
    NoOp,
    // SEC
    SetCarryFlag,
    // SED
    SetDecimalFlag,
    // SEI
    SetInterruptDisable,
    // TAX / TAY / TSX / TXA / TXS / TYA
    TransferBetweenRegisters { to: CpuRegister, from: CpuRegister },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Instruction {
    Read(ReadInstruction),
    ReadModifyWrite(ModifyInstruction),
    RegistersOnly(RegistersInstruction),
    Branch(BranchCondition),
    // STA / STX / STY
    StoreRegister(CpuRegister, AddressingMode),
    // BRK
    ForceInterrupt,
    // JMP
    Jump(AddressingMode),
    // JSR
    JumpToSubroutine,
    // PHA / PHP
    PushStack(CpuRegister),
    // PLA / PLP
    PullStack(CpuRegister),
    // RTI
    ReturnFromInterrupt,
    // RTS
    ReturnFromSubroutine,
}

impl Instruction {
    fn from_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            0x00 => Some(Self::ForceInterrupt),
            0x01 => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::IndirectX,
            ))),
            0x05 => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::ZeroPage,
            ))),
            0x06 => Some(Self::ReadModifyWrite(ModifyInstruction::ShiftLeft(
                AddressingMode::ZeroPage,
            ))),
            0x08 => Some(Self::PushStack(CpuRegister::P)),
            0x09 => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::Immediate,
            ))),
            0x0A => Some(Self::ReadModifyWrite(ModifyInstruction::ShiftLeft(
                AddressingMode::Accumulator,
            ))),
            0x0D => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::Absolute,
            ))),
            0x0E => Some(Self::ReadModifyWrite(ModifyInstruction::ShiftLeft(
                AddressingMode::Absolute,
            ))),
            0x10 => Some(Self::Branch(BranchCondition::Positive)),
            0x11 => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::IndirectY,
            ))),
            0x15 => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::ZeroPageX,
            ))),
            0x16 => Some(Self::ReadModifyWrite(ModifyInstruction::ShiftLeft(
                AddressingMode::ZeroPageX,
            ))),
            0x18 => Some(Self::RegistersOnly(RegistersInstruction::ClearCarryFlag)),
            0x19 => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::AbsoluteY,
            ))),
            0x1D => Some(Self::Read(ReadInstruction::InclusiveOr(
                AddressingMode::AbsoluteX,
            ))),
            0x1E => Some(Self::ReadModifyWrite(ModifyInstruction::ShiftLeft(
                AddressingMode::AbsoluteX,
            ))),
            0x20 => Some(Self::JumpToSubroutine),
            0x21 => Some(Self::Read(ReadInstruction::And(AddressingMode::IndirectX))),
            0x24 => Some(Self::Read(ReadInstruction::BitTest(
                AddressingMode::ZeroPage,
            ))),
            0x25 => Some(Self::Read(ReadInstruction::And(AddressingMode::ZeroPage))),
            0x26 => Some(Self::ReadModifyWrite(ModifyInstruction::RotateLeft(
                AddressingMode::ZeroPage,
            ))),
            0x28 => Some(Self::PullStack(CpuRegister::P)),
            0x29 => Some(Self::Read(ReadInstruction::And(AddressingMode::Immediate))),
            0x2A => Some(Self::ReadModifyWrite(ModifyInstruction::RotateLeft(
                AddressingMode::Accumulator,
            ))),
            0x2C => Some(Self::Read(ReadInstruction::BitTest(
                AddressingMode::Absolute,
            ))),
            0x2D => Some(Self::Read(ReadInstruction::And(AddressingMode::Absolute))),
            0x2E => Some(Self::ReadModifyWrite(ModifyInstruction::RotateLeft(
                AddressingMode::Absolute,
            ))),
            0x30 => Some(Self::Branch(BranchCondition::Minus)),
            0x31 => Some(Self::Read(ReadInstruction::And(AddressingMode::IndirectY))),
            0x35 => Some(Self::Read(ReadInstruction::And(AddressingMode::ZeroPageX))),
            0x36 => Some(Self::ReadModifyWrite(ModifyInstruction::RotateLeft(
                AddressingMode::ZeroPageX,
            ))),
            0x38 => Some(Self::RegistersOnly(RegistersInstruction::SetCarryFlag)),
            0x39 => Some(Self::Read(ReadInstruction::And(AddressingMode::AbsoluteY))),
            0x3D => Some(Self::Read(ReadInstruction::And(AddressingMode::AbsoluteX))),
            0x3E => Some(Self::ReadModifyWrite(ModifyInstruction::RotateLeft(
                AddressingMode::AbsoluteX,
            ))),
            0x40 => Some(Self::ReturnFromInterrupt),
            0x41 => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::IndirectX,
            ))),
            0x45 => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::ZeroPage,
            ))),
            0x46 => Some(Self::ReadModifyWrite(ModifyInstruction::LogicalShiftRight(
                AddressingMode::ZeroPage,
            ))),
            0x48 => Some(Self::PushStack(CpuRegister::A)),
            0x49 => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::Immediate,
            ))),
            0x4A => Some(Self::ReadModifyWrite(ModifyInstruction::LogicalShiftRight(
                AddressingMode::Accumulator,
            ))),
            0x4C => Some(Self::Jump(AddressingMode::Absolute)),
            0x4D => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::Absolute,
            ))),
            0x4E => Some(Self::ReadModifyWrite(ModifyInstruction::LogicalShiftRight(
                AddressingMode::Absolute,
            ))),
            0x50 => Some(Self::Branch(BranchCondition::OverflowClear)),
            0x51 => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::IndirectY,
            ))),
            0x55 => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::ZeroPageX,
            ))),
            0x56 => Some(Self::ReadModifyWrite(ModifyInstruction::LogicalShiftRight(
                AddressingMode::ZeroPageX,
            ))),
            0x58 => Some(Self::RegistersOnly(
                RegistersInstruction::ClearInterruptDisable,
            )),
            0x59 => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::AbsoluteY,
            ))),
            0x5D => Some(Self::Read(ReadInstruction::ExclusiveOr(
                AddressingMode::AbsoluteX,
            ))),
            0x5E => Some(Self::ReadModifyWrite(ModifyInstruction::LogicalShiftRight(
                AddressingMode::AbsoluteX,
            ))),
            0x60 => Some(Self::ReturnFromSubroutine),
            0x61 => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::IndirectX,
            ))),
            0x65 => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::ZeroPage,
            ))),
            0x66 => Some(Self::ReadModifyWrite(ModifyInstruction::RotateRight(
                AddressingMode::ZeroPage,
            ))),
            0x68 => Some(Self::PullStack(CpuRegister::A)),
            0x69 => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::Immediate,
            ))),
            0x6A => Some(Self::ReadModifyWrite(ModifyInstruction::RotateRight(
                AddressingMode::Accumulator,
            ))),
            0x6C => Some(Self::Jump(AddressingMode::Indirect)),
            0x6D => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::Absolute,
            ))),
            0x6E => Some(Self::ReadModifyWrite(ModifyInstruction::RotateRight(
                AddressingMode::Absolute,
            ))),
            0x70 => Some(Self::Branch(BranchCondition::OverflowSet)),
            0x71 => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::IndirectY,
            ))),
            0x75 => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::ZeroPageX,
            ))),
            0x76 => Some(Self::ReadModifyWrite(ModifyInstruction::RotateRight(
                AddressingMode::ZeroPageX,
            ))),
            0x78 => Some(Self::RegistersOnly(
                RegistersInstruction::SetInterruptDisable,
            )),
            0x79 => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::AbsoluteY,
            ))),
            0x7D => Some(Self::Read(ReadInstruction::AddWithCarry(
                AddressingMode::AbsoluteX,
            ))),
            0x7E => Some(Self::ReadModifyWrite(ModifyInstruction::RotateRight(
                AddressingMode::AbsoluteX,
            ))),
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
            0x88 => Some(Self::RegistersOnly(
                RegistersInstruction::DecrementRegister(CpuRegister::Y),
            )),
            0x8A => Some(Self::RegistersOnly(
                RegistersInstruction::TransferBetweenRegisters {
                    to: CpuRegister::A,
                    from: CpuRegister::X,
                },
            )),
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
            0x98 => Some(Self::RegistersOnly(
                RegistersInstruction::TransferBetweenRegisters {
                    to: CpuRegister::A,
                    from: CpuRegister::Y,
                },
            )),
            0x99 => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteY,
            )),
            0x9A => Some(Self::RegistersOnly(
                RegistersInstruction::TransferBetweenRegisters {
                    to: CpuRegister::S,
                    from: CpuRegister::X,
                },
            )),
            0x9D => Some(Self::StoreRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteX,
            )),
            0xA0 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::Y,
                AddressingMode::Immediate,
            ))),
            0xA1 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::IndirectX,
            ))),
            0xA2 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::X,
                AddressingMode::Immediate,
            ))),
            0xA4 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::Y,
                AddressingMode::ZeroPage,
            ))),
            0xA5 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::ZeroPage,
            ))),
            0xA6 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::X,
                AddressingMode::ZeroPage,
            ))),
            0xA8 => Some(Self::RegistersOnly(
                RegistersInstruction::TransferBetweenRegisters {
                    to: CpuRegister::Y,
                    from: CpuRegister::A,
                },
            )),
            0xA9 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::Immediate,
            ))),
            0xAA => Some(Self::RegistersOnly(
                RegistersInstruction::TransferBetweenRegisters {
                    to: CpuRegister::X,
                    from: CpuRegister::A,
                },
            )),
            0xAC => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::Y,
                AddressingMode::Absolute,
            ))),
            0xAD => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::Absolute,
            ))),
            0xAE => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::X,
                AddressingMode::Absolute,
            ))),
            0xB0 => Some(Self::Branch(BranchCondition::CarrySet)),
            0xB1 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::IndirectY,
            ))),
            0xB4 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::Y,
                AddressingMode::ZeroPageX,
            ))),
            0xB5 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::ZeroPageX,
            ))),
            0xB6 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::X,
                AddressingMode::ZeroPageY,
            ))),
            0xB8 => Some(Self::RegistersOnly(RegistersInstruction::ClearOverflowFlag)),
            0xB9 => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteY,
            ))),
            0xBA => Some(Self::RegistersOnly(
                RegistersInstruction::TransferBetweenRegisters {
                    to: CpuRegister::X,
                    from: CpuRegister::S,
                },
            )),
            0xBC => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::Y,
                AddressingMode::AbsoluteX,
            ))),
            0xBD => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::A,
                AddressingMode::AbsoluteX,
            ))),
            0xBE => Some(Self::Read(ReadInstruction::LoadRegister(
                CpuRegister::X,
                AddressingMode::AbsoluteY,
            ))),
            0xC0 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::Y,
                AddressingMode::Immediate,
            ))),
            0xC1 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::IndirectX,
            ))),
            0xC4 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::Y,
                AddressingMode::ZeroPage,
            ))),
            0xC5 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::ZeroPage,
            ))),
            0xC6 => Some(Self::ReadModifyWrite(ModifyInstruction::DecrementMemory(
                AddressingMode::ZeroPage,
            ))),
            0xC8 => Some(Self::RegistersOnly(
                RegistersInstruction::IncrementRegister(CpuRegister::Y),
            )),
            0xC9 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::Immediate,
            ))),
            0xCA => Some(Self::RegistersOnly(
                RegistersInstruction::DecrementRegister(CpuRegister::X),
            )),
            0xCC => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::Y,
                AddressingMode::Absolute,
            ))),
            0xCD => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::Absolute,
            ))),
            0xCE => Some(Self::ReadModifyWrite(ModifyInstruction::DecrementMemory(
                AddressingMode::Absolute,
            ))),
            0xD0 => Some(Self::Branch(BranchCondition::NotEqual)),
            0xD1 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::IndirectY,
            ))),
            0xD5 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::ZeroPageX,
            ))),
            0xD6 => Some(Self::ReadModifyWrite(ModifyInstruction::DecrementMemory(
                AddressingMode::ZeroPageX,
            ))),
            0xD8 => Some(Self::RegistersOnly(RegistersInstruction::ClearDecimalFlag)),
            0xD9 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::AbsoluteY,
            ))),
            0xDD => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::A,
                AddressingMode::AbsoluteX,
            ))),
            0xDE => Some(Self::ReadModifyWrite(ModifyInstruction::DecrementMemory(
                AddressingMode::AbsoluteX,
            ))),
            0xE0 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::X,
                AddressingMode::Immediate,
            ))),
            0xE1 => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::IndirectX,
            ))),
            0xE4 => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::X,
                AddressingMode::ZeroPage,
            ))),
            0xE5 => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::ZeroPage,
            ))),
            0xE6 => Some(Self::ReadModifyWrite(ModifyInstruction::IncrementMemory(
                AddressingMode::ZeroPage,
            ))),
            0xE8 => Some(Self::RegistersOnly(
                RegistersInstruction::IncrementRegister(CpuRegister::X),
            )),
            0xE9 => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::Immediate,
            ))),
            0xEA => Some(Self::RegistersOnly(RegistersInstruction::NoOp)),
            0xEC => Some(Self::Read(ReadInstruction::Compare(
                CpuRegister::X,
                AddressingMode::Absolute,
            ))),
            0xED => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::Absolute,
            ))),
            0xEE => Some(Self::ReadModifyWrite(ModifyInstruction::IncrementMemory(
                AddressingMode::Absolute,
            ))),
            0xF0 => Some(Self::Branch(BranchCondition::Equal)),
            0xF1 => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::IndirectY,
            ))),
            0xF5 => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::ZeroPageX,
            ))),
            0xF6 => Some(Self::ReadModifyWrite(ModifyInstruction::IncrementMemory(
                AddressingMode::ZeroPageX,
            ))),
            0xF8 => Some(Self::RegistersOnly(RegistersInstruction::SetDecimalFlag)),
            0xF9 => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::AbsoluteY,
            ))),
            0xFD => Some(Self::Read(ReadInstruction::SubtractWithCarry(
                AddressingMode::AbsoluteX,
            ))),
            0xFE => Some(Self::ReadModifyWrite(ModifyInstruction::IncrementMemory(
                AddressingMode::AbsoluteX,
            ))),
            _ => {
                // Unused or undocumented opcode
                None
            }
        }
    }
}
