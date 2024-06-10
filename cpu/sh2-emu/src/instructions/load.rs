use crate::bus::BusInterface;
use crate::instructions::{extend_i16, extend_i8, rm, rn};
use crate::Sh2;

// MOV Rm, Rn
// Load from a register
pub fn mov_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gpr[m];
}

// MOV #imm, Rn
// Loads an 8-bit immediate (sign extended) into a register
pub fn mov_b_immediate_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = opcode as i8 as u32;
}

// MOV.B @Rm, Rn
// Loads a byte from memory using indirect register addressing
pub fn mov_b_indirect_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.read_byte(cpu.registers.gpr[m], bus);
    cpu.registers.gpr[n] = extend_i8(value);
}

// MOV.W @Rm, Rn
// Loads a word from memory using indirect register addressing
pub fn mov_w_indirect_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.read_word(cpu.registers.gpr[m], bus);
    cpu.registers.gpr[n] = extend_i16(value);
}

// MOV.L @Rm, Rn
// Loads a longword from memory using indirect register addressing
pub fn mov_l_indirect_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    cpu.registers.gpr[n] = cpu.read_longword(cpu.registers.gpr[m], bus);
}

// MOV.B Rm, @Rn
// Stores a byte into memory using indirect register addressing
pub fn mov_b_rm_indirect<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.registers.gpr[m] as u8;
    cpu.write_byte(cpu.registers.gpr[n], value, bus);
}

// MOV.W Rm, @Rn
// Stores a word into memory using indirect register addressing
pub fn mov_w_rm_indirect<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.registers.gpr[m] as u16;
    cpu.write_word(cpu.registers.gpr[n], value, bus);
}

// MOV.L Rm, @Rn
// Stores a longword into memory using indirect register addressing
pub fn mov_l_rm_indirect<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.registers.gpr[m];
    cpu.write_longword(cpu.registers.gpr[n], value, bus);
}

// MOV.B @Rm+, Rn
// Loads a byte into a register using post-increment indirect register addressing
pub fn mov_b_postinc_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.read_byte(cpu.registers.gpr[m], bus);
    cpu.registers.gpr[m] = cpu.registers.gpr[m].wrapping_add(1);

    cpu.registers.gpr[n] = extend_i8(value);
}

// MOV.W @Rm+, Rn
// Loads a word into a register using post-increment indirect register addressing
pub fn mov_w_postinc_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.read_word(cpu.registers.gpr[m], bus);
    cpu.registers.gpr[m] = cpu.registers.gpr[m].wrapping_add(2);

    cpu.registers.gpr[n] = extend_i16(value);
}

// MOV.L @Rm+, Rn
// Loads a longword into a register using post-increment indirect register addressing
pub fn mov_l_postinc_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.read_longword(cpu.registers.gpr[m], bus);
    cpu.registers.gpr[m] = cpu.registers.gpr[m].wrapping_add(4);

    cpu.registers.gpr[n] = value;
}

// MOV.B Rm, @-Rn
// Stores a byte into memory using pre-decrement indirect register addressing
pub fn mov_b_rm_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[n].wrapping_sub(1);
    cpu.registers.gpr[n] = address;

    let value = cpu.registers.gpr[m] as u8;
    cpu.write_byte(address, value, bus);
}

// MOV.W Rm, @-Rn
// Stores a word into memory using pre-decrement indirect register addressing
pub fn mov_w_rm_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[n].wrapping_sub(2);
    cpu.registers.gpr[n] = address;

    let value = cpu.registers.gpr[m] as u16;
    cpu.write_word(address, value, bus);
}

// MOV.L Rm, @-Rn
// Stores a longword into memory using pre-decrement indirect register addressing
pub fn mov_l_rm_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[n].wrapping_sub(4);
    cpu.registers.gpr[n] = address;

    cpu.write_longword(address, cpu.registers.gpr[m], bus);
}

// MOV.W @(disp,PC), Rn
// Loads a word (sign extended) into a register using PC relative with displacement addressing
pub fn mov_w_immediate_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xFF) << 1;
    let address = cpu.registers.next_pc.wrapping_add(disp.into());
    let value = cpu.read_word(address, bus);

    let n = rn(opcode);
    cpu.registers.gpr[n] = extend_i16(value);
}

// MOV.L @(disp,PC), Rn
// Loads a longword into a register using PC relative with displacement addressing
pub fn mov_l_immediate_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xFF) << 2;
    let address = (cpu.registers.next_pc & !3).wrapping_add(disp.into());
    let value = cpu.read_longword(address, bus);

    let n = rn(opcode);
    cpu.registers.gpr[n] = value;
}

// MOV.B R0, @(disp,Rn)
// Stores a byte into memory using indirect register with displacement addressing
pub fn mov_b_r0_rn_displacement<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = opcode & 0xF;
    let n = rm(opcode);
    let address = cpu.registers.gpr[n].wrapping_add(disp.into());
    cpu.write_byte(address, cpu.registers.gpr[0] as u8, bus);
}

// MOV.W R0, @(disp,Rn)
// Stores a word into memory using indirect register with displacement addressing
pub fn mov_w_r0_rn_displacement<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xF) << 1;
    let n = rm(opcode);
    let address = cpu.registers.gpr[n].wrapping_add(disp.into());
    cpu.write_word(address, cpu.registers.gpr[0] as u16, bus);
}

// MOV.B @(disp,Rm), R0
// Load a byte from memory using indirect register with displacement addressing
pub fn mov_b_rm_displacement_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = opcode & 0xF;
    let m = rm(opcode);

    let address = cpu.registers.gpr[m].wrapping_add(disp.into());
    let value = cpu.read_byte(address, bus);
    cpu.registers.gpr[0] = extend_i8(value);
}

// MOV.W @(disp,Rm), R0
// Load a word from memory using indirect register with displacement addressing
pub fn mov_w_rm_displacement_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xF) << 1;
    let m = rm(opcode);

    let address = cpu.registers.gpr[m].wrapping_add(disp.into());
    let value = cpu.read_word(address, bus);
    cpu.registers.gpr[0] = extend_i16(value);
}

// MOV.L Rm, @(disp,Rn)
// Stores a longword into memory using indirect register with displacement addressing
pub fn mov_l_rm_rn_displacement<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let disp = (opcode & 0xF) << 2;
    let address = cpu.registers.gpr[n].wrapping_add(disp.into());
    cpu.write_longword(address, cpu.registers.gpr[m], bus);
}

// MOV.L @(disp,Rm), Rn
// Loads a longword from memory using indirect register with displacement addressing
pub fn mov_l_rm_displacement_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let disp = (opcode & 0xF) << 2;
    let address = cpu.registers.gpr[m].wrapping_add(disp.into());
    cpu.registers.gpr[n] = cpu.read_longword(address, bus);
}

// MOV.B Rm, @(R0,Rn)
// Stores a byte into memory using indirect indexed register addressing
pub fn mov_b_rm_indirect_indexed<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[0].wrapping_add(cpu.registers.gpr[n]);
    cpu.write_byte(address, cpu.registers.gpr[m] as u8, bus);
}

// MOV.W Rm, @(R0,Rn)
// Stores a word into memory using indirect indexed register addressing
pub fn mov_w_rm_indirect_indexed<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[0].wrapping_add(cpu.registers.gpr[n]);
    cpu.write_word(address, cpu.registers.gpr[m] as u16, bus);
}

// MOV.L Rm, @(R0,Rn)
// Stores a longword into memory using indirect indexed register addressing
pub fn mov_l_rm_indirect_indexed<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[0].wrapping_add(cpu.registers.gpr[n]);
    cpu.write_longword(address, cpu.registers.gpr[m], bus);
}

// MOV.B @(R0,Rm), Rn
// Loads a byte from memory using indirect indexed register addressing
pub fn mov_b_indirect_indexed_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[0].wrapping_add(cpu.registers.gpr[m]);
    let value = cpu.read_byte(address, bus);
    cpu.registers.gpr[n] = extend_i8(value);
}

// MOV.W @(R0,Rm), Rn
// Loads a word from memory using indirect indexed register addressing
pub fn mov_w_indirect_indexed_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[0].wrapping_add(cpu.registers.gpr[m]);
    let value = cpu.read_word(address, bus);
    cpu.registers.gpr[n] = extend_i16(value);
}

// MOV.L @(R0,Rm), Rn
// Loads a longword from memory using indirect indexed register addressing
pub fn mov_l_indirect_indexed_rn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let address = cpu.registers.gpr[0].wrapping_add(cpu.registers.gpr[m]);
    cpu.registers.gpr[n] = cpu.read_longword(address, bus);
}

// MOV.B @(disp,GBR), R0
// Loads a byte into R0 using indirect GBR with displacement addressing
pub fn mov_b_disp_gbr_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = opcode & 0xFF;
    let address = cpu.registers.gbr.wrapping_add(disp.into());
    let value = cpu.read_byte(address, bus);
    cpu.registers.gpr[0] = extend_i8(value);
}

// MOV.W @(disp,GBR), R0
// Loads a word into R0 using indirect GBR with displacement addressing
pub fn mov_w_disp_gbr_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xFF) << 1;
    let address = cpu.registers.gbr.wrapping_add(disp.into());
    let value = cpu.read_word(address, bus);
    cpu.registers.gpr[0] = extend_i16(value);
}

// MOV.L @(disp,GBR), R0
// Loads a longword into R0 using indirect GBR with displacement addressing
pub fn mov_l_disp_gbr_r0<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xFF) << 2;
    let address = cpu.registers.gbr.wrapping_add(disp.into());
    cpu.registers.gpr[0] = cpu.read_longword(address, bus);
}

// MOV.B R0, @(disp,GBR)
// Stores a byte into memory using indirect GBR with displacement addressing
pub fn mov_b_r0_disp_gbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = opcode & 0xFF;
    let address = cpu.registers.gbr.wrapping_add(disp.into());
    cpu.write_byte(address, cpu.registers.gpr[0] as u8, bus);
}

// MOV.W R0, @(disp,GBR)
// Stores a word into memory using indirect GBR with displacement addressing
pub fn mov_w_r0_disp_gbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xFF) << 1;
    let address = cpu.registers.gbr.wrapping_add(disp.into());
    cpu.write_word(address, cpu.registers.gpr[0] as u16, bus);
}

// MOV.L R0, @(disp,GBR)
// Stores a longword into memory using indirect GBR with displacement addressing
pub fn mov_l_r0_disp_gbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let disp = (opcode & 0xFF) << 2;
    let address = cpu.registers.gbr.wrapping_add(disp.into());
    cpu.write_longword(address, cpu.registers.gpr[0], bus);
}

// MOVA @(disp,PC), R0
// Move effective address
pub fn mova(cpu: &mut Sh2, opcode: u16) {
    let disp = (opcode & 0xFF) << 2;
    cpu.registers.gpr[0] = (cpu.registers.next_pc & !3).wrapping_add(disp.into());
}

// MOVT Rn
// Loads T into a general-purpose register
pub fn movt(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.sr.t.into();
}

// LDC Rm, SR
// Loads the status register from a general-purpose register
pub fn ldc_rm_sr(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.sr = cpu.registers.gpr[n].into();
}

// LDC Rm, GBR
// Loads GBR from a general-purpose register
pub fn ldc_rm_gbr(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gbr = cpu.registers.gpr[n];
}

// LDC Rm, VBR
// Loads VBR from a general-purpose register
pub fn ldc_rm_vbr(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.vbr = cpu.registers.gpr[n];
}

// LDC.L @Rm+, SR
// Load SR from memory using post-increment register indirect addressing
pub fn ldc_postinc_sr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rn(opcode);
    let address = cpu.registers.gpr[m];
    cpu.registers.gpr[m] = address.wrapping_add(4);

    cpu.registers.sr = cpu.read_longword(address, bus).into();
}

// LDC.L @Rm+, GBR
// Load GBR from memory using post-increment register indirect addressing
pub fn ldc_postinc_gbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rn(opcode);
    let address = cpu.registers.gpr[m];
    cpu.registers.gpr[m] = address.wrapping_add(4);

    cpu.registers.gbr = cpu.read_longword(address, bus);
}

// LDC.L @Rm+, VBR
// Load VBR from memory using post-increment register indirect addressing
pub fn ldc_postinc_vbr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rn(opcode);
    let address = cpu.registers.gpr[m];
    cpu.registers.gpr[m] = address.wrapping_add(4);

    cpu.registers.vbr = cpu.read_longword(address, bus);
}

// LDS Rm, MACH
// Loads MACH from a general-purpose register
pub fn lds_rm_mach(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.mach = cpu.registers.gpr[n];
}

// LDS Rm, MACL
// Loads MACL from a general-purpose register
pub fn lds_rm_macl(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.macl = cpu.registers.gpr[n];
}

// LDS Rm, PR
// Loads PR from a general-purpose register
pub fn lds_rm_pr(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.pr = cpu.registers.gpr[n];
}

// LDS.L @Rm+, MACH
// Loads MACH from memory using post-increment indirect register addressing
pub fn lds_postinc_mach<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n];
    cpu.registers.gpr[n] = address.wrapping_add(4);

    cpu.registers.mach = cpu.read_longword(address, bus);
}

// LDS.L @Rm+, MACL
// Loads MACL from memory using post-increment indirect register addressing
pub fn lds_postinc_macl<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n];
    cpu.registers.gpr[n] = address.wrapping_add(4);

    cpu.registers.macl = cpu.read_longword(address, bus);
}

// LDS.L @Rm+, PR
// Loads PR from memory using post-increment indirect register addressing
pub fn lds_postinc_pr<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n];
    cpu.registers.gpr[n] = address.wrapping_add(4);

    cpu.registers.pr = cpu.read_longword(address, bus);
}

// STC SR, Rn
// Store SR into a general-purpose register
pub fn stc_sr_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.sr.into();
}

// STC GBR, Rn
// Store GBR into a general-purpose register
pub fn stc_gbr_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gbr;
}

// STC VBR, Rn
// Store VBR into a general-purpose register
pub fn stc_vbr_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.vbr;
}

// STC.L SR, @-Rn
// Store SR in memory using pre-decrement register addressing
pub fn stc_sr_rn_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n].wrapping_sub(4);
    cpu.registers.gpr[n] = address;

    cpu.write_longword(address, cpu.registers.sr.into(), bus);
}

// STC.L GBR, @-Rn
// Store GBR in memory using pre-decrement register addressing
pub fn stc_gbr_rn_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n].wrapping_sub(4);
    cpu.registers.gpr[n] = address;

    cpu.write_longword(address, cpu.registers.gbr, bus);
}

// STC.L VBR, @-Rn
// Store VBR in memory using pre-decrement register addressing
pub fn stc_vbr_rn_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n].wrapping_sub(4);
    cpu.registers.gpr[n] = address;

    cpu.write_longword(address, cpu.registers.vbr, bus);
}

// STS MACH, Rn
// Store MACH into a general-purpose register
pub fn sts_mach_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.mach;
}

// STS MACL, Rn
// Store MACL into a general-purpose register
pub fn sts_macl_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.macl;
}

// STS PR, Rn
// Store PR into a general-purpose register
pub fn sts_pr_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.pr;
}

// STS.L MACH, @-Rn
// Store MACH in memory using pre-decrement register addressing
pub fn sts_mach_rn_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n].wrapping_sub(4);
    cpu.registers.gpr[n] = address;

    cpu.write_longword(address, cpu.registers.mach, bus);
}

// STS.L MACL, @-Rn
// Store MACL in memory using pre-decrement register addressing
pub fn sts_macl_rn_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n].wrapping_sub(4);
    cpu.registers.gpr[n] = address;

    cpu.write_longword(address, cpu.registers.macl, bus);
}

// STS.L PR, @-Rn
// Store PR in memory using pre-decrement indirect register addressing
pub fn sts_pr_rn_predec<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let n = rn(opcode);
    let address = cpu.registers.gpr[n].wrapping_sub(4);
    cpu.registers.gpr[n] = address;

    cpu.write_longword(address, cpu.registers.pr, bus);
}

// CLRT
// Clear the T flag
pub fn clrt(cpu: &mut Sh2) {
    cpu.registers.sr.t = false;
}

// SETT
// Set the T flag
pub fn sett(cpu: &mut Sh2) {
    cpu.registers.sr.t = true;
}

// CLRMAC
// Clear the MAC registers
pub fn clrmac(cpu: &mut Sh2) {
    cpu.registers.macl = 0;
    cpu.registers.mach = 0;
}

// SWAP.B Rm, Rn
// Swaps the lowest two bytes of a register
pub fn swap_b(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.registers.gpr[m];
    cpu.registers.gpr[n] = (value & 0xFFFF0000) | ((value & 0xFF) << 8) | ((value >> 8) & 0xFF);
}

// SWAP.W Rm, Rn
// Swaps the words of a register
pub fn swap_w(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let value = cpu.registers.gpr[m];
    cpu.registers.gpr[n] = (value >> 16) | (value << 16);
}

// XTRCT Rm, Rn
// Extract the center 32 bits of the 64-bit value formed by Rm and Rn
pub fn xtrct(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    cpu.registers.gpr[n] = (cpu.registers.gpr[m] << 16) | (cpu.registers.gpr[n] >> 16);
}
