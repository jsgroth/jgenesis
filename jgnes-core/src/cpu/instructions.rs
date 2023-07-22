//! Code for emulating the 6502 CPU instruction set.
//!
//! Instructions are broken down into a list of cycle ops that specify exactly what the CPU should
//! do on each cycle of each instruction.

#[cfg(test)]
mod tests;

use crate::bus;
use crate::bus::CpuBus;
use crate::cpu::{CpuRegisters, StatusFlags, StatusReadContext};
use crate::num::GetBit;
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use tinyvec::ArrayVec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum CpuRegister {
    A,
    X,
    Y,
    S,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum PushableRegister {
    A,
    P,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum StorableRegister {
    A,
    X,
    Y,
    AX,
}

impl StorableRegister {
    fn read(self, registers: &CpuRegisters) -> u8 {
        match self {
            Self::A => registers.accumulator,
            Self::X => registers.x,
            Self::Y => registers.y,
            Self::AX => registers.accumulator & registers.x,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum ReadInstruction {
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
    Load(CpuRegister, AddressingMode),
    // ORA
    InclusiveOr(AddressingMode),
    // SBC
    SubtractWithCarry(AddressingMode),
    // ANC (unofficial AND + ASL/ROL)
    AndWithShiftLeft,
    // ALR (unofficial AND + LSR)
    AndWithShiftRight,
    // ARR (unofficial AND + ROR)
    AndWithRotateRight,
    // LAX (unofficial LDA + TAX)
    LoadTransferAX(AddressingMode),
    // XAA (unofficial A := X & #imm)
    LoadAndXImmediate,
    // AXS (unofficial X := (A&X) - #imm)
    AXSubtract,
    // unofficial NOPs
    NoOp(AddressingMode),
}

impl ReadInstruction {
    fn execute(self, value: u8, registers: &mut CpuRegisters) {
        match self {
            Self::AddWithCarry(..) => {
                registers.accumulator = add(registers.accumulator, value, &mut registers.status);
            }
            Self::And(..) => {
                registers.accumulator = and(registers.accumulator, value, &mut registers.status);
            }
            Self::BitTest(..) => {
                bit_test(registers.accumulator, value, &mut registers.status);
            }
            Self::Compare(register, ..) => {
                let register_value = read_register(registers, register);
                compare(register_value, value, &mut registers.status);
            }
            Self::ExclusiveOr(..) => {
                registers.accumulator = xor(registers.accumulator, value, &mut registers.status);
            }
            Self::Load(register, ..) => {
                write_register(registers, register, value);
                registers
                    .status
                    .set_negative(value.bit(7))
                    .set_zero(value == 0);
            }
            Self::InclusiveOr(..) => {
                registers.accumulator = or(registers.accumulator, value, &mut registers.status);
            }
            Self::SubtractWithCarry(..) => {
                registers.accumulator =
                    subtract(registers.accumulator, value, &mut registers.status);
            }
            Self::AndWithShiftLeft => {
                // ANC performs an AND and then sets the C flag the way that ASL does
                registers.accumulator = and(registers.accumulator, value, &mut registers.status);
                registers.status.carry = registers.accumulator.bit(7);
            }
            Self::AndWithShiftRight => {
                // ALR simply performs an AND followed by an LSR
                let and_value = and(registers.accumulator, value, &mut registers.status);
                registers.accumulator = logical_shift_right(and_value, &mut registers.status);
            }
            Self::AndWithRotateRight => {
                // ARR is like a mix of AND, ROR, and ADC; the accumulator is set to (A & #imm) rotated,
                // but the flags are set differently from ROR

                let and_value = and(registers.accumulator, value, &mut StatusFlags::new());
                registers.accumulator = (and_value >> 1) | (u8::from(registers.status.carry) << 7);

                // The overflow flag is set as if an ADC was performed between the AND and ROR, and
                // the carry flag is set based on what was bit 7 prior to the rotation
                let overflow = registers.accumulator.bit(6) ^ registers.accumulator.bit(5);
                registers
                    .status
                    .set_negative(registers.accumulator.bit(7))
                    .set_overflow(overflow)
                    .set_carry(registers.accumulator.bit(6))
                    .set_zero(registers.accumulator == 0);
            }
            Self::LoadTransferAX(..) => {
                // LAX simply performs LDA and LDX simultaneously

                registers.accumulator = value;
                registers.x = value;

                registers
                    .status
                    .set_negative(value.bit(7))
                    .set_zero(value == 0);
            }
            Self::LoadAndXImmediate => {
                registers.accumulator = registers.x & value;
                registers
                    .status
                    .set_negative(registers.accumulator.bit(7))
                    .set_zero(registers.accumulator == 0);
            }
            Self::AXSubtract => {
                // AXS sets X to (A&X) - #imm, while ignoring the current carry flag. The flags
                // are set not from the subtraction operation but from a CMP between (A&X) and #imm

                let ax = registers.accumulator & registers.x;
                let mut flags = StatusFlags {
                    // Set carry to true because SBC inverts the carry flag for borrowing
                    carry: true,
                    ..StatusFlags::new()
                };
                registers.x = subtract(ax, value, &mut flags);

                compare(ax, value, &mut registers.status);
            }
            Self::NoOp(_) => {}
        }
    }

    fn addressing_mode(self) -> AddressingMode {
        match self {
            Self::AddWithCarry(addressing_mode)
            | Self::And(addressing_mode)
            | Self::BitTest(addressing_mode)
            | Self::Compare(_, addressing_mode)
            | Self::ExclusiveOr(addressing_mode)
            | Self::Load(_, addressing_mode)
            | Self::InclusiveOr(addressing_mode)
            | Self::SubtractWithCarry(addressing_mode)
            | Self::NoOp(addressing_mode)
            | Self::LoadTransferAX(addressing_mode) => addressing_mode,
            Self::AndWithShiftLeft
            | Self::AndWithShiftRight
            | Self::AndWithRotateRight
            | Self::LoadAndXImmediate
            | Self::AXSubtract => AddressingMode::Immediate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum ModifyInstruction {
    // ASL
    ShiftLeft(AddressingMode),
    // DEC
    Decrement(AddressingMode),
    // INC
    Increment(AddressingMode),
    // LSR
    LogicalShiftRight(AddressingMode),
    // ROL
    RotateLeft(AddressingMode),
    // ROR
    RotateRight(AddressingMode),
    // SLO (unofficial ASL + ORA)
    ShiftLeftOr(AddressingMode),
    // RLA (unofficial ROL + AND)
    RotateLeftAnd(AddressingMode),
    // SRE (unofficial LSR + EOR)
    ShiftRightExclusiveOr(AddressingMode),
    // RRA (unofficial ROR + ADC)
    RotateRightAdd(AddressingMode),
    // DCP (unofficial DEC + CMP)
    DecrementCompare(AddressingMode),
    // ISC (unofficial INC + SBC)
    IncrementSubtract(AddressingMode),
}

impl ModifyInstruction {
    fn addressing_mode(self) -> AddressingMode {
        match self {
            Self::ShiftLeft(addressing_mode)
            | Self::Decrement(addressing_mode)
            | Self::Increment(addressing_mode)
            | Self::LogicalShiftRight(addressing_mode)
            | Self::RotateLeft(addressing_mode)
            | Self::RotateRight(addressing_mode)
            | Self::ShiftLeftOr(addressing_mode)
            | Self::RotateLeftAnd(addressing_mode)
            | Self::ShiftRightExclusiveOr(addressing_mode)
            | Self::RotateRightAdd(addressing_mode)
            | Self::DecrementCompare(addressing_mode)
            | Self::IncrementSubtract(addressing_mode) => addressing_mode,
        }
    }

    fn execute(self, value: u8, registers: &mut CpuRegisters) -> u8 {
        match self {
            Self::ShiftLeft(..) => shift_left(value, &mut registers.status),
            Self::Decrement(..) => decrement(value, &mut registers.status),
            Self::Increment(..) => increment(value, &mut registers.status),
            Self::LogicalShiftRight(..) => logical_shift_right(value, &mut registers.status),
            Self::RotateLeft(..) => rotate_left(value, &mut registers.status),
            Self::RotateRight(..) => rotate_right(value, &mut registers.status),
            Self::ShiftLeftOr(..) => {
                let shifted = shift_left(value, &mut registers.status);
                registers.accumulator = or(registers.accumulator, shifted, &mut registers.status);
                shifted
            }
            Self::RotateLeftAnd(..) => {
                let rotated = rotate_left(value, &mut registers.status);
                registers.accumulator = and(registers.accumulator, rotated, &mut registers.status);
                rotated
            }
            Self::ShiftRightExclusiveOr(..) => {
                let shifted = logical_shift_right(value, &mut registers.status);
                registers.accumulator = xor(registers.accumulator, shifted, &mut registers.status);
                shifted
            }
            Self::RotateRightAdd(..) => {
                let rotated = rotate_right(value, &mut registers.status);
                registers.accumulator = add(registers.accumulator, rotated, &mut registers.status);
                rotated
            }
            Self::DecrementCompare(..) => {
                let decremented = decrement(value, &mut registers.status);
                compare(registers.accumulator, decremented, &mut registers.status);
                decremented
            }
            Self::IncrementSubtract(..) => {
                let incremented = increment(value, &mut registers.status);
                registers.accumulator =
                    subtract(registers.accumulator, incremented, &mut registers.status);
                incremented
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum RegistersInstruction {
    // CLC
    ClearCarryFlag,
    // CLD
    ClearDecimalFlag,
    // CLI
    ClearInterruptDisable,
    // CLV
    ClearOverflowFlag,
    // DEX / DEY
    Decrement(CpuRegister),
    // INX / INY
    Increment(CpuRegister),
    // NOP
    NoOp,
    // SEC
    SetCarryFlag,
    // SED
    SetDecimalFlag,
    // SEI
    SetInterruptDisable,
    // TAX / TAY / TSX / TXA / TXS / TYA
    Transfer { to: CpuRegister, from: CpuRegister },
}

impl RegistersInstruction {
    fn execute(self, registers: &mut CpuRegisters) {
        match self {
            Self::ClearCarryFlag => {
                registers.status.carry = false;
            }
            Self::ClearDecimalFlag => {
                registers.status.decimal = false;
            }
            Self::ClearInterruptDisable => {
                registers.status.interrupt_disable = false;
            }
            Self::ClearOverflowFlag => {
                registers.status.overflow = false;
            }
            Self::Decrement(register) => {
                let value = read_register(registers, register).wrapping_sub(1);
                write_register(registers, register, value);
                registers
                    .status
                    .set_negative(value.bit(7))
                    .set_zero(value == 0);
            }
            Self::Increment(register) => {
                let value = read_register(registers, register).wrapping_add(1);
                write_register(registers, register, value);
                registers
                    .status
                    .set_negative(value.bit(7))
                    .set_zero(value == 0);
            }
            Self::NoOp => {}
            Self::SetCarryFlag => {
                registers.status.carry = true;
            }
            Self::SetDecimalFlag => {
                registers.status.decimal = true;
            }
            Self::SetInterruptDisable => {
                registers.status.interrupt_disable = true;
            }
            Self::Transfer { to, from } => {
                let value = read_register(registers, from);
                write_register(registers, to, value);
                if to != CpuRegister::S {
                    registers
                        .status
                        .set_negative(value.bit(7))
                        .set_zero(value == 0);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
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
    fn check(self, flags: StatusFlags) -> bool {
        match self {
            Self::CarryClear => !flags.carry,
            Self::CarrySet => flags.carry,
            Self::Equal => flags.zero,
            Self::Minus => flags.negative,
            Self::NotEqual => !flags.zero,
            Self::Positive => !flags.negative,
            Self::OverflowClear => !flags.overflow,
            Self::OverflowSet => flags.overflow,
        }
    }
}

type OpVec = ArrayVec<[CycleOp; 7]>;

#[derive(Debug, Clone, Encode, Decode)]
pub struct InstructionState {
    #[bincode(with_serde)]
    pub ops: OpVec,
    pub op_index: u8,
    pub operand_first_byte: u8,
    pub operand_second_byte: u8,
    pub target_first_byte: u8,
    pub target_second_byte: u8,
    pub indirect_byte: u8,
    pub interrupt_vector: u16,
    pub pending_interrupt: bool,
}

impl InstructionState {
    pub fn from_ops(ops: OpVec) -> Self {
        Self {
            ops,
            op_index: 0,
            operand_first_byte: 0,
            operand_second_byte: 0,
            target_first_byte: 0,
            target_second_byte: 0,
            indirect_byte: 0,
            interrupt_vector: 0,
            pending_interrupt: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum Index {
    X,
    Y,
}

impl Index {
    fn get(self, registers: &CpuRegisters) -> u8 {
        match self {
            Self::X => registers.x,
            Self::Y => registers.y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum CycleOp {
    FetchOperand1,
    FetchOperand2,
    SpuriousOperandRead,
    SpuriousStackRead,
    ZeroPageIndexAddress,
    FetchZeroPage1,
    FetchZeroPage2,
    FetchZeroPageIndexed1,
    FetchZeroPageIndexed2,
    FetchAbsolute,
    FetchAbsoluteIndexed(Index),
    FetchIndexedIndirect,
    FetchIndirectIndexed,
    ZeroPageWriteBack,
    ZeroPageIndexedWriteBack,
    AbsoluteWriteBack,
    AbsoluteIndexedWriteBack(Index),
    IndexedIndirectWriteBack,
    IndirectIndexedWriteBack,
    AbsoluteIndexedFixHighByte(Index),
    IndirectIndexedFixHighByte,
    ExecuteRegistersOnly(RegistersInstruction),
    ExecuteAccumulatorModify(ModifyInstruction),
    ExecuteImmediateRead(ReadInstruction),
    ExecuteZeroPageRead(ReadInstruction),
    ExecuteZeroPageStore(StorableRegister),
    ExecuteZeroPageModify(ModifyInstruction),
    ExecuteZeroPageIndexedRead(Index, ReadInstruction),
    ExecuteZeroPageIndexedStore(Index, StorableRegister),
    ExecuteZeroPageIndexedModify(ModifyInstruction),
    ExecuteAbsoluteRead(ReadInstruction),
    ExecuteAbsoluteStore(StorableRegister),
    ExecuteAbsoluteModify(ModifyInstruction),
    ExecuteAbsoluteIndexedRead(Index, ReadInstruction),
    ExecuteAbsoluteIndexedReadDelayed(Index, ReadInstruction),
    ExecuteAbsoluteIndexedStore(Index, StorableRegister),
    ExecuteAbsoluteIndexedModify(Index, ModifyInstruction),
    ExecuteIndexedIndirectRead(ReadInstruction),
    ExecuteIndexedIndirectStore(StorableRegister),
    ExecuteIndexedIndirectModify(ModifyInstruction),
    ExecuteIndirectIndexedRead(ReadInstruction),
    ExecuteIndirectIndexedReadDelayed(ReadInstruction),
    ExecuteIndirectIndexedStore(StorableRegister),
    ExecuteIndirectIndexedModify(ModifyInstruction),
    ExecuteUnofficialStore(Index, CpuRegister),
    CheckBranchCondition(BranchCondition),
    CheckBranchHighByte,
    FixBranchHighByte,
    ExecuteJumpAbsolute,
    ExecuteJumpIndirect,
    ExecutePush(PushableRegister),
    ExecutePull(PushableRegister),
    PushPCHigh,
    PushPCLow,
    PullPCLow,
    PullPCHigh,
    InterruptPushStatus(StatusReadContext),
    InterruptPullPCLow,
    InterruptPullPCHigh,
}

// Needed for ArrayVec
impl Default for CycleOp {
    fn default() -> Self {
        Self::FetchOperand1
    }
}

impl CycleOp {
    pub fn execute(
        self,
        mut state: InstructionState,
        registers: &mut CpuRegisters,
        bus: &mut CpuBus<'_>,
    ) -> InstructionState {
        // Check for interrupts before executing op because op could modify the I flag
        let is_last_cycle = usize::from(state.op_index) == state.ops.len() - 1;
        if is_last_cycle
            || matches!(
                self,
                Self::CheckBranchCondition(..)
                    | Self::ExecuteAbsoluteIndexedRead(..)
                    | Self::ExecuteIndirectIndexedRead(..)
            )
        {
            let interrupt_pending = bus.interrupt_lines().nmi_triggered()
                || (!registers.status.interrupt_disable && bus.interrupt_lines().irq_triggered());
            if self == Self::FixBranchHighByte {
                // Last cycle of branch instruction
                state.pending_interrupt |= interrupt_pending;
            } else {
                state.pending_interrupt = interrupt_pending;
            }
        }

        match self {
            Self::FetchOperand1 => {
                state.operand_first_byte = bus.read_address(registers.pc);
                registers.pc += 1;
            }
            Self::FetchOperand2 => {
                state.operand_second_byte = bus.read_address(registers.pc);
                registers.pc += 1;
            }
            Self::SpuriousOperandRead => {
                bus.read_address(registers.pc);
            }
            Self::SpuriousStackRead => {
                bus.read_address(u16::from_be_bytes([0x01, registers.sp]));
            }
            Self::ZeroPageIndexAddress => {
                // Spurious read
                bus.read_address(u16::from(state.operand_first_byte));
            }
            Self::FetchZeroPage1 => {
                state.target_first_byte = bus.read_address(u16::from(state.operand_first_byte));
            }
            Self::FetchZeroPage2 => {
                state.target_second_byte =
                    bus.read_address(u16::from(state.operand_first_byte.wrapping_add(1)));
            }
            Self::FetchZeroPageIndexed1 => {
                let address = u16::from(state.operand_first_byte.wrapping_add(registers.x));
                state.target_first_byte = bus.read_address(address);
            }
            Self::FetchZeroPageIndexed2 => {
                let address = u16::from(
                    state
                        .operand_first_byte
                        .wrapping_add(registers.x)
                        .wrapping_add(1),
                );
                state.target_second_byte = bus.read_address(address);
            }
            Self::FetchAbsolute => {
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                state.target_first_byte = bus.read_address(address);
            }
            Self::FetchAbsoluteIndexed(index) => {
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let index = index.get(registers);
                let indexed_address = address.wrapping_add(index.into());

                state.target_first_byte = bus.read_address(indexed_address);
            }
            Self::FetchIndexedIndirect => {
                let effective_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte]);
                state.indirect_byte = bus.read_address(effective_address);
            }
            Self::FetchIndirectIndexed => {
                let effective_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte])
                        .wrapping_add(registers.y.into());
                state.indirect_byte = bus.read_address(effective_address);
            }
            Self::ZeroPageWriteBack => {
                // Spurious write
                bus.write_address(u16::from(state.operand_first_byte), state.target_first_byte);
            }
            Self::ZeroPageIndexedWriteBack => {
                // Spurious write
                let address = u16::from(state.operand_first_byte.wrapping_add(registers.x));
                bus.write_address(address, state.target_first_byte);
            }
            Self::AbsoluteWriteBack => {
                // Spurious write
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                bus.write_address(address, state.target_first_byte);
            }
            Self::AbsoluteIndexedWriteBack(index) => {
                // Spurious write
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let index = index.get(registers);
                let indexed_address = address.wrapping_add(index.into());

                bus.write_address(indexed_address, state.target_first_byte);
            }
            Self::IndexedIndirectWriteBack => {
                let effective_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte]);
                bus.write_address(effective_address, state.indirect_byte);
            }
            Self::IndirectIndexedWriteBack => {
                let effective_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte])
                        .wrapping_add(registers.y.into());
                bus.write_address(effective_address, state.indirect_byte);
            }
            Self::AbsoluteIndexedFixHighByte(index) => {
                let index = index.get(registers);
                let indexed_low_byte = state.operand_first_byte.wrapping_add(index);
                bus.read_address(u16::from_le_bytes([
                    indexed_low_byte,
                    state.operand_second_byte,
                ]));
            }
            Self::IndirectIndexedFixHighByte => {
                let indexed_low_byte = state.target_first_byte.wrapping_add(registers.y);
                bus.read_address(u16::from_le_bytes([
                    indexed_low_byte,
                    state.target_second_byte,
                ]));
            }
            Self::ExecuteRegistersOnly(instruction) => {
                // Spurious bus read
                bus.read_address(registers.pc);

                instruction.execute(registers);
            }
            Self::ExecuteAccumulatorModify(instruction) => {
                // Spurious bus read
                bus.read_address(registers.pc);

                registers.accumulator = instruction.execute(registers.accumulator, registers);
            }
            Self::ExecuteImmediateRead(instruction) => {
                let operand = bus.read_address(registers.pc);
                registers.pc += 1;

                instruction.execute(operand, registers);
            }
            Self::ExecuteZeroPageRead(instruction) => {
                let value = bus.read_address(u16::from(state.operand_first_byte));
                instruction.execute(value, registers);
            }
            Self::ExecuteZeroPageStore(register) => {
                let value = register.read(registers);
                let address = u16::from(state.operand_first_byte);

                bus.write_address(address, value);
            }
            Self::ExecuteZeroPageModify(instruction) => {
                let value = instruction.execute(state.target_first_byte, registers);
                bus.write_address(u16::from(state.operand_first_byte), value);
            }
            Self::ExecuteZeroPageIndexedRead(index, instruction) => {
                let index = index.get(registers);
                let indexed_address = u16::from(state.operand_first_byte.wrapping_add(index));
                let value = bus.read_address(indexed_address);

                instruction.execute(value, registers);
            }
            Self::ExecuteZeroPageIndexedStore(index, register) => {
                let index = index.get(registers);
                let indexed_address = u16::from(state.operand_first_byte.wrapping_add(index));
                let value = register.read(registers);

                bus.write_address(indexed_address, value);
            }
            Self::ExecuteZeroPageIndexedModify(instruction) => {
                let value = instruction.execute(state.target_first_byte, registers);

                let indexed_address = u16::from(state.operand_first_byte.wrapping_add(registers.x));
                bus.write_address(indexed_address, value);
            }
            Self::ExecuteAbsoluteRead(instruction) => {
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let value = bus.read_address(address);

                instruction.execute(value, registers);
            }
            Self::ExecuteAbsoluteStore(register) => {
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let value = register.read(registers);

                bus.write_address(address, value);
            }
            Self::ExecuteAbsoluteModify(instruction) => {
                let value = instruction.execute(state.target_first_byte, registers);

                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                bus.write_address(address, value);
            }
            Self::ExecuteAbsoluteIndexedRead(index, instruction) => {
                let index = index.get(registers);
                let (indexed_low_byte, overflowed) =
                    state.operand_first_byte.overflowing_add(index);

                let address = u16::from_le_bytes([indexed_low_byte, state.operand_second_byte]);
                let value = bus.read_address(address);

                if !overflowed {
                    instruction.execute(value, registers);

                    // Skip next (last) cycle
                    state.op_index += 1;
                } else {
                    log::trace!(
                        "  ExecuteAbsoluteIndexedRead: Low byte overflowed, taking extra cycle"
                    );
                }
            }
            Self::ExecuteAbsoluteIndexedReadDelayed(index, instruction) => {
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let index = index.get(registers);
                let indexed_address = address.wrapping_add(u16::from(index));
                let value = bus.read_address(indexed_address);

                instruction.execute(value, registers);
            }
            Self::ExecuteAbsoluteIndexedStore(index, register) => {
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let index = index.get(registers);
                let indexed_address = address.wrapping_add(index.into());

                let value = register.read(registers);

                bus.write_address(indexed_address, value);
            }
            Self::ExecuteUnofficialStore(index, register) => {
                // This is a buggy instruction that is only implemented because CPU test ROMs test
                // it.
                // This implementation ANDs the X/Y register with the high byte of the address plus 1
                // and then stores that value, but only if the indexing did not overflow.

                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let index = index.get(registers);
                let indexed_address = address.wrapping_add(index.into());
                let (_, overflowed) = state.operand_first_byte.overflowing_add(index);

                let value =
                    read_register(registers, register) & state.operand_second_byte.wrapping_add(1);

                if !overflowed {
                    bus.write_address(indexed_address, value);
                }
            }
            Self::ExecuteAbsoluteIndexedModify(index, instruction) => {
                let address =
                    u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
                let index = index.get(registers);
                let indexed_address = address.wrapping_add(u16::from(index));

                let value = instruction.execute(state.target_first_byte, registers);

                bus.write_address(indexed_address, value);
            }
            Self::ExecuteIndexedIndirectRead(instruction) => {
                let effective_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte]);
                let value = bus.read_address(effective_address);

                instruction.execute(value, registers);
            }
            Self::ExecuteIndexedIndirectStore(register) => {
                let effective_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte]);
                let value = register.read(registers);

                bus.write_address(effective_address, value);
            }
            Self::ExecuteIndexedIndirectModify(instruction) => {
                let effective_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte]);
                let value = instruction.execute(state.indirect_byte, registers);
                bus.write_address(effective_address, value);
            }
            Self::ExecuteIndirectIndexedRead(instruction) => {
                let (indexed_low_byte, overflowed) =
                    state.target_first_byte.overflowing_add(registers.y);
                let address = u16::from_le_bytes([indexed_low_byte, state.target_second_byte]);
                let value = bus.read_address(address);

                if !overflowed {
                    instruction.execute(value, registers);

                    // Skip next (last) cycle
                    state.op_index += 1;
                } else {
                    log::trace!(
                        "  ExecuteIndirectIndexedRead: low byte overflowed, taking extra cycle"
                    );
                }
            }
            Self::ExecuteIndirectIndexedReadDelayed(instruction) => {
                let indexed_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte])
                        .wrapping_add(registers.y.into());
                let value = bus.read_address(indexed_address);

                instruction.execute(value, registers);
            }
            Self::ExecuteIndirectIndexedStore(register) => {
                let indexed_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte])
                        .wrapping_add(registers.y.into());
                let value = register.read(registers);

                bus.write_address(indexed_address, value);
            }
            Self::ExecuteIndirectIndexedModify(instruction) => {
                let indexed_address =
                    u16::from_le_bytes([state.target_first_byte, state.target_second_byte])
                        .wrapping_add(registers.y.into());
                let value = instruction.execute(state.indirect_byte, registers);

                bus.write_address(indexed_address, value);
            }
            Self::CheckBranchCondition(branch_condition) => {
                state.operand_first_byte = bus.read_address(registers.pc);
                registers.pc += 1;

                if !branch_condition.check(registers.status) {
                    // Skip rest of branch cycles
                    state.op_index += 2;
                } else {
                    log::trace!("  CheckBranchCondition: {branch_condition:?} evaluated to true");
                }
            }
            Self::CheckBranchHighByte => {
                // Spurious read when branch is taken
                bus.read_address(registers.pc);

                let offset = state.operand_first_byte as i8;
                let new_pc = (i32::from(registers.pc) + i32::from(offset)) as u16;

                if registers.pc & 0xFF00 == new_pc & 0xFF00 {
                    // Skip last branch cycle
                    registers.pc = new_pc;
                    state.op_index += 1;
                } else {
                    log::trace!(
                        "  CheckBranchHighByte: High byte needs to be fixed, taking extra cycle"
                    );
                }
            }
            Self::FixBranchHighByte => {
                let offset = state.operand_first_byte as i8;
                let new_pc = (i32::from(registers.pc) + i32::from(offset)) as u16;

                // Spurious read
                bus.read_address((registers.pc & 0xFF00) | (new_pc & 0x00FF));

                registers.pc = new_pc;
            }
            Self::ExecuteJumpAbsolute => {
                let address_msb = bus.read_address(registers.pc);

                registers.pc = u16::from_le_bytes([state.operand_first_byte, address_msb]);
            }
            Self::ExecuteJumpIndirect => {
                let address_lsb = state.operand_first_byte.wrapping_add(1);
                let address = u16::from_le_bytes([address_lsb, state.operand_second_byte]);
                let effective_address_msb = bus.read_address(address);

                registers.pc = u16::from_le_bytes([state.target_first_byte, effective_address_msb]);
            }
            Self::ExecutePush(register) => {
                let value = match register {
                    PushableRegister::A => registers.accumulator,
                    PushableRegister::P => registers.status.to_byte(StatusReadContext::PushStack),
                };
                let stack_address = u16::from_be_bytes([0x01, registers.sp]);

                bus.write_address(stack_address, value);
                registers.sp = registers.sp.wrapping_sub(1);
            }
            Self::ExecutePull(register) => {
                registers.sp = registers.sp.wrapping_add(1);
                let stack_address = u16::from_be_bytes([0x01, registers.sp]);
                let value = bus.read_address(stack_address);

                match register {
                    PushableRegister::A => {
                        registers.accumulator = value;
                        registers
                            .status
                            .set_negative(value.bit(7))
                            .set_zero(value == 0);
                    }
                    PushableRegister::P => {
                        registers.status = StatusFlags::from_byte(value);
                    }
                }
            }
            Self::PushPCHigh => {
                let stack_address = u16::from_be_bytes([0x01, registers.sp]);
                bus.write_address(stack_address, (registers.pc >> 8) as u8);
                registers.sp = registers.sp.wrapping_sub(1);
            }
            Self::PushPCLow => {
                let stack_address = u16::from_be_bytes([0x01, registers.sp]);
                bus.write_address(stack_address, (registers.pc & 0x00FF) as u8);
                registers.sp = registers.sp.wrapping_sub(1);
            }
            Self::PullPCLow => {
                registers.sp = registers.sp.wrapping_add(1);
                let stack_address = u16::from_be_bytes([0x01, registers.sp]);

                registers.pc = u16::from(bus.read_address(stack_address));
            }
            Self::PullPCHigh => {
                registers.sp = registers.sp.wrapping_add(1);
                let stack_address = u16::from_be_bytes([0x01, registers.sp]);
                let pc_msb = bus.read_address(stack_address);

                registers.pc |= u16::from(pc_msb) << 8;
            }
            Self::InterruptPushStatus(read_ctx) => {
                let stack_address = u16::from_be_bytes([0x01, registers.sp]);
                bus.write_address(stack_address, registers.status.to_byte(read_ctx));
                registers.sp = registers.sp.wrapping_sub(1);

                state.interrupt_vector = if bus.interrupt_lines().nmi_triggered() {
                    log::trace!("  InterruptPushStatus: Jumping to NMI vector");

                    bus.interrupt_lines().clear_nmi_triggered();
                    bus::CPU_NMI_VECTOR
                } else {
                    log::trace!("  InterruptPushStatus: Jumping to IRQ vector");

                    bus::CPU_IRQ_VECTOR
                };
            }
            Self::InterruptPullPCLow => {
                registers.pc = u16::from(bus.read_address(state.interrupt_vector));
                registers.status.interrupt_disable = true;
            }
            Self::InterruptPullPCHigh => {
                let pc_msb = bus.read_address(state.interrupt_vector + 1);
                registers.pc |= u16::from(pc_msb) << 8;
            }
        }

        state.op_index += 1;
        state
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    Read(ReadInstruction),
    Modify(ModifyInstruction),
    RegistersOnly(RegistersInstruction),
    Branch(BranchCondition),
    // STA / STX / STY / SAX (SAX == unofficial STA + STX)
    StoreRegister(StorableRegister, AddressingMode),
    // SHY / SHX (buggy unofficial opcodes)
    UnofficialStore(CpuRegister),
    // BRK
    ForceInterrupt,
    // JMP
    Jump(AddressingMode),
    // JSR
    JumpToSubroutine,
    // PHA / PHP
    PushStack(PushableRegister),
    // PLA / PLP
    PullStack(PushableRegister),
    // RTI
    ReturnFromInterrupt,
    // RTS
    ReturnFromSubroutine,
}

impl Instruction {
    pub fn get_cycle_ops(self) -> OpVec {
        match self {
            Self::Read(instruction) => get_read_cycle_ops(instruction),
            Self::StoreRegister(register, addressing_mode) => {
                get_store_cycle_ops(register, addressing_mode)
            }
            Self::UnofficialStore(CpuRegister::X) => [
                CycleOp::FetchOperand1,
                CycleOp::FetchOperand2,
                CycleOp::AbsoluteIndexedFixHighByte(Index::Y),
                CycleOp::ExecuteUnofficialStore(Index::Y, CpuRegister::X),
            ]
            .into_iter()
            .collect(),
            Self::UnofficialStore(CpuRegister::Y) => [
                CycleOp::FetchOperand1,
                CycleOp::FetchOperand2,
                CycleOp::AbsoluteIndexedFixHighByte(Index::X),
                CycleOp::ExecuteUnofficialStore(Index::X, CpuRegister::Y),
            ]
            .into_iter()
            .collect(),
            Self::Modify(instruction) => get_modify_cycle_ops(instruction),
            Self::RegistersOnly(instruction) => [CycleOp::ExecuteRegistersOnly(instruction)]
                .into_iter()
                .collect(),
            Self::Branch(branch_condition) => [
                CycleOp::CheckBranchCondition(branch_condition),
                CycleOp::CheckBranchHighByte,
                CycleOp::FixBranchHighByte,
            ]
            .into_iter()
            .collect(),
            Self::Jump(AddressingMode::Absolute) => {
                [CycleOp::FetchOperand1, CycleOp::ExecuteJumpAbsolute]
                    .into_iter()
                    .collect()
            }
            Self::Jump(AddressingMode::Indirect) => [
                CycleOp::FetchOperand1,
                CycleOp::FetchOperand2,
                CycleOp::FetchAbsolute,
                CycleOp::ExecuteJumpIndirect,
            ]
            .into_iter()
            .collect(),
            Self::PushStack(register) => {
                [CycleOp::SpuriousOperandRead, CycleOp::ExecutePush(register)]
                    .into_iter()
                    .collect()
            }
            Self::PullStack(register) => [
                CycleOp::SpuriousOperandRead,
                CycleOp::SpuriousStackRead,
                CycleOp::ExecutePull(register),
            ]
            .into_iter()
            .collect(),
            Self::JumpToSubroutine => [
                CycleOp::FetchOperand1,
                CycleOp::SpuriousStackRead,
                CycleOp::PushPCHigh,
                CycleOp::PushPCLow,
                CycleOp::ExecuteJumpAbsolute,
            ]
            .into_iter()
            .collect(),
            Self::ReturnFromSubroutine => [
                CycleOp::SpuriousOperandRead,
                CycleOp::SpuriousStackRead,
                CycleOp::PullPCLow,
                CycleOp::PullPCHigh,
                CycleOp::FetchOperand1,
            ]
            .into_iter()
            .collect(),
            Self::ReturnFromInterrupt => [
                CycleOp::SpuriousOperandRead,
                CycleOp::SpuriousStackRead,
                CycleOp::ExecutePull(PushableRegister::P),
                CycleOp::PullPCLow,
                CycleOp::PullPCHigh,
            ]
            .into_iter()
            .collect(),
            Self::ForceInterrupt => [
                CycleOp::FetchOperand1,
                CycleOp::PushPCHigh,
                CycleOp::PushPCLow,
                CycleOp::InterruptPushStatus(StatusReadContext::Brk),
                CycleOp::InterruptPullPCLow,
                CycleOp::InterruptPullPCHigh,
            ]
            .into_iter()
            .collect(),
            Self::Jump(addressing_mode) => {
                panic!("invalid jump addressing mode: {addressing_mode:?}")
            }
            Self::UnofficialStore(register) => {
                panic!("invalid unofficial store register: {register:?}")
            }
        }
    }

    pub fn from_opcode(opcode: u8) -> Option<Self> {
        use AddressingMode as AM;
        use ModifyInstruction as MI;
        use ReadInstruction as RI;

        match opcode {
            0x00 => Some(Self::ForceInterrupt),
            0x01 => Some(Self::Read(RI::InclusiveOr(AM::IndirectX))),
            0x03 => Some(Self::Modify(MI::ShiftLeftOr(AM::IndirectX))),
            0x04 | 0x44 | 0x64 => Some(Self::Read(RI::NoOp(AM::ZeroPage))),
            0x05 => Some(Self::Read(RI::InclusiveOr(AM::ZeroPage))),
            0x06 => Some(Self::Modify(MI::ShiftLeft(AM::ZeroPage))),
            0x07 => Some(Self::Modify(MI::ShiftLeftOr(AM::ZeroPage))),
            0x08 => Some(Self::PushStack(PushableRegister::P)),
            0x09 => Some(Self::Read(RI::InclusiveOr(AM::Immediate))),
            0x0A => Some(Self::Modify(MI::ShiftLeft(AM::Accumulator))),
            0x0B | 0x2B => Some(Self::Read(RI::AndWithShiftLeft)),
            0x0C => Some(Self::Read(RI::NoOp(AM::Absolute))),
            0x0D => Some(Self::Read(RI::InclusiveOr(AM::Absolute))),
            0x0E => Some(Self::Modify(MI::ShiftLeft(AM::Absolute))),
            0x0F => Some(Self::Modify(MI::ShiftLeftOr(AM::Absolute))),
            0x10 => Some(Self::Branch(BranchCondition::Positive)),
            0x11 => Some(Self::Read(RI::InclusiveOr(AM::IndirectY))),
            0x13 => Some(Self::Modify(MI::ShiftLeftOr(AM::IndirectY))),
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => Some(Self::Read(RI::NoOp(AM::ZeroPageX))),
            0x15 => Some(Self::Read(RI::InclusiveOr(AM::ZeroPageX))),
            0x16 => Some(Self::Modify(MI::ShiftLeft(AM::ZeroPageX))),
            0x17 => Some(Self::Modify(MI::ShiftLeftOr(AM::ZeroPageX))),
            0x18 => Some(Self::RegistersOnly(RegistersInstruction::ClearCarryFlag)),
            0x19 => Some(Self::Read(RI::InclusiveOr(AM::AbsoluteY))),
            0x1D => Some(Self::Read(RI::InclusiveOr(AM::AbsoluteX))),
            0x1E => Some(Self::Modify(MI::ShiftLeft(AM::AbsoluteX))),
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xEA | 0xFA => {
                Some(Self::RegistersOnly(RegistersInstruction::NoOp))
            }
            0x1B => Some(Self::Modify(MI::ShiftLeftOr(AM::AbsoluteY))),
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => Some(Self::Read(RI::NoOp(AM::AbsoluteX))),
            0x1F => Some(Self::Modify(MI::ShiftLeftOr(AM::AbsoluteX))),
            0x20 => Some(Self::JumpToSubroutine),
            0x21 => Some(Self::Read(RI::And(AM::IndirectX))),
            0x23 => Some(Self::Modify(MI::RotateLeftAnd(AM::IndirectX))),
            0x24 => Some(Self::Read(RI::BitTest(AM::ZeroPage))),
            0x25 => Some(Self::Read(RI::And(AM::ZeroPage))),
            0x26 => Some(Self::Modify(MI::RotateLeft(AM::ZeroPage))),
            0x27 => Some(Self::Modify(MI::RotateLeftAnd(AM::ZeroPage))),
            0x28 => Some(Self::PullStack(PushableRegister::P)),
            0x29 => Some(Self::Read(RI::And(AM::Immediate))),
            0x2A => Some(Self::Modify(MI::RotateLeft(AM::Accumulator))),
            0x2C => Some(Self::Read(RI::BitTest(AM::Absolute))),
            0x2D => Some(Self::Read(RI::And(AM::Absolute))),
            0x2E => Some(Self::Modify(MI::RotateLeft(AM::Absolute))),
            0x2F => Some(Self::Modify(MI::RotateLeftAnd(AM::Absolute))),
            0x30 => Some(Self::Branch(BranchCondition::Minus)),
            0x31 => Some(Self::Read(RI::And(AM::IndirectY))),
            0x33 => Some(Self::Modify(MI::RotateLeftAnd(AM::IndirectY))),
            0x35 => Some(Self::Read(RI::And(AM::ZeroPageX))),
            0x36 => Some(Self::Modify(MI::RotateLeft(AM::ZeroPageX))),
            0x37 => Some(Self::Modify(MI::RotateLeftAnd(AM::ZeroPageX))),
            0x38 => Some(Self::RegistersOnly(RegistersInstruction::SetCarryFlag)),
            0x39 => Some(Self::Read(RI::And(AM::AbsoluteY))),
            0x3B => Some(Self::Modify(MI::RotateLeftAnd(AM::AbsoluteY))),
            0x3D => Some(Self::Read(RI::And(AM::AbsoluteX))),
            0x3E => Some(Self::Modify(MI::RotateLeft(AM::AbsoluteX))),
            0x3F => Some(Self::Modify(MI::RotateLeftAnd(AM::AbsoluteX))),
            0x40 => Some(Self::ReturnFromInterrupt),
            0x41 => Some(Self::Read(RI::ExclusiveOr(AM::IndirectX))),
            0x43 => Some(Self::Modify(MI::ShiftRightExclusiveOr(AM::IndirectX))),
            0x45 => Some(Self::Read(RI::ExclusiveOr(AM::ZeroPage))),
            0x46 => Some(Self::Modify(MI::LogicalShiftRight(AM::ZeroPage))),
            0x47 => Some(Self::Modify(MI::ShiftRightExclusiveOr(AM::ZeroPage))),
            0x48 => Some(Self::PushStack(PushableRegister::A)),
            0x49 => Some(Self::Read(RI::ExclusiveOr(AM::Immediate))),
            0x4A => Some(Self::Modify(MI::LogicalShiftRight(AM::Accumulator))),
            0x4B => Some(Self::Read(RI::AndWithShiftRight)),
            0x4C => Some(Self::Jump(AM::Absolute)),
            0x4D => Some(Self::Read(RI::ExclusiveOr(AM::Absolute))),
            0x4E => Some(Self::Modify(MI::LogicalShiftRight(AM::Absolute))),
            0x4F => Some(Self::Modify(MI::ShiftRightExclusiveOr(AM::Absolute))),
            0x50 => Some(Self::Branch(BranchCondition::OverflowClear)),
            0x51 => Some(Self::Read(RI::ExclusiveOr(AM::IndirectY))),
            0x53 => Some(Self::Modify(MI::ShiftRightExclusiveOr(AM::IndirectY))),
            0x55 => Some(Self::Read(RI::ExclusiveOr(AM::ZeroPageX))),
            0x56 => Some(Self::Modify(MI::LogicalShiftRight(AM::ZeroPageX))),
            0x57 => Some(Self::Modify(MI::ShiftRightExclusiveOr(AM::ZeroPageX))),
            0x58 => Some(Self::RegistersOnly(
                RegistersInstruction::ClearInterruptDisable,
            )),
            0x59 => Some(Self::Read(RI::ExclusiveOr(AM::AbsoluteY))),
            0x5B => Some(Self::Modify(MI::ShiftRightExclusiveOr(AM::AbsoluteY))),
            0x5D => Some(Self::Read(RI::ExclusiveOr(AM::AbsoluteX))),
            0x5E => Some(Self::Modify(MI::LogicalShiftRight(AM::AbsoluteX))),
            0x5F => Some(Self::Modify(MI::ShiftRightExclusiveOr(AM::AbsoluteX))),
            0x60 => Some(Self::ReturnFromSubroutine),
            0x61 => Some(Self::Read(RI::AddWithCarry(AM::IndirectX))),
            0x63 => Some(Self::Modify(MI::RotateRightAdd(AM::IndirectX))),
            0x65 => Some(Self::Read(RI::AddWithCarry(AM::ZeroPage))),
            0x66 => Some(Self::Modify(MI::RotateRight(AM::ZeroPage))),
            0x67 => Some(Self::Modify(MI::RotateRightAdd(AM::ZeroPage))),
            0x68 => Some(Self::PullStack(PushableRegister::A)),
            0x69 => Some(Self::Read(RI::AddWithCarry(AM::Immediate))),
            0x6A => Some(Self::Modify(MI::RotateRight(AM::Accumulator))),
            0x6B => Some(Self::Read(RI::AndWithRotateRight)),
            0x6C => Some(Self::Jump(AM::Indirect)),
            0x6D => Some(Self::Read(RI::AddWithCarry(AM::Absolute))),
            0x6E => Some(Self::Modify(MI::RotateRight(AM::Absolute))),
            0x6F => Some(Self::Modify(MI::RotateRightAdd(AM::Absolute))),
            0x70 => Some(Self::Branch(BranchCondition::OverflowSet)),
            0x71 => Some(Self::Read(RI::AddWithCarry(AM::IndirectY))),
            0x73 => Some(Self::Modify(MI::RotateRightAdd(AM::IndirectY))),
            0x75 => Some(Self::Read(RI::AddWithCarry(AM::ZeroPageX))),
            0x76 => Some(Self::Modify(MI::RotateRight(AM::ZeroPageX))),
            0x77 => Some(Self::Modify(MI::RotateRightAdd(AM::ZeroPageX))),
            0x78 => Some(Self::RegistersOnly(
                RegistersInstruction::SetInterruptDisable,
            )),
            0x79 => Some(Self::Read(RI::AddWithCarry(AM::AbsoluteY))),
            0x7B => Some(Self::Modify(MI::RotateRightAdd(AM::AbsoluteY))),
            0x7D => Some(Self::Read(RI::AddWithCarry(AM::AbsoluteX))),
            0x7E => Some(Self::Modify(MI::RotateRight(AM::AbsoluteX))),
            0x7F => Some(Self::Modify(MI::RotateRightAdd(AM::AbsoluteX))),
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => Some(Self::Read(RI::NoOp(AM::Immediate))),
            0x81 => Some(Self::StoreRegister(StorableRegister::A, AM::IndirectX)),
            0x83 => Some(Self::StoreRegister(StorableRegister::AX, AM::IndirectX)),
            0x84 => Some(Self::StoreRegister(StorableRegister::Y, AM::ZeroPage)),
            0x85 => Some(Self::StoreRegister(StorableRegister::A, AM::ZeroPage)),
            0x86 => Some(Self::StoreRegister(StorableRegister::X, AM::ZeroPage)),
            0x87 => Some(Self::StoreRegister(StorableRegister::AX, AM::ZeroPage)),
            0x88 => Some(Self::RegistersOnly(RegistersInstruction::Decrement(
                CpuRegister::Y,
            ))),
            0x8A => Some(Self::RegistersOnly(RegistersInstruction::Transfer {
                to: CpuRegister::A,
                from: CpuRegister::X,
            })),
            0x8B => Some(Self::Read(RI::LoadAndXImmediate)),
            0x8C => Some(Self::StoreRegister(StorableRegister::Y, AM::Absolute)),
            0x8D => Some(Self::StoreRegister(StorableRegister::A, AM::Absolute)),
            0x8E => Some(Self::StoreRegister(StorableRegister::X, AM::Absolute)),
            0x8F => Some(Self::StoreRegister(StorableRegister::AX, AM::Absolute)),
            0x90 => Some(Self::Branch(BranchCondition::CarryClear)),
            0x91 => Some(Self::StoreRegister(StorableRegister::A, AM::IndirectY)),
            0x94 => Some(Self::StoreRegister(StorableRegister::Y, AM::ZeroPageX)),
            0x95 => Some(Self::StoreRegister(StorableRegister::A, AM::ZeroPageX)),
            0x96 => Some(Self::StoreRegister(StorableRegister::X, AM::ZeroPageY)),
            0x97 => Some(Self::StoreRegister(StorableRegister::AX, AM::ZeroPageY)),
            0x98 => Some(Self::RegistersOnly(RegistersInstruction::Transfer {
                to: CpuRegister::A,
                from: CpuRegister::Y,
            })),
            0x99 => Some(Self::StoreRegister(StorableRegister::A, AM::AbsoluteY)),
            0x9A => Some(Self::RegistersOnly(RegistersInstruction::Transfer {
                to: CpuRegister::S,
                from: CpuRegister::X,
            })),
            0x9C => Some(Self::UnofficialStore(CpuRegister::Y)),
            0x9D => Some(Self::StoreRegister(StorableRegister::A, AM::AbsoluteX)),
            0x9E => Some(Self::UnofficialStore(CpuRegister::X)),
            0xA0 => Some(Self::Read(RI::Load(CpuRegister::Y, AM::Immediate))),
            0xA1 => Some(Self::Read(RI::Load(CpuRegister::A, AM::IndirectX))),
            0xA2 => Some(Self::Read(RI::Load(CpuRegister::X, AM::Immediate))),
            0xA3 => Some(Self::Read(RI::LoadTransferAX(AM::IndirectX))),
            0xA4 => Some(Self::Read(RI::Load(CpuRegister::Y, AM::ZeroPage))),
            0xA5 => Some(Self::Read(RI::Load(CpuRegister::A, AM::ZeroPage))),
            0xA6 => Some(Self::Read(RI::Load(CpuRegister::X, AM::ZeroPage))),
            0xA7 => Some(Self::Read(RI::LoadTransferAX(AM::ZeroPage))),
            0xA8 => Some(Self::RegistersOnly(RegistersInstruction::Transfer {
                to: CpuRegister::Y,
                from: CpuRegister::A,
            })),
            0xA9 => Some(Self::Read(RI::Load(CpuRegister::A, AM::Immediate))),
            0xAA => Some(Self::RegistersOnly(RegistersInstruction::Transfer {
                to: CpuRegister::X,
                from: CpuRegister::A,
            })),
            0xAB => Some(Self::Read(RI::LoadTransferAX(AM::Immediate))),
            0xAC => Some(Self::Read(RI::Load(CpuRegister::Y, AM::Absolute))),
            0xAD => Some(Self::Read(RI::Load(CpuRegister::A, AM::Absolute))),
            0xAE => Some(Self::Read(RI::Load(CpuRegister::X, AM::Absolute))),
            0xAF => Some(Self::Read(RI::LoadTransferAX(AM::Absolute))),
            0xB0 => Some(Self::Branch(BranchCondition::CarrySet)),
            0xB1 => Some(Self::Read(RI::Load(CpuRegister::A, AM::IndirectY))),
            0xB3 => Some(Self::Read(RI::LoadTransferAX(AM::IndirectY))),
            0xB4 => Some(Self::Read(RI::Load(CpuRegister::Y, AM::ZeroPageX))),
            0xB5 => Some(Self::Read(RI::Load(CpuRegister::A, AM::ZeroPageX))),
            0xB6 => Some(Self::Read(RI::Load(CpuRegister::X, AM::ZeroPageY))),
            0xB7 => Some(Self::Read(RI::LoadTransferAX(AM::ZeroPageY))),
            0xB8 => Some(Self::RegistersOnly(RegistersInstruction::ClearOverflowFlag)),
            0xB9 => Some(Self::Read(RI::Load(CpuRegister::A, AM::AbsoluteY))),
            0xBA => Some(Self::RegistersOnly(RegistersInstruction::Transfer {
                to: CpuRegister::X,
                from: CpuRegister::S,
            })),
            0xBC => Some(Self::Read(RI::Load(CpuRegister::Y, AM::AbsoluteX))),
            0xBD => Some(Self::Read(RI::Load(CpuRegister::A, AM::AbsoluteX))),
            0xBE => Some(Self::Read(RI::Load(CpuRegister::X, AM::AbsoluteY))),
            0xBF => Some(Self::Read(RI::LoadTransferAX(AM::AbsoluteY))),
            0xC0 => Some(Self::Read(RI::Compare(CpuRegister::Y, AM::Immediate))),
            0xC1 => Some(Self::Read(RI::Compare(CpuRegister::A, AM::IndirectX))),
            0xC3 => Some(Self::Modify(MI::DecrementCompare(AM::IndirectX))),
            0xC4 => Some(Self::Read(RI::Compare(CpuRegister::Y, AM::ZeroPage))),
            0xC5 => Some(Self::Read(RI::Compare(CpuRegister::A, AM::ZeroPage))),
            0xC6 => Some(Self::Modify(MI::Decrement(AM::ZeroPage))),
            0xC7 => Some(Self::Modify(MI::DecrementCompare(AM::ZeroPage))),
            0xC8 => Some(Self::RegistersOnly(RegistersInstruction::Increment(
                CpuRegister::Y,
            ))),
            0xC9 => Some(Self::Read(RI::Compare(CpuRegister::A, AM::Immediate))),
            0xCA => Some(Self::RegistersOnly(RegistersInstruction::Decrement(
                CpuRegister::X,
            ))),
            0xCB => Some(Self::Read(RI::AXSubtract)),
            0xCC => Some(Self::Read(RI::Compare(CpuRegister::Y, AM::Absolute))),
            0xCD => Some(Self::Read(RI::Compare(CpuRegister::A, AM::Absolute))),
            0xCE => Some(Self::Modify(MI::Decrement(AM::Absolute))),
            0xCF => Some(Self::Modify(MI::DecrementCompare(AM::Absolute))),
            0xD0 => Some(Self::Branch(BranchCondition::NotEqual)),
            0xD1 => Some(Self::Read(RI::Compare(CpuRegister::A, AM::IndirectY))),
            0xD3 => Some(Self::Modify(MI::DecrementCompare(AM::IndirectY))),
            0xD5 => Some(Self::Read(RI::Compare(CpuRegister::A, AM::ZeroPageX))),
            0xD6 => Some(Self::Modify(MI::Decrement(AM::ZeroPageX))),
            0xD7 => Some(Self::Modify(MI::DecrementCompare(AM::ZeroPageX))),
            0xD8 => Some(Self::RegistersOnly(RegistersInstruction::ClearDecimalFlag)),
            0xD9 => Some(Self::Read(RI::Compare(CpuRegister::A, AM::AbsoluteY))),
            0xDB => Some(Self::Modify(MI::DecrementCompare(AM::AbsoluteY))),
            0xDD => Some(Self::Read(RI::Compare(CpuRegister::A, AM::AbsoluteX))),
            0xDE => Some(Self::Modify(MI::Decrement(AM::AbsoluteX))),
            0xDF => Some(Self::Modify(MI::DecrementCompare(AM::AbsoluteX))),
            0xE0 => Some(Self::Read(RI::Compare(CpuRegister::X, AM::Immediate))),
            0xE1 => Some(Self::Read(RI::SubtractWithCarry(AM::IndirectX))),
            0xE3 => Some(Self::Modify(MI::IncrementSubtract(AM::IndirectX))),
            0xE4 => Some(Self::Read(RI::Compare(CpuRegister::X, AM::ZeroPage))),
            0xE5 => Some(Self::Read(RI::SubtractWithCarry(AM::ZeroPage))),
            0xE6 => Some(Self::Modify(MI::Increment(AM::ZeroPage))),
            0xE7 => Some(Self::Modify(MI::IncrementSubtract(AM::ZeroPage))),
            0xE8 => Some(Self::RegistersOnly(RegistersInstruction::Increment(
                CpuRegister::X,
            ))),
            0xE9 | 0xEB => Some(Self::Read(RI::SubtractWithCarry(AM::Immediate))),
            0xEC => Some(Self::Read(RI::Compare(CpuRegister::X, AM::Absolute))),
            0xED => Some(Self::Read(RI::SubtractWithCarry(AM::Absolute))),
            0xEE => Some(Self::Modify(MI::Increment(AM::Absolute))),
            0xEF => Some(Self::Modify(MI::IncrementSubtract(AM::Absolute))),
            0xF0 => Some(Self::Branch(BranchCondition::Equal)),
            0xF1 => Some(Self::Read(RI::SubtractWithCarry(AM::IndirectY))),
            0xF3 => Some(Self::Modify(MI::IncrementSubtract(AM::IndirectY))),
            0xF5 => Some(Self::Read(RI::SubtractWithCarry(AM::ZeroPageX))),
            0xF6 => Some(Self::Modify(MI::Increment(AM::ZeroPageX))),
            0xF7 => Some(Self::Modify(MI::IncrementSubtract(AM::ZeroPageX))),
            0xF8 => Some(Self::RegistersOnly(RegistersInstruction::SetDecimalFlag)),
            0xF9 => Some(Self::Read(RI::SubtractWithCarry(AM::AbsoluteY))),
            0xFB => Some(Self::Modify(MI::IncrementSubtract(AM::AbsoluteY))),
            0xFD => Some(Self::Read(RI::SubtractWithCarry(AM::AbsoluteX))),
            0xFE => Some(Self::Modify(MI::Increment(AM::AbsoluteX))),
            0xFF => Some(Self::Modify(MI::IncrementSubtract(AM::AbsoluteX))),
            _ => {
                // Unused or unofficial opcode
                None
            }
        }
    }
}

pub const INTERRUPT_HANDLER_OPS: [CycleOp; 6] = [
    CycleOp::SpuriousOperandRead,
    CycleOp::PushPCHigh,
    CycleOp::PushPCLow,
    CycleOp::InterruptPushStatus(StatusReadContext::HardwareInterruptHandler),
    CycleOp::InterruptPullPCLow,
    CycleOp::InterruptPullPCHigh,
];

fn get_read_cycle_ops(instruction: ReadInstruction) -> OpVec {
    match instruction.addressing_mode() {
        AddressingMode::Immediate => [CycleOp::ExecuteImmediateRead(instruction)]
            .into_iter()
            .collect(),
        AddressingMode::ZeroPage => [
            CycleOp::FetchOperand1,
            CycleOp::ExecuteZeroPageRead(instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::ZeroPageX => [
            CycleOp::FetchOperand1,
            CycleOp::ZeroPageIndexAddress,
            CycleOp::ExecuteZeroPageIndexedRead(Index::X, instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::ZeroPageY => [
            CycleOp::FetchOperand1,
            CycleOp::ZeroPageIndexAddress,
            CycleOp::ExecuteZeroPageIndexedRead(Index::Y, instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::Absolute => [
            CycleOp::FetchOperand1,
            CycleOp::FetchOperand2,
            CycleOp::ExecuteAbsoluteRead(instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::AbsoluteX => [
            CycleOp::FetchOperand1,
            CycleOp::FetchOperand2,
            CycleOp::ExecuteAbsoluteIndexedRead(Index::X, instruction),
            CycleOp::ExecuteAbsoluteIndexedReadDelayed(Index::X, instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::AbsoluteY => [
            CycleOp::FetchOperand1,
            CycleOp::FetchOperand2,
            CycleOp::ExecuteAbsoluteIndexedRead(Index::Y, instruction),
            CycleOp::ExecuteAbsoluteIndexedReadDelayed(Index::Y, instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::IndirectX => [
            CycleOp::FetchOperand1,
            CycleOp::ZeroPageIndexAddress,
            CycleOp::FetchZeroPageIndexed1,
            CycleOp::FetchZeroPageIndexed2,
            CycleOp::ExecuteIndexedIndirectRead(instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::IndirectY => [
            CycleOp::FetchOperand1,
            CycleOp::FetchZeroPage1,
            CycleOp::FetchZeroPage2,
            CycleOp::ExecuteIndirectIndexedRead(instruction),
            CycleOp::ExecuteIndirectIndexedReadDelayed(instruction),
        ]
        .into_iter()
        .collect(),
        _ => panic!("unsupported addressing mode for a read instruction: {instruction:?}"),
    }
}

fn get_store_cycle_ops(register: StorableRegister, addressing_mode: AddressingMode) -> OpVec {
    match addressing_mode {
        AddressingMode::ZeroPage => [
            CycleOp::FetchOperand1,
            CycleOp::ExecuteZeroPageStore(register),
        ]
        .into_iter()
        .collect(),
        AddressingMode::ZeroPageX => [
            CycleOp::FetchOperand1,
            CycleOp::ZeroPageIndexAddress,
            CycleOp::ExecuteZeroPageIndexedStore(Index::X, register),
        ]
        .into_iter()
        .collect(),
        AddressingMode::ZeroPageY => [
            CycleOp::FetchOperand1,
            CycleOp::ZeroPageIndexAddress,
            CycleOp::ExecuteZeroPageIndexedStore(Index::Y, register),
        ]
        .into_iter()
        .collect(),
        AddressingMode::Absolute => [
            CycleOp::FetchOperand1,
            CycleOp::FetchOperand2,
            CycleOp::ExecuteAbsoluteStore(register),
        ]
        .into_iter()
        .collect(),
        AddressingMode::AbsoluteX => [
            CycleOp::FetchOperand1,
            CycleOp::FetchOperand2,
            CycleOp::AbsoluteIndexedFixHighByte(Index::X),
            CycleOp::ExecuteAbsoluteIndexedStore(Index::X, register),
        ]
        .into_iter()
        .collect(),
        AddressingMode::AbsoluteY => [
            CycleOp::FetchOperand1,
            CycleOp::FetchOperand2,
            CycleOp::AbsoluteIndexedFixHighByte(Index::Y),
            CycleOp::ExecuteAbsoluteIndexedStore(Index::Y, register),
        ]
        .into_iter()
        .collect(),
        AddressingMode::IndirectX => [
            CycleOp::FetchOperand1,
            CycleOp::ZeroPageIndexAddress,
            CycleOp::FetchZeroPageIndexed1,
            CycleOp::FetchZeroPageIndexed2,
            CycleOp::ExecuteIndexedIndirectStore(register),
        ]
        .into_iter()
        .collect(),
        AddressingMode::IndirectY => [
            CycleOp::FetchOperand1,
            CycleOp::FetchZeroPage1,
            CycleOp::FetchZeroPage2,
            CycleOp::IndirectIndexedFixHighByte,
            CycleOp::ExecuteIndirectIndexedStore(register),
        ]
        .into_iter()
        .collect(),
        _ => panic!("unsupported addressing mode for StoreRegister: {addressing_mode:?}"),
    }
}

fn get_modify_cycle_ops(instruction: ModifyInstruction) -> OpVec {
    match instruction.addressing_mode() {
        AddressingMode::Accumulator => [CycleOp::ExecuteAccumulatorModify(instruction)]
            .into_iter()
            .collect(),
        AddressingMode::ZeroPage => [
            CycleOp::FetchOperand1,
            CycleOp::FetchZeroPage1,
            CycleOp::ZeroPageWriteBack,
            CycleOp::ExecuteZeroPageModify(instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::ZeroPageX => [
            CycleOp::FetchOperand1,
            CycleOp::FetchZeroPage1,
            CycleOp::FetchZeroPageIndexed1,
            CycleOp::ZeroPageIndexedWriteBack,
            CycleOp::ExecuteZeroPageIndexedModify(instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::Absolute => [
            CycleOp::FetchOperand1,
            CycleOp::FetchOperand2,
            CycleOp::FetchAbsolute,
            CycleOp::AbsoluteWriteBack,
            CycleOp::ExecuteAbsoluteModify(instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::AbsoluteX => absolute_indexed_modify_cycle_ops(Index::X, instruction),
        AddressingMode::AbsoluteY => absolute_indexed_modify_cycle_ops(Index::Y, instruction),
        AddressingMode::IndirectX => [
            CycleOp::FetchOperand1,
            CycleOp::ZeroPageIndexAddress,
            CycleOp::FetchZeroPageIndexed1,
            CycleOp::FetchZeroPageIndexed2,
            CycleOp::FetchIndexedIndirect,
            CycleOp::IndexedIndirectWriteBack,
            CycleOp::ExecuteIndexedIndirectModify(instruction),
        ]
        .into_iter()
        .collect(),
        AddressingMode::IndirectY => [
            CycleOp::FetchOperand1,
            CycleOp::FetchZeroPage1,
            CycleOp::FetchZeroPage2,
            CycleOp::IndirectIndexedFixHighByte,
            CycleOp::FetchIndirectIndexed,
            CycleOp::IndirectIndexedWriteBack,
            CycleOp::ExecuteIndirectIndexedModify(instruction),
        ]
        .into_iter()
        .collect(),
        _ => {
            panic!("unsupported addressing mode for read-modify-write instruction: {instruction:?}")
        }
    }
}

fn absolute_indexed_modify_cycle_ops(index: Index, instruction: ModifyInstruction) -> OpVec {
    [
        CycleOp::FetchOperand1,
        CycleOp::FetchOperand2,
        CycleOp::AbsoluteIndexedFixHighByte(index),
        CycleOp::FetchAbsoluteIndexed(index),
        CycleOp::AbsoluteIndexedWriteBack(index),
        CycleOp::ExecuteAbsoluteIndexedModify(index, instruction),
    ]
    .into_iter()
    .collect()
}

fn read_register(registers: &CpuRegisters, register: CpuRegister) -> u8 {
    match register {
        CpuRegister::A => registers.accumulator,
        CpuRegister::X => registers.x,
        CpuRegister::Y => registers.y,
        CpuRegister::S => registers.sp,
    }
}

fn write_register(registers: &mut CpuRegisters, register: CpuRegister, value: u8) {
    let field = match register {
        CpuRegister::A => &mut registers.accumulator,
        CpuRegister::X => &mut registers.x,
        CpuRegister::Y => &mut registers.y,
        CpuRegister::S => &mut registers.sp,
    };
    *field = value;
}

fn add(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let existing_carry = flags.carry;

    let (result, new_carry) = match accumulator.overflowing_add(value) {
        (sum, true) => (sum + u8::from(existing_carry), true),
        (sum, false) => sum.overflowing_add(u8::from(existing_carry)),
    };

    let bit_6_carry = (accumulator & 0x7F) + (value & 0x7F) + u8::from(existing_carry) >= 0x80;
    let overflow = new_carry ^ bit_6_carry;

    flags
        .set_negative(result.bit(7))
        .set_overflow(overflow)
        .set_zero(result == 0)
        .set_carry(new_carry);

    result
}

fn subtract(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    // Carry flag is inverted in subtraction
    let existing_borrow = u8::from(!flags.carry);

    let (result, borrowed) = match accumulator.overflowing_sub(value) {
        (difference, true) => (difference - existing_borrow, true),
        (difference, false) => difference.overflowing_sub(existing_borrow),
    };

    let bit_6_borrowed = accumulator & 0x7F < (value & 0x7F) + existing_borrow;
    let overflow = borrowed ^ bit_6_borrowed;

    flags
        .set_negative(result.bit(7))
        .set_overflow(overflow)
        .set_zero(result == 0)
        .set_carry(!borrowed);

    result
}

fn and(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator & value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

fn or(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator | value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

fn xor(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator ^ value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

fn compare(register: u8, value: u8, flags: &mut StatusFlags) {
    flags
        .set_negative(register.wrapping_sub(value).bit(7))
        .set_zero(register == value)
        .set_carry(register >= value);
}

fn bit_test(accumulator: u8, value: u8, flags: &mut StatusFlags) {
    flags
        .set_negative(value.bit(7))
        .set_overflow(value.bit(6))
        .set_zero(accumulator & value == 0);
}

fn increment(value: u8, flags: &mut StatusFlags) -> u8 {
    let incremented = value.wrapping_add(1);
    flags
        .set_negative(incremented.bit(7))
        .set_zero(incremented == 0);
    incremented
}

fn decrement(value: u8, flags: &mut StatusFlags) -> u8 {
    let decremented = value.wrapping_sub(1);
    flags
        .set_negative(decremented.bit(7))
        .set_zero(decremented == 0);
    decremented
}

fn shift_left(value: u8, flags: &mut StatusFlags) -> u8 {
    let shifted = value << 1;
    flags
        .set_carry(value.bit(7))
        .set_negative(shifted.bit(7))
        .set_zero(shifted == 0);
    shifted
}

fn logical_shift_right(value: u8, flags: &mut StatusFlags) -> u8 {
    let shifted = value >> 1;
    flags
        .set_carry(value.bit(0))
        .set_negative(false)
        .set_zero(shifted == 0);
    shifted
}

fn rotate_left(value: u8, flags: &mut StatusFlags) -> u8 {
    let rotated = (value << 1) | u8::from(flags.carry);
    flags
        .set_carry(value.bit(7))
        .set_negative(rotated.bit(7))
        .set_zero(rotated == 0);
    rotated
}

fn rotate_right(value: u8, flags: &mut StatusFlags) -> u8 {
    let rotated = (value >> 1) | (u8::from(flags.carry) << 7);
    flags
        .set_carry(value.bit(0))
        .set_negative(rotated.bit(7))
        .set_zero(rotated == 0);
    rotated
}
