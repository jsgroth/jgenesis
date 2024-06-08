use crate::instructions::{parse_register_high, parse_register_low, parse_unsigned_immediate};
use crate::Sh2;
use jgenesis_common::num::GetBit;

// AND Rm, Rn
// Logical and
pub fn and_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.gpr[destination] &= cpu.registers.gpr[source];
}

// AND #imm, R0
// Logical and
pub fn and_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let immediate = parse_unsigned_immediate(opcode);
    cpu.registers.gpr[0] &= immediate;
}

// XOR Rm, Rn
// Exclusive or
pub fn xor_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.gpr[destination] ^= cpu.registers.gpr[source];
}

// XOR #imm, R0
// Exclusive or
pub fn xor_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let immediate = parse_unsigned_immediate(opcode);
    cpu.registers.gpr[0] ^= immediate;
}

// SHLL Rn
// Logical shift left
pub fn shll(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.sr.t = cpu.registers.gpr[register].bit(31);
    cpu.registers.gpr[register] <<= 1;
}

// SHLLn Rn
// Logical shift left by N bits
pub fn shlln<const N: usize>(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.gpr[register] <<= N;
}

// SHLR Rn
// Logical shift right
pub fn shlr(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.sr.t = cpu.registers.gpr[register].bit(0);
    cpu.registers.gpr[register] >>= 1;
}

// SHLRn Rn
// Logical shift right by N bits
pub fn shlrn<const N: usize>(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.gpr[register] >>= N;
}

// ROTCL Rn
// Rotate with carry left
pub fn rotcl(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    let carry_out = cpu.registers.gpr[register].bit(31);
    cpu.registers.gpr[register] =
        (cpu.registers.gpr[register] << 1) | u32::from(cpu.registers.sr.t);
    cpu.registers.sr.t = carry_out;
}

// TST Rm, Rn
// Sets the T bit if (Rm & Rn) is 0
pub fn tst_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.sr.t = cpu.registers.gpr[source] & cpu.registers.gpr[destination] == 0;
}

// TST #imm, R0
// Sets the T bit if (#imm & R0) is 0
pub fn tst_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let immediate = parse_unsigned_immediate(opcode);
    cpu.registers.sr.t = immediate & cpu.registers.gpr[0] == 0;
}
