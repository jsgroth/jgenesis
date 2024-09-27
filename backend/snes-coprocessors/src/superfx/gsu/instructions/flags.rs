use crate::superfx::gsu::GraphicsSupportUnit;
use crate::superfx::gsu::instructions::{
    MemoryType, clear_prefix_flags, read_register, write_register,
};
use jgenesis_common::num::{GetBit, SignBit};

pub(super) fn from(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // FROM: Set source register
    // If B flag is set, this executes a MOVE that also sets flags (MOVES)
    let cycles = if !gsu.b {
        gsu.sreg = opcode & 0x0F;
        0
    } else {
        let register = opcode & 0x0F;
        let value = read_register(gsu, register);
        let cycles = write_register(gsu, gsu.dreg, value, rom, ram);

        gsu.zero_flag = value == 0;
        gsu.overflow_flag = value.bit(7);
        gsu.sign_flag = value.sign_bit();

        clear_prefix_flags(gsu);
        cycles
    };

    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn to(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // TO: Set destination register
    // If B flag is set, this executes a MOVE
    let cycles = if !gsu.b {
        gsu.dreg = opcode & 0x0F;
        0
    } else {
        let register = opcode & 0xF;
        let value = read_register(gsu, gsu.sreg);
        let cycles = write_register(gsu, register, value, rom, ram);

        clear_prefix_flags(gsu);
        cycles
    };

    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn with(opcode: u8, memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // WITH: Set source and destination registers
    let register = opcode & 0x0F;
    gsu.sreg = register;
    gsu.dreg = register;
    gsu.b = true;

    memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn alt1(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // ALT1: Set ALT1 flag (modifies instructions)
    gsu.alt1 = true;
    memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn alt2(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // ALT2: Set ALT2 flag (modifies instructions)
    gsu.alt2 = true;
    memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn alt3(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // ALT3: Set both ALT1 and ALT2 flags (modifies instructions)
    gsu.alt1 = true;
    gsu.alt2 = true;
    memory_type.access_cycles(gsu.clock_speed)
}
