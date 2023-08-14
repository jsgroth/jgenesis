use crate::core::{AddressingMode, ConditionCodes, DataRegister, InstructionExecutor, OpSize};
use crate::traits::{BusInterface, SignBit};

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn mov(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) {
        let value = source.read_from(self.registers, self.bus, size);
        dest.write_to(self.registers, self.bus, value);

        if !matches!(dest, AddressingMode::AddressDirect(..)) {
            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: value.is_zero(),
                negative: value.sign_bit(),
                ..self.registers.ccr
            };
        }
    }

    pub(super) fn move_to_ccr(&mut self, source: AddressingMode) {
        let value = source.read_byte_from(self.registers, self.bus);
        self.registers.ccr = value.into();
    }

    pub(super) fn move_from_sr(&mut self, dest: AddressingMode) {
        let value = self.registers.status_register();
        dest.write_word_to(self.registers, self.bus, value);
    }

    pub(super) fn move_to_sr(&mut self, source: AddressingMode) {
        let value = source.read_word_from(self.registers, self.bus);
        self.registers.set_status_register(value);
    }

    pub(super) fn movem(&mut self) {
        todo!()
    }

    pub(super) fn movep(&mut self) {
        todo!()
    }

    pub(super) fn moveq(&mut self, dest: DataRegister, opcode: u16) {
        let value = opcode as i8 as u32;
        dest.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            ..self.registers.ccr
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::bus::InMemoryBus;
    use crate::core::{AddressingMode, DataRegister, InstructionExecutor, OpSize, Registers};

    #[test]
    fn mov_data_to_data() {
        let mut bus = InMemoryBus::new();

        for from in 0..8 {
            for to in 0..8 {
                for size in OpSize::ALL {
                    let mut registers = Registers::new();

                    registers.data[from as usize] = 0x11223344;
                    registers.data[to as usize] = 0x55667788;

                    let source = AddressingMode::DataDirect(DataRegister(from));
                    let dest = AddressingMode::DataDirect(DataRegister(to));

                    InstructionExecutor::new(&mut registers, &mut bus).mov(size, source, dest);

                    if from != to {
                        let expected = match size {
                            OpSize::Byte => 0x55667744,
                            OpSize::Word => 0x55663344,
                            OpSize::LongWord => 0x11223344,
                        };
                        assert_eq!(
                            registers.data[to as usize], expected,
                            "op size {size:?}, expected 0x{expected:08X}, actual 0x{:08X}",
                            registers.data[to as usize]
                        );
                        assert_eq!(
                            registers.data[from as usize], 0x11223344,
                            "op size {size:?}"
                        );
                    } else {
                        assert_eq!(registers.data[to as usize], 0x55667788);
                    }
                }
            }
        }
    }
}
