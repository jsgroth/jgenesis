use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use jgenesis_traits::frontend::PartialClone;
use std::fmt::{Display, Formatter};
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
    HiRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CartridgeType {
    LoRom,
    HiRom,
}

impl Display for CartridgeType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoRom => write!(f, "LoROM"),
            Self::HiRom => write!(f, "HiROM"),
        }
    }
}

impl Cartridge {
    pub fn create(rom: Box<[u8]>, initial_sram: Option<Vec<u8>>) -> Self {
        let cartridge_type = guess_cartridge_type(&rom).unwrap_or_else(|| {
            log::error!("Unable to determine ROM type; defaulting to LoROM");
            CartridgeType::LoRom
        });

        let rom_header_addr = match cartridge_type {
            CartridgeType::LoRom => 0x7FC0,
            CartridgeType::HiRom => 0xFFC0,
        };

        // In LoROM, $007FD8 contains SRAM size as a kilobytes power of 2
        let sram_header_byte = rom[rom_header_addr | 0x0018];
        let sram_len = if sram_header_byte == 0 { 0 } else { 1 << (10 + sram_header_byte) };

        let sram = match initial_sram {
            Some(sram) if sram.len() == sram_len => sram.into_boxed_slice(),
            _ => vec![0; sram_len].into_boxed_slice(),
        };

        log::info!("Using mapper {cartridge_type} with SRAM size {sram_len}");

        match cartridge_type {
            CartridgeType::LoRom => Self::LoRom { rom: Rom(rom), sram },
            CartridgeType::HiRom => Self::HiRom { rom: Rom(rom), sram },
        }
    }

    pub fn read(&self, address: u32) -> u8 {
        match self {
            Self::LoRom { rom, sram } => {
                match lorom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::None => {
                        // TODO open bus
                        0x00
                    }
                    CartridgeAddress::Rom(rom_addr) => rom[rom_addr as usize],
                    CartridgeAddress::Sram(sram_addr) => sram[sram_addr as usize],
                }
            }
            Self::HiRom { rom, sram } => {
                match hirom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::None => {
                        // TODO open bus
                        0x00
                    }
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
                    CartridgeAddress::Rom(_) | CartridgeAddress::None => {}
                    CartridgeAddress::Sram(sram_addr) => {
                        sram[sram_addr as usize] = value;
                    }
                }
            }
            Self::HiRom { rom, sram } => {
                match hirom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::Rom(_) | CartridgeAddress::None => {}
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
            Self::HiRom { rom, .. } => rom,
        };

        match self {
            Self::LoRom { rom, .. } => {
                *rom = mem::take(other_rom);
            }
            Self::HiRom { rom, .. } => {
                *rom = mem::take(other_rom);
            }
        }
    }

    pub fn sram(&self) -> Option<&[u8]> {
        match self {
            Self::LoRom { sram, .. } | Self::HiRom { sram, .. } if !sram.is_empty() => Some(sram),
            Self::LoRom { .. } | Self::HiRom { .. } => None,
        }
    }
}

fn guess_cartridge_type(rom: &[u8]) -> Option<CartridgeType> {
    if rom.len() < 0x8000 {
        log::error!("ROM is too small; all ROMs should be at least 32KB, was {} bytes", rom.len());
        return None;
    }

    let lorom_type_byte = rom[0x7FD5] & 0x3F;
    if lorom_type_byte == 0x20 || lorom_type_byte == 0x30 {
        // Very likely LoROM
        return Some(CartridgeType::LoRom);
    }

    if rom.len() >= 0x10000 {
        let hirom_type_byte = rom[0xFFD5] & 0x3F;
        if hirom_type_byte == 0x21 || hirom_type_byte == 0x31 {
            // Very likely HiROM
            return Some(CartridgeType::HiRom);
        }
    }

    None
}

enum CartridgeAddress {
    None,
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

fn hirom_map_address(address: u32, rom_len: u32, sram_len: u32) -> CartridgeAddress {
    let bank = address >> 16;
    let offset = address & 0xFFFF;
    match (bank, offset) {
        (0x40..=0x7D | 0xC0..=0xFF, _) | (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) => {
            // ROM
            let rom_addr = hirom_map_rom_address(address, rom_len);
            CartridgeAddress::Rom(rom_addr)
        }
        (0x20..=0x3F | 0xA0..=0xBF, 0x6000..=0x7FFF) if sram_len != 0 => {
            // SRAM, if mapped
            let sram_bank = bank & 0x1F;
            let sram_addr = (sram_bank << 13) | (offset & 0x1FFF);
            CartridgeAddress::Sram(sram_addr & (sram_len - 1))
        }
        _ => CartridgeAddress::None,
    }
}

fn hirom_map_rom_address(address: u32, rom_len: u32) -> u32 {
    // TODO better handle unusual ROM sizes (and ExHiROM?)
    address % rom_len
}
