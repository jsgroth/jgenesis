//! SH-2 instruction implementations

mod alu;
mod bits;
mod branch;
mod load;

use crate::Sh2;
use crate::bus::BusInterface;
use std::array;

pub type OpcodeFn<Bus> = fn(&mut Sh2, u16, &mut Bus);

pub struct OpcodeTable<Bus: BusInterface>(Box<[OpcodeFn<Bus>; 4096]>);

impl<Bus: BusInterface> Default for OpcodeTable<Bus> {
    fn default() -> Self {
        // Use bits 15-12 and 7-0 for the opcode lookup instead of all 16 bits to keep the lookup table
        // size down (32KB vs. 512KB for 64-bit), making it more likely that most of it will fit in cache.
        //
        // For opcodes where bits 11-8 are needed to determine which instruction function to call, the
        // returned OpcodeFn will match on those bits when called. This is only needed for opcodes where
        // the highest 4 bits are either 0b1000 or 0b1100.
        Self(Box::new(array::from_fn(|i| {
            let opcode = (i & 0xFF) | ((i & 0xF00) << 4);
            decode_inner(opcode as u16)
        })))
    }
}

impl<Bus: BusInterface> OpcodeTable<Bus> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn decode(&self, opcode: u16) -> OpcodeFn<Bus> {
        self.0[((opcode & 0xFF) | ((opcode & 0xF000) >> 4)) as usize]
    }
}

impl<Bus: BusInterface> Clone for OpcodeTable<Bus> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

macro_rules! cpu {
    ($($name:tt)*) => {
        |cpu, _, _| Sh2::$($name)*(cpu)
    }
}

macro_rules! cpu_op {
    ($($name:tt)*) => {
        |cpu, opcode, _| Sh2::$($name)*(cpu, opcode)
    }
}

fn nop<Bus: BusInterface>(_cpu: &mut Sh2, _opcode: u16, _bus: &mut Bus) {}

fn sleep<Bus: BusInterface>(cpu: &mut Sh2, _opcode: u16, _bus: &mut Bus) {
    // In actual hardware, SLEEP causes the CPU to enter a low-power state.
    // Since nothing uses SLEEP except for a handful of demos, use a simpler implementation that
    // is inefficient but doesn't require checking whether the CPU is sleeping after every instruction
    cpu.registers.next_pc = cpu.registers.pc;
    cpu.registers.pc = cpu.registers.pc.wrapping_sub(2);
}

fn illegal_opcode<Bus: BusInterface>(cpu: &mut Sh2, opcode: u16, bus: &mut Bus) {
    const ILLEGAL_OPCODE_VECTOR_NUMBER: u32 = 4;

    // Roll back PC to point to the illegal opcode
    cpu.registers.pc = cpu.registers.pc.wrapping_sub(2);

    log::error!(
        "[{}] SH-2 executed illegal opcode {opcode:04X}, PC={:08X}",
        cpu.name,
        cpu.registers.pc
    );

    cpu.handle_exception(None, ILLEGAL_OPCODE_VECTOR_NUMBER, bus);
}

fn decode_inner<Bus: BusInterface>(opcode: u16) -> OpcodeFn<Bus> {
    match opcode & 0b1111_0000_0000_1111 {
        0b0110_0000_0000_0011 => cpu_op!(mov_rm_rn),
        0b0010_0000_0000_0000 => Sh2::mov_b_rm_indirect,
        0b0010_0000_0000_0001 => Sh2::mov_w_rm_indirect,
        0b0010_0000_0000_0010 => Sh2::mov_l_rm_indirect,
        0b0110_0000_0000_0000 => Sh2::mov_b_indirect_rn,
        0b0110_0000_0000_0001 => Sh2::mov_w_indirect_rn,
        0b0110_0000_0000_0010 => Sh2::mov_l_indirect_rn,
        0b0010_0000_0000_0100 => Sh2::mov_b_rm_predec,
        0b0010_0000_0000_0101 => Sh2::mov_w_rm_predec,
        0b0010_0000_0000_0110 => Sh2::mov_l_rm_predec,
        0b0110_0000_0000_0100 => Sh2::mov_b_postinc_rn,
        0b0110_0000_0000_0101 => Sh2::mov_w_postinc_rn,
        0b0110_0000_0000_0110 => Sh2::mov_l_postinc_rn,
        0b0000_0000_0000_0100 => Sh2::mov_b_rm_indirect_indexed,
        0b0000_0000_0000_0101 => Sh2::mov_w_rm_indirect_indexed,
        0b0000_0000_0000_0110 => Sh2::mov_l_rm_indirect_indexed,
        0b0000_0000_0000_1100 => Sh2::mov_b_indirect_indexed_rn,
        0b0000_0000_0000_1101 => Sh2::mov_w_indirect_indexed_rn,
        0b0000_0000_0000_1110 => Sh2::mov_l_indirect_indexed_rn,
        0b0110_0000_0000_1000 => cpu_op!(swap_b),
        0b0110_0000_0000_1001 => cpu_op!(swap_w),
        0b0010_0000_0000_1101 => cpu_op!(xtrct),
        0b0011_0000_0000_1100 => cpu_op!(add_rm_rn),
        0b0011_0000_0000_1110 => cpu_op!(addc),
        0b0011_0000_0000_1111 => cpu_op!(addv),
        0b0011_0000_0000_0000 => cpu_op!(cmp_eq_rm_rn),
        0b0011_0000_0000_0010 => cpu_op!(cmp_hs),
        0b0011_0000_0000_0011 => cpu_op!(cmp_ge),
        0b0011_0000_0000_0110 => cpu_op!(cmp_hi),
        0b0011_0000_0000_0111 => cpu_op!(cmp_gt),
        0b0010_0000_0000_1100 => cpu_op!(cmp_str),
        0b0011_0000_0000_0100 => cpu_op!(div1),
        0b0010_0000_0000_0111 => cpu_op!(div0s),
        0b0011_0000_0000_1101 => cpu_op!(dmuls),
        0b0011_0000_0000_0101 => cpu_op!(dmulu),
        0b0110_0000_0000_1110 => cpu_op!(exts_b),
        0b0110_0000_0000_1111 => cpu_op!(exts_w),
        0b0110_0000_0000_1100 => cpu_op!(extu_b),
        0b0110_0000_0000_1101 => cpu_op!(extu_w),
        0b0000_0000_0000_1111 => Sh2::mac_l,
        0b0100_0000_0000_1111 => Sh2::mac_w,
        0b0000_0000_0000_0111 => cpu_op!(mul),
        0b0010_0000_0000_1111 => cpu_op!(muls),
        0b0010_0000_0000_1110 => cpu_op!(mulu),
        0b0110_0000_0000_1011 => cpu_op!(neg),
        0b0110_0000_0000_1010 => cpu_op!(negc),
        0b0011_0000_0000_1000 => cpu_op!(sub_rm_rn),
        0b0011_0000_0000_1010 => cpu_op!(subc),
        0b0011_0000_0000_1011 => cpu_op!(subv),
        0b0010_0000_0000_1001 => cpu_op!(and_rm_rn),
        0b0110_0000_0000_0111 => cpu_op!(not),
        0b0010_0000_0000_1011 => cpu_op!(or_rm_rn),
        0b0010_0000_0000_1000 => cpu_op!(tst_rm_rn),
        0b0010_0000_0000_1010 => cpu_op!(xor_rm_rn),
        _ => match opcode & 0b1111_0000_1111_1111 {
            0b0000_0000_0010_1001 => cpu_op!(movt),
            0b0100_0000_0001_0001 => cpu_op!(cmp_pz),
            0b0100_0000_0001_0101 => cpu_op!(cmp_pl),
            0b0100_0000_0001_0000 => cpu_op!(dt),
            0b0100_0000_0001_1011 => Sh2::tas,
            0b0100_0000_0000_0100 => cpu_op!(rotl),
            0b0100_0000_0000_0101 => cpu_op!(rotr),
            0b0100_0000_0010_0100 => cpu_op!(rotcl),
            0b0100_0000_0010_0101 => cpu_op!(rotcr),
            // SHAL and SHLL behave identically; use SHLL implementation for both
            0b0100_0000_0010_0000 | 0b0100_0000_0000_0000 => cpu_op!(shll),
            0b0100_0000_0010_0001 => cpu_op!(shar),
            0b0100_0000_0000_0001 => cpu_op!(shlr),
            0b0100_0000_0000_1000 => cpu_op!(shlln::<2>),
            0b0100_0000_0000_1001 => cpu_op!(shlrn::<2>),
            0b0100_0000_0001_1000 => cpu_op!(shlln::<8>),
            0b0100_0000_0001_1001 => cpu_op!(shlrn::<8>),
            0b0100_0000_0010_1000 => cpu_op!(shlln::<16>),
            0b0100_0000_0010_1001 => cpu_op!(shlrn::<16>),
            0b0000_0000_0010_0011 => Sh2::braf,
            0b0000_0000_0000_0011 => Sh2::bsrf,
            0b0100_0000_0010_1011 => Sh2::jmp,
            0b0100_0000_0000_1011 => Sh2::jsr,
            0b0100_0000_0000_1110 => cpu_op!(ldc_rm_sr),
            0b0100_0000_0001_1110 => cpu_op!(ldc_rm_gbr),
            0b0100_0000_0010_1110 => cpu_op!(ldc_rm_vbr),
            0b0100_0000_0000_0111 => Sh2::ldc_postinc_sr,
            0b0100_0000_0001_0111 => Sh2::ldc_postinc_gbr,
            0b0100_0000_0010_0111 => Sh2::ldc_postinc_vbr,
            0b0100_0000_0000_1010 => cpu_op!(lds_rm_mach),
            0b0100_0000_0001_1010 => cpu_op!(lds_rm_macl),
            0b0100_0000_0010_1010 => cpu_op!(lds_rm_pr),
            0b0100_0000_0000_0110 => Sh2::lds_postinc_mach,
            0b0100_0000_0001_0110 => Sh2::lds_postinc_macl,
            0b0100_0000_0010_0110 => Sh2::lds_postinc_pr,
            0b0000_0000_0000_0010 => cpu_op!(stc_sr_rn),
            0b0000_0000_0001_0010 => cpu_op!(stc_gbr_rn),
            0b0000_0000_0010_0010 => cpu_op!(stc_vbr_rn),
            0b0100_0000_0000_0011 => Sh2::stc_sr_rn_predec,
            0b0100_0000_0001_0011 => Sh2::stc_gbr_rn_predec,
            0b0100_0000_0010_0011 => Sh2::stc_vbr_rn_predec,
            0b0000_0000_0000_1010 => cpu_op!(sts_mach_rn),
            0b0000_0000_0001_1010 => cpu_op!(sts_macl_rn),
            0b0000_0000_0010_1010 => cpu_op!(sts_pr_rn),
            0b0100_0000_0000_0010 => Sh2::sts_mach_rn_predec,
            0b0100_0000_0001_0010 => Sh2::sts_macl_rn_predec,
            0b0100_0000_0010_0010 => Sh2::sts_pr_rn_predec,
            0b0000_0000_0001_1001 => cpu!(div0u),
            0b0000_0000_0000_1011 => |cpu, _, bus| cpu.rts(bus),
            0b0000_0000_0000_1000 => cpu!(clrt),
            0b0000_0000_0010_1000 => cpu!(clrmac),
            0b0000_0000_0000_1001 => nop,
            0b0000_0000_0010_1011 => |cpu, _, bus| cpu.rte(bus),
            0b0000_0000_0001_1000 => cpu!(sett),
            0b0000_0000_0001_1011 => sleep,
            _ => match opcode & 0b1111_0000_0000_0000 {
                0b1110_0000_0000_0000 => cpu_op!(mov_b_immediate_rn),
                0b1001_0000_0000_0000 => Sh2::mov_w_immediate_rn,
                0b1101_0000_0000_0000 => Sh2::mov_l_immediate_rn,
                0b0001_0000_0000_0000 => Sh2::mov_l_rm_rn_displacement,
                0b0101_0000_0000_0000 => Sh2::mov_l_rm_displacement_rn,
                0b0111_0000_0000_0000 => cpu_op!(add_imm_rn),
                0b1010_0000_0000_0000 => Sh2::bra,
                0b1011_0000_0000_0000 => Sh2::bsr,
                // Opcode begins with 0b1000; need to match on bits 11-8
                0b1000_0000_0000_0000 => |cpu, opcode, bus| match (opcode >> 8) & 0b1111 {
                    0b0000 => cpu.mov_b_r0_rn_displacement(opcode, bus),
                    0b0001 => cpu.mov_w_r0_rn_displacement(opcode, bus),
                    0b0100 => cpu.mov_b_rm_displacement_r0(opcode, bus),
                    0b0101 => cpu.mov_w_rm_displacement_r0(opcode, bus),
                    0b1000 => cpu.cmp_eq_imm_r0(opcode),
                    0b1001 => cpu.bt(opcode, bus),
                    0b1011 => cpu.bf(opcode, bus),
                    0b1101 => cpu.bt_s(opcode, bus),
                    0b1111 => cpu.bf_s(opcode, bus),
                    _ => illegal_opcode(cpu, opcode, bus),
                },
                // Opcode begins with 0b1100; need to match on bits 11-8
                0b1100_0000_0000_0000 => |cpu, opcode, bus| match (opcode >> 8) & 0b1111 {
                    0b0000 => cpu.mov_b_r0_disp_gbr(opcode, bus),
                    0b0001 => cpu.mov_w_r0_disp_gbr(opcode, bus),
                    0b0010 => cpu.mov_l_r0_disp_gbr(opcode, bus),
                    0b0011 => cpu.trapa(opcode, bus),
                    0b0100 => cpu.mov_b_disp_gbr_r0(opcode, bus),
                    0b0101 => cpu.mov_w_disp_gbr_r0(opcode, bus),
                    0b0110 => cpu.mov_l_disp_gbr_r0(opcode, bus),
                    0b0111 => cpu.mova(opcode),
                    0b1000 => cpu.tst_imm_r0(opcode),
                    0b1001 => cpu.and_imm_r0(opcode),
                    0b1010 => cpu.xor_imm_r0(opcode),
                    0b1011 => cpu.or_imm_r0(opcode),
                    0b1100 => cpu.tst_imm_gbr_indexed(opcode, bus),
                    0b1101 => cpu.and_imm_gbr_indexed(opcode, bus),
                    0b1110 => cpu.xor_imm_gbr_indexed(opcode, bus),
                    0b1111 => cpu.or_imm_gbr_indexed(opcode, bus),
                    _ => unreachable!("value & 0b1111 will always be one of the above values"),
                },
                _ => illegal_opcode,
            },
        },
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
