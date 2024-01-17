use crate::sm83::Sm83;

impl Sm83 {
    // SCF: Set carry flag
    pub(super) fn scf(&mut self) {
        self.registers.f.subtract = false;
        self.registers.f.half_carry = false;
        self.registers.f.carry = true;
    }

    // CCF: Complement carry flag
    pub(super) fn ccf(&mut self) {
        self.registers.f.subtract = false;
        self.registers.f.half_carry = false;
        self.registers.f.carry = !self.registers.f.carry;
    }

    // EI: Enable interrupts
    pub(super) fn ei(&mut self) {
        // EI has a 1-instruction delay before it takes effect, and if the next instruction is DI
        // then it effectively cancels the EI.
        self.state.pending_ime_set = true;
    }

    // DI: Disable interrupts
    pub(super) fn di(&mut self) {
        self.registers.ime = false;
        self.state.pending_ime_set = false;
    }
}
