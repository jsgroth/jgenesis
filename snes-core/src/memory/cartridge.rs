use bincode::{Decode, Encode};
use jgenesis_common::frontend::PartialClone;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::cmp::Ordering;
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
            log::error!("Unable to confidently determine ROM type; defaulting to LoROM");
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

    pub fn take_rom(&mut self) -> Vec<u8> {
        match self {
            Self::LoRom { rom, .. } | Self::HiRom { rom, .. } => mem::take(&mut rom.0).into_vec(),
        }
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        let other_rom = match other {
            Self::LoRom { rom, .. } | Self::HiRom { rom, .. } => rom,
        };

        match self {
            Self::LoRom { rom, .. } | Self::HiRom { rom, .. } => {
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

const LOROM_HEADER_ADDR: usize = 0x7FC0;
const HIROM_HEADER_ADDR: usize = 0xFFC0;

const HEADER_TYPE_OFFSET: usize = 0x15;

const LOROM_RESET_VECTOR: usize = 0x7FFC;
const HIROM_RESET_VECTOR: usize = 0xFFFC;

fn guess_cartridge_type(rom: &[u8]) -> Option<CartridgeType> {
    if rom.len() < 0x8000 {
        log::error!("ROM is too small; all ROMs should be at least 32KB, was {} bytes", rom.len());
        return None;
    }

    if rom.len() < 0x10000 {
        // Any ROM less than 64KB must be LoROM; HiROM <64KB wouldn't have anywhere to store
        // the 65816 interrupt vectors
        return Some(CartridgeType::LoRom);
    }

    let mut lorom_points = 0;
    let mut hirom_points = 0;

    let lorom_type_byte = rom[LOROM_HEADER_ADDR + HEADER_TYPE_OFFSET];
    if lorom_type_byte == 0x20 || lorom_type_byte == 0x30 {
        // $20 = LoROM, $30 = LoROM + FastROM
        lorom_points += 1;
    }

    let hirom_type_byte = rom[HIROM_HEADER_ADDR + HEADER_TYPE_OFFSET];
    if hirom_type_byte == 0x21 || hirom_type_byte == 0x31 {
        // $21 = HiROM, $31 = HiROM + FastROM
        hirom_points += 1;
    }

    // All LoROM vectors should be in the range $8000-$FFFF, and A15 is ignored for mapping to ROM
    let lorom_vector = u16::from_le_bytes([rom[LOROM_RESET_VECTOR], rom[LOROM_RESET_VECTOR + 1]]);
    if lorom_vector >= 0x8000 && seems_like_valid_reset_vector(rom, lorom_vector & 0x7FFF) {
        lorom_points += 1;
    }

    let hirom_vector = u16::from_le_bytes([rom[HIROM_RESET_VECTOR], rom[HIROM_RESET_VECTOR + 1]]);
    if seems_like_valid_reset_vector(rom, hirom_vector) {
        hirom_points += 1;
    }

    match lorom_points.cmp(&hirom_points) {
        Ordering::Less => Some(CartridgeType::HiRom),
        Ordering::Greater => Some(CartridgeType::LoRom),
        Ordering::Equal => None,
    }
}

const CLC_OPCODE: u8 = 0x18;
const SEI_OPCODE: u8 = 0x78;

fn seems_like_valid_reset_vector(rom: &[u8], vector: u16) -> bool {
    // Nearly all games execute either SEI or CLC as the first instruction at the RESET vector
    let vector = vector as usize;
    vector < rom.len() && (rom[vector] == CLC_OPCODE || rom[vector] == SEI_OPCODE)
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
    (address & 0x3FFFFF) % rom_len
}
