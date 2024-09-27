use crate::superfx::gsu::GraphicsSupportUnit;
use crate::superfx::gsu::instructions::{
    MemoryType, clear_prefix_flags, fetch_opcode, fill_cache_from_pc, fill_cache_to_pc,
    read_register,
};
use jgenesis_common::num::SignBit;

pub(super) fn link(opcode: u8, memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // LINK #n: Link return address
    let n = opcode & 0x0F;
    gsu.r[11] = gsu.r[15].wrapping_add(n.into()).wrapping_sub(1);

    clear_prefix_flags(gsu);
    memory_type.access_cycles(gsu.clock_speed)
}

macro_rules! impl_branch {
    ($name:ident $(, $flag:ident $(^ $other:ident)? == $value:expr)?) => {
        pub(super) fn $name(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit, rom: &[u8], ram: &[u8]) -> u8 {
            let e = gsu.state.opcode_buffer as i8;
            fetch_opcode(gsu, rom, ram);

            $(
                if gsu.$flag $(^ gsu.$other)? != $value {
                    return 2 * memory_type.access_cycles(gsu.clock_speed);
                }
            )?

            let cycles = fill_cache_from_pc(gsu, rom, ram);

            gsu.r[15] = gsu.r[15].wrapping_add(e as u16).wrapping_sub(1);
            gsu.state.just_jumped = true;

            cycles + 2 * memory_type.access_cycles(gsu.clock_speed)
        }
    }
}

// Branch instructions
impl_branch!(bra);
impl_branch!(bge, sign_flag ^ overflow_flag == false);
impl_branch!(blt, sign_flag ^ overflow_flag == true);
impl_branch!(bne, zero_flag == false);
impl_branch!(beq, zero_flag == true);
impl_branch!(bpl, sign_flag == false);
impl_branch!(bmi, sign_flag == true);
impl_branch!(bcc, carry_flag == false);
impl_branch!(bcs, carry_flag == true);
impl_branch!(bvc, overflow_flag == false);
impl_branch!(bvs, overflow_flag == true);

pub(super) fn jmp(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // JMP Rn: Jump
    let cycles = fill_cache_from_pc(gsu, rom, ram);

    gsu.r[15] = gsu.r[(opcode & 0x0F) as usize];
    gsu.state.just_jumped = true;

    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn ljmp(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LJMP Rn: Long jump
    gsu.r[15] = read_register(gsu, gsu.sreg);
    gsu.pbr = gsu.r[(opcode & 0x0F) as usize] as u8;

    let cbr = gsu.r[15] & 0xFFF0;
    gsu.code_cache.update_cbr(cbr);
    let cycles = fill_cache_to_pc(gsu, gsu.r[15], rom, ram);

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn loop_(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LOOP: Loop
    gsu.r[12] = gsu.r[12].wrapping_sub(1);
    gsu.zero_flag = gsu.r[12] == 0;
    gsu.sign_flag = gsu.r[12].sign_bit();

    let cycles = if !gsu.zero_flag {
        let cycles = fill_cache_from_pc(gsu, rom, ram);
        gsu.r[15] = gsu.r[13];
        gsu.state.just_jumped = true;

        cycles
    } else {
        0
    };

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}
