use crate::bus::BusInterface;
use crate::instructions::{parse_branch_displacement, parse_register_high, parse_signed_immediate};
use crate::{Sh2, SP};

// JMP @Rm
// Unconditional jump
pub fn jmp(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.next_pc = cpu.registers.gpr[register];
    cpu.registers.next_op_in_delay_slot = true;
}

// JSR @Rm
// Jump to subroutine
pub fn jsr(cpu: &mut Sh2, opcode: u16) {
    cpu.registers.pr = cpu.registers.next_pc;
    jmp(cpu, opcode);
}

// BRA label
// Unconditional branch
pub fn bra(cpu: &mut Sh2, opcode: u16) {
    let displacement = parse_branch_displacement(opcode) << 1;
    cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(displacement as u32);
    cpu.registers.next_op_in_delay_slot = true;
}

// BRAF Rm
// Unconditional branch far
pub fn braf(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(cpu.registers.gpr[register]);
    cpu.registers.next_op_in_delay_slot = true;
}

// BSR label
// Branch to subroutine
pub fn bsr(cpu: &mut Sh2, opcode: u16) {
    cpu.registers.pr = cpu.registers.next_pc;
    bra(cpu, opcode);
}

// BSRF Rm
// Branch to subroutine far
pub fn bsrf(cpu: &mut Sh2, opcode: u16) {
    cpu.registers.pr = cpu.registers.next_pc;

    let register = parse_register_high(opcode) as usize;
    cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(cpu.registers.gpr[register]);
    cpu.registers.next_op_in_delay_slot = true;
}

macro_rules! impl_conditional_branch {
    ($name:ident $(, $not:tt)?) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            if $($not)? cpu.registers.sr.t {
                let displacement = parse_signed_immediate(opcode) << 1;
                cpu.registers.pc = cpu.registers.next_pc.wrapping_add(displacement as u32);
                cpu.registers.next_pc = cpu.registers.pc.wrapping_add(2);
            }
        }
    }
}

// BF label
// Branch if false
impl_conditional_branch!(bf, !);

// BT label
// Branch if true
impl_conditional_branch!(bt);

macro_rules! impl_delayed_conditional_branch {
    ($name:ident $(, $not:tt)?) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            if $($not)? cpu.registers.sr.t {
                let displacement = parse_signed_immediate(opcode) << 1;
                cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(displacement as u32);
                cpu.registers.next_op_in_delay_slot = true;
            }
        }
    }
}

// BF/S label
// Branch if false with delay slot
impl_delayed_conditional_branch!(bf_s, !);

// BT/S label
// Branch if true with delay slot
impl_delayed_conditional_branch!(bt_s);

// RTS
// Return from subroutine
pub fn rts(cpu: &mut Sh2) {
    cpu.registers.next_pc = cpu.registers.pr;
    cpu.registers.next_op_in_delay_slot = true;
}

// RTE
// Return from exception
pub fn rte<B: BusInterface>(cpu: &mut Sh2, bus: &mut B) {
    let mut sp = cpu.registers.gpr[SP];
    cpu.registers.next_pc = bus.read_longword(sp);
    cpu.registers.next_op_in_delay_slot = true;
    sp = sp.wrapping_add(4);

    cpu.registers.sr = bus.read_longword(sp).into();
    sp = sp.wrapping_add(4);

    cpu.registers.gpr[SP] = sp;
}
