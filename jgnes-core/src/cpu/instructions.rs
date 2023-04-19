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
pub enum CpuRegister {
    A,
    X,
    Y,
    S,
    P,
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
    ClearDecimalMode,
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
            0xD8 => Some(Self::ClearDecimalMode),
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
}
