use crate::superfx::gsu::instructions::{
    clear_prefix_flags, fetch_opcode, read_register, write_register, MemoryType,
};
use crate::superfx::gsu::{ClockSpeed, GraphicsSupportUnit};
use jgenesis_common::num::{SignBit, U16Ext};

pub(super) fn ldb(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LDB (Rm): Load byte from RAM
    let m = opcode & 0x0F;
    let ram_addr = (gsu.r[m as usize] as usize) & (ram.len() - 1);
    let value = ram[ram_addr];

    let cycles = write_register(gsu, gsu.dreg, value.into(), rom, ram);

    gsu.state.ram_address_buffer = ram_addr as u16;

    log::trace!("Loaded {value:02X} from RAM[{ram_addr:X}]");

    clear_prefix_flags(gsu);
    cycles
        + match (gsu.clock_speed, memory_type) {
            (ClockSpeed::Slow, MemoryType::CodeCache) => 5,
            (ClockSpeed::Fast, MemoryType::CodeCache) => 7,
            (ClockSpeed::Slow, MemoryType::Rom) => 8,
            (ClockSpeed::Fast, MemoryType::Rom) => 9,
            (ClockSpeed::Slow, MemoryType::Ram) => 10,
            (ClockSpeed::Fast, MemoryType::Ram) => 11,
        }
}

pub(super) fn ldw(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LDW (Rm): Load word from RAM
    let m = opcode & 0x0F;
    let ram_addr = (gsu.r[m as usize] as usize) & (ram.len() - 1);
    let value_lsb = ram[ram_addr];
    let value_msb = ram[ram_addr ^ 1];
    let value = u16::from_le_bytes([value_lsb, value_msb]);

    let cycles = write_register(gsu, gsu.dreg, value, rom, ram);

    log::trace!("Loaded {value:04X} from RAM[{ram_addr:X}]");

    gsu.state.ram_address_buffer = ram_addr as u16;

    clear_prefix_flags(gsu);
    cycles
        + match (gsu.clock_speed, memory_type) {
            (ClockSpeed::Slow, MemoryType::CodeCache) => 7,
            (ClockSpeed::Fast, MemoryType::CodeCache) => 11,
            (ClockSpeed::Slow, MemoryType::Rom) => 10,
            (ClockSpeed::Fast, MemoryType::Rom) => 13,
            (ClockSpeed::Slow, MemoryType::Ram) => 12,
            (ClockSpeed::Fast, MemoryType::Ram) => 15,
        }
}

pub(super) fn stb(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    ram: &mut [u8],
) -> u8 {
    // STB (Rm): Store byte in RAM
    let byte = read_register(gsu, gsu.sreg) as u8;

    let register = opcode & 0x0F;
    let ram_addr = (gsu.r[register as usize] as usize) & (ram.len() - 1);
    ram[ram_addr] = byte;

    log::trace!("Stored {byte:02X} at RAM[{ram_addr:X}]");

    let cycles = gsu.state.ram_buffer_wait_cycles;
    gsu.state.ram_buffer_wait_cycles = gsu.clock_speed.memory_access_cycles();
    gsu.state.ram_buffer_written = true;

    clear_prefix_flags(gsu);
    cycles
        + match memory_type {
            MemoryType::CodeCache => 1,
            MemoryType::Rom => 3,
            MemoryType::Ram => 6,
        }
}

pub(super) fn stw(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    ram: &mut [u8],
) -> u8 {
    // STW (Rm): Store word in RAM
    let source = read_register(gsu, gsu.sreg);
    let [source_lsb, source_msb] = source.to_le_bytes();

    let register = opcode & 0x0F;
    let ram_addr = (gsu.r[register as usize] as usize) & (ram.len() - 1);
    ram[ram_addr] = source_lsb;
    ram[ram_addr ^ 1] = source_msb;

    log::trace!("Stored {source:04X} at RAM[{ram_addr:X}]");

    let cycles = gsu.state.ram_buffer_wait_cycles;
    gsu.state.ram_buffer_wait_cycles = 2 * gsu.clock_speed.memory_access_cycles();
    gsu.state.ram_buffer_written = true;

    clear_prefix_flags(gsu);
    match memory_type {
        MemoryType::CodeCache => 1 + cycles.saturating_sub(1),
        MemoryType::Rom | MemoryType::Ram => gsu.clock_speed.memory_access_cycles() + cycles,
    }
}

pub(super) fn ibt(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // IBT Rn, #pp: Load immediate byte (sign extended to 16 bits)
    let pp = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);

    let register = opcode & 0xF;
    let value = (pp as i8) as u16;
    let write_cycles = write_register(gsu, register, value, rom, ram);

    clear_prefix_flags(gsu);
    write_cycles + 2 * memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn iwt(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // IWT Rn, #xx: Load immediate word
    let lsb = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);
    let msb = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);

    let value = u16::from_le_bytes([lsb, msb]);
    let register = opcode & 0x0F;
    let cycles = write_register(gsu, register, value, rom, ram);

    clear_prefix_flags(gsu);
    cycles + 3 * memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn lm(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LM Rn, (xx): Load from RAM
    let ram_addr_lsb = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);
    let ram_addr_msb = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);

    let ram_addr = (u16::from_le_bytes([ram_addr_lsb, ram_addr_msb]) as usize) & (ram.len() - 1);
    let value_lsb = ram[ram_addr];
    let value_msb = ram[ram_addr ^ 1];
    let value = u16::from_le_bytes([value_lsb, value_msb]);

    log::trace!("Loaded {value:04X} from RAM[{ram_addr:X}");

    let register = opcode & 0x0F;
    let cycles = write_register(gsu, register, value, rom, ram);

    gsu.state.ram_address_buffer = ram_addr as u16;

    clear_prefix_flags(gsu);
    cycles
        + match (gsu.clock_speed, memory_type) {
            (ClockSpeed::Slow, MemoryType::CodeCache) => 9,
            (ClockSpeed::Fast, MemoryType::CodeCache) => 13,
            (ClockSpeed::Slow, MemoryType::Rom) => 17,
            (ClockSpeed::Fast, MemoryType::Rom) => 24,
            (ClockSpeed::Slow, MemoryType::Ram) => 18,
            (ClockSpeed::Fast, MemoryType::Ram) => 25,
        }
}

pub(super) fn lms(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LMS Rn, (yy): Load from RAM short
    let kk = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);

    let yy = u16::from(kk) << 1;
    let ram_addr = (yy as usize) & (ram.len() - 1);
    let lsb = ram[ram_addr];
    let msb = ram[ram_addr ^ 1];
    let value = u16::from_le_bytes([lsb, msb]);

    let register = opcode & 0x0F;
    let cycles = write_register(gsu, register, value, rom, ram);

    log::trace!("Loaded {value:04X} from RAM[{ram_addr:X}]");

    gsu.state.ram_address_buffer = ram_addr as u16;

    clear_prefix_flags(gsu);
    cycles
        + match (gsu.clock_speed, memory_type) {
            (ClockSpeed::Slow, MemoryType::CodeCache) => 8,
            (ClockSpeed::Fast, MemoryType::CodeCache) => 12,
            (ClockSpeed::Slow, MemoryType::Rom | MemoryType::Ram) => 15,
            (ClockSpeed::Fast, MemoryType::Rom | MemoryType::Ram) => 20,
        }
}

pub(super) fn sm(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &mut [u8],
) -> u8 {
    // SM (xx), Rn: Store in RAM
    let ram_addr_lsb = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);
    let ram_addr_msb = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);

    let ram_addr = (u16::from_le_bytes([ram_addr_lsb, ram_addr_msb]) as usize) & (ram.len() - 1);

    let register = opcode & 0x0F;
    let [value_lsb, value_msb] = read_register(gsu, register).to_le_bytes();
    ram[ram_addr] = value_lsb;
    ram[ram_addr ^ 1] = value_msb;

    log::trace!("Stored {value_msb:02X}{value_lsb:02X} at RAM[{ram_addr:X}]");

    let cycles = gsu.state.ram_buffer_wait_cycles;
    gsu.state.ram_buffer_wait_cycles = 2 * gsu.clock_speed.memory_access_cycles();
    gsu.state.ram_buffer_written = true;

    clear_prefix_flags(gsu);
    cycles
        + match memory_type {
            MemoryType::CodeCache => 2,
            MemoryType::Rom => 9,
            MemoryType::Ram => 15,
        }
}

pub(super) fn sms(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &mut [u8],
) -> u8 {
    // SMS (yy), Rn: Store in RAM short
    let kk = gsu.state.opcode_buffer;
    fetch_opcode(gsu, rom, ram);

    let yy = u16::from(kk) << 1;
    let ram_addr = (yy as usize) & (ram.len() - 1);

    let register = opcode & 0x0F;
    let [lsb, msb] = read_register(gsu, register).to_le_bytes();
    ram[ram_addr] = lsb;
    ram[ram_addr ^ 1] = msb;

    log::trace!("Stored {msb:02X}{lsb:02X} at RAM[{ram_addr:X}]");

    let cycles = gsu.state.ram_buffer_wait_cycles;
    gsu.state.ram_buffer_wait_cycles = 2 * gsu.clock_speed.memory_access_cycles();
    gsu.state.ram_buffer_written = true;

    clear_prefix_flags(gsu);
    cycles
        + match memory_type {
            MemoryType::CodeCache => 1,
            MemoryType::Rom => 6,
            MemoryType::Ram => 10,
        }
}

pub(super) fn sbk(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit, ram: &mut [u8]) -> u8 {
    // SBK: Store word in RAM at last address used
    let [source_lsb, source_msb] = read_register(gsu, gsu.sreg).to_le_bytes();
    let ram_addr = gsu.state.ram_address_buffer;
    ram[ram_addr as usize] = source_lsb;
    ram[(ram_addr ^ 1) as usize] = source_msb;

    log::trace!("Stored {source_msb:02X}{source_lsb:02X} at RAM[{ram_addr:X}]");

    let cycles = gsu.state.ram_buffer_wait_cycles;
    gsu.state.ram_buffer_wait_cycles = 2 * gsu.clock_speed.memory_access_cycles();
    gsu.state.ram_buffer_written = true;

    clear_prefix_flags(gsu);
    match memory_type {
        MemoryType::CodeCache => 1 + cycles.saturating_sub(1),
        MemoryType::Rom | MemoryType::Ram => gsu.clock_speed.memory_access_cycles() + cycles,
    }
}

pub(super) fn romb(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // ROMB: Set ROM bank register
    gsu.rombr = read_register(gsu, gsu.sreg) as u8;

    log::trace!("  Set ROMBR to {:02X}", gsu.rombr);

    let cycles = gsu.state.rom_buffer_wait_cycles;
    gsu.state.rom_buffer_wait_cycles = 0;

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn getb(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // GETB/GETBH/GETBL/GETBS: Get byte from ROM
    // !ALT1 && !ALT2: GETB (Write to LSB, zero extend)
    // ALT1 && !ALT2: GETBH (Write to MSB, leave LSB unchanged)
    // !ALT1 && ALT2: GETBL (Write to LSB, leave MSB unchanged)
    // ALT1 && ALT2: GETBS (Write to LSB, sign extend)
    let byte = gsu.state.rom_buffer;
    let source = read_register(gsu, gsu.sreg);

    let value = match (gsu.alt1, gsu.alt2) {
        (false, false) => u16::from(byte),
        (true, false) => u16::from_le_bytes([source.lsb(), byte]),
        (false, true) => u16::from_le_bytes([byte, source.msb()]),
        (true, true) => byte as i8 as u16,
    };

    let mut cycles = write_register(gsu, gsu.dreg, value, rom, ram);

    cycles += gsu.state.rom_buffer_wait_cycles;
    gsu.state.rom_buffer_wait_cycles = 0;

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn hib(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // HIB: High byte of register
    let source = read_register(gsu, gsu.sreg);
    let source_msb = source.msb();
    let cycles = write_register(gsu, gsu.dreg, source_msb.into(), rom, ram);

    gsu.zero_flag = source_msb == 0;
    gsu.sign_flag = source_msb.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn lob(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LOB: Low byte of register
    let source = read_register(gsu, gsu.sreg);
    let source_lsb = source as u8;
    let cycles = write_register(gsu, gsu.dreg, source_lsb.into(), rom, ram);

    gsu.zero_flag = source_lsb == 0;
    gsu.sign_flag = source_lsb.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn swap(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // SWAP: Swap bytes
    let source = read_register(gsu, gsu.sreg);
    let swapped = source.swap_bytes();
    let cycles = write_register(gsu, gsu.dreg, swapped, rom, ram);

    gsu.zero_flag = swapped == 0;
    gsu.sign_flag = swapped.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn merge(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // MERGE: Merge high bytes
    // Dreg.H = R7.H, Dreg.L = R8.H
    let merged = (gsu.r[7] & 0xFF00) | (gsu.r[8] >> 8);
    let cycles = write_register(gsu, gsu.dreg, merged, rom, ram);

    gsu.zero_flag = merged & 0xF0F0 != 0;
    gsu.carry_flag = merged & 0xE0E0 != 0;
    gsu.sign_flag = merged & 0x8080 != 0;
    gsu.overflow_flag = merged & 0xC0C0 != 0;

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}
