use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, DataRegister, Direction, Exception,
    ExecuteResult, Instruction, InstructionExecutor, OpSize, SizedValue,
};
use crate::traits::{BusInterface, GetBit, SignBit};

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn add(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<()> {
        if let AddressingMode::AddressDirect(register) = dest {
            return self.adda(size, source, register);
        }

        let operand_l: u32 = self.read(source, size)?.into();

        let dest_resolved = self.resolve_address(dest, size)?;
        dest_resolved.apply_post(self.registers);
        let operand_r: u32 = self.read_resolved(dest_resolved, size)?.into();

        let (sum, carry, overflow) = match size {
            OpSize::Byte => add_bytes(operand_l as u8, operand_r as u8),
            OpSize::Word => add_words(operand_l as u16, operand_r as u16),
            OpSize::LongWord => add_long_words(operand_l, operand_r),
        };

        if !dest.is_address_direct() {
            self.registers.ccr = ConditionCodes {
                carry,
                overflow,
                zero: sum.is_zero(),
                negative: sum.sign_bit(),
                extend: carry,
            };
        }

        self.write_resolved(dest_resolved, sum)?;

        Ok(())
    }

    fn adda(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressRegister,
    ) -> ExecuteResult<()> {
        let operand_l = match size {
            OpSize::Word => self.read_word(source)? as i16 as u32,
            OpSize::LongWord => self.read_long_word(source)?,
            OpSize::Byte => panic!("ADDA does not support bytes"),
        };
        let operand_r = dest.read_from(self.registers);

        let sum = operand_l.wrapping_add(operand_r);
        dest.write_long_word_to(self.registers, sum);

        Ok(())
    }
}

fn add_bytes(operand_l: u8, operand_r: u8) -> (SizedValue, bool, bool) {
    let (sum, carry) = operand_l.overflowing_add(operand_r);
    let bit_6_carry = (operand_l & 0x7F) + (operand_r & 0x7F) >= 0x80;
    let overflow = bit_6_carry != carry;

    (sum.into(), carry, overflow)
}

fn add_words(operand_l: u16, operand_r: u16) -> (SizedValue, bool, bool) {
    let (sum, carry) = operand_l.overflowing_add(operand_r);
    let bit_14_carry = (operand_l & 0x7FFF) + (operand_r & 0x7FFF) >= 0x8000;
    let overflow = bit_14_carry != carry;

    (sum.into(), carry, overflow)
}

fn add_long_words(operand_l: u32, operand_r: u32) -> (SizedValue, bool, bool) {
    let (sum, carry) = operand_l.overflowing_add(operand_r);
    let bit_31_carry = (operand_l & 0x7FFF_FFFF) + (operand_r & 0x7FFF_FFFF) >= 0x8000_0000;
    let overflow = bit_31_carry != carry;

    (sum.into(), carry, overflow)
}

pub(super) fn decode_add(opcode: u16) -> ExecuteResult<Instruction> {
    let register = ((opcode >> 9) & 0x07) as u8;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let size = OpSize::parse_from_opcode(opcode);
    match size {
        Ok(size) => {
            // ADD (TODO: ADDX)
            let direction = if opcode.bit(8) {
                Direction::RegisterToMemory
            } else {
                Direction::MemoryToRegister
            };

            if direction == Direction::RegisterToMemory && !addressing_mode.is_writable() {
                return Err(Exception::IllegalInstruction(opcode));
            }

            let register_am = AddressingMode::DataDirect(DataRegister(register));
            let (source, dest) = match direction {
                Direction::RegisterToMemory => (register_am, addressing_mode),
                Direction::MemoryToRegister => (addressing_mode, register_am),
            };

            Ok(Instruction::Add { size, source, dest })
        }
        Err(_) => {
            // ADDA

            let size = if opcode.bit(8) {
                OpSize::LongWord
            } else {
                OpSize::Word
            };

            Ok(Instruction::Add {
                size,
                source: addressing_mode,
                dest: AddressingMode::AddressDirect(AddressRegister(register)),
            })
        }
    }
}

pub(super) fn decode_addq_subq(opcode: u16, size: OpSize) -> ExecuteResult<Instruction> {
    let dest = AddressingMode::parse_from_opcode(opcode)?;
    let operand = ((opcode >> 9) & 0x07) as u8;
    let operand = if operand == 0 { 8 } else { operand };

    if !dest.is_writable() || (size == OpSize::Byte && dest.is_address_direct()) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    if !opcode.bit(8) {
        Ok(Instruction::Add {
            size,
            source: AddressingMode::Quick(operand),
            dest,
        })
    } else {
        todo!("SUBQ")
    }
}

pub(super) fn decode_addi(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    if addressing_mode.is_address_direct() || !addressing_mode.is_writable() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Add {
        size,
        source: AddressingMode::Immediate,
        dest: addressing_mode,
    })
}
