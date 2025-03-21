//! SH-2 branch and jump instructions

use crate::bus::BusInterface;
use crate::instructions::rn;
use crate::{SP, Sh2};

// JMP @Rm
// Unconditional jump
pub fn jmp(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.next_pc = cpu.registers.gpr[n];
    cpu.registers.next_op_in_delay_slot = true;
}

// JSR @Rm
// Jump to subroutine
pub fn jsr(cpu: &mut Sh2, opcode: u16) {
    cpu.registers.pr = cpu.registers.next_pc;
    jmp(cpu, opcode);
}

fn i12(opcode: u16) -> i16 {
    ((opcode as i16) << 4) >> 4
}

// BRA label
// Unconditional branch
pub fn bra(cpu: &mut Sh2, opcode: u16) {
    let disp = i12(opcode) << 1;
    cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(disp as u32);
    cpu.registers.next_op_in_delay_slot = true;
}

// BRAF Rm
// Unconditional branch far
pub fn braf(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(cpu.registers.gpr[n]);
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
    braf(cpu, opcode);
}

macro_rules! impl_conditional_branch {
    ($name:ident $(, $not:tt)?) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            if $($not)? cpu.registers.sr.t {
                let disp = i32::from(opcode as i8) << 1;
                cpu.registers.pc = cpu.registers.next_pc.wrapping_add(disp as u32);
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
                let disp = i32::from(opcode as i8) << 1;
                cpu.registers.next_pc = cpu.registers.next_pc.wrapping_add(disp as u32);
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
pub fn rte<B: BusInterface + ?Sized>(cpu: &mut Sh2, bus: &mut B) {
    cpu.registers.next_pc = cpu.read_longword(cpu.registers.gpr[SP], bus);
    cpu.registers.next_op_in_delay_slot = true;
    cpu.registers.gpr[SP] = cpu.registers.gpr[SP].wrapping_add(4);

    cpu.registers.sr = cpu.read_longword(cpu.registers.gpr[SP], bus).into();
    cpu.registers.gpr[SP] = cpu.registers.gpr[SP].wrapping_add(4);
}

// TRAPA #imm
// Trap always
pub fn trapa<B: BusInterface + ?Sized>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    cpu.registers.gpr[SP] = cpu.registers.gpr[SP].wrapping_sub(4);
    cpu.write_longword(cpu.registers.gpr[SP], cpu.registers.sr.into(), bus);

    cpu.registers.gpr[SP] = cpu.registers.gpr[SP].wrapping_sub(4);
    cpu.write_longword(cpu.registers.gpr[SP], cpu.registers.next_pc, bus);

    let vector_number = opcode & 0xFF;
    let vector_addr = cpu.registers.vbr.wrapping_add((vector_number << 2).into());
    cpu.registers.pc = cpu.read_longword(vector_addr, bus);
    cpu.registers.next_pc = cpu.registers.pc.wrapping_add(2);
}
