//! SH-2 branch and jump instructions

use crate::bus::BusInterface;
use crate::instructions::rn;
use crate::{SP, Sh2};

fn i12(opcode: u16) -> i16 {
    ((opcode as i16) << 4) >> 4
}

macro_rules! impl_conditional_branch {
    ($name:ident $(, $not:tt)?) => {
        pub(crate) fn $name(&mut self, opcode: u16, bus: &mut Bus) {
            if $($not)? self.registers.sr.t {
                let disp = i32::from(opcode as i8) << 1;
                self.registers.pc = self.registers.next_pc.wrapping_add(disp as u32);
                self.registers.next_pc = self.registers.pc.wrapping_add(2);

                bus.increment_cycle_counter(2);
            }
        }
    }
}

macro_rules! impl_delayed_conditional_branch {
    ($name:ident $(, $not:tt)?) => {
        pub(crate) fn $name(&mut self, opcode: u16, bus: &mut Bus) {
            if $($not)? self.registers.sr.t {
                let disp = i32::from(opcode as i8) << 1;
                self.registers.next_pc = self.registers.next_pc.wrapping_add(disp as u32);
                self.registers.next_op_in_delay_slot = true;

                bus.increment_cycle_counter(1);
            }
        }
    }
}

impl<Bus: BusInterface> Sh2<Bus> {
    // JMP @Rm
    // Unconditional jump
    pub(crate) fn jmp(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        self.registers.next_pc = self.registers.gpr[n];
        self.registers.next_op_in_delay_slot = true;

        bus.increment_cycle_counter(1);
    }

    // JSR @Rm
    // Jump to subroutine
    pub(crate) fn jsr(&mut self, opcode: u16, bus: &mut Bus) {
        self.registers.pr = self.registers.next_pc;
        self.jmp(opcode, bus);
    }

    // BRA label
    // Unconditional branch
    pub(crate) fn bra(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = i12(opcode) << 1;
        self.registers.next_pc = self.registers.next_pc.wrapping_add(disp as u32);
        self.registers.next_op_in_delay_slot = true;

        bus.increment_cycle_counter(1);
    }

    // BRAF Rm
    // Unconditional branch far
    pub(crate) fn braf(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        self.registers.next_pc = self.registers.next_pc.wrapping_add(self.registers.gpr[n]);
        self.registers.next_op_in_delay_slot = true;

        bus.increment_cycle_counter(1);
    }

    // BSR label
    // Branch to subroutine
    pub(crate) fn bsr(&mut self, opcode: u16, bus: &mut Bus) {
        self.registers.pr = self.registers.next_pc;
        self.bra(opcode, bus);
    }

    // BSRF Rm
    // Branch to subroutine far
    pub(crate) fn bsrf(&mut self, opcode: u16, bus: &mut Bus) {
        self.registers.pr = self.registers.next_pc;
        self.braf(opcode, bus);
    }

    // BF label
    // Branch if false
    impl_conditional_branch!(bf, !);

    // BT label
    // Branch if true
    impl_conditional_branch!(bt);

    // BF/S label
    // Branch if false with delay slot
    impl_delayed_conditional_branch!(bf_s, !);

    // BT/S label
    // Branch if true with delay slot
    impl_delayed_conditional_branch!(bt_s);

    // RTS
    // Return from subroutine
    pub(crate) fn rts(&mut self, bus: &mut Bus) {
        self.registers.next_pc = self.registers.pr;
        self.registers.next_op_in_delay_slot = true;

        bus.increment_cycle_counter(1);
    }

    // RTE
    // Return from exception
    pub(crate) fn rte(&mut self, bus: &mut Bus) {
        self.registers.next_pc = self.read_longword(self.registers.gpr[SP], bus);
        self.registers.next_op_in_delay_slot = true;
        self.registers.gpr[SP] = self.registers.gpr[SP].wrapping_add(4);

        self.registers.sr = self.read_longword(self.registers.gpr[SP], bus).into();
        self.registers.gpr[SP] = self.registers.gpr[SP].wrapping_add(4);
    }

    // TRAPA #imm
    // Trap always
    pub(crate) fn trapa(&mut self, opcode: u16, bus: &mut Bus) {
        self.registers.gpr[SP] = self.registers.gpr[SP].wrapping_sub(4);
        self.write_longword(self.registers.gpr[SP], self.registers.sr.into(), bus);

        self.registers.gpr[SP] = self.registers.gpr[SP].wrapping_sub(4);
        self.write_longword(self.registers.gpr[SP], self.registers.next_pc, bus);

        let vector_number = opcode & 0xFF;
        let vector_addr = self.registers.vbr.wrapping_add((vector_number << 2).into());
        self.registers.pc = self.read_longword(vector_addr, bus);
        self.registers.next_pc = self.registers.pc.wrapping_add(2);

        bus.increment_cycle_counter(7);
    }
}
