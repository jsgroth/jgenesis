use crate::sm83::bus::BusInterface;
use crate::sm83::{Flags, Sm83};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpCondition {
    Zero,
    NotZero,
    Carry,
    NoCarry,
}

impl JumpCondition {
    fn from_opcode(opcode: u8) -> Self {
        match (opcode >> 3) & 0x3 {
            0x0 => Self::NotZero,
            0x1 => Self::Zero,
            0x2 => Self::NoCarry,
            0x3 => Self::Carry,
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        }
    }

    fn check(self, flags: Flags) -> bool {
        match self {
            Self::Zero => flags.zero,
            Self::NotZero => !flags.zero,
            Self::Carry => flags.carry,
            Self::NoCarry => !flags.carry,
        }
    }
}

impl Sm83 {
    // JP u16: Unconditional absolute jump
    pub(super) fn jp_nn<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.pc = self.fetch_operand_u16(bus);

        // JP nn takes 4 M-cycles: opcode read + 16-bit operand read + idle cycle
        bus.idle();
    }

    // JP HL: Unconditional absolute jump
    pub(super) fn jp_hl(&mut self) {
        self.registers.pc = self.registers.hl();
    }

    // JP cc, u16: Conditional absolute jump
    pub(super) fn jp_cc_nn<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        // Target address is always read
        let address = self.fetch_operand_u16(bus);

        let condition = JumpCondition::from_opcode(opcode);
        if condition.check(self.registers.f) {
            self.registers.pc = address;
            bus.idle();
        }
    }

    // JR i8: Unconditional relative jump
    pub(super) fn jr_e<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus) as i8;
        self.registers.pc = self.registers.pc.wrapping_add(operand as u16);

        // JR e takes 3 M-cycles: opcode read + operand read + idle cycle
        bus.idle();
    }

    // JR cc, i8: Conditional relative jump
    pub(super) fn jr_cc_e<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.fetch_operand(bus) as i8;

        let condition = JumpCondition::from_opcode(opcode);
        if condition.check(self.registers.f) {
            self.registers.pc = self.registers.pc.wrapping_add(operand as u16);
            bus.idle();
        }
    }

    // CALL u16: Unconditional call
    pub(super) fn call_nn<B: BusInterface>(&mut self, bus: &mut B) {
        let address = self.fetch_operand_u16(bus);

        // Idle cycle in between address read and stack push
        bus.idle();

        self.push_stack_u16(bus, self.registers.pc);
        self.registers.pc = address;
    }

    // CALL cc, u16: Conditional call
    pub(super) fn call_cc_nn<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let address = self.fetch_operand_u16(bus);

        let condition = JumpCondition::from_opcode(opcode);
        if !condition.check(self.registers.f) {
            return;
        }

        // Idle cycle in between address read and stack push
        bus.idle();

        self.push_stack_u16(bus, self.registers.pc);
        self.registers.pc = address;
    }

    // RET: Unconditional return
    pub(super) fn ret<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.pc = self.pop_stack_u16(bus);
        bus.idle();
    }

    // RET cc: Conditional return
    pub(super) fn ret_cc<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        bus.idle();

        let condition = JumpCondition::from_opcode(opcode);
        if !condition.check(self.registers.f) {
            return;
        }

        self.registers.pc = self.pop_stack_u16(bus);
        bus.idle();
    }

    // RETI: Return from interrupt handler
    pub(super) fn reti<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.pc = self.pop_stack_u16(bus);
        self.registers.ime = true;
        bus.idle();
    }

    // RST: Restart
    pub(super) fn rst<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        bus.idle();

        self.push_stack_u16(bus, self.registers.pc);
        self.registers.pc = (opcode & 0x38).into();
    }

    // HALT: Halt the CPU until an interrupt occurs
    pub(super) fn halt<B: BusInterface>(&mut self, bus: &mut B) {
        self.state.halted = true;

        // HALT bug: if a HALT instruction is executed while IME=0 and an interrupt is pending, PC does not increment
        // after the next opcode fetch
        self.state.halt_bug_triggered = !self.registers.ime && bus.interrupt_pending();
    }

    // STOP: Perform a CGB speed switch if KEY1 bit 0 is set, otherwise enters an extreme low-power state
    pub(super) fn stop<B: BusInterface>(&mut self, bus: &mut B) {
        // STOP always reads the following opcode and just doesn't do anything with it
        self.fetch_operand(bus);

        if bus.speed_switch_armed() {
            bus.perform_speed_switch();
        } else {
            todo!("STOP instruction executed outside of speed switch")
        }
    }
}
