//! Assumptions made to simplify implementation at the cost of timing accuracy (and possibly bus contention accuracy):
//!   * Every instruction's opcode and operands will be located within the same memory area (code cache / ROM / RAM)
//!   * If an instruction's opcode is cached, the operands are also cached

mod alu;
mod disassemble;
mod flags;
mod flow;
mod load;
mod plot;

use crate::superfx;
use crate::superfx::gsu::{BusAccess, ClockSpeed, GraphicsSupportUnit};

pub use plot::PlotState;

pub fn execute(gsu: &mut GraphicsSupportUnit, rom: &[u8], ram: &mut [u8]) -> u8 {
    let memory_type = next_opcode_memory_type(gsu);
    let opcode = gsu.state.opcode_buffer;
    if (gsu.rom_access == BusAccess::Snes
        && (memory_type == MemoryType::Rom || is_rom_access_opcode(opcode)))
        || (gsu.ram_access == BusAccess::Snes
            && (memory_type == MemoryType::Ram || is_ram_access_opcode(opcode)))
    {
        // GSU is waiting for ROM/RAM access
        return 1;
    }

    let mut cycles = 0;

    if gsu.state.just_jumped {
        gsu.state.just_jumped = false;
        cycles += fill_cache_to_pc(gsu, gsu.r[15], rom, ram);
    }

    if memory_type == MemoryType::Rom && gsu.state.rom_buffer_wait_cycles != 0 {
        cycles += gsu.state.rom_buffer_wait_cycles;
        gsu.state.rom_buffer_wait_cycles = 0;
    }

    if memory_type == MemoryType::Ram && gsu.state.ram_buffer_wait_cycles != 0 {
        cycles += gsu.state.ram_buffer_wait_cycles;
        gsu.state.ram_buffer_wait_cycles = 0;
    }

    log::trace!(
        "Executing opcode {opcode:02X} ({}); PBR={:02X}, R15={:04X}",
        disassemble::instruction_str(opcode, gsu.alt1, gsu.alt2),
        gsu.pbr,
        gsu.r[15],
    );
    log::trace!(
        "  R0={:04X}, R1={:04X}, R2={:04X}, R3={:04X}, R4={:04X}, R5={:04X}, R6={:04X}, R7={:04X}, R8={:04X}, R9={:04X}, R10={:04X}, R11={:04X}, R12={:04X}, R13={:04X}, R14={:04X}, Z={}, C={}, S={}, OV={}",
        gsu.r[0],
        gsu.r[1],
        gsu.r[2],
        gsu.r[3],
        gsu.r[4],
        gsu.r[5],
        gsu.r[6],
        gsu.r[7],
        gsu.r[8],
        gsu.r[9],
        gsu.r[10],
        gsu.r[11],
        gsu.r[12],
        gsu.r[13],
        gsu.r[14],
        u8::from(gsu.zero_flag),
        u8::from(gsu.carry_flag),
        u8::from(gsu.sign_flag),
        u8::from(gsu.overflow_flag)
    );

    fetch_opcode(gsu, rom, ram);
    cycles += execute_opcode(opcode, memory_type, gsu, rom, ram);

    log::trace!("  Cycle count: {cycles}");

    if gsu.state.rom_pointer_changed {
        gsu.state.rom_pointer_changed = false;
    } else {
        gsu.state.rom_buffer_wait_cycles = gsu.state.rom_buffer_wait_cycles.saturating_sub(cycles);
    }

    if gsu.state.ram_buffer_written {
        gsu.state.ram_buffer_written = false;
    } else {
        gsu.state.ram_buffer_wait_cycles = gsu.state.ram_buffer_wait_cycles.saturating_sub(cycles);
    }

    gsu.plot_state.tick(cycles);

    cycles
}

fn is_rom_access_opcode(opcode: u8) -> bool {
    // GETB/GETBH/GETBL/GETBS ($EF)
    // GETC/ROMB ($DF)
    opcode == 0xDF || opcode == 0xEF
}

fn is_ram_access_opcode(opcode: u8) -> bool {
    // STB/STW ($3x for x=0-B)
    // LDB/LDW ($4x for x=0-B)
    // LM/SM ($Fn for n=0-F)
    // LMS/SMS ($An for n=0-F)
    // SBK ($90)
    matches!(opcode, 0x30..=0x3B | 0x40..=0x4B | 0x90 | 0xA0..=0xAF | 0xF0..=0xFF)
}

fn u24_address(bank: u8, offset: u16) -> u32 {
    (u32::from(bank) << 16) | u32::from(offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryType {
    CodeCache,
    Rom,
    Ram,
}

impl MemoryType {
    fn access_cycles(self, clock_speed: ClockSpeed) -> u8 {
        match self {
            Self::CodeCache => 1,
            Self::Rom | Self::Ram => clock_speed.memory_access_cycles(),
        }
    }
}

fn read_memory(bank: u8, address: u16, rom: &[u8], ram: &[u8]) -> (u8, MemoryType) {
    match bank {
        0x00..=0x3F => {
            // ROM, LoROM mapping (mirrored in $0000-$7FFF and $8000-$FFFF)
            let rom_addr = superfx::map_lorom_address(u24_address(bank, address), rom.len() as u32);
            (rom[rom_addr as usize], MemoryType::Rom)
        }
        0x40..=0x5F => {
            // ROM, HiROM mapping
            let rom_addr = superfx::map_hirom_address(u24_address(bank, address), rom.len() as u32);
            (rom[rom_addr as usize], MemoryType::Rom)
        }
        0x70..=0x71 => {
            // RAM
            // Ignore bank since no existing Super FX cartridges have more than 64KB of RAM
            let ram_addr = (address as usize) & (ram.len() - 1);
            (ram[ram_addr], MemoryType::Ram)
        }
        _ => {
            log::error!("GSU read unmapped address: ${bank:02X}:{address:04X}");
            (0, MemoryType::CodeCache)
        }
    }
}

fn fetch_opcode(gsu: &mut GraphicsSupportUnit, rom: &[u8], ram: &[u8]) {
    if gsu.pbr == 0x00 && gsu.r[15] < 0x0200 {
        // Executing from code cache
        let opcode = gsu.code_cache.read_ram(gsu.r[15]);
        gsu.state.opcode_buffer = opcode;
        gsu.r[15] = gsu.r[15].wrapping_add(1);
        return;
    }

    let is_cacheable = gsu.code_cache.pc_is_cacheable(gsu.r[15]);
    if is_cacheable {
        if let Some(opcode) = gsu.code_cache.get(gsu.r[15]) {
            gsu.state.opcode_buffer = opcode;
            gsu.r[15] = gsu.r[15].wrapping_add(1);
            return;
        }
    }

    let (opcode, _) = read_memory(gsu.pbr, gsu.r[15], rom, ram);
    gsu.state.opcode_buffer = opcode;

    log::trace!("  Read opcode {opcode:02X}");

    if is_cacheable {
        gsu.code_cache.set(gsu.r[15], opcode);
    }

    gsu.r[15] = gsu.r[15].wrapping_add(1);
}

fn next_opcode_memory_type(gsu: &GraphicsSupportUnit) -> MemoryType {
    if gsu.pbr == 0x00 && gsu.r[15] < 0x0200 {
        return MemoryType::CodeCache;
    }

    if gsu.code_cache.pc_is_cacheable(gsu.r[15]) && gsu.code_cache.get(gsu.r[15]).is_some() {
        MemoryType::CodeCache
    } else {
        match gsu.pbr {
            0x00..=0x5F => MemoryType::Rom,
            0x70..=0x71 => MemoryType::Ram,
            _ => panic!("invalid GSU bank {:02X}", gsu.pbr),
        }
    }
}

#[must_use]
fn fill_cache_to_pc(gsu: &mut GraphicsSupportUnit, pc: u16, rom: &[u8], ram: &[u8]) -> u8 {
    if gsu.pbr == 0x00 && pc < 0x0200 {
        // Executing in code cache
        return 0;
    }

    if !gsu.code_cache.pc_is_cacheable(pc) || gsu.code_cache.get(pc).is_some() {
        // Not cacheable or already cached
        return 0;
    }

    for i in 0..(pc & 0xF) {
        let cache_addr = (pc & 0xFFF0) | i;
        let (opcode, _) = read_memory(gsu.pbr, cache_addr, rom, ram);
        gsu.code_cache.set(cache_addr, opcode);
    }

    gsu.clock_speed.memory_access_cycles() * (pc & 0xF) as u8
}

#[must_use]
fn cache_at_pc(gsu: &mut GraphicsSupportUnit, pc: u16, rom: &[u8], ram: &[u8]) -> u8 {
    if gsu.pbr == 0x00 && pc < 0x0200 {
        // Executing in code cache
        return 0;
    }

    if !gsu.code_cache.pc_is_cacheable(pc) || gsu.code_cache.get(pc).is_some() {
        // Not cacheable or already cached
        return 0;
    }

    let (opcode, _) = read_memory(gsu.pbr, pc, rom, ram);
    gsu.code_cache.set(pc, opcode);

    gsu.clock_speed.memory_access_cycles()
}

#[must_use]
fn fill_cache_from_pc(gsu: &mut GraphicsSupportUnit, rom: &[u8], ram: &[u8]) -> u8 {
    if gsu.r[15] & 0xF == 0x0 {
        // PC is at the beginning of a cache line; no need to fill
        return 0;
    }

    if gsu.pbr == 0x00 && gsu.r[15] < 0x0200 {
        // Executing in code cache
        return 0;
    }

    if !gsu.code_cache.pc_is_cacheable(gsu.r[15]) || gsu.code_cache.get(gsu.r[15]).is_some() {
        // Not cacheable or already cached
        return 0;
    }

    for i in (gsu.r[15] & 0xF)..0x10 {
        let cache_addr = (gsu.r[15] & 0xFFF0) | i;
        let (opcode, _) = read_memory(gsu.pbr, cache_addr, rom, ram);
        gsu.code_cache.set(cache_addr, opcode);
    }

    gsu.clock_speed.memory_access_cycles() * (0x10 - (gsu.r[15] & 0xF) as u8)
}

fn read_register(gsu: &GraphicsSupportUnit, register: u8) -> u16 {
    match register {
        // Subtract 1 from R15 to account for PC increment happening concurrently with execution
        15 => gsu.r[15].wrapping_sub(1),
        _ => gsu.r[register as usize],
    }
}

#[must_use]
fn write_register(
    gsu: &mut GraphicsSupportUnit,
    register: u8,
    value: u16,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    let cycles = if register == 14 {
        // Writing to R14 triggers a ROM buffer reload
        // Note that changing ROMBR does *not* reload the ROM buffer until R14 is written to
        let (byte, _) = read_memory(gsu.rombr, value, rom, ram);
        gsu.state.rom_buffer = byte;
        gsu.state.rom_buffer_wait_cycles = gsu.clock_speed.rom_buffer_wait_cycles();
        gsu.state.rom_pointer_changed = true;

        0
    } else if register == 15 {
        // Writing to R15 fills out the remainder of the current cache line
        gsu.state.just_jumped = true;
        fill_cache_from_pc(gsu, rom, ram)
    } else {
        0
    };

    gsu.r[register as usize] = value;

    log::trace!("  Wrote {value:04X} to R{register}");

    cycles
}

fn clear_prefix_flags(gsu: &mut GraphicsSupportUnit) {
    gsu.alt1 = false;
    gsu.alt2 = false;
    gsu.b = false;
    gsu.sreg = 0;
    gsu.dreg = 0;
}

fn execute_opcode(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &mut [u8],
) -> u8 {
    match opcode {
        0x00 => stop(memory_type, gsu),
        0x01 => nop(memory_type, gsu),
        0x02 => cache(memory_type, gsu, rom, ram),
        0x03 => alu::lsr(memory_type, gsu, rom, ram),
        0x04 => alu::rol(memory_type, gsu, rom, ram),
        0x05 => flow::bra(memory_type, gsu, rom, ram),
        0x06 => flow::bge(memory_type, gsu, rom, ram),
        0x07 => flow::blt(memory_type, gsu, rom, ram),
        0x08 => flow::bne(memory_type, gsu, rom, ram),
        0x09 => flow::beq(memory_type, gsu, rom, ram),
        0x0A => flow::bpl(memory_type, gsu, rom, ram),
        0x0B => flow::bmi(memory_type, gsu, rom, ram),
        0x0C => flow::bcc(memory_type, gsu, rom, ram),
        0x0D => flow::bcs(memory_type, gsu, rom, ram),
        0x0E => flow::bvc(memory_type, gsu, rom, ram),
        0x0F => flow::bvs(memory_type, gsu, rom, ram),
        0x10..=0x1F => flags::to(opcode, memory_type, gsu, rom, ram),
        0x20..=0x2F => flags::with(opcode, memory_type, gsu),
        0x30..=0x3B => {
            if gsu.alt1 {
                load::stb(opcode, memory_type, gsu, ram)
            } else {
                load::stw(opcode, memory_type, gsu, ram)
            }
        }
        0x3C => flow::loop_(memory_type, gsu, rom, ram),
        0x3D => flags::alt1(memory_type, gsu),
        0x3E => flags::alt2(memory_type, gsu),
        0x3F => flags::alt3(memory_type, gsu),
        0x40..=0x4B => {
            if gsu.alt1 {
                load::ldb(opcode, memory_type, gsu, rom, ram)
            } else {
                load::ldw(opcode, memory_type, gsu, rom, ram)
            }
        }
        0x4C => {
            if gsu.alt1 {
                plot::rpix(memory_type, gsu, rom, ram)
            } else {
                plot::plot(memory_type, gsu, ram)
            }
        }
        0x4D => load::swap(memory_type, gsu, rom, ram),
        0x4E => {
            if gsu.alt1 {
                plot::cmode(memory_type, gsu)
            } else {
                plot::color(memory_type, gsu)
            }
        }
        0x4F => alu::not(memory_type, gsu, rom, ram),
        0x50..=0x5F => alu::add(opcode, memory_type, gsu, rom, ram),
        0x60..=0x6F => alu::sub(opcode, memory_type, gsu, rom, ram),
        0x70 => load::merge(memory_type, gsu, rom, ram),
        0x71..=0x7F => alu::and(opcode, memory_type, gsu, rom, ram),
        0x80..=0x8F => alu::mult(opcode, memory_type, gsu, rom, ram),
        0x90 => load::sbk(memory_type, gsu, ram),
        0x91..=0x94 => flow::link(opcode, memory_type, gsu),
        0x95 => alu::sex(memory_type, gsu, rom, ram),
        0x96 => alu::asr(memory_type, gsu, rom, ram),
        0x97 => alu::ror(memory_type, gsu, rom, ram),
        0x98..=0x9D => {
            if gsu.alt1 {
                flow::ljmp(opcode, memory_type, gsu, rom, ram)
            } else {
                flow::jmp(opcode, memory_type, gsu, rom, ram)
            }
        }
        0x9E => load::lob(memory_type, gsu, rom, ram),
        0x9F => alu::fmult(memory_type, gsu, rom, ram),
        0xA0..=0xAF => match (gsu.alt2, gsu.alt1) {
            (false, false) => load::ibt(opcode, memory_type, gsu, rom, ram),
            (_, true) => load::lms(opcode, memory_type, gsu, rom, ram),
            (true, false) => load::sms(opcode, memory_type, gsu, rom, ram),
        },
        0xB0..=0xBF => flags::from(opcode, memory_type, gsu, rom, ram),
        0xC0 => load::hib(memory_type, gsu, rom, ram),
        0xC1..=0xCF => alu::or(opcode, memory_type, gsu, rom, ram),
        0xD0..=0xDE => alu::inc(opcode, memory_type, gsu, rom, ram),
        0xDF => match (gsu.alt2, gsu.alt1) {
            (false, _) => plot::getc(memory_type, gsu),
            (true, false) => {
                // RAMB; treat as a NOP because no Super FX cartridge has more than 64KB of RAM
                nop(memory_type, gsu)
            }
            (true, true) => load::romb(memory_type, gsu),
        },
        0xE0..=0xEE => alu::dec(opcode, memory_type, gsu, rom, ram),
        0xEF => load::getb(memory_type, gsu, rom, ram),
        0xF0..=0xFF => match (gsu.alt2, gsu.alt1) {
            (false, false) => load::iwt(opcode, memory_type, gsu, rom, ram),
            (_, true) => load::lm(opcode, memory_type, gsu, rom, ram),
            (true, false) => load::sm(opcode, memory_type, gsu, rom, ram),
        },
    }
}

fn stop(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // STOP: Stop the GSU
    gsu.go = false;
    gsu.irq = true;

    clear_prefix_flags(gsu);
    memory_type.access_cycles(gsu.clock_speed)
}

fn nop(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // NOP: No-op
    clear_prefix_flags(gsu);
    memory_type.access_cycles(gsu.clock_speed)
}

fn cache(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit, rom: &[u8], ram: &[u8]) -> u8 {
    // CACHE: Set cache bank register
    let cbr = gsu.r[15].wrapping_sub(1) & 0xFFF0;
    let (updated, cycles) = if cbr != gsu.code_cache.cbr() {
        gsu.code_cache.update_cbr(cbr);

        let mut cycles = fill_cache_to_pc(gsu, gsu.r[15].wrapping_sub(1), rom, ram);
        cycles += cache_at_pc(gsu, gsu.r[15].wrapping_sub(1), rom, ram);

        (true, cycles)
    } else {
        (false, 0)
    };

    cycles
        + match memory_type {
            MemoryType::CodeCache => 1,
            MemoryType::Rom | MemoryType::Ram => {
                memory_type.access_cycles(gsu.clock_speed) + u8::from(updated)
            }
        }
}
