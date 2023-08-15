use crate::core::instructions::Instruction;
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
        let ResolvedAddress::Memory(address) = resolved
        else {
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
    ) -> ExecuteResult<()> {
        let address = self.resolve_to_memory_address(source)?;
        register.write_long_word_to(self.registers, address);

        Ok(())
    }

    pub(super) fn pea(&mut self, source: AddressingMode) -> ExecuteResult<()> {
        let address = self.resolve_to_memory_address(source)?;
        self.push_stack_u32(address)?;

        Ok(())
    }

    pub(super) fn jmp(&mut self, source: AddressingMode) -> ExecuteResult<()> {
        let address = self.resolve_to_memory_address(source)?;
        self.registers.pc = self.check_jump_address(address)?;

        Ok(())
    }

    pub(super) fn jsr(&mut self, source: AddressingMode) -> ExecuteResult<()> {
        let address = self.resolve_to_memory_address(source)?;
        let old_pc = self.registers.pc;
        self.registers.pc = self.check_jump_address(address)?;
        self.push_stack_u32(old_pc)?;

        Ok(())
    }

    pub(super) fn link(&mut self, register: AddressRegister) -> ExecuteResult<()> {
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

        Ok(())
    }

    pub(super) fn unlk(&mut self, register: AddressRegister) -> ExecuteResult<()> {
        self.registers.set_sp(register.read_from(self.registers));

        let address = self.pop_stack_u32()?;
        register.write_long_word_to(self.registers, address);

        Ok(())
    }

    pub(super) fn ret(&mut self, restore_ccr: bool) -> ExecuteResult<()> {
        if restore_ccr {
            let word = self.pop_stack_u16()?;
            self.registers.ccr = (word as u8).into();
        }

        let pc = self.pop_stack_u32()?;
        self.registers.pc = self.check_jump_address(pc)?;

        Ok(())
    }

    pub(super) fn rte(&mut self) -> ExecuteResult<()> {
        let sr = self.pop_stack_u16()?;

        let pc = self.pop_stack_u32()?;
        self.registers.set_status_register(sr);
        self.registers.pc = self.check_jump_address(pc)?;

        Ok(())
    }

    pub(super) fn trapv(&self) -> ExecuteResult<()> {
        if self.registers.ccr.overflow {
            Err(Exception::Trap(OVERFLOW_VECTOR))
        } else {
            Ok(())
        }
    }

    pub(super) fn chk(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<()> {
        let upper_bound = self.read_word(source)? as i16;

        let value = register.read_from(self.registers) as i16;

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: false,
            ..self.registers.ccr
        };

        if value < 0 {
            self.registers.ccr.negative = true;
            Err(Exception::CheckRegister)
        } else if value > upper_bound {
            self.registers.ccr.negative = false;
            Err(Exception::CheckRegister)
        } else {
            Ok(())
        }
    }
}

pub(super) fn trap(vector: u32) -> ExecuteResult<()> {
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
