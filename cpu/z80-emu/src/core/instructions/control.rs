use crate::core::InterruptMode;
use crate::core::instructions::InstructionExecutor;
use crate::traits::BusInterface;

pub(super) const fn nop() -> u32 {
    4
}

impl<B: BusInterface> InstructionExecutor<'_, '_, B> {
    pub(super) fn halt(&mut self) -> u32 {
        self.cpu.registers.halted = true;

        4
    }

    pub(super) fn di(&mut self) -> u32 {
        self.cpu.registers.iff1 = false;
        self.cpu.registers.iff2 = false;

        4
    }

    pub(super) fn ei(&mut self) -> u32 {
        self.cpu.registers.iff1 = true;
        self.cpu.registers.iff2 = true;
        self.cpu.registers.interrupt_delay = true;

        4
    }

    pub(super) fn im(&mut self, mode: InterruptMode) -> u32 {
        self.cpu.registers.interrupt_mode = mode;

        8
    }
}
