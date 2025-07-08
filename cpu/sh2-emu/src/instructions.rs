//! SH-2 instruction implementations

mod alu;
mod bits;
mod branch;
mod load;

use crate::Sh2;
use crate::bus::BusInterface;
use std::array;
use std::sync::LazyLock;

pub type OpcodeFn = fn(&mut Sh2, u16, &mut dyn BusInterface);

pub fn decode(opcode: u16) -> OpcodeFn {
    // Use bits 15-12 and 7-0 for the opcode lookup instead of all 16 bits to keep the lookup table
    // size down (32KB vs. 512KB for 64-bit), making it more likely that most of it will fit in cache.
    //
    // For opcodes where bits 11-8 are needed to determine which instruction function to call, the
    // returned OpcodeFn will match on those bits when called. This is only needed for opcodes where
    // the highest 4 bits are either 0b1000 or 0b1100.
    static TABLE: LazyLock<Box<[OpcodeFn; 4096]>> = LazyLock::new(|| {
        Box::new(array::from_fn(|i| {
            let opcode = (i & 0xFF) | ((i & 0xF00) << 4);
            decode_inner(opcode as u16)
        }))
    });

    TABLE[((opcode & 0xFF) | ((opcode & 0xF000) >> 4)) as usize]
}

macro_rules! cpu {
    ($($name:tt)*) => {
        |cpu, _, _| $($name)*(cpu)
    }
}

macro_rules! cpu_op {
    ($($name:tt)*) => {
        |cpu, opcode, _| $($name)*(cpu, opcode)
    }
}

macro_rules! cpu_op_bus {
    ($($name:tt)*) => {
        |cpu, opcode, bus| $($name)*(cpu, opcode, bus)
    }
}

fn nop(_cpu: &mut Sh2, _opcode: u16, _bus: &mut dyn BusInterface) {}

fn sleep(cpu: &mut Sh2, _opcode: u16, _bus: &mut dyn BusInterface) {
    // In actual hardware, SLEEP causes the CPU to enter a low-power state.
    // Since nothing uses SLEEP except for a handful of demos, use a simpler implementation that
    // is inefficient but doesn't require checking whether the CPU is sleeping after every instruction
    cpu.registers.pc = cpu.registers.pc.wrapping_sub(2);
}

fn illegal_opcode(cpu: &mut Sh2, opcode: u16, bus: &mut dyn BusInterface) {
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

fn decode_inner(opcode: u16) -> OpcodeFn {
    match opcode & 0b1111_0000_0000_1111 {
        0b0110_0000_0000_0011 => cpu_op!(load::mov_rm_rn),
        0b0010_0000_0000_0000 => cpu_op_bus!(load::mov_b_rm_indirect),
        0b0010_0000_0000_0001 => cpu_op_bus!(load::mov_w_rm_indirect),
        0b0010_0000_0000_0010 => cpu_op_bus!(load::mov_l_rm_indirect),
        0b0110_0000_0000_0000 => cpu_op_bus!(load::mov_b_indirect_rn),
        0b0110_0000_0000_0001 => cpu_op_bus!(load::mov_w_indirect_rn),
        0b0110_0000_0000_0010 => cpu_op_bus!(load::mov_l_indirect_rn),
        0b0010_0000_0000_0100 => cpu_op_bus!(load::mov_b_rm_predec),
        0b0010_0000_0000_0101 => cpu_op_bus!(load::mov_w_rm_predec),
        0b0010_0000_0000_0110 => cpu_op_bus!(load::mov_l_rm_predec),
        0b0110_0000_0000_0100 => cpu_op_bus!(load::mov_b_postinc_rn),
        0b0110_0000_0000_0101 => cpu_op_bus!(load::mov_w_postinc_rn),
        0b0110_0000_0000_0110 => cpu_op_bus!(load::mov_l_postinc_rn),
        0b0000_0000_0000_0100 => cpu_op_bus!(load::mov_b_rm_indirect_indexed),
        0b0000_0000_0000_0101 => cpu_op_bus!(load::mov_w_rm_indirect_indexed),
        0b0000_0000_0000_0110 => cpu_op_bus!(load::mov_l_rm_indirect_indexed),
        0b0000_0000_0000_1100 => cpu_op_bus!(load::mov_b_indirect_indexed_rn),
        0b0000_0000_0000_1101 => cpu_op_bus!(load::mov_w_indirect_indexed_rn),
        0b0000_0000_0000_1110 => cpu_op_bus!(load::mov_l_indirect_indexed_rn),
        0b0110_0000_0000_1000 => cpu_op!(load::swap_b),
        0b0110_0000_0000_1001 => cpu_op!(load::swap_w),
        0b0010_0000_0000_1101 => cpu_op!(load::xtrct),
        0b0011_0000_0000_1100 => cpu_op!(alu::add_rm_rn),
        0b0011_0000_0000_1110 => cpu_op!(alu::addc),
        0b0011_0000_0000_1111 => cpu_op!(alu::addv),
        0b0011_0000_0000_0000 => cpu_op!(alu::cmp_eq_rm_rn),
        0b0011_0000_0000_0010 => cpu_op!(alu::cmp_hs),
        0b0011_0000_0000_0011 => cpu_op!(alu::cmp_ge),
        0b0011_0000_0000_0110 => cpu_op!(alu::cmp_hi),
        0b0011_0000_0000_0111 => cpu_op!(alu::cmp_gt),
        0b0010_0000_0000_1100 => cpu_op!(alu::cmp_str),
        0b0011_0000_0000_0100 => cpu_op!(alu::div1),
        0b0010_0000_0000_0111 => cpu_op!(alu::div0s),
        0b0011_0000_0000_1101 => cpu_op!(alu::dmuls),
        0b0011_0000_0000_0101 => cpu_op!(alu::dmulu),
        0b0110_0000_0000_1110 => cpu_op!(alu::exts_b),
        0b0110_0000_0000_1111 => cpu_op!(alu::exts_w),
        0b0110_0000_0000_1100 => cpu_op!(alu::extu_b),
        0b0110_0000_0000_1101 => cpu_op!(alu::extu_w),
        0b0000_0000_0000_1111 => cpu_op_bus!(alu::mac_l),
        0b0100_0000_0000_1111 => cpu_op_bus!(alu::mac_w),
        0b0000_0000_0000_0111 => cpu_op!(alu::mul),
        0b0010_0000_0000_1111 => cpu_op!(alu::muls),
        0b0010_0000_0000_1110 => cpu_op!(alu::mulu),
        0b0110_0000_0000_1011 => cpu_op!(alu::neg),
        0b0110_0000_0000_1010 => cpu_op!(alu::negc),
        0b0011_0000_0000_1000 => cpu_op!(alu::sub_rm_rn),
        0b0011_0000_0000_1010 => cpu_op!(alu::subc),
        0b0011_0000_0000_1011 => cpu_op!(alu::subv),
        0b0010_0000_0000_1001 => cpu_op!(bits::and_rm_rn),
        0b0110_0000_0000_0111 => cpu_op!(bits::not),
        0b0010_0000_0000_1011 => cpu_op!(bits::or_rm_rn),
        0b0010_0000_0000_1000 => cpu_op!(bits::tst_rm_rn),
        0b0010_0000_0000_1010 => cpu_op!(bits::xor_rm_rn),
        _ => match opcode & 0b1111_0000_1111_1111 {
            0b0000_0000_0010_1001 => cpu_op!(load::movt),
            0b0100_0000_0001_0001 => cpu_op!(alu::cmp_pz),
            0b0100_0000_0001_0101 => cpu_op!(alu::cmp_pl),
            0b0100_0000_0001_0000 => cpu_op!(alu::dt),
            0b0100_0000_0001_1011 => cpu_op_bus!(bits::tas),
            0b0100_0000_0000_0100 => cpu_op!(bits::rotl),
            0b0100_0000_0000_0101 => cpu_op!(bits::rotr),
            0b0100_0000_0010_0100 => cpu_op!(bits::rotcl),
            0b0100_0000_0010_0101 => cpu_op!(bits::rotcr),
            // SHAL and SHLL behave identically; use SHLL implementation for both
            0b0100_0000_0010_0000 | 0b0100_0000_0000_0000 => cpu_op!(bits::shll),
            0b0100_0000_0010_0001 => cpu_op!(bits::shar),
            0b0100_0000_0000_0001 => cpu_op!(bits::shlr),
            0b0100_0000_0000_1000 => cpu_op!(bits::shlln::<2>),
            0b0100_0000_0000_1001 => cpu_op!(bits::shlrn::<2>),
            0b0100_0000_0001_1000 => cpu_op!(bits::shlln::<8>),
            0b0100_0000_0001_1001 => cpu_op!(bits::shlrn::<8>),
            0b0100_0000_0010_1000 => cpu_op!(bits::shlln::<16>),
            0b0100_0000_0010_1001 => cpu_op!(bits::shlrn::<16>),
            0b0000_0000_0010_0011 => cpu_op!(branch::braf),
            0b0000_0000_0000_0011 => cpu_op!(branch::bsrf),
            0b0100_0000_0010_1011 => cpu_op!(branch::jmp),
            0b0100_0000_0000_1011 => cpu_op!(branch::jsr),
            0b0100_0000_0000_1110 => cpu_op!(load::ldc_rm_sr),
            0b0100_0000_0001_1110 => cpu_op!(load::ldc_rm_gbr),
            0b0100_0000_0010_1110 => cpu_op!(load::ldc_rm_vbr),
            0b0100_0000_0000_0111 => cpu_op_bus!(load::ldc_postinc_sr),
            0b0100_0000_0001_0111 => cpu_op_bus!(load::ldc_postinc_gbr),
            0b0100_0000_0010_0111 => cpu_op_bus!(load::ldc_postinc_vbr),
            0b0100_0000_0000_1010 => cpu_op!(load::lds_rm_mach),
            0b0100_0000_0001_1010 => cpu_op!(load::lds_rm_macl),
            0b0100_0000_0010_1010 => cpu_op!(load::lds_rm_pr),
            0b0100_0000_0000_0110 => cpu_op_bus!(load::lds_postinc_mach),
            0b0100_0000_0001_0110 => cpu_op_bus!(load::lds_postinc_macl),
            0b0100_0000_0010_0110 => cpu_op_bus!(load::lds_postinc_pr),
            0b0000_0000_0000_0010 => cpu_op!(load::stc_sr_rn),
            0b0000_0000_0001_0010 => cpu_op!(load::stc_gbr_rn),
            0b0000_0000_0010_0010 => cpu_op!(load::stc_vbr_rn),
            0b0100_0000_0000_0011 => cpu_op_bus!(load::stc_sr_rn_predec),
            0b0100_0000_0001_0011 => cpu_op_bus!(load::stc_gbr_rn_predec),
            0b0100_0000_0010_0011 => cpu_op_bus!(load::stc_vbr_rn_predec),
            0b0000_0000_0000_1010 => cpu_op!(load::sts_mach_rn),
            0b0000_0000_0001_1010 => cpu_op!(load::sts_macl_rn),
            0b0000_0000_0010_1010 => cpu_op!(load::sts_pr_rn),
            0b0100_0000_0000_0010 => cpu_op_bus!(load::sts_mach_rn_predec),
            0b0100_0000_0001_0010 => cpu_op_bus!(load::sts_macl_rn_predec),
            0b0100_0000_0010_0010 => cpu_op_bus!(load::sts_pr_rn_predec),
            0b0000_0000_0001_1001 => cpu!(alu::div0u),
            0b0000_0000_0000_1011 => cpu!(branch::rts),
            0b0000_0000_0000_1000 => cpu!(load::clrt),
            0b0000_0000_0010_1000 => cpu!(load::clrmac),
            0b0000_0000_0000_1001 => nop,
            0b0000_0000_0010_1011 => |cpu, _, bus| branch::rte(cpu, bus),
            0b0000_0000_0001_1000 => cpu!(load::sett),
            0b0000_0000_0001_1011 => sleep,
            _ => match opcode & 0b1111_0000_0000_0000 {
                0b1110_0000_0000_0000 => cpu_op!(load::mov_b_immediate_rn),
                0b1001_0000_0000_0000 => cpu_op_bus!(load::mov_w_immediate_rn),
                0b1101_0000_0000_0000 => cpu_op_bus!(load::mov_l_immediate_rn),
                0b0001_0000_0000_0000 => cpu_op_bus!(load::mov_l_rm_rn_displacement),
                0b0101_0000_0000_0000 => cpu_op_bus!(load::mov_l_rm_displacement_rn),
                0b0111_0000_0000_0000 => cpu_op!(alu::add_imm_rn),
                0b1010_0000_0000_0000 => cpu_op!(branch::bra),
                0b1011_0000_0000_0000 => cpu_op!(branch::bsr),
                // Opcode begins with 0b1000; need to match on bits 11-8
                0b1000_0000_0000_0000 => |cpu, opcode, bus| match (opcode >> 8) & 0b1111 {
                    0b0000 => load::mov_b_r0_rn_displacement(cpu, opcode, bus),
                    0b0001 => load::mov_w_r0_rn_displacement(cpu, opcode, bus),
                    0b0100 => load::mov_b_rm_displacement_r0(cpu, opcode, bus),
                    0b0101 => load::mov_w_rm_displacement_r0(cpu, opcode, bus),
                    0b1000 => alu::cmp_eq_imm_r0(cpu, opcode),
                    0b1001 => branch::bt(cpu, opcode),
                    0b1011 => branch::bf(cpu, opcode),
                    0b1101 => branch::bt_s(cpu, opcode),
                    0b1111 => branch::bf_s(cpu, opcode),
                    _ => illegal_opcode(cpu, opcode, bus),
                },
                // Opcode begins with 0b1100; need to match on bits 11-8
                0b1100_0000_0000_0000 => |cpu, opcode, bus| match (opcode >> 8) & 0b1111 {
                    0b0000 => load::mov_b_r0_disp_gbr(cpu, opcode, bus),
                    0b0001 => load::mov_w_r0_disp_gbr(cpu, opcode, bus),
                    0b0010 => load::mov_l_r0_disp_gbr(cpu, opcode, bus),
                    0b0011 => branch::trapa(cpu, opcode, bus),
                    0b0100 => load::mov_b_disp_gbr_r0(cpu, opcode, bus),
                    0b0101 => load::mov_w_disp_gbr_r0(cpu, opcode, bus),
                    0b0110 => load::mov_l_disp_gbr_r0(cpu, opcode, bus),
                    0b0111 => load::mova(cpu, opcode),
                    0b1000 => bits::tst_imm_r0(cpu, opcode),
                    0b1001 => bits::and_imm_r0(cpu, opcode),
                    0b1010 => bits::xor_imm_r0(cpu, opcode),
                    0b1011 => bits::or_imm_r0(cpu, opcode),
                    0b1100 => bits::tst_imm_gbr_indexed(cpu, opcode, bus),
                    0b1101 => bits::and_imm_gbr_indexed(cpu, opcode, bus),
                    0b1110 => bits::xor_imm_gbr_indexed(cpu, opcode, bus),
                    0b1111 => bits::or_imm_gbr_indexed(cpu, opcode, bus),
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
