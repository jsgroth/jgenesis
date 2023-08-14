use crate::core::{AddressingMode, ConditionCodes, ExecuteResult, InstructionExecutor, OpSize};
use crate::traits::{BusInterface, SignBit};

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn move_(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<()> {
        let value = self.read(source, size)?;

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value.is_zero(),
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        self.write(dest, value)?;

        Ok(())
    }
}
