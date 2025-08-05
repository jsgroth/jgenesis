//! GBA cartridge / game pak code

mod eeprom;
mod flashrom;

use crate::cartridge::eeprom::{Eeprom8K, Eeprom512};
use crate::cartridge::flashrom::{FlashRom64K, FlashRom128K};
use crate::dma::TransferUnit;
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::mem;
use std::ops::Deref;

const SRAM_LEN: usize = 32 * 1024;

#[derive(Debug, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

impl Default for Rom {
    fn default() -> Self {
        Self(vec![].into_boxed_slice())
    }
}

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum RwMemory {
    Unknown,
    Sram(BoxedByteArray<SRAM_LEN>),
    EepromUnknownSize,
    Eeprom512(Box<Eeprom512>),
    Eeprom8K(Box<Eeprom8K>),
    FlashRom64K(FlashRom64K),
    FlashRom128K(FlashRom128K),
}

impl RwMemory {
    fn new_sram(initial_save: Option<&Vec<u8>>) -> Self {
        let mut sram = BoxedByteArray::<SRAM_LEN>::new();

        if let Some(initial_save) = initial_save
            && initial_save.len() >= SRAM_LEN
        {
            sram.copy_from_slice(&initial_save[..SRAM_LEN]);
        } else {
            sram.fill(0xFF);
        }

        Self::Sram(sram)
    }

    fn new_eeprom(_initial_save: Option<&Vec<u8>>) -> Self {
        Self::EepromUnknownSize
    }

    fn new_flash_rom_64k(initial_save: Option<&Vec<u8>>) -> Self {
        Self::FlashRom64K(FlashRom64K::new(initial_save))
    }

    fn new_flash_rom_128k(initial_save: Option<&Vec<u8>>) -> Self {
        Self::FlashRom128K(FlashRom128K::new(initial_save))
    }

    fn initial(rom: &[u8], initial_save: Option<&Vec<u8>>) -> Self {
        type RwMemoryFn = fn(Option<&Vec<u8>>) -> RwMemory;

        // If the given ASCII string exists in ROM and followed by 3 digits, assume that type
        // of save memory (e.g. "SRAM_V123" indicates SRAM)
        const MEMORY_STRINGS: &[(&[u8], &str, RwMemoryFn)] = &[
            (b"SRAM_V", "SRAM", RwMemory::new_sram),
            (b"EEPROM_V", "EEPROM", |_| RwMemory::EepromUnknownSize),
            (b"FLASH_V", "Flash ROM 64 KB", RwMemory::new_flash_rom_64k),
            (b"FLASH512_V", "Flash ROM 64 KB", RwMemory::new_flash_rom_64k),
            (b"FLASH1M_V", "Flash ROM 128 KB", RwMemory::new_flash_rom_128k),
        ];

        for &(string, name, init_fn) in MEMORY_STRINGS {
            for i in 0..rom.len().saturating_sub(string.len() + 3) {
                if &rom[i..i + string.len()] != string {
                    continue;
                }

                if !(i + string.len()..i + string.len() + 3).all(|j| rom[j].is_ascii_digit()) {
                    continue;
                }

                log::info!(
                    "Auto-detected save memory type {name} from string in ROM at ${i:07X}: {}",
                    str::from_utf8(&rom[i..i + string.len() + 3]).unwrap()
                );

                return init_fn(initial_save);
            }
        }

        log::info!("No matching save memory string found in ROM; will auto-detect based on usage");

        Self::Unknown
    }

    fn min_eeprom_address(&self, rom_len: u32) -> u32 {
        // EEPROM is at $D000000-$DFFFFFF for ROMs <=32MB, and $DFFFF00-$DFFFFFF for >32MB
        match self {
            Self::EepromUnknownSize | Self::Eeprom8K(_) | Self::Eeprom512(_) => {
                if rom_len <= 32 * 1024 * 1024 { 0xD000000 } else { 0xDFFFF00 }
            }
            _ => 0xFFFFFFFF,
        }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
    rw_memory: RwMemory,
    rw_memory_dirty: bool,
    min_eeprom_address: u32,
    // Kept here so that it can be copied into R/W memory after auto-detection
    initial_save: Option<Vec<u8>>,
}

impl Cartridge {
    pub fn new(mut rom: Vec<u8>, initial_save: Option<Vec<u8>>) -> Self {
        jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);

        let rw_memory = RwMemory::initial(&rom, initial_save.as_ref());
        let min_eeprom_address = rw_memory.min_eeprom_address(rom.len() as u32);

        Self {
            rom: Rom(rom.into_boxed_slice()),
            rw_memory,
            rw_memory_dirty: false,
            min_eeprom_address,
            initial_save,
        }
    }

    pub fn read_rom_byte(&mut self, address: u32) -> u8 {
        if address >= self.min_eeprom_address
            && let Some(bit) = self.try_eeprom_read()
        {
            return bit.into();
        }

        let rom_addr = (address as usize) & (self.rom.len() - 1);
        self.rom[rom_addr]
    }

    pub fn read_rom_halfword(&mut self, address: u32) -> u16 {
        if address >= self.min_eeprom_address
            && let Some(bit) = self.try_eeprom_read()
        {
            return bit.into();
        }

        let rom_addr = (address as usize) & (self.rom.len() - 1) & !1;
        u16::from_le_bytes(self.rom[rom_addr..rom_addr + 2].try_into().unwrap())
    }

    pub fn read_rom_word(&mut self, address: u32) -> u32 {
        if address >= self.min_eeprom_address {
            // TODO what actually happens with 32-bit EEPROM reads?
            let first_bit = self.try_eeprom_read().unwrap_or(false);
            let second_bit = self.try_eeprom_read().unwrap_or(false);
            return u32::from(first_bit) | (u32::from(second_bit) << 16);
        }

        let rom_addr = (address as usize) & (self.rom.len() - 1) & !3;
        u32::from_le_bytes(self.rom[rom_addr..rom_addr + 4].try_into().unwrap())
    }

    fn try_eeprom_read(&mut self) -> Option<bool> {
        match &mut self.rw_memory {
            RwMemory::Eeprom512(eeprom) => Some(eeprom.read()),
            RwMemory::Eeprom8K(eeprom) => Some(eeprom.read()),
            _ => None,
        }
    }

    pub fn write_rom(&mut self, address: u32, value: u16) {
        if address < self.min_eeprom_address {
            log::warn!("Ignoring write to ROM address: {address:08X} {value:04X}");
            return;
        }

        self.rw_memory_dirty = true;

        match &mut self.rw_memory {
            RwMemory::Eeprom512(eeprom) => eeprom.write(value.bit(0)),
            RwMemory::Eeprom8K(eeprom) => eeprom.write(value.bit(0)),
            _ => {
                log::warn!("Ignoring write to ROM address: {address:08X} {value:04X}");
            }
        }
    }

    pub fn notify_dma_to_rom(&mut self, address: u32, length: u16, unit: TransferUnit) {
        if unit != TransferUnit::Halfword {
            // Word transfers will never be the correct length (and don't make sense for EEPROM anyway)
            return;
        }

        if !matches!(self.rw_memory, RwMemory::Unknown | RwMemory::EepromUnknownSize) {
            return;
        }

        let rom_len = self.rom.len() as u32;
        if address < RwMemory::EepromUnknownSize.min_eeprom_address(rom_len) {
            return;
        }

        match length {
            9 => {
                // 6-bit address; 512 B EEPROM
                self.rw_memory =
                    RwMemory::Eeprom512(Box::new(Eeprom512::new(self.initial_save.as_ref())));
                self.min_eeprom_address = self.rw_memory.min_eeprom_address(rom_len);

                log::info!("Auto-detected EEPROM size of 512 bytes from DMA of length {length}");
            }
            17 => {
                // 14-bit address; 8 KB EEPROM
                self.rw_memory =
                    RwMemory::Eeprom8K(Box::new(Eeprom8K::new(self.initial_save.as_ref())));
                self.min_eeprom_address = self.rw_memory.min_eeprom_address(rom_len);

                log::info!("Auto-detected EEPROM size of 8 KB from DMA of length {length}");
            }
            _ => {
                log::warn!("Unexpected initial EEPROM DMA length: {length}");
            }
        }
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom.0).into_vec()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom = mem::take(&mut other.rom);
    }

    pub fn read_sram(&mut self, address: u32) -> Option<u8> {
        if matches!(self.rw_memory, RwMemory::Unknown) {
            self.rw_memory = RwMemory::new_sram(self.initial_save.as_ref());
            log::info!("Auto-detected save memory type as SRAM due to read ${address:08X}");
        }

        match &self.rw_memory {
            RwMemory::Sram(sram) => {
                let sram_addr = (address as usize) & (SRAM_LEN - 1);
                Some(sram[sram_addr])
            }
            RwMemory::FlashRom64K(flash_rom) => Some(flash_rom.read(address)),
            RwMemory::FlashRom128K(flash_rom) => Some(flash_rom.read(address)),
            _ => None,
        }
    }

    pub fn write_sram(&mut self, address: u32, value: u8) {
        if matches!(self.rw_memory, RwMemory::Unknown) {
            if address & 0xFFFF == 0x5555 && value == 0xAA {
                // Probably Flash ROM
                // TODO is it possible to auto-detect 64K vs. 128K?
                self.rw_memory = RwMemory::new_flash_rom_128k(self.initial_save.as_ref());
                log::info!(
                    "Auto-detected save memory type as Flash ROM 64 KB due to write ${address:08X} {value:02X}"
                );
            } else {
                // Probably SRAM
                self.rw_memory = RwMemory::new_sram(self.initial_save.as_ref());
                log::info!(
                    "Auto-detected save memory type as SRAM due to write ${address:08X} 0x{value:02X}"
                );
            }
        }

        self.rw_memory_dirty = true;

        match &mut self.rw_memory {
            RwMemory::Sram(sram) => {
                let sram_addr = (address as usize) & (SRAM_LEN - 1);
                sram[sram_addr] = value;
            }
            RwMemory::FlashRom64K(flash_rom) => flash_rom.write(address, value),
            RwMemory::FlashRom128K(flash_rom) => flash_rom.write(address, value),
            _ => {
                log::warn!("Unexpected SRAM address write {address:08X} {value:02X}");
            }
        }
    }

    pub fn take_rw_memory_dirty(&mut self) -> bool {
        mem::take(&mut self.rw_memory_dirty)
    }

    pub fn rw_memory(&self) -> Option<&[u8]> {
        match &self.rw_memory {
            RwMemory::Sram(sram) => Some(sram.as_slice()),
            RwMemory::Eeprom8K(eeprom) => Some(eeprom.memory()),
            RwMemory::Eeprom512(eeprom) => Some(eeprom.memory()),
            RwMemory::FlashRom64K(flash_rom) => Some(flash_rom.memory()),
            RwMemory::FlashRom128K(flash_rom) => Some(flash_rom.memory()),
            RwMemory::Unknown | RwMemory::EepromUnknownSize => None,
        }
    }
}
