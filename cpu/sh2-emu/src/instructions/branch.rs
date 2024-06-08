use crate::bus::BusInterface;
use crate::instructions::{parse_branch_displacement, parse_register_high, parse_signed_immediate};
use crate::Sh2;

// JMP @Rm
// Unconditional jump
pub fn jmp(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.next_pc = cpu.registers.gpr[register];
    cpu.registers.next_op_in_delay_slot = true;
}

// BRA label
// Unconditional branch
pub fn bra(cpu: &mut Sh2, opcode: u16) {
    let displacement = parse_branch_displacement(opcode) << 1;
    cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(displacement as u32);
    cpu.registers.next_op_in_delay_slot = true;
}

// BSR label
// Branch to subroutine
pub fn bsr(cpu: &mut Sh2, opcode: u16) {
    cpu.registers.pr = cpu.registers.next_pc;

    let displacement = parse_branch_displacement(opcode) << 1;
    cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(displacement as u32);
    cpu.registers.next_op_in_delay_slot = true;
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

// RTS
// Return from subroutine
pub fn rts(cpu: &mut Sh2) {
    cpu.registers.next_pc = cpu.registers.pr;
    cpu.registers.next_op_in_delay_slot = true;
}
