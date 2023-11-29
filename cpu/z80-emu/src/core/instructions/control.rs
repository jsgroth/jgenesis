use crate::core::instructions::InstructionExecutor;
use crate::core::InterruptMode;
use crate::traits::BusInterface;

pub(super) const fn nop() -> u32 {
    4
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn halt(&mut self) -> u32 {
        self.registers.halted = true;

        4
    }

    pub(super) fn di(&mut self) -> u32 {
        self.registers.iff1 = false;
        self.registers.iff2 = false;

        4
    }

    pub(super) fn ei(&mut self) -> u32 {
        self.registers.iff1 = true;
        self.registers.iff2 = true;
        self.registers.interrupt_delay = true;

        4
    }

    pub(super) fn im(&mut self, mode: InterruptMode) -> u32 {
        self.registers.interrupt_mode = mode;

        8
    }
}
