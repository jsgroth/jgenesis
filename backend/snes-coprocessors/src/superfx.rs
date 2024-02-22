//! Super FX GSU (Graphics Support Unit), a programmable custom RISC-like CPU
//!
//! There were 3 different Super FX chips used: Mario Chip 1, GSU-1, and GSU-2. The only differences
//! between chips seem to be clock speed and memory capacity.
//!
//! Mario Chip 1 runs at 10.74 MHz, GSU-1 and GSU-2 run at 21.47 MHz.
//!
//! Mario Chip 1 and GSU-1 apparently only supported up to 1MB of ROM, while GSU-2 supported up
//! to 2MB of ROM. GSU-2 also supported "backup RAM" and "CPU ROM" but no released cartridges used
//! these features.
//!
//! Used by 8 released games including Star Fox and Yoshi's Island, as well as the originally unreleased Star Fox 2

mod gsu;

use crate::common;
use crate::common::{impl_take_set_rom, Rom};
use crate::superfx::gsu::{BusAccess, GraphicsSupportUnit};
use bincode::{Decode, Encode};
use jgenesis_common::num::U16Ext;
use jgenesis_proc_macros::PartialClone;
use std::num::NonZeroU64;

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct SuperFx {
    #[partial_clone(default)]
    rom: Rom,
    ram: Box<[u8]>,
    gsu: GraphicsSupportUnit,
    gsu_overclock_factor: u64,
}

impl SuperFx {
    #[must_use]
    pub fn new(rom: Box<[u8]>, ram: Box<[u8]>, gsu_overclock_factor: NonZeroU64) -> Self {
        Self {
            rom: Rom(rom),
            ram,
            gsu: GraphicsSupportUnit::new(),
            gsu_overclock_factor: gsu_overclock_factor.get(),
        }
    }

    #[inline]
    #[must_use]
    pub fn read(&mut self, address: u32) -> Option<u8> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x3000..=0x30FF | 0x3300..=0x34FF) => {
                // GSU I/O ports
                self.gsu.read_register(address)
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x3100..=0x32FF) => {
                // GSU code cache RAM
                self.gsu.read_code_cache_ram(address)
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) => {
                // ROM, LoROM mapping
                match (self.gsu.is_running(), self.gsu.rom_access()) {
                    (false, _) | (true, BusAccess::Snes) => {
                        let rom_addr = map_lorom_address(address, self.rom.len() as u32);
                        Some(self.rom[rom_addr as usize])
                    }
                    (true, BusAccess::Gsu) => fixed_sfx_interrupt_vector(address),
                }
            }
            (0x40..=0x5F | 0xC0..=0xDF, _) => {
                // ROM, HiROM mapping
                match (self.gsu.is_running(), self.gsu.rom_access()) {
                    (false, _) | (true, BusAccess::Snes) => {
                        let rom_addr = map_hirom_address(address, self.rom.len() as u32);
                        Some(self.rom[rom_addr as usize])
                    }
                    (true, BusAccess::Gsu) => fixed_sfx_interrupt_vector(address),
                }
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // First 8KB of RAM
                (!self.gsu.is_running() || self.gsu.ram_access() == BusAccess::Snes)
                    .then(|| self.ram[(address & 0x1FFF) as usize])
            }
            (0x70..=0x71 | 0xF0..=0xF1, _) => {
                // RAM
                (!self.gsu.is_running() || self.gsu.ram_access() == BusAccess::Snes)
                    .then(|| self.ram[(address as usize) & (self.ram.len() - 1)])
            }
            _ => None,
        }
    }

    #[inline]
    pub fn write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x3000..=0x30FF | 0x3300..=0x34FF) => {
                // GSU I/O ports
                self.gsu.write_register(address, value);
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x3100..=0x32FF) => {
                // GSU code cache RAM
                self.gsu.write_code_cache_ram(address, value);
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // First 8KB of RAM
                if !self.gsu.is_running() || self.gsu.ram_access() == BusAccess::Snes {
                    self.ram[(address & 0x1FFF) as usize] = value;
                }
            }
            (0x70..=0x71 | 0xF0..=0xF1, _) => {
                // RAM
                if !self.gsu.is_running() || self.gsu.ram_access() == BusAccess::Snes {
                    self.ram[(address as usize) & (self.ram.len() - 1)] = value;
                }
            }
            _ => {}
        }
    }

    #[inline]
    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        self.gsu.tick(self.gsu_overclock_factor * master_cycles_elapsed, &self.rom, &mut self.ram);
    }

    #[inline]
    #[must_use]
    pub fn irq(&self) -> bool {
        self.gsu.irq()
    }

    pub fn reset(&mut self) {
        self.gsu.reset();
    }

    #[inline]
    #[must_use]
    pub fn has_battery(&self) -> bool {
        // Most of the Super FX games do not have battery backup but some do, e.g. Yoshi's Island
        // This is indicated by a chipset byte of $15 or $1A instead of $13 or $14
        let chipset_byte = self.rom[common::LOROM_CHIPSET_BYTE_ADDRESS];
        chipset_byte == 0x15 || chipset_byte == 0x1A
    }

    #[inline]
    #[must_use]
    pub fn sram(&self) -> &[u8] {
        self.ram.as_ref()
    }

    impl_take_set_rom!(rom);

    pub fn update_gsu_overclock_factor(&mut self, overclock_factor: NonZeroU64) {
        self.gsu_overclock_factor = overclock_factor.get();
    }
}

fn map_lorom_address(address: u32, rom_len: u32) -> u32 {
    let rom_addr = (address & 0x7FFF) | ((address & 0x7F0000) >> 1);
    rom_addr & (rom_len - 1)
}

fn map_hirom_address(address: u32, rom_len: u32) -> u32 {
    let rom_addr = address & 0x3FFFFF;
    rom_addr & (rom_len - 1)
}

const SFX_COP_VECTOR: u16 = 0x0104;
const SFX_BRK_VECTOR: u16 = 0x0100;
const SFX_ABORT_VECTOR: u16 = 0x0100;
const SFX_NMI_VECTOR: u16 = 0x0108;
const SFX_IRQ_VECTOR: u16 = 0x010C;

fn fixed_sfx_interrupt_vector(address: u32) -> Option<u8> {
    // If the SNES CPU accesses ROM while the GSU is running and has control of the ROM bus, the
    // SNES CPU reads fixed values based on the last 4 bits of the address (intended to allow the
    // SNES to read interrupt vectors while the GSU is running)
    match address & 0xF {
        0x4 => Some(SFX_COP_VECTOR.lsb()),
        0x5 => Some(SFX_COP_VECTOR.msb()),
        0x6 => Some(SFX_BRK_VECTOR.lsb()),
        0x7 => Some(SFX_BRK_VECTOR.msb()),
        0x8 => Some(SFX_ABORT_VECTOR.lsb()),
        0x9 => Some(SFX_ABORT_VECTOR.msb()),
        0xA => Some(SFX_NMI_VECTOR.lsb()),
        0xB => Some(SFX_NMI_VECTOR.msb()),
        0xE => Some(SFX_IRQ_VECTOR.lsb()),
        0xF => Some(SFX_IRQ_VECTOR.msb()),
        _ => None,
    }
}

#[must_use]
pub fn guess_ram_len(rom: &[u8]) -> usize {
    // $7FDA == maker code; $33 indicates extended header
    // $7FBD == expansion RAM size in extended header, as kilobytes as a power of 2
    // Older Super FX games don't have an extended header, so default to 32KB if the header doesn't
    // explicitly specify 64KB
    match (rom[0x7FDA], rom[0x7FBD]) {
        (0x33, 0x06) => 64 * 1024,
        _ => 32 * 1024,
    }
}
