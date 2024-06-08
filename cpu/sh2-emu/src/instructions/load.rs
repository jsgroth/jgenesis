use crate::bus::BusInterface;
use crate::instructions::{
    extend_i16, extend_i8, parse_4bit_displacement, parse_8bit_displacement, parse_register_high,
    parse_register_low, parse_signed_immediate, pc_relative_displacement_long,
    pc_relative_displacement_word,
};
use crate::Sh2;

// MOV Rm, Rn
// Load from a register
pub fn mov_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.gpr[destination] = cpu.registers.gpr[source];
}

// MOV #imm, Rn
// Loads an 8-bit immediate (sign extended) into a register
pub fn mov_b_immediate_rn(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode);
    let immediate = parse_signed_immediate(opcode);
    cpu.registers.gpr[register as usize] = immediate as u32;
}

// MOV.W @Rm, Rn
// Loads a word from memory using indirect register addressing
pub fn mov_w_indirect_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let address = cpu.registers.gpr[source];
    let value = cpu.read_word(address, bus);
    cpu.registers.gpr[destination] = extend_i16(value);
}

// MOV.L @Rm, Rn
// Loads a longword from memory using indirect register addressing
pub fn mov_l_indirect_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let address = cpu.registers.gpr[source];
    cpu.registers.gpr[destination] = cpu.read_longword(address, bus);
}

// MOV.B Rm, @Rn
// Stores a byte into memory using indirect register addressing
pub fn mov_b_rm_indirect<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode);
    let destination = parse_register_high(opcode);

    let value = cpu.registers.gpr[source as usize] as u8;
    let address = cpu.registers.gpr[destination as usize];
    cpu.write_byte(address, value, bus);
}

// MOV.W Rm, @Rn
// Stores a word into memory using indirect register addressing
pub fn mov_w_rm_indirect<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode);
    let destination = parse_register_high(opcode);

    let value = cpu.registers.gpr[source as usize] as u16;
    let address = cpu.registers.gpr[destination as usize];
    cpu.write_word(address, value, bus);
}

// MOV.L Rm, @Rn
// Stores a longword into memory using indirect register addressing
pub fn mov_l_rm_indirect<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let value = cpu.registers.gpr[source];
    let address = cpu.registers.gpr[destination];
    cpu.write_longword(address, value, bus);
}

// MOV.W @Rm+, Rn
// Loads a word into a register using post-increment indirect register addressing
pub fn mov_w_postinc_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let value = cpu.read_word(cpu.registers.gpr[source], bus);
    cpu.registers.gpr[source] = cpu.registers.gpr[source].wrapping_add(2);
    cpu.registers.gpr[destination] = extend_i16(value);
}

// MOV.L @Rm+, Rn
// Loads a longword into a register using post-increment indirect register addressing
pub fn mov_l_postinc_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let value = cpu.read_longword(cpu.registers.gpr[source], bus);
    cpu.registers.gpr[source] = cpu.registers.gpr[source].wrapping_add(4);
    cpu.registers.gpr[destination] = value;
}

// MOV.W Rm, @-Rn
// Stores a word into memory using pre-decrement indirect register addressing
pub fn mov_w_rm_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let value = cpu.registers.gpr[source] as u16;
    let address = cpu.registers.gpr[destination].wrapping_sub(2);
    cpu.registers.gpr[destination] = address;
    cpu.write_word(address, value, bus);
}

// MOV.L Rm, @-Rn
// Stores a longword into memory using pre-decrement indirect register addressing
pub fn mov_l_rm_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let value = cpu.registers.gpr[source];
    let address = cpu.registers.gpr[destination].wrapping_sub(4);
    cpu.registers.gpr[destination] = address;
    cpu.write_longword(address, value, bus);
}

// MOV.W @(disp,PC), Rn
// Loads a word (sign extended) into a register using PC relative with displacement addressing
pub fn mov_w_immediate_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let address = pc_relative_displacement_word(&cpu.registers, opcode);
    let value = cpu.read_word(address, bus);

    let register = parse_register_high(opcode);
    cpu.registers.gpr[register as usize] = extend_i16(value);
}

// MOV.L @(disp,PC), Rn
// Loads a longword into a register using PC relative with displacement addressing
pub fn mov_l_immediate_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let address = pc_relative_displacement_long(&cpu.registers, opcode);
    let value = cpu.read_longword(address, bus);

    let register = parse_register_high(opcode);
    cpu.registers.gpr[register as usize] = value;
}

// MOV.B R0, @(disp,Rn)
// Stores a byte into memory using indirect register with displacement addressing
pub fn mov_b_r0_rn_displacement<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_4bit_displacement(opcode);
    let destination = parse_register_low(opcode) as usize;
    let address = cpu.registers.gpr[destination].wrapping_add(displacement);
    cpu.write_byte(address, cpu.registers.gpr[0] as u8, bus);
}

// MOV.W R0, @(disp,Rn)
// Stores a word into memory using indirect register with displacement addressing
pub fn mov_w_r0_rn_displacement<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_4bit_displacement(opcode) << 1;
    let destination = parse_register_low(opcode) as usize;
    let address = cpu.registers.gpr[destination].wrapping_add(displacement);
    cpu.write_word(address, cpu.registers.gpr[0] as u16, bus);
}

// MOV.B @(disp,Rm), R0
// Load a byte from memory using indirect register with displacement addressing
pub fn mov_b_rm_displacement_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_4bit_displacement(opcode);
    let source = parse_register_low(opcode) as usize;

    let address = cpu.registers.gpr[source].wrapping_add(displacement);
    let value = cpu.read_byte(address, bus);
    cpu.registers.gpr[0] = extend_i8(value);
}

// MOV.L Rm, @(disp,Rn)
// Stores a longword into memory using indirect register with displacement addressing
pub fn mov_l_rm_rn_displacement<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode);
    let destination = parse_register_high(opcode);

    let displacement = parse_4bit_displacement(opcode) << 2;
    let address = cpu.registers.gpr[destination as usize].wrapping_add(displacement);
    cpu.write_longword(address, cpu.registers.gpr[source as usize], bus);
}

// MOV.L @(disp,Rm), Rn
// Loads a longword from memory using indirect register with displacement addressing
pub fn mov_l_rm_displacement_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let displacement = parse_4bit_displacement(opcode) << 2;
    let address = cpu.registers.gpr[source].wrapping_add(displacement);
    cpu.registers.gpr[destination] = cpu.read_longword(address, bus);
}

// MOV.L @(R0,Rm), Rn
// Loads a longword from memory using indirect indexed register addressing
pub fn mov_l_indirect_indexed_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let address = cpu.registers.gpr[0].wrapping_add(cpu.registers.gpr[source]);
    cpu.registers.gpr[destination] = cpu.read_longword(address, bus);
}

// MOV.B @(disp,GBR), R0
// Loads a byte into R0 using indirect GBR with displacement addressing
pub fn mov_b_disp_gbr_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_8bit_displacement(opcode);
    let address = cpu.registers.gbr.wrapping_add(displacement);
    let value = cpu.read_byte(address, bus);

    cpu.registers.gpr[0] = extend_i8(value);
}

// MOV.L @(disp,GBR), R0
// Loads a longword into R0 using indirect GBR with displacement addressing
pub fn mov_l_disp_gbr_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_8bit_displacement(opcode) << 2;
    let address = cpu.registers.gbr.wrapping_add(displacement);
    let value = cpu.read_longword(address, bus);

    cpu.registers.gpr[0] = value;
}

// MOV.B R0, @(disp,GBR)
// Stores a byte into memory using indirect GBR with displacement addressing
pub fn mov_b_r0_disp_gbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_8bit_displacement(opcode);
    let address = cpu.registers.gbr.wrapping_add(displacement);
    cpu.write_byte(address, cpu.registers.gpr[0] as u8, bus);
}

// MOV.W R0, @(disp,GBR)
// Stores a word into memory using indirect GBR with displacement addressing
pub fn mov_w_r0_disp_gbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_8bit_displacement(opcode) << 1;
    let address = cpu.registers.gbr.wrapping_add(displacement);
    cpu.write_word(address, cpu.registers.gpr[0] as u16, bus);
}

// MOV.L R0, @(disp,GBR)
// Stores a longword into memory using indirect GBR with displacement addressing
pub fn mov_l_r0_disp_gbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let displacement = parse_8bit_displacement(opcode) << 2;
    let address = cpu.registers.gbr.wrapping_add(displacement);
    cpu.write_longword(address, cpu.registers.gpr[0], bus);
}

// MOVA @(disp,PC), R0
// Move effective address
pub fn mova(cpu: &mut Sh2, opcode: u16) {
    let displacement = parse_8bit_displacement(opcode) << 2;
    cpu.registers.gpr[0] = cpu.registers.next_pc.wrapping_add(displacement);
}

// LDC Rm, SR
// Loads the status register from a general-purpose register
pub fn ldc_rm_sr(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode);
    cpu.registers.sr = cpu.registers.gpr[register as usize].into();
}

// LDC Rm, GBR
// Loads GBR from a general-purpose register
pub fn ldc_rm_gbr(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode);
    cpu.registers.gbr = cpu.registers.gpr[register as usize];
}

// LDC Rm, VBR
// Loads VBR from a general-purpose register
pub fn ldc_rm_vbr(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode);
    cpu.registers.vbr = cpu.registers.gpr[register as usize];
}

// STS.L PR, @-Rn
// Store PR in memory using pre-decrement indirect register addressing
pub fn sts_pr_rn_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let register = parse_register_high(opcode) as usize;
    let address = cpu.registers.gpr[register].wrapping_sub(4);
    cpu.registers.gpr[register] = address;
    cpu.write_longword(address, cpu.registers.pr, bus);
}
