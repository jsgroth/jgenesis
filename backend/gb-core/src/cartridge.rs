//! Game Boy cartridge loading and mapping

mod mappers;

use crate::api::GameBoyLoadError;
use crate::cartridge::mappers::{Mbc1, Mbc2, Mbc3, Mbc5};
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::mem;
use std::ops::Deref;

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

trait HasBasicRamMapping {
    fn map_ram_address(&self, address: u16) -> Option<u32>;

    fn read_ram(&self, address: u16, sram: &[u8]) -> u8 {
        let Some(ram_addr) = self.map_ram_address(address) else { return 0xFF };
        sram.get(ram_addr as usize).copied().unwrap_or(0xFF)
    }

    fn write_ram(&mut self, address: u16, value: u8, sram: &mut [u8]) {
        let Some(ram_addr) = self.map_ram_address(address) else { return };

        if ram_addr < sram.len() as u32 {
            sram[ram_addr as usize] = value;
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum Mapper {
    None,
    Mbc1(Mbc1),
    Mbc2(Mbc2),
    Mbc3(Mbc3),
    Mbc5(Mbc5),
}

impl Mapper {
    fn map_rom_address(&self, address: u16) -> u32 {
        match self {
            Self::None => address.into(),
            Self::Mbc1(mbc1) => mbc1.map_rom_address(address),
            Self::Mbc2(mbc2) => mbc2.map_rom_address(address),
            Self::Mbc3(mbc3) => mbc3.map_rom_address(address),
            Self::Mbc5(mbc5) => mbc5.map_rom_address(address),
        }
    }

    fn read_ram(&self, address: u16, sram: &[u8]) -> u8 {
        match self {
            Self::None => sram.get((address & 0x1FFF) as usize).copied().unwrap_or(0xFF),
            Self::Mbc1(mbc1) => mbc1.read_ram(address, sram),
            Self::Mbc2(mbc2) => mbc2.read_ram(address),
            Self::Mbc3(mbc3) => mbc3.read_ram(address, sram),
            Self::Mbc5(mbc5) => mbc5.read_ram(address, sram),
        }
    }

    fn write_ram(&mut self, address: u16, value: u8, sram: &mut [u8]) {
        match self {
            Self::None => {
                if let Some(ram_value) = sram.get_mut((address & 0x1FFF) as usize) {
                    *ram_value = value;
                }
            }
            Self::Mbc1(mbc1) => mbc1.write_ram(address, value, sram),
            Self::Mbc2(mbc2) => mbc2.write_ram(address, value),
            Self::Mbc3(mbc3) => mbc3.write_ram(address, value, sram),
            Self::Mbc5(mbc5) => mbc5.write_ram(address, value, sram),
        }
    }

    fn write_rom_address(&mut self, address: u16, value: u8) {
        match self {
            Self::None => {}
            Self::Mbc1(mbc1) => mbc1.write_rom_address(address, value),
            Self::Mbc2(mbc2) => mbc2.write_rom_address(address, value),
            Self::Mbc3(mbc3) => mbc3.write_rom_address(address, value),
            Self::Mbc5(mbc5) => mbc5.write_rom_address(address, value),
        }
    }

    fn mapper_type(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Mbc1(..) => "MBC1",
            Self::Mbc2(..) => "MBC2",
            Self::Mbc3(..) => "MBC3",
            Self::Mbc5(..) => "MBC5",
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
        // Cartridge type is always at $0147 in ROM
        let mapper_byte = rom[0x0147];
        let is_mbc2 = mapper_byte == 0x05 || mapper_byte == 0x06;

        let sram_len = if is_mbc2 {
            // MBC2 has a fixed 512x4 bits of RAM
            mappers::MBC2_RAM_LEN
        } else {
            // The byte at $0149 in the ROM header indicates SRAM size
            let sram_len_byte = rom[0x0149];
            match sram_len_byte {
                0x00 => 0,
                0x02 => 8 * 1024,
                0x03 => 32 * 1024,
                0x04 => 128 * 1024,
                0x05 => 64 * 1024,
                _ => return Err(GameBoyLoadError::InvalidSramByte(sram_len_byte)),
            }
        };

        log::info!("SRAM size: {sram_len} bytes");

        let mut sram = match initial_sram {
            Some(initial_sram) if initial_sram.len() == sram_len => initial_sram,
            _ => vec![0; sram_len],
        };

        let mapper = match mapper_byte {
            0x00 => Mapper::None,
            0x01..=0x03 => Mapper::Mbc1(Mbc1::new(rom.len() as u32, sram_len as u32)),
            0x05..=0x06 => Mapper::Mbc2(Mbc2::new(rom.len() as u32, mem::take(&mut sram))),
            0x0F..=0x13 => Mapper::Mbc3(Mbc3::new(rom.len() as u32, sram_len as u32)),
            0x19..=0x1E => Mapper::Mbc5(Mbc5::new(rom.len() as u32, sram_len as u32)),
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
        self.mapper.read_ram(address, &self.sram)
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        self.mapper.write_ram(address, value, &mut self.sram);
    }
}
