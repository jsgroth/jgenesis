mod alu;
mod bits;
mod branch;
mod load;

use crate::bus::BusInterface;
use crate::registers::Sh2Registers;
use crate::Sh2;

pub fn execute<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode {
        0b0000_0000_0001_1001 => todo!("DIV0U"),
        0b0000_0000_0000_1011 => branch::rts(cpu),
        0b0000_0000_0000_1000 => todo!("CLRT"),
        0b0000_0000_0010_1000 => todo!("CLRMAC"),
        // NOP
        0b0000_0000_0000_1001 => {}
        0b0000_0000_0010_1011 => todo!("RTE"),
        0b0000_0000_0001_1000 => todo!("SETT"),
        0b0000_0000_0001_1011 => todo!("SLEEP"),
        _ => execute_xnnx(cpu, opcode, bus),
    }
}

#[inline]
fn execute_xnnx<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode & 0b1111_0000_0000_1111 {
        0b0110_0000_0000_0011 => load::mov_rm_rn(cpu, opcode),
        0b0010_0000_0000_0000 => load::mov_b_rm_indirect(cpu, opcode, bus),
        0b0010_0000_0000_0001 => load::mov_w_rm_indirect(cpu, opcode, bus),
        0b0010_0000_0000_0010 => load::mov_l_rm_indirect(cpu, opcode, bus),
        0b0110_0000_0000_0000 => todo!("MOV.B @Rm, Rn"),
        0b0110_0000_0000_0001 => load::mov_w_indirect_rn(cpu, opcode, bus),
        0b0110_0000_0000_0010 => load::mov_l_indirect_rn(cpu, opcode, bus),
        0b0010_0000_0000_0100 => todo!("MOV.B Rm, @-Rn"),
        0b0010_0000_0000_0101 => load::mov_w_rm_predec(cpu, opcode, bus),
        0b0010_0000_0000_0110 => load::mov_l_rm_predec(cpu, opcode, bus),
        0b0110_0000_0000_0100 => todo!("MOV.B @Rm+, Rn"),
        0b0110_0000_0000_0101 => load::mov_w_postinc_rn(cpu, opcode, bus),
        0b0110_0000_0000_0110 => load::mov_l_postinc_rn(cpu, opcode, bus),
        0b0000_0000_0000_0100 => todo!("MOV.B Rm, @(R0,Rn)"),
        0b0000_0000_0000_0101 => todo!("MOV.W Rm, @(R0,Rn)"),
        0b0000_0000_0000_0110 => todo!("MOV.L Rm, @(R0,Rn)"),
        0b0000_0000_0000_1100 => todo!("MOV.B @(R0,Rm), Rn"),
        0b0000_0000_0000_1101 => todo!("MOV.W @(R0,Rm), Rn"),
        0b0000_0000_0000_1110 => load::mov_l_indirect_indexed_rn(cpu, opcode, bus),
        0b0110_0000_0000_1000 => todo!("SWAP.B Rm, Rn"),
        0b0110_0000_0000_1001 => todo!("SWAP.W Rm, Rn"),
        0b0010_0000_0000_1101 => todo!("XTRCT Rm, Rn"),
        0b0011_0000_0000_1100 => alu::add_rm_rn(cpu, opcode),
        0b0011_0000_0000_1110 => todo!("ADDC Rm, Rn"),
        0b0011_0000_0000_1111 => todo!("ADDV Rm, Rn"),
        0b0011_0000_0000_0000 => alu::cmp_eq_rm_rn(cpu, opcode),
        0b0011_0000_0000_0010 => todo!("CMP/HS Rm, Rn"),
        0b0011_0000_0000_0011 => todo!("CMP/GE Rm, Rn"),
        0b0011_0000_0000_0110 => todo!("CMP/HI Rm, Rn"),
        0b0011_0000_0000_0111 => todo!("CMP/GT Rm, Rn"),
        0b0010_0000_0000_1100 => todo!("CMP/ST Rm, Rn"),
        0b0011_0000_0000_0100 => todo!("DIV1 Rm, Rn"),
        0b0010_0000_0000_0111 => todo!("DIV0S Rm, Rn"),
        0b0011_0000_0000_1101 => todo!("DMULS.L Rm, Rn"),
        0b0011_0000_0000_0101 => todo!("DMULU.L Rm, Rn"),
        0b0110_0000_0000_1110 => todo!("EXTS.B Rm, Rn"),
        0b0110_0000_0000_1111 => todo!("EXTS.W Rm, Rn"),
        0b0110_0000_0000_1100 => alu::extu_b(cpu, opcode),
        0b0110_0000_0000_1101 => alu::extu_w(cpu, opcode),
        0b0000_0000_0000_1111 => todo!("MAC.L @Rm+, @Rn+"),
        0b0100_0000_0000_1111 => todo!("MAC @Rm+, @Rn+"),
        0b0000_0000_0000_0111 => todo!("MUL.L Rm, Rn"),
        0b0010_0000_0000_1111 => todo!("MULS.W Rm, Rn"),
        0b0010_0000_0000_1110 => todo!("MULU.W Rm, Rn"),
        0b0110_0000_0000_1011 => todo!("NEG Rm, Rn"),
        0b0110_0000_0000_1010 => todo!("NEGC Rm, Rn"),
        0b0011_0000_0000_1000 => alu::sub_rm_rn(cpu, opcode),
        0b0011_0000_0000_1010 => todo!("SUBC Rm, Rn"),
        0b0011_0000_0000_1011 => todo!("SUBV Rm, Rn"),
        0b0010_0000_0000_1001 => bits::and_rm_rn(cpu, opcode),
        0b0110_0000_0000_0111 => todo!("NOT Rm, Rn"),
        0b0010_0000_0000_1011 => todo!("OR Rm, Rn"),
        0b0010_0000_0000_1000 => bits::tst_rm_rn(cpu, opcode),
        0b0010_0000_0000_1010 => todo!("XOR Rm, Rn"),
        _ => execute_xxnn(cpu, opcode, bus),
    }
}

#[inline]
fn execute_xxnn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode & 0b1111_1111_0000_0000 {
        0b1000_0000_0000_0000 => load::mov_b_r0_rn_displacement(cpu, opcode, bus),
        0b1000_0001_0000_0000 => load::mov_w_r0_rn_displacement(cpu, opcode, bus),
        0b1000_0100_0000_0000 => load::mov_b_rm_displacement_r0(cpu, opcode, bus),
        0b1000_0101_0000_0000 => todo!("MOV.W @(disp,Rm), R0"),
        0b1100_0000_0000_0000 => load::mov_b_r0_disp_gbr(cpu, opcode, bus),
        0b1100_0001_0000_0000 => load::mov_w_r0_disp_gbr(cpu, opcode, bus),
        0b1100_0010_0000_0000 => load::mov_l_r0_disp_gbr(cpu, opcode, bus),
        0b1100_0100_0000_0000 => load::mov_b_disp_gbr_r0(cpu, opcode, bus),
        0b1100_0101_0000_0000 => todo!("MOV.W @(disp,GBR), R0"),
        0b1100_0110_0000_0000 => load::mov_l_disp_gbr_r0(cpu, opcode, bus),
        0b1100_0111_0000_0000 => load::mova(cpu, opcode),
        0b1000_1000_0000_0000 => alu::cmp_eq_imm_r0(cpu, opcode),
        0b1100_1001_0000_0000 => todo!("AND #imm, R0"),
        0b1100_1101_0000_0000 => todo!("AND.B #imm, @(R0,GBR)"),
        0b1100_1011_0000_0000 => todo!("OR #imm, R0"),
        0b1100_1111_0000_0000 => todo!("OR.B #imm, @(R0,GBR)"),
        0b1100_1000_0000_0000 => bits::tst_imm_r0(cpu, opcode),
        0b1100_1100_0000_0000 => todo!("TST.B #imm, @(R0,GBR)"),
        0b1100_1010_0000_0000 => todo!("XOR #imm, R0"),
        0b1100_1110_0000_0000 => todo!("XOR.B #imm, @(R0,GBR)"),
        0b1000_1011_0000_0000 => branch::bf(cpu, opcode),
        0b1000_1111_0000_0000 => todo!("BF/S label"),
        0b1000_1001_0000_0000 => branch::bt(cpu, opcode),
        0b1000_1101_0000_0000 => todo!("BT/S label"),
        0b1100_0011_0000_0000 => todo!("TRAPA #imm"),
        _ => execute_xnxx(cpu, opcode, bus),
    }
}

#[inline]
fn execute_xnxx<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode & 0b1111_0000_1111_1111 {
        0b0000_0000_0010_1001 => todo!("MOVT Rn"),
        0b0100_0000_0001_0001 => todo!("CMP/PZ Rn"),
        0b0100_0000_0001_0101 => todo!("CMP/PL Rn"),
        0b0100_0000_0001_0000 => alu::dt(cpu, opcode),
        0b0100_0000_0001_1011 => todo!("TAS.B @Rn"),
        0b0100_0000_0000_0100 => todo!("ROTL Rn"),
        0b0100_0000_0000_0101 => todo!("ROTR Rn"),
        0b0100_0000_0010_0100 => todo!("ROTCL Rn"),
        0b0100_0000_0010_0101 => todo!("ROTCR Rn"),
        0b0100_0000_0010_0000 => todo!("SHAL Rn"),
        0b0100_0000_0010_0001 => todo!("SHAR Rn"),
        0b0100_0000_0000_0000 => todo!("SHLL Rn"),
        0b0100_0000_0000_0001 => bits::shlr(cpu, opcode),
        0b0100_0000_0000_1000 => bits::shlln::<2>(cpu, opcode),
        0b0100_0000_0000_1001 => todo!("SHLR2 Rn"),
        0b0100_0000_0001_1000 => bits::shlln::<8>(cpu, opcode),
        0b0100_0000_0001_1001 => todo!("SHLR8 Rn"),
        0b0100_0000_0010_1000 => bits::shlln::<16>(cpu, opcode),
        0b0100_0000_0010_1001 => todo!("SHLR16 Rn"),
        0b0000_0000_0010_0011 => todo!("BRAF Rm"),
        0b0000_0000_0000_0011 => branch::bsrf(cpu, opcode),
        0b0100_0000_0010_1011 => branch::jmp(cpu, opcode),
        0b0100_0000_0000_1011 => todo!("JSR @Rm"),
        0b0100_0000_0000_1110 => load::ldc_rm_sr(cpu, opcode),
        0b0100_0000_0001_1110 => load::ldc_rm_gbr(cpu, opcode),
        0b0100_0000_0010_1110 => load::ldc_rm_vbr(cpu, opcode),
        0b0100_0000_0000_0111 => todo!("LDC.L @Rm+, SR"),
        0b0100_0000_0001_0111 => todo!("LDC.L @Rm+, GBR"),
        0b0100_0000_0010_0111 => todo!("LDC.L @Rm+, VBR"),
        0b0100_0000_0000_1010 => todo!("LDS Rm, MACH"),
        0b0100_0000_0001_1010 => todo!("LDS Rm, MACL"),
        0b0100_0000_0010_1010 => todo!("LDS Rm, PR"),
        0b0100_0000_0000_0110 => todo!("LDS.L @Rm+, MACH"),
        0b0100_0000_0001_0110 => todo!("LDS.L @Rm+, MACL"),
        0b0100_0000_0010_0110 => todo!("LDS.L @Rm+, PR"),
        0b0000_0000_0000_0010 => todo!("STC SR, Rn"),
        0b0000_0000_0001_0010 => todo!("STC GBR, Rn"),
        0b0000_0000_0010_0010 => todo!("STC VBR, Rn"),
        0b0100_0000_0000_0011 => todo!("STC.L SR, @-Rn"),
        0b0100_0000_0001_0011 => todo!("STC.L GBR, @-Rn"),
        0b0100_0000_0010_0011 => todo!("STC.L VBR, @-Rn"),
        0b0000_0000_0000_1010 => todo!("STS MACH, Rn"),
        0b0000_0000_0001_1010 => todo!("STS MACL, Rn"),
        0b0000_0000_0010_1010 => todo!("STS PR, Rn"),
        0b0100_0000_0000_0010 => todo!("STS.L MACH, @-Rn"),
        0b0100_0000_0001_0010 => todo!("STS.L MACL, @-Rn"),
        0b0100_0000_0010_0010 => load::sts_pr_rn_predec(cpu, opcode, bus),
        _ => execute_xnnn(cpu, opcode, bus),
    }
}

#[inline]
fn execute_xnnn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode & 0b1111_0000_0000_0000 {
        0b1110_0000_0000_0000 => load::mov_b_immediate_rn(cpu, opcode),
        0b1001_0000_0000_0000 => load::mov_w_immediate_rn(cpu, opcode, bus),
        0b1101_0000_0000_0000 => load::mov_l_immediate_rn(cpu, opcode, bus),
        0b0001_0000_0000_0000 => load::mov_l_rm_rn_displacement(cpu, opcode, bus),
        0b0101_0000_0000_0000 => load::mov_l_rm_displacement_rn(cpu, opcode, bus),
        0b0111_0000_0000_0000 => alu::add_imm_rn(cpu, opcode),
        0b1010_0000_0000_0000 => branch::bra(cpu, opcode),
        0b1011_0000_0000_0000 => branch::bsr(cpu, opcode),
        _ => todo!("illegal (?) SH-2 opcode {opcode:04X}"),
    }
}

// Parse 4-bit register number from bits 8-11
// Often destination register but not always
#[inline]
fn parse_register_high(opcode: u16) -> u16 {
    (opcode >> 8) & 0xF
}

// Parse 4-bit register number from bits 4-7
// Often source register but not always
#[inline]
fn parse_register_low(opcode: u16) -> u16 {
    (opcode >> 4) & 0xF
}

#[inline]
fn parse_4bit_displacement(opcode: u16) -> u32 {
    (opcode & 0xF).into()
}

#[inline]
fn parse_8bit_displacement(opcode: u16) -> u32 {
    (opcode & 0xFF).into()
}

#[inline]
fn parse_signed_immediate(opcode: u16) -> i32 {
    (opcode as i8).into()
}

#[inline]
fn parse_unsigned_immediate(opcode: u16) -> u32 {
    (opcode & 0xFF).into()
}

#[inline]
fn parse_branch_displacement(opcode: u16) -> i32 {
    // Signed 12-bit
    (((opcode as i16) << 4) >> 4).into()
}

#[inline]
fn extend_i8(value: u8) -> u32 {
    value as i8 as u32
}

#[inline]
fn extend_i16(value: u16) -> u32 {
    value as i16 as u32
}

#[inline]
fn pc_relative_displacement_word(registers: &Sh2Registers, opcode: u16) -> u32 {
    let displacement = (opcode & 0xFF) << 1;
    registers.next_pc.wrapping_add(displacement.into())
}

#[inline]
fn pc_relative_displacement_long(registers: &Sh2Registers, opcode: u16) -> u32 {
    let displacement = (opcode & 0xFF) << 2;
    (registers.next_pc & !3).wrapping_add(displacement.into())
}
