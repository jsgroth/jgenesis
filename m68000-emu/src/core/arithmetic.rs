use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, Direction, Exception, ExecuteResult,
    ExtendOpMode, Instruction, InstructionExecutor, OpSize, ResolvedAddress, SizedValue,
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

        let dest_resolved = self.resolve_address_with_post(dest, size)?;
        let operand_r: u32 = self.read_resolved(dest_resolved, size)?.into();

        let (sum, carry, overflow) = match size {
            OpSize::Byte => add_bytes(operand_l as u8, operand_r as u8, false),
            OpSize::Word => add_words(operand_l as u16, operand_r as u16, false),
            OpSize::LongWord => add_long_words(operand_l, operand_r, false),
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

    // TODO probably generalize this
    fn read_addx_operand(
        &mut self,
        size: OpSize,
        addressing_mode: AddressingMode,
    ) -> ExecuteResult<(ResolvedAddress, SizedValue)> {
        match (size, addressing_mode) {
            (OpSize::LongWord, AddressingMode::AddressIndirectPredecrement(register)) => {
                let address = register.read_from(self.registers).wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                let low_word = self.read_bus_word(address)?;

                let address = address.wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                let high_word = self.read_bus_word(address)?;

                let value = (u32::from(high_word) << 16) | u32::from(low_word);
                Ok((ResolvedAddress::Memory(address), value.into()))
            }
            _ => {
                let address = self.resolve_address_with_post(addressing_mode, size)?;
                let value = self.read_resolved(address, size)?;
                Ok((address, value))
            }
        }
    }

    pub(super) fn addx(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<()> {
        let (_, operand_l) = self.read_addx_operand(size, source)?;
        let operand_l: u32 = operand_l.into();

        let (dest_resolved, operand_r) = self.read_addx_operand(size, dest)?;
        let operand_r: u32 = operand_r.into();

        let extend = self.registers.ccr.extend;
        let (sum, carry, overflow) = match size {
            OpSize::Byte => add_bytes(operand_l as u8, operand_r as u8, extend),
            OpSize::Word => add_words(operand_l as u16, operand_r as u16, extend),
            OpSize::LongWord => add_long_words(operand_l, operand_r, extend),
        };

        self.registers.ccr = ConditionCodes {
            carry,
            overflow,
            zero: self.registers.ccr.zero && sum.is_zero(),
            negative: sum.sign_bit(),
            extend: carry,
        };

        self.write_resolved(dest_resolved, sum)?;

        Ok(())
    }
}

macro_rules! impl_add_fn {
    ($name:ident, $t:ty, $overflow_mask:expr) => {
        fn $name(operand_l: $t, operand_r: $t, extend: bool) -> (SizedValue, bool, bool) {
            let extend_operand = <$t>::from(extend);
            let (sum, carry) = match operand_l.overflowing_add(operand_r) {
                (sum, true) => (sum + extend_operand, true),
                (sum, false) => sum.overflowing_add(extend_operand),
            };

            let bit_m1_carry =
                (operand_l & $overflow_mask) + (operand_r & $overflow_mask) + extend_operand
                    > $overflow_mask;
            let overflow = bit_m1_carry != carry;

            (sum.into(), carry, overflow)
        }
    };
}

impl_add_fn!(add_bytes, u8, 0x7F);
impl_add_fn!(add_words, u16, 0x7FFF);
impl_add_fn!(add_long_words, u32, 0x7FFF_FFFF);

pub(super) fn decode_add(opcode: u16) -> ExecuteResult<Instruction> {
    let register = ((opcode >> 9) & 0x07) as u8;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let size = OpSize::parse_from_opcode(opcode);
    match size {
        Ok(size) => {
            // ADD (TODO: ADDX)
            let direction = Direction::parse_from_opcode(opcode);

            if direction == Direction::RegisterToMemory && !addressing_mode.is_writable() {
                return Err(Exception::IllegalInstruction(opcode));
            }

            let register_am = AddressingMode::DataDirect(register.into());
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
                dest: AddressingMode::AddressDirect(register.into()),
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

pub(super) fn decode_addx(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let mode = ExtendOpMode::parse_from_opcode(opcode);
    let source = (opcode & 0x07) as u8;
    let dest = ((opcode >> 9) & 0x07) as u8;

    match mode {
        ExtendOpMode::DataDirect => Ok(Instruction::AddExtend {
            size,
            source: AddressingMode::DataDirect(source.into()),
            dest: AddressingMode::DataDirect(dest.into()),
        }),
        ExtendOpMode::AddressIndirectPredecrement => Ok(Instruction::AddExtend {
            size,
            source: AddressingMode::AddressIndirectPredecrement(source.into()),
            dest: AddressingMode::AddressIndirectPredecrement(dest.into()),
        }),
    }
}
