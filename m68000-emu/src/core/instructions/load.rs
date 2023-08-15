use crate::core::instructions::UspDirection;
use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult,
    Instruction, InstructionExecutor, OpSize,
};
use crate::traits::{BusInterface, GetBit, SignBit};

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn move_(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<()> {
        let value = self.read(source, size)?;

        if !dest.is_address_direct() {
            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: value.is_zero(),
                negative: value.sign_bit(),
                ..self.registers.ccr
            };
        }

        self.write(dest, value)?;

        Ok(())
    }

    pub(super) fn move_from_sr(&mut self, dest: AddressingMode) -> ExecuteResult<()> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Word)?;
        self.write_word_resolved(dest_resolved, self.registers.status_register())
    }

    pub(super) fn move_to_ccr(&mut self, source: AddressingMode) -> ExecuteResult<()> {
        let value = self.read_word(source)?;

        self.registers.ccr = (value as u8).into();

        Ok(())
    }

    pub(super) fn move_to_sr(&mut self, source: AddressingMode) -> ExecuteResult<()> {
        let value = self.read_word(source)?;

        self.registers.set_status_register(value);

        Ok(())
    }

    pub(super) fn moveq(&mut self, data: i8, register: DataRegister) {
        register.write_long_word_to(self.registers, data as u32);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: data == 0,
            negative: data < 0,
            ..self.registers.ccr
        };
    }

    pub(super) fn move_usp(&mut self, direction: UspDirection, register: AddressRegister) {
        match direction {
            UspDirection::RegisterToUsp => {
                let value = register.read_from(self.registers);
                self.registers.usp = value;
            }
            UspDirection::UspToRegister => {
                register.write_long_word_to(self.registers, self.registers.usp);
            }
        }
    }
}

pub(super) fn decode_move(opcode: u16) -> ExecuteResult<Instruction> {
    let size = match opcode & 0xF000 {
        0x1000 => OpSize::Byte,
        0x2000 => OpSize::LongWord,
        0x3000 => OpSize::Word,
        _ => unreachable!("nested match expressions"),
    };

    let source = AddressingMode::parse_from_opcode(opcode)?;

    let dest_mode = (opcode >> 6) as u8;
    let dest_register = (opcode >> 9) as u8;
    let dest = AddressingMode::parse_from(dest_mode, dest_register)?;

    if !dest.is_writable() || (dest.is_address_direct() && size == OpSize::Byte) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Move { size, source, dest })
}

pub(super) fn decode_movq(opcode: u16) -> ExecuteResult<Instruction> {
    if opcode.bit(8) {
        Err(Exception::IllegalInstruction(opcode))
    } else {
        // MOVEQ
        let data = opcode as i8;
        let register = ((opcode >> 9) & 0x07) as u8;
        Ok(Instruction::MoveQuick(data, register.into()))
    }
}

pub(super) fn decode_move_from_sr(opcode: u16) -> ExecuteResult<Instruction> {
    let dest = AddressingMode::parse_from_opcode(opcode)?;

    if !dest.is_writable() || dest.is_address_direct() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::MoveFromSr(dest))
}

pub(super) fn decode_move_to_ccr(opcode: u16) -> ExecuteResult<Instruction> {
    let source = AddressingMode::parse_from_opcode(opcode)?;

    if source.is_address_direct() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::MoveToCcr(source))
}

pub(super) fn decode_move_to_sr(opcode: u16, supervisor_mode: bool) -> ExecuteResult<Instruction> {
    if !supervisor_mode {
        return Err(Exception::PrivilegeViolation);
    }

    let source = AddressingMode::parse_from_opcode(opcode)?;

    if source.is_address_direct() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::MoveToSr(source))
}

pub(super) fn decode_move_usp(opcode: u16, supervisor_mode: bool) -> ExecuteResult<Instruction> {
    if !supervisor_mode {
        return Err(Exception::PrivilegeViolation);
    }

    let register = (opcode & 0x07) as u8;
    let direction = if opcode.bit(3) {
        UspDirection::UspToRegister
    } else {
        UspDirection::RegisterToUsp
    };

    Ok(Instruction::MoveUsp(direction, register.into()))
}
