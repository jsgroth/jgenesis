use crate::core::{
    AddressingMode, ConditionCodes, DataRegister, Direction, Exception, ExecuteResult, Instruction,
    InstructionExecutor, OpSize, SizedValue,
};
use crate::traits::{BusInterface, GetBit, SignBit};

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn and(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<()> {
        let operand_l = self.read(source, size)?;

        let dest_resolved = self.resolve_address(dest, size)?;
        dest_resolved.apply_post(self.registers);
        let operand_r = self.read_resolved(dest_resolved, size)?;

        let value = u32::from(operand_l) & u32::from(operand_r);
        let value = SizedValue::from_size(value, size);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value.is_zero(),
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        self.write_resolved(dest_resolved, value)?;

        Ok(())
    }
}

pub(super) fn decode_and(opcode: u16) -> ExecuteResult<Instruction> {
    let register = ((opcode >> 9) & 0x07) as u8;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let direction = if opcode.bit(8) {
        Direction::RegisterToMemory
    } else {
        Direction::MemoryToRegister
    };
    let size = OpSize::parse_from_opcode(opcode)?;

    if addressing_mode.is_address_direct()
        || (direction == Direction::RegisterToMemory && !addressing_mode.is_writable())
    {
        return Err(Exception::IllegalInstruction(opcode));
    }

    let register_am = AddressingMode::DataDirect(DataRegister(register));
    let (source, dest) = match direction {
        Direction::RegisterToMemory => (register_am, addressing_mode),
        Direction::MemoryToRegister => (addressing_mode, register_am),
    };

    Ok(Instruction::And { size, source, dest })
}

pub(super) fn decode_andi(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    if addressing_mode == AddressingMode::Immediate {
        todo!("ANDI to CCR / ANDI to SSR");
    }

    if addressing_mode.is_address_direct() || !addressing_mode.is_writable() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::And {
        size,
        source: AddressingMode::Immediate,
        dest: addressing_mode,
    })
}
