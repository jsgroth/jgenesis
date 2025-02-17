//! GBA cartridge code

pub mod flashrom;

use crate::cartridge::flashrom::FlashRom;
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::array;
use std::fmt::{Display, Formatter};
use std::ops::Deref;

const SRAM_LEN: usize = 32 * 1024;
const FLASH_ROM_SMALL_LEN: usize = 64 * 1024;
const FLASH_ROM_LARGE_LEN: usize = 128 * 1024;

#[derive(Debug, FakeEncode, FakeDecode)]
pub struct Rom(pub Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for Rom {
    fn default() -> Self {
        Self(vec![].into_boxed_slice())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuessedSaveMemoryType {
    Sram,
    Eeprom,
    FlashRomSmall,
    FlashRomLarge,
}

impl Display for GuessedSaveMemoryType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sram => write!(f, "SRAM"),
            Self::Eeprom => write!(f, "EEPROM"),
            Self::FlashRomSmall => write!(f, "Flash ROM (64KB)"),
            Self::FlashRomLarge => write!(f, "Flash ROM (128KB)"),
        }
    }
}

impl GuessedSaveMemoryType {
    fn from_rom(rom: &[u8]) -> Option<Self> {
        // From https://mgba-emu.github.io/gbatek/#id-strings
        const TARGET_STRS: &[(&[u8], GuessedSaveMemoryType)] = &[
            (b"SRAM_V", GuessedSaveMemoryType::Sram),
            (b"EEPROM_V", GuessedSaveMemoryType::Eeprom),
            (b"FLASH_V", GuessedSaveMemoryType::FlashRomSmall),
            (b"FLASH512_V", GuessedSaveMemoryType::FlashRomSmall),
            (b"FLASH1M_V", GuessedSaveMemoryType::FlashRomLarge),
        ];

        for i in (0..rom.len()).step_by(4) {
            for &(target_str, mem_type) in TARGET_STRS {
                if i + target_str.len() > rom.len() {
                    continue;
                }

                if &rom[i..i + target_str.len()] == target_str {
                    return Some(mem_type);
                }
            }
        }

        None
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum SaveMemory {
    Sram(BoxedByteArray<SRAM_LEN>),
    FlashRomSmall(FlashRom<FLASH_ROM_SMALL_LEN>),
    FlashRomLarge(FlashRom<FLASH_ROM_LARGE_LEN>),
}

impl SaveMemory {
    fn from_guessed_type(guessed: GuessedSaveMemoryType) -> Self {
        match guessed {
            GuessedSaveMemoryType::Sram => Self::Sram(BoxedByteArray::new()),
            GuessedSaveMemoryType::FlashRomSmall => Self::FlashRomSmall(FlashRom::new()),
            GuessedSaveMemoryType::FlashRomLarge => Self::FlashRomLarge(FlashRom::new()),
            GuessedSaveMemoryType::Eeprom => todo!("EEPROM"),
        }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Cartridge {
    #[partial_clone(default)]
    pub rom: Rom,
    pub save_memory: SaveMemory,
}

impl Cartridge {
    pub fn new(mut rom: Vec<u8>) -> Self {
        jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);

        let save_memory_type = GuessedSaveMemoryType::from_rom(&rom).unwrap_or_else(|| {
            log::warn!("Unable to determine save memory type from ROM; defaulting to 32KB of SRAM");
            GuessedSaveMemoryType::Sram
        });

        log::info!("Save memory type: {save_memory_type}");

        let save_memory = SaveMemory::from_guessed_type(save_memory_type);

        Self { rom: Rom(rom.into_boxed_slice()), save_memory }
    }

    pub fn read_rom_byte(&self, address: u32) -> u8 {
        self.rom[(address as usize) & (self.rom.len() - 1)]
    }

    pub fn read_rom_halfword(&self, address: u32) -> u16 {
        let rom_addr = (address as usize) & (self.rom.len() - 1) & !1;
        u16::from_le_bytes(array::from_fn(|i| self.rom[rom_addr + i]))
    }

    pub fn read_rom_word(&self, address: u32) -> u32 {
        let rom_addr = (address as usize) & (self.rom.len() - 1) & !3;
        u32::from_le_bytes(array::from_fn(|i| self.rom[rom_addr + i]))
    }

    pub fn read_ram_byte(&mut self, address: u32) -> u8 {
        match &mut self.save_memory {
            SaveMemory::Sram(data) => data[(address as usize) & (SRAM_LEN - 1)],
            SaveMemory::FlashRomSmall(flash_rom) => flash_rom.read(address),
            SaveMemory::FlashRomLarge(flash_rom) => flash_rom.read(address),
        }
    }

    pub fn write_ram_byte(&mut self, address: u32, value: u8) {
        match &mut self.save_memory {
            SaveMemory::Sram(data) => {
                data[(address as usize) & (SRAM_LEN - 1)] = value;
            }
            SaveMemory::FlashRomSmall(flash_rom) => flash_rom.write(address, value),
            SaveMemory::FlashRomLarge(flash_rom) => flash_rom.write(address, value),
        }
    }
}
