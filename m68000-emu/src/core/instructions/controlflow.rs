use crate::core::instructions::{BranchCondition, Instruction};
use crate::core::{
    AddressRegister, AddressingMode, BusOpType, ConditionCodes, DataRegister, Exception,
    ExecuteResult, InstructionExecutor, OpSize, ResolvedAddress,
};
use crate::traits::BusInterface;

const TRAP_VECTOR_OFFSET: u32 = 32;
const OVERFLOW_VECTOR: u32 = 7;

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    fn resolve_to_memory_address(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let resolved = self.resolve_address(source, OpSize::LongWord)?;
        let ResolvedAddress::Memory(address) = resolved else {
            panic!("effective address operations should only accept addressing modes that resolve to an effective address");
        };

        Ok(address)
    }

    fn check_jump_address(&mut self, address: u32) -> ExecuteResult<u32> {
        if address % 2 != 0 {
            self.registers.pc = address.wrapping_sub(2);
            return Err(Exception::AddressError(address, BusOpType::Jump));
        }

        Ok(address)
    }

    pub(super) fn lea(
        &mut self,
        source: AddressingMode,
        register: AddressRegister,
    ) -> ExecuteResult<u32> {
        let address = self.resolve_to_memory_address(source)?;
        register.write_long_word_to(self.registers, address);

        Ok(effective_address_cycles(source))
    }

    pub(super) fn pea(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let address = self.resolve_to_memory_address(source)?;
        self.push_stack_u32(address)?;

        Ok(8 + effective_address_cycles(source))
    }

    pub(super) fn jmp(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let address = self.resolve_to_memory_address(source)?;
        self.registers.pc = self.check_jump_address(address)?;

        Ok(jump_cycles(source))
    }

    pub(super) fn jsr(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let address = self.resolve_to_memory_address(source)?;
        let old_pc = self.registers.pc;
        self.registers.pc = self.check_jump_address(address)?;
        self.push_stack_u32(old_pc)?;

        Ok(8 + jump_cycles(source))
    }

    pub(super) fn link(&mut self, register: AddressRegister) -> ExecuteResult<u32> {
        let extension = self.fetch_operand()?;
        let displacement = extension as i16;

        if register.is_stack_pointer() {
            self.push_stack_u32(self.registers.sp().wrapping_sub(4))?;
        } else {
            self.push_stack_u32(register.read_from(self.registers))?;
        }

        let sp = self.registers.sp();
        register.write_long_word_to(self.registers, sp);
        self.registers.set_sp(sp.wrapping_add(displacement as u32));

        Ok(16)
    }

    pub(super) fn unlk(&mut self, register: AddressRegister) -> ExecuteResult<u32> {
        self.registers.set_sp(register.read_from(self.registers));

        let address = self.pop_stack_u32()?;
        register.write_long_word_to(self.registers, address);

        Ok(12)
    }

    pub(super) fn ret(&mut self, restore_ccr: bool) -> ExecuteResult<u32> {
        if restore_ccr {
            let word = self.pop_stack_u16()?;
            self.registers.ccr = (word as u8).into();
        }

        let pc = self.pop_stack_u32()?;
        self.registers.pc = self.check_jump_address(pc)?;

        Ok(if restore_ccr { 20 } else { 16 })
    }

    pub(super) fn rte(&mut self) -> ExecuteResult<u32> {
        let sr = self.pop_stack_u16()?;

        let pc = self.pop_stack_u32()?;
        self.registers.set_status_register(sr);
        self.registers.pc = self.check_jump_address(pc)?;

        Ok(20)
    }

    pub(super) fn trapv(&self) -> ExecuteResult<u32> {
        if self.registers.ccr.overflow {
            Err(Exception::Trap(OVERFLOW_VECTOR))
        } else {
            Ok(4)
        }
    }

    pub(super) fn chk(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<u32> {
        let upper_bound = self.read_word(source)? as i16;

        let value = register.read_from(self.registers) as i16;

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: false,
            ..self.registers.ccr
        };

        let address_cycles = source.address_calculation_cycles(OpSize::Word);

        if value > upper_bound {
            self.registers.ccr.negative = value < 0;
            Err(Exception::CheckRegister {
                cycles: address_cycles + 8,
            })
        } else if value < 0 {
            self.registers.ccr.negative = true;
            Err(Exception::CheckRegister {
                cycles: address_cycles + 10,
            })
        } else {
            Ok(address_cycles + 10)
        }
    }

    fn fetch_branch_displacement(&mut self, displacement: i8) -> ExecuteResult<(i16, bool)> {
        Ok(if displacement == 0 {
            let extension = self.fetch_operand()?;
            (extension as i16, true)
        } else {
            (displacement.into(), false)
        })
    }

    pub(super) fn branch(
        &mut self,
        condition: BranchCondition,
        displacement: i8,
    ) -> ExecuteResult<u32> {
        let pc = self.registers.pc;
        let (displacement, fetched_extension) = self.fetch_branch_displacement(displacement)?;

        if condition.check(self.registers.ccr) {
            let address = pc.wrapping_add(displacement as u32);
            self.registers.pc = self.check_jump_address(address)?;

            Ok(10)
        } else if fetched_extension {
            Ok(12)
        } else {
            Ok(8)
        }
    }

    pub(super) fn bsr(&mut self, displacement: i8) -> ExecuteResult<u32> {
        let pc = self.registers.pc;
        let (displacement, _) = self.fetch_branch_displacement(displacement)?;

        self.push_stack_u32(self.registers.pc)?;

        let address = pc.wrapping_add(displacement as u32);
        self.registers.pc = self.check_jump_address(address)?;

        Ok(18)
    }

    pub(super) fn dbcc(
        &mut self,
        condition: BranchCondition,
        register: DataRegister,
    ) -> ExecuteResult<u32> {
        let pc = self.registers.pc;
        let displacement = self.fetch_operand()? as i16;

        if !condition.check(self.registers.ccr) {
            let value = register.read_from(self.registers) as u16;
            register.write_word_to(self.registers, value.wrapping_sub(1));

            if value != 0 {
                let address = pc.wrapping_add(displacement as u32);
                self.registers.pc = self.check_jump_address(address)?;

                Ok(10)
            } else {
                Ok(14)
            }
        } else {
            Ok(12)
        }
    }

    pub(super) fn scc(
        &mut self,
        condition: BranchCondition,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let cc = condition.check(self.registers.ccr);
        let value = if cc { 0xFF } else { 0x00 };

        self.write_byte(dest, value)?;

        Ok(if dest.is_data_direct() {
            4 + if cc { 2 } else { 0 }
        } else {
            8 + dest.address_calculation_cycles(OpSize::Byte)
        })
    }
}

fn jump_cycles(addressing_mode: AddressingMode) -> u32 {
    match addressing_mode {
        AddressingMode::AddressIndirect(..) => 8,
        AddressingMode::AddressIndirectDisplacement(..)
        | AddressingMode::PcRelativeDisplacement
        | AddressingMode::AbsoluteShort => 10,
        AddressingMode::AddressIndirectIndexed(..) | AddressingMode::PcRelativeIndexed => 14,
        AddressingMode::AbsoluteLong => 12,
        _ => panic!("invalid jump addressing mode: {addressing_mode:?}"),
    }
}

fn effective_address_cycles(addressing_mode: AddressingMode) -> u32 {
    match addressing_mode {
        AddressingMode::AddressIndirectIndexed(..) | AddressingMode::PcRelativeIndexed => 12,
        _ => addressing_mode.address_calculation_cycles(OpSize::Byte),
    }
}

pub(super) const fn nop() -> u32 {
    4
}

pub(super) fn trap(vector: u32) -> ExecuteResult<u32> {
    Err(Exception::Trap(TRAP_VECTOR_OFFSET + vector))
}

fn is_effective_address_mode(addressing_mode: AddressingMode) -> bool {
    !matches!(
        addressing_mode,
        AddressingMode::DataDirect(..)
            | AddressingMode::AddressDirect(..)
            | AddressingMode::AddressIndirectPostincrement(..)
            | AddressingMode::AddressIndirectPredecrement(..)
            | AddressingMode::Immediate
    )
}

pub(super) fn decode_jmp(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    if !is_effective_address_mode(addressing_mode) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Jump(addressing_mode))
}

pub(super) fn decode_lea(opcode: u16) -> ExecuteResult<Instruction> {
    let source = AddressingMode::parse_from_opcode(opcode)?;
    let register = ((opcode >> 9) & 0x07) as u8;

    if !is_effective_address_mode(source) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::LoadEffectiveAddress(source, register.into()))
}

pub(super) fn decode_pea(opcode: u16) -> ExecuteResult<Instruction> {
    let source = AddressingMode::parse_from_opcode(opcode)?;

    if !is_effective_address_mode(source) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::PushEffectiveAddress(source))
}

pub(super) fn decode_jsr(opcode: u16) -> ExecuteResult<Instruction> {
    let source = AddressingMode::parse_from_opcode(opcode)?;

    if !is_effective_address_mode(source) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::JumpToSubroutine(source))
}

pub(super) fn decode_link(opcode: u16) -> Instruction {
    let register = (opcode & 0x07) as u8;
    Instruction::Link(register.into())
}

pub(super) fn decode_unlk(opcode: u16) -> Instruction {
    let register = (opcode & 0x07) as u8;
    Instruction::Unlink(register.into())
}

pub(super) fn decode_rte(opcode: u16, supervisor_mode: bool) -> ExecuteResult<Instruction> {
    if !supervisor_mode {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::ReturnFromException)
}

pub(super) fn decode_trap(opcode: u16) -> Instruction {
    let vector = opcode & 0x000F;

    Instruction::Trap(vector.into())
}

pub(super) fn decode_chk(opcode: u16) -> ExecuteResult<Instruction> {
    let source = AddressingMode::parse_from_opcode(opcode)?;
    let register = ((opcode >> 9) & 0x07) as u8;

    Ok(Instruction::CheckRegister(register.into(), source))
}

pub(super) fn decode_branch(opcode: u16) -> Instruction {
    let condition = BranchCondition::parse_from_opcode(opcode);
    let displacement = opcode as i8;

    match condition {
        BranchCondition::False => Instruction::BranchToSubroutine(displacement),
        _ => Instruction::Branch(condition, displacement),
    }
}

pub(super) fn decode_dbcc(opcode: u16) -> Instruction {
    let condition = BranchCondition::parse_from_opcode(opcode);
    let register = (opcode & 0x07) as u8;

    Instruction::BranchDecrement(condition, register.into())
}

pub(super) fn decode_scc(opcode: u16) -> ExecuteResult<Instruction> {
    let condition = BranchCondition::parse_from_opcode(opcode);
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    Ok(Instruction::Set(condition, addressing_mode))
}
