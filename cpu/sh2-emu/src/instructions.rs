//! SH-2 instruction implementations

mod alu;
mod bits;
mod branch;
mod load;

use crate::Sh2;
use crate::bus::BusInterface;

pub fn execute<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode {
        0b0000_0000_0001_1001 => alu::div0u(cpu),
        0b0000_0000_0000_1011 => branch::rts(cpu),
        0b0000_0000_0000_1000 => load::clrt(cpu),
        0b0000_0000_0010_1000 => load::clrmac(cpu),
        // NOP
        0b0000_0000_0000_1001 => {}
        0b0000_0000_0010_1011 => branch::rte(cpu, bus),
        0b0000_0000_0001_1000 => load::sett(cpu),
        0b0000_0000_0001_1011 => todo!("SLEEP"),
        _ => execute_xnnx(cpu, opcode, bus),
    }
}

#[inline(always)]
fn execute_xnnx<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode & 0b1111_0000_0000_1111 {
        0b0110_0000_0000_0011 => load::mov_rm_rn(cpu, opcode),
        0b0010_0000_0000_0000 => load::mov_b_rm_indirect(cpu, opcode, bus),
        0b0010_0000_0000_0001 => load::mov_w_rm_indirect(cpu, opcode, bus),
        0b0010_0000_0000_0010 => load::mov_l_rm_indirect(cpu, opcode, bus),
        0b0110_0000_0000_0000 => load::mov_b_indirect_rn(cpu, opcode, bus),
        0b0110_0000_0000_0001 => load::mov_w_indirect_rn(cpu, opcode, bus),
        0b0110_0000_0000_0010 => load::mov_l_indirect_rn(cpu, opcode, bus),
        0b0010_0000_0000_0100 => load::mov_b_rm_predec(cpu, opcode, bus),
        0b0010_0000_0000_0101 => load::mov_w_rm_predec(cpu, opcode, bus),
        0b0010_0000_0000_0110 => load::mov_l_rm_predec(cpu, opcode, bus),
        0b0110_0000_0000_0100 => load::mov_b_postinc_rn(cpu, opcode, bus),
        0b0110_0000_0000_0101 => load::mov_w_postinc_rn(cpu, opcode, bus),
        0b0110_0000_0000_0110 => load::mov_l_postinc_rn(cpu, opcode, bus),
        0b0000_0000_0000_0100 => load::mov_b_rm_indirect_indexed(cpu, opcode, bus),
        0b0000_0000_0000_0101 => load::mov_w_rm_indirect_indexed(cpu, opcode, bus),
        0b0000_0000_0000_0110 => load::mov_l_rm_indirect_indexed(cpu, opcode, bus),
        0b0000_0000_0000_1100 => load::mov_b_indirect_indexed_rn(cpu, opcode, bus),
        0b0000_0000_0000_1101 => load::mov_w_indirect_indexed_rn(cpu, opcode, bus),
        0b0000_0000_0000_1110 => load::mov_l_indirect_indexed_rn(cpu, opcode, bus),
        0b0110_0000_0000_1000 => load::swap_b(cpu, opcode),
        0b0110_0000_0000_1001 => load::swap_w(cpu, opcode),
        0b0010_0000_0000_1101 => load::xtrct(cpu, opcode),
        0b0011_0000_0000_1100 => alu::add_rm_rn(cpu, opcode),
        0b0011_0000_0000_1110 => alu::addc(cpu, opcode),
        0b0011_0000_0000_1111 => alu::addv(cpu, opcode),
        0b0011_0000_0000_0000 => alu::cmp_eq_rm_rn(cpu, opcode),
        0b0011_0000_0000_0010 => alu::cmp_hs(cpu, opcode),
        0b0011_0000_0000_0011 => alu::cmp_ge(cpu, opcode),
        0b0011_0000_0000_0110 => alu::cmp_hi(cpu, opcode),
        0b0011_0000_0000_0111 => alu::cmp_gt(cpu, opcode),
        0b0010_0000_0000_1100 => alu::cmp_str(cpu, opcode),
        0b0011_0000_0000_0100 => alu::div1(cpu, opcode),
        0b0010_0000_0000_0111 => alu::div0s(cpu, opcode),
        0b0011_0000_0000_1101 => alu::dmuls(cpu, opcode),
        0b0011_0000_0000_0101 => alu::dmulu(cpu, opcode),
        0b0110_0000_0000_1110 => alu::exts_b(cpu, opcode),
        0b0110_0000_0000_1111 => alu::exts_w(cpu, opcode),
        0b0110_0000_0000_1100 => alu::extu_b(cpu, opcode),
        0b0110_0000_0000_1101 => alu::extu_w(cpu, opcode),
        0b0000_0000_0000_1111 => alu::mac_l(cpu, opcode, bus),
        0b0100_0000_0000_1111 => alu::mac_w(cpu, opcode, bus),
        0b0000_0000_0000_0111 => alu::mul(cpu, opcode),
        0b0010_0000_0000_1111 => alu::muls(cpu, opcode),
        0b0010_0000_0000_1110 => alu::mulu(cpu, opcode),
        0b0110_0000_0000_1011 => alu::neg(cpu, opcode),
        0b0110_0000_0000_1010 => alu::negc(cpu, opcode),
        0b0011_0000_0000_1000 => alu::sub_rm_rn(cpu, opcode),
        0b0011_0000_0000_1010 => alu::subc(cpu, opcode),
        0b0011_0000_0000_1011 => alu::subv(cpu, opcode),
        0b0010_0000_0000_1001 => bits::and_rm_rn(cpu, opcode),
        0b0110_0000_0000_0111 => bits::not(cpu, opcode),
        0b0010_0000_0000_1011 => bits::or_rm_rn(cpu, opcode),
        0b0010_0000_0000_1000 => bits::tst_rm_rn(cpu, opcode),
        0b0010_0000_0000_1010 => bits::xor_rm_rn(cpu, opcode),
        _ => execute_xxnn(cpu, opcode, bus),
    }
}

#[inline(always)]
fn execute_xxnn<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode & 0b1111_1111_0000_0000 {
        0b1000_0000_0000_0000 => load::mov_b_r0_rn_displacement(cpu, opcode, bus),
        0b1000_0001_0000_0000 => load::mov_w_r0_rn_displacement(cpu, opcode, bus),
        0b1000_0100_0000_0000 => load::mov_b_rm_displacement_r0(cpu, opcode, bus),
        0b1000_0101_0000_0000 => load::mov_w_rm_displacement_r0(cpu, opcode, bus),
        0b1100_0000_0000_0000 => load::mov_b_r0_disp_gbr(cpu, opcode, bus),
        0b1100_0001_0000_0000 => load::mov_w_r0_disp_gbr(cpu, opcode, bus),
        0b1100_0010_0000_0000 => load::mov_l_r0_disp_gbr(cpu, opcode, bus),
        0b1100_0100_0000_0000 => load::mov_b_disp_gbr_r0(cpu, opcode, bus),
        0b1100_0101_0000_0000 => load::mov_w_disp_gbr_r0(cpu, opcode, bus),
        0b1100_0110_0000_0000 => load::mov_l_disp_gbr_r0(cpu, opcode, bus),
        0b1100_0111_0000_0000 => load::mova(cpu, opcode),
        0b1000_1000_0000_0000 => alu::cmp_eq_imm_r0(cpu, opcode),
        0b1100_1001_0000_0000 => bits::and_imm_r0(cpu, opcode),
        0b1100_1101_0000_0000 => bits::and_imm_gbr_indexed(cpu, opcode, bus),
        0b1100_1011_0000_0000 => bits::or_imm_r0(cpu, opcode),
        0b1100_1111_0000_0000 => bits::or_imm_gbr_indexed(cpu, opcode, bus),
        0b1100_1000_0000_0000 => bits::tst_imm_r0(cpu, opcode),
        0b1100_1100_0000_0000 => bits::tst_imm_gbr_indexed(cpu, opcode, bus),
        0b1100_1010_0000_0000 => bits::xor_imm_r0(cpu, opcode),
        0b1100_1110_0000_0000 => bits::xor_imm_gbr_indexed(cpu, opcode, bus),
        0b1000_1011_0000_0000 => branch::bf(cpu, opcode),
        0b1000_1111_0000_0000 => branch::bf_s(cpu, opcode),
        0b1000_1001_0000_0000 => branch::bt(cpu, opcode),
        0b1000_1101_0000_0000 => branch::bt_s(cpu, opcode),
        0b1100_0011_0000_0000 => branch::trapa(cpu, opcode, bus),
        _ => execute_xnxx(cpu, opcode, bus),
    }
}

#[inline(always)]
fn execute_xnxx<B: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    match opcode & 0b1111_0000_1111_1111 {
        0b0000_0000_0010_1001 => load::movt(cpu, opcode),
        0b0100_0000_0001_0001 => alu::cmp_pz(cpu, opcode),
        0b0100_0000_0001_0101 => alu::cmp_pl(cpu, opcode),
        0b0100_0000_0001_0000 => alu::dt(cpu, opcode),
        0b0100_0000_0001_1011 => bits::tas(cpu, opcode, bus),
        0b0100_0000_0000_0100 => bits::rotl(cpu, opcode),
        0b0100_0000_0000_0101 => bits::rotr(cpu, opcode),
        0b0100_0000_0010_0100 => bits::rotcl(cpu, opcode),
        0b0100_0000_0010_0101 => bits::rotcr(cpu, opcode),
        // SHAL and SHLL behave identically; use SHLL implementation for both
        0b0100_0000_0010_0000 | 0b0100_0000_0000_0000 => bits::shll(cpu, opcode),
        0b0100_0000_0010_0001 => bits::shar(cpu, opcode),
        0b0100_0000_0000_0001 => bits::shlr(cpu, opcode),
        0b0100_0000_0000_1000 => bits::shlln::<2>(cpu, opcode),
        0b0100_0000_0000_1001 => bits::shlrn::<2>(cpu, opcode),
        0b0100_0000_0001_1000 => bits::shlln::<8>(cpu, opcode),
        0b0100_0000_0001_1001 => bits::shlrn::<8>(cpu, opcode),
        0b0100_0000_0010_1000 => bits::shlln::<16>(cpu, opcode),
        0b0100_0000_0010_1001 => bits::shlrn::<16>(cpu, opcode),
        0b0000_0000_0010_0011 => branch::braf(cpu, opcode),
        0b0000_0000_0000_0011 => branch::bsrf(cpu, opcode),
        0b0100_0000_0010_1011 => branch::jmp(cpu, opcode),
        0b0100_0000_0000_1011 => branch::jsr(cpu, opcode),
        0b0100_0000_0000_1110 => load::ldc_rm_sr(cpu, opcode),
        0b0100_0000_0001_1110 => load::ldc_rm_gbr(cpu, opcode),
        0b0100_0000_0010_1110 => load::ldc_rm_vbr(cpu, opcode),
        0b0100_0000_0000_0111 => load::ldc_postinc_sr(cpu, opcode, bus),
        0b0100_0000_0001_0111 => load::ldc_postinc_gbr(cpu, opcode, bus),
        0b0100_0000_0010_0111 => load::ldc_postinc_vbr(cpu, opcode, bus),
        0b0100_0000_0000_1010 => load::lds_rm_mach(cpu, opcode),
        0b0100_0000_0001_1010 => load::lds_rm_macl(cpu, opcode),
        0b0100_0000_0010_1010 => load::lds_rm_pr(cpu, opcode),
        0b0100_0000_0000_0110 => load::lds_postinc_mach(cpu, opcode, bus),
        0b0100_0000_0001_0110 => load::lds_postinc_macl(cpu, opcode, bus),
        0b0100_0000_0010_0110 => load::lds_postinc_pr(cpu, opcode, bus),
        0b0000_0000_0000_0010 => load::stc_sr_rn(cpu, opcode),
        0b0000_0000_0001_0010 => load::stc_gbr_rn(cpu, opcode),
        0b0000_0000_0010_0010 => load::stc_vbr_rn(cpu, opcode),
        0b0100_0000_0000_0011 => load::stc_sr_rn_predec(cpu, opcode, bus),
        0b0100_0000_0001_0011 => load::stc_gbr_rn_predec(cpu, opcode, bus),
        0b0100_0000_0010_0011 => load::stc_vbr_rn_predec(cpu, opcode, bus),
        0b0000_0000_0000_1010 => load::sts_mach_rn(cpu, opcode),
        0b0000_0000_0001_1010 => load::sts_macl_rn(cpu, opcode),
        0b0000_0000_0010_1010 => load::sts_pr_rn(cpu, opcode),
        0b0100_0000_0000_0010 => load::sts_mach_rn_predec(cpu, opcode, bus),
        0b0100_0000_0001_0010 => load::sts_macl_rn_predec(cpu, opcode, bus),
        0b0100_0000_0010_0010 => load::sts_pr_rn_predec(cpu, opcode, bus),
        _ => execute_xnnn(cpu, opcode, bus),
    }
}

#[inline(always)]
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
        _ => todo!(
            "[{}] illegal (?) SH-2 opcode {opcode:04X}, PC={:08X}",
            cpu.name,
            cpu.registers.pc
        ),
    }
}

#[inline(always)]
fn rn(opcode: u16) -> usize {
    ((opcode >> 8) & 0xF) as usize
}

#[inline(always)]
fn rm(opcode: u16) -> usize {
    ((opcode >> 4) & 0xF) as usize
}

#[inline(always)]
fn extend_i8(value: u8) -> u32 {
    value as i8 as u32
}

#[inline(always)]
fn extend_i16(value: u16) -> u32 {
    value as i16 as u32
}
