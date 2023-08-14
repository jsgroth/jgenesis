use crate::core::{AddressingMode, ConditionCodes, InstructionExecutor, OpSize};
use crate::traits::BusInterface;

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    // MOVE / MOVEA
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
}

#[cfg(test)]
mod tests {
    use crate::core::{AddressingMode, DataRegister, InstructionExecutor, OpSize, Registers};
    use crate::traits::InMemoryBus;

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
