use crate::core::{
    AddressingMode, ConditionCodes, ExecuteResult, InstructionExecutor, OpSize, SizedValue,
};
use crate::traits::{BusInterface, SignBit};

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn and(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<()> {
        let operand_l = self.read(source, size)?;

        let dest_resolved = self.resolve_address(dest, size)?;
        let operand_r = self.read_resolved(dest_resolved, size)?;

        let value = u32::from(operand_l) & u32::from(operand_r);
        let value = SizedValue::from(value, size);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value.is_zero(),
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        self.write_resolved(dest_resolved, value)?;
        dest_resolved.apply_post(self.registers);

        Ok(())
    }
}
