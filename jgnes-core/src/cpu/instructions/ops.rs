use crate::bus;
use crate::bus::CpuBus;
use crate::cpu::instructions::{
    BranchCondition, CpuRegister, InstructionState, ModifyInstruction, PushableRegister,
    ReadInstruction, RegistersInstruction, StorableRegister,
};
use crate::cpu::{instructions, CpuRegisters, StatusFlags, StatusReadContext};
use crate::num::GetBit;
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

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
    // Read the first byte after the opcode and increment PC
    FetchOperand1,
    // Read the second byte after the opcode and increment PC
    FetchOperand2,
    // Read the byte at PC without incrementing
    SpuriousOperandRead,
    // Read the current stack byte without decrementing S
    SpuriousStackRead,
    // Treat the first operand byte as a zero page address and read it without doing anything with it
    ZeroPageIndexAddress,
    // Treat the first operand byte as a zero page address and read it into state
    FetchZeroPage1,
    // Treat the first operand byte as a zero page address and read the following byte into state
    FetchZeroPage2,
    // Treat the first operand byte as a zero page address, index with X, and read it into state
    FetchZeroPageIndexed1,
    // Treat the first operand byte as a zero page address, index with X, and read the following byte into state
    FetchZeroPageIndexed2,
    // Treat the two operand bytes as an absolute address and read that byte into state
    FetchAbsolute,
    // Treat the two operand bytes as an absolute address, index with X/Y, and read that byte into state
    FetchAbsoluteIndexed(Index),
    // Treat the two target bytes as an absolute address and read that byte into state (as indirect_byte)
    FetchIndexedIndirect,
    // Treat the two target bytes as an absolute address, index with Y, and read that byte into state (as indirect_byte)
    FetchIndirectIndexed,
    // Treat the first operand byte as a zero page address and write the first target byte back to that address
    ZeroPageWriteBack,
    // Treat the first operand byte as a zero page address, index with X, and write the first target byte back to that address
    ZeroPageIndexedWriteBack,
    // Treat the two operand bytes as an absolute address and write the first target byte back to that address
    AbsoluteWriteBack,
    // Treat the two operand bytes as an absolute address, index with X/Y, and write the first target byte back to that address
    AbsoluteIndexedWriteBack(Index),
    // Treat the two target bytes as an absolute address and write indirect_byte back to that address
    IndexedIndirectWriteBack,
    // Treat the two target bytes as an absolute address, index with Y, and write indirect_byte back to that address
    IndirectIndexedWriteBack,
    // Treat the two operand bytes as an absolute address, index with X/Y, and read from that address *without*
    // carrying overflow from the lower byte into the higher byte
    AbsoluteIndexedFixHighByte(Index),
    // Treat the two target bytes as an absolute address, index with Y, and read from that address *without*
    // carrying overflow from the lower byte into the higher byte
    IndirectIndexedFixHighByte,
    // Execute an instruction that operates exclusively on CPU registers
    ExecuteRegistersOnly(RegistersInstruction),
    // Execute a read-modify-write instruction with Accumulator addressing mode
    ExecuteAccumulatorModify(ModifyInstruction),
    // Execute a read instruction with Immediate addressing mode
    ExecuteImmediateRead(ReadInstruction),
    // Execute a read instruction with Zero Page addressing mode
    ExecuteZeroPageRead(ReadInstruction),
    // Execute a store instruction with Zero Page addressing mode
    ExecuteZeroPageStore(StorableRegister),
    // Execute a read-modify-write instruction with Zero Page addressing mode
    ExecuteZeroPageModify(ModifyInstruction),
    // Execute a read instruction with Zero Page, X/Y addressing mode
    ExecuteZeroPageIndexedRead(Index, ReadInstruction),
    // Execute a store instruction with Zero Page, X/Y addressing mode
    ExecuteZeroPageIndexedStore(Index, StorableRegister),
    // Execute a read-modify-write instruction with Zero Page, X addressing mode
    ExecuteZeroPageIndexedModify(ModifyInstruction),
    // Execute a read instruction with Absolute addressing mode
    ExecuteAbsoluteRead(ReadInstruction),
    // Execute a store instruction with Absolute addressing mode
    ExecuteAbsoluteStore(StorableRegister),
    // Execute a read-modify-write instruction with Absolute addressing mode
    ExecuteAbsoluteModify(ModifyInstruction),
    // Execute a read instruction with Absolute, X/Y addressing mode
    ExecuteAbsoluteIndexedRead(Index, ReadInstruction),
    // Execute a read instruction with Absolute, X/Y addressing mode; this is an extra cycle that
    // only executes if the high byte of the address incremented during indexing
    ExecuteAbsoluteIndexedReadDelayed(Index, ReadInstruction),
    // Execute a store instruction with Absolute, X/Y addressing mode
    ExecuteAbsoluteIndexedStore(Index, StorableRegister),
    // Execute a read-modify-write instruction with Absolute, X/Y addressing mode
    ExecuteAbsoluteIndexedModify(Index, ModifyInstruction),
    // Execute a read instruction with Indirect, X addressing mode
    ExecuteIndexedIndirectRead(ReadInstruction),
    // Execute a store instruction with Indirect, X addressing mode
    ExecuteIndexedIndirectStore(StorableRegister),
    // Execute a read-modify-write instruction with Indirect, X addressing mode
    ExecuteIndexedIndirectModify(ModifyInstruction),
    // Execute a read instruction with Indirect, Y addressing mode
    ExecuteIndirectIndexedRead(ReadInstruction),
    // Execute a read instruction with Indirect, Y addressing mode; this is an extra cycle that
    // only executes if the high byte of the address incremented during indexing
    ExecuteIndirectIndexedReadDelayed(ReadInstruction),
    // Execute a store instruction with Indirect, Y addressing mode
    ExecuteIndirectIndexedStore(StorableRegister),
    // Execute a read-modify-write instruction with Indirect, Y addressing mode
    ExecuteIndirectIndexedModify(ModifyInstruction),
    // Check whether the branch condition is true; if false, end the branch instruction here
    CheckBranchCondition(BranchCondition),
    // Update the low byte of PC during a branch; if high byte does not need to be updated, end the branch instruction here
    CheckBranchHighByte,
    // Update the high byte of PC during a branch (only executed if it needs updating)
    FixBranchHighByte,
    // Execute a jump instruction with Absolute addressing mode
    ExecuteJumpAbsolute,
    // Execute a jump instruction with Indirect addressing mode
    ExecuteJumpIndirect,
    // Push the given register onto the stack
    ExecutePush(PushableRegister),
    // Pull from the stack into the given register
    ExecutePull(PushableRegister),
    // Push the high byte of PC onto the stack
    PushPCHigh,
    // Push the low byte of PC onto the stack
    PushPCLow,
    // Pull from the stack into the low byte of PC
    PullPCLow,
    // Pull from the stack into the high byte of PC
    PullPCHigh,
    // Push P onto the stack, and lock in the interrupt vector to jump to (either NMI or IRQ)
    InterruptPushStatus(StatusReadContext),
    // Disable IRQ interrupts and pull from the interrupt vector into the low byte of PC
    InterruptPullPCLow,
    // Pull from the second byte of the interrupt vector into the high byte of PC
    InterruptPullPCHigh,
    // Execute one of the buggy unofficial store instructions ($9C / $9E)
    ExecuteUnofficialStore(Index, CpuRegister),
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
        state: &mut InstructionState,
        registers: &mut CpuRegisters,
        bus: &mut CpuBus<'_>,
    ) {
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
                execute_absolute_indexed_read(state, registers, bus, instruction, index);
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
                execute_indirect_indexed_read(state, registers, bus, instruction);
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
                check_branch_high_byte(state, registers, bus);
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
                execute_pull(registers, bus, register);
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
                interrupt_push_status(state, registers, bus, read_ctx);
            }
            Self::InterruptPullPCLow => {
                registers.pc = u16::from(bus.read_address(state.interrupt_vector));
                registers.status.interrupt_disable = true;
            }
            Self::InterruptPullPCHigh => {
                let pc_msb = bus.read_address(state.interrupt_vector + 1);
                registers.pc |= u16::from(pc_msb) << 8;
            }
            Self::ExecuteUnofficialStore(index, register) => {
                execute_unofficial_store(state, registers, bus, register, index);
            }
        }

        state.op_index += 1;
    }
}

fn execute_absolute_indexed_read(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
    instruction: ReadInstruction,
    index: Index,
) {
    let index = index.get(registers);
    let (indexed_low_byte, overflowed) = state.operand_first_byte.overflowing_add(index);

    let address = u16::from_le_bytes([indexed_low_byte, state.operand_second_byte]);
    let value = bus.read_address(address);

    if !overflowed {
        instruction.execute(value, registers);

        // Skip next (last) cycle
        state.op_index += 1;
    }
}

fn execute_indirect_indexed_read(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
    instruction: ReadInstruction,
) {
    let (indexed_low_byte, overflowed) = state.target_first_byte.overflowing_add(registers.y);
    let address = u16::from_le_bytes([indexed_low_byte, state.target_second_byte]);
    let value = bus.read_address(address);

    if !overflowed {
        instruction.execute(value, registers);

        // Skip next (last) cycle
        state.op_index += 1;
    }
}

fn check_branch_high_byte(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    // Spurious read when branch is taken
    bus.read_address(registers.pc);

    let offset = state.operand_first_byte as i8;
    let new_pc = (i32::from(registers.pc) + i32::from(offset)) as u16;

    if registers.pc & 0xFF00 == new_pc & 0xFF00 {
        // Skip last branch cycle
        registers.pc = new_pc;
        state.op_index += 1;
    }
}

fn execute_pull(registers: &mut CpuRegisters, bus: &mut CpuBus<'_>, register: PushableRegister) {
    registers.sp = registers.sp.wrapping_add(1);
    let value = bus.read_address(u16::from_be_bytes([0x01, registers.sp]));

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

fn interrupt_push_status(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
    read_ctx: StatusReadContext,
) {
    let stack_address = u16::from_be_bytes([0x01, registers.sp]);
    bus.write_address(stack_address, registers.status.to_byte(read_ctx));
    registers.sp = registers.sp.wrapping_sub(1);

    state.interrupt_vector = if bus.interrupt_lines().nmi_triggered() {
        bus.interrupt_lines().clear_nmi_triggered();
        bus::CPU_NMI_VECTOR
    } else {
        bus::CPU_IRQ_VECTOR
    };
}

fn execute_unofficial_store(
    state: &InstructionState,
    registers: &CpuRegisters,
    bus: &mut CpuBus<'_>,
    register: CpuRegister,
    index: Index,
) {
    // This is a buggy instruction that is only implemented because CPU test ROMs test
    // it.
    // This implementation ANDs the X/Y register with the high byte of the address plus 1
    // and then stores that value, but only if the indexing did not overflow.

    let index = index.get(registers);
    let address = u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte])
        .wrapping_add(index.into());
    let (_, overflowed) = state.operand_first_byte.overflowing_add(index);

    let value = instructions::read_register(registers, register)
        & state.operand_second_byte.wrapping_add(1);

    if !overflowed {
        bus.write_address(address, value);
    }
}
