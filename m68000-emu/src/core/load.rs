use crate::core::{
    AddressingMode, ConditionCodes, DataRegister, ExecuteResult, InstructionExecutor, OpSize,
};
use crate::traits::{BusInterface, SignBit};

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

        match (size, dest) {
            (OpSize::LongWord, AddressingMode::AddressIndirectPredecrement(register)) => {
                let value: u32 = value.into();
                let high_word = (value >> 16) as u16;
                let low_word = value as u16;

                let address = register.read_from(self.registers).wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                self.write_bus_word(address, low_word)?;

                let address = address.wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                self.write_bus_word(address, high_word)?;
            }
            _ => {
                self.write(dest, value)?;
            }
        }

        Ok(())
    }

    pub(super) fn move_from_sr(&mut self, dest: AddressingMode) -> ExecuteResult<()> {
        let sr = self.registers.status_register();
        self.write_word(dest, sr)
    }

    pub(super) fn move_to_ccr(&mut self, source: AddressingMode) -> ExecuteResult<()> {
        let value = self.read_word(source)?;

        self.registers.ccr = (value as u8).into();

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
}
