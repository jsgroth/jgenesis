use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, ExecuteResult, InstructionExecutor, OpSize,
    SizedValue,
};
use crate::traits::{BusInterface, SignBit};

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
