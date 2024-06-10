use crate::instructions::{rm, rn};
use crate::Sh2;
use jgenesis_common::num::GetBit;

// AND Rm, Rn
// Logical and
pub fn and_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] &= cpu.registers.gpr[m];
}

// AND #imm, R0
// Logical and
pub fn and_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let imm: u32 = (opcode & 0xFF).into();
    cpu.registers.gpr[0] &= imm;
}

// OR Rm, Rn
// Logical or
pub fn or_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] |= cpu.registers.gpr[m];
}

// OR #imm, R0
// Logical or
pub fn or_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let imm: u32 = (opcode & 0xFF).into();
    cpu.registers.gpr[0] |= imm;
}

// XOR Rm, Rn
// Exclusive or
pub fn xor_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] ^= cpu.registers.gpr[m];
}

// XOR #imm, R0
// Exclusive or
pub fn xor_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let imm: u32 = (opcode & 0xFF).into();
    cpu.registers.gpr[0] ^= imm;
}

// NOT Rm, Rn
// Logical complement
pub fn not(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = !cpu.registers.gpr[m];
}

// SHLL Rn
// Logical shift left
pub fn shll(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.sr.t = cpu.registers.gpr[n].bit(31);
    cpu.registers.gpr[n] <<= 1;
}

// SHLLn Rn
// Logical shift left by N bits
pub fn shlln<const N: usize>(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] <<= N;
}

// SHAR Rn
// Arithmetic shift right
pub fn shar(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.sr.t = cpu.registers.gpr[n].bit(0);
    cpu.registers.gpr[n] = (cpu.registers.gpr[n] >> 1) | (cpu.registers.gpr[n] & (1 << 31));
}

// SHLR Rn
// Logical shift right
pub fn shlr(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.sr.t = cpu.registers.gpr[n].bit(0);
    cpu.registers.gpr[n] >>= 1;
}

// SHLRn Rn
// Logical shift right by N bits
pub fn shlrn<const N: usize>(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] >>= N;
}

// ROTL Rn
// Rotate left
pub fn rotl(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.sr.t = cpu.registers.gpr[n].bit(31);
    cpu.registers.gpr[n] = (cpu.registers.gpr[n] << 1) | (cpu.registers.gpr[n] >> 31);
}

// ROTCL Rn
// Rotate with carry left
pub fn rotcl(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    let carry_out = cpu.registers.gpr[n].bit(31);
    cpu.registers.gpr[n] = (cpu.registers.gpr[n] << 1) | u32::from(cpu.registers.sr.t);
    cpu.registers.sr.t = carry_out;
}

// ROTR Rn
// Rotate right
pub fn rotr(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    let carry = cpu.registers.gpr[n].bit(0);
    cpu.registers.sr.t = carry;
    cpu.registers.gpr[n] = (cpu.registers.gpr[n] >> 1) | (cpu.registers.gpr[n] << 31);
}

// ROTCR Rn
// Rotate with carry right
pub fn rotcr(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    let carry_out = cpu.registers.gpr[n].bit(0);
    cpu.registers.gpr[n] = (cpu.registers.gpr[n] >> 1) | (u32::from(cpu.registers.sr.t) << 31);
    cpu.registers.sr.t = carry_out;
}

// TST Rm, Rn
// Sets the T bit if (Rm & Rn) is 0
pub fn tst_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.sr.t = cpu.registers.gpr[m] & cpu.registers.gpr[n] == 0;
}

// TST #imm, R0
// Sets the T bit if (#imm & R0) is 0
pub fn tst_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let imm: u32 = (opcode & 0xFF).into();
    cpu.registers.sr.t = imm & cpu.registers.gpr[0] == 0;
}
