//! Game Boy cartridge loading and mapping

mod mappers;

use crate::api::GameBoyLoadError;
use crate::cartridge::mappers::Mbc1;
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::ops::Deref;

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum Mapper {
    None,
    Mbc1(Mbc1),
}

impl Mapper {
    fn map_rom_address(&self, address: u16) -> u32 {
        match self {
            Self::None => address.into(),
            Self::Mbc1(mbc1) => mbc1.map_rom_address(address),
        }
    }

    fn map_ram_address(&self, address: u16) -> u32 {
        match self {
            Self::None => (address & 0x1FFF).into(),
            Self::Mbc1(mbc1) => mbc1.map_ram_address(address),
        }
    }

    fn is_ram_enabled(&self) -> bool {
        match self {
            Self::None => true,
            Self::Mbc1(mbc1) => mbc1.is_ram_enabled(),
        }
    }

    fn write_rom_address(&mut self, address: u16, value: u8) {
        match self {
            Self::None => {}
            Self::Mbc1(mbc1) => mbc1.write_rom_address(address, value),
        }
    }

    fn mapper_type(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Mbc1(..) => "MBC1",
        }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
    sram: Box<[u8]>,
    mapper: Mapper,
}

impl Cartridge {
    pub fn create(rom: Box<[u8]>, initial_sram: Option<Vec<u8>>) -> Result<Self, GameBoyLoadError> {
        // The byte at $0149 in the ROM header indicates SRAM size
        let sram_len_byte = rom[0x0149];
        let sram_len = match sram_len_byte {
            0x00 => 0,
            0x02 => 8 * 1024,
            0x03 => 32 * 1024,
            0x04 => 128 * 1024,
            0x05 => 64 * 1024,
            _ => return Err(GameBoyLoadError::InvalidSramByte(sram_len_byte)),
        };

        log::info!("SRAM size: {sram_len} bytes");

        let sram = match initial_sram {
            Some(initial_sram) if initial_sram.len() == sram_len => initial_sram,
            _ => vec![0; sram_len],
        };

        // Cartridge type is always at $0147 in ROM
        let mapper_byte = rom[0x0147];
        let mapper = match mapper_byte {
            0x00 => Mapper::None,
            0x01..=0x03 => Mapper::Mbc1(Mbc1::new(rom.len() as u32, sram_len as u32)),
            _ => return Err(GameBoyLoadError::UnsupportedMapperByte(mapper_byte)),
        };

        log::info!("Using mapper {}", mapper.mapper_type());

        Ok(Self { rom: Rom(rom), sram: sram.into_boxed_slice(), mapper })
    }

    pub fn read_rom(&self, address: u16) -> u8 {
        let rom_addr = self.mapper.map_rom_address(address);
        self.rom[rom_addr as usize]
    }

    pub fn write_rom(&mut self, address: u16, value: u8) {
        self.mapper.write_rom_address(address, value);
    }

    pub fn read_ram(&self, address: u16) -> u8 {
        if !self.mapper.is_ram_enabled() || self.sram.is_empty() {
            return 0xFF;
        }

        let ram_addr = self.mapper.map_ram_address(address);
        self.sram[ram_addr as usize]
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        if !self.mapper.is_ram_enabled() || self.sram.is_empty() {
            return;
        }

        let ram_addr = self.mapper.map_ram_address(address);
        self.sram[ram_addr as usize] = value;
    }
}
