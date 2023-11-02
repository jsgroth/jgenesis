use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use jgenesis_traits::frontend::PartialClone;
use std::mem;
use std::ops::Deref;

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct Rom(Box<[u8]>);

impl Default for Rom {
    fn default() -> Self {
        Rom(vec![].into_boxed_slice())
    }
}

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub enum Cartridge {
    LoRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
    },
}

impl Cartridge {
    pub fn create(rom: Box<[u8]>, initial_sram: Option<Vec<u8>>) -> Self {
        // TODO handle cartridge types other than basic LoROM

        // In LoROM, $007FD8 contains SRAM size as a kilobytes power of 2
        let sram_header_byte = rom[0x7FD8];
        let sram_len = if sram_header_byte == 0 { 0 } else { 1 << (10 + sram_header_byte) };

        let sram = match initial_sram {
            Some(sram) if sram.len() == sram_len => sram.into_boxed_slice(),
            _ => vec![0; sram_len].into_boxed_slice(),
        };

        log::info!("Using mapper LoROM with SRAM size {sram_len}");

        Self::LoRom { rom: Rom(rom), sram }
    }

    pub fn read(&self, address: u32) -> u8 {
        match self {
            Self::LoRom { rom, sram } => {
                match lorom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::Rom(rom_addr) => rom[rom_addr as usize],
                    CartridgeAddress::Sram(sram_addr) => sram[sram_addr as usize],
                }
            }
        }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        match self {
            Self::LoRom { rom, sram } => {
                match lorom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::Rom(_) => {}
                    CartridgeAddress::Sram(sram_addr) => {
                        sram[sram_addr as usize] = value;
                    }
                }
            }
        }
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        let other_rom = match other {
            Self::LoRom { rom, .. } => rom,
        };

        match self {
            Self::LoRom { rom, .. } => {
                *rom = mem::take(other_rom);
            }
        }
    }

    pub fn sram(&self) -> Option<&[u8]> {
        match self {
            Self::LoRom { sram, .. } if !sram.is_empty() => Some(sram),
            Self::LoRom { .. } => None,
        }
    }
}

enum CartridgeAddress {
    Rom(u32),
    Sram(u32),
}

fn lorom_map_address(address: u32, rom_len: u32, sram_len: u32) -> CartridgeAddress {
    let bank = address >> 16;
    let offset = address & 0xFFFF;
    match (bank, offset) {
        (0x00..=0x6F | 0x80..=0xEF, _) | (0x70..=0x7D | 0xF0..=0xFF, 0x8000..=0xFFFF) => {
            // ROM; typically at $8000-$FFFF and sometimes mirrored into $0000-$7FFF
            let rom_addr = lorom_map_rom_address(address, rom_len);
            CartridgeAddress::Rom(rom_addr)
        }
        (0x70..=0x7D | 0xF0..=0xFF, 0x0000..=0x7FFF) => {
            // SRAM, if mapped
            if sram_len != 0 {
                // SRAM size is always a power of 2; use that to mask address
                // TODO apparently some games have >32KB of SRAM?
                let sram_addr = address & (sram_len - 1);
                CartridgeAddress::Sram(sram_addr)
            } else {
                // Treat as ROM mirror
                let rom_addr = lorom_map_rom_address(address, rom_len);
                CartridgeAddress::Rom(rom_addr)
            }
        }
        _ => panic!("invalid cartridge address: {address:06X}"),
    }
}

fn lorom_map_rom_address(address: u32, rom_len: u32) -> u32 {
    let rom_addr = ((address & 0x7F0000) >> 1) | (address & 0x007FFF);
    // TODO better handle unusual ROM sizes
    rom_addr % rom_len
}
