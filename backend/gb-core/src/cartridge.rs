//! Game Boy cartridge loading and mapping

mod mappers;

use crate::api::GameBoyLoadError;
use crate::cartridge::mappers::mbc3::Mbc3Rtc;
use crate::cartridge::mappers::{Mbc1, Mbc2, Mbc3, Mbc5};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftwareType {
    DmgOnly,
    CgbEnhanced,
    CgbOnly,
}

impl SoftwareType {
    pub fn from_rom(rom: &[u8]) -> Self {
        // $0143 in header contains CGB flag:
        // https://gbdev.io/pandocs/The_Cartridge_Header.html#0143--cgb-flag
        match rom.get(0x0143) {
            Some(0x80) => Self::CgbEnhanced,
            Some(0xC0) => Self::CgbOnly,
            _ => Self::DmgOnly,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
    sram: Box<[u8]>,
    mapper: Mapper,
    has_battery: bool,
}

impl Cartridge {
    pub fn create<S: SaveWriter>(
        rom: Box<[u8]>,
        initial_sram: Option<Vec<u8>>,
        save_writer: &mut S,
    ) -> Result<Self, GameBoyLoadError> {
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

        let (mapper, has_battery) = match mapper_byte {
            0x00 => (Mapper::None, false),
            0x01..=0x03 => {
                let mapper = Mapper::Mbc1(Mbc1::new(rom.len() as u32, sram_len as u32));
                let has_battery = mapper_byte == 0x03;

                (mapper, has_battery)
            }
            0x05..=0x06 => {
                let mapper = Mapper::Mbc2(Mbc2::new(rom.len() as u32, mem::take(&mut sram)));
                let has_battery = mapper_byte == 0x06;

                (mapper, has_battery)
            }
            0x0F..=0x13 => {
                let has_rtc = mapper_byte == 0x0F || mapper_byte == 0x10;
                let rtc = has_rtc
                    .then(|| save_writer.load_serialized("rtc").ok().unwrap_or_else(Mbc3Rtc::new));
                let mapper = Mapper::Mbc3(Mbc3::new(rom.len() as u32, sram_len as u32, rtc));
                let has_battery = matches!(mapper_byte, 0x0F | 0x10 | 0x13);

                log::info!("MBC3 real-time clock: {has_rtc}");

                (mapper, has_battery)
            }
            0x19..=0x1E => {
                let mapper = Mapper::Mbc5(Mbc5::new(rom.len() as u32, sram_len as u32));
                let has_battery = mapper_byte == 0x1B || mapper_byte == 0x1E;

                (mapper, has_battery)
            }
            _ => return Err(GameBoyLoadError::UnsupportedMapperByte(mapper_byte)),
        };

        log::info!("Using mapper {}", mapper.mapper_type());

        Ok(Self { rom: Rom(rom), sram: sram.into_boxed_slice(), mapper, has_battery })
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

    pub fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom.0).into_vec()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom = mem::take(&mut other.rom);
    }

    pub fn has_battery(&self) -> bool {
        self.has_battery
    }

    pub fn sram(&self) -> &[u8] {
        match &self.mapper {
            Mapper::Mbc2(mbc2) => mbc2.ram(),
            _ => &self.sram,
        }
    }

    pub fn update_rtc_time(&mut self) {
        if let Mapper::Mbc3(mbc3) = &mut self.mapper {
            mbc3.update_rtc_time();
        }
    }

    pub fn save_rtc_state<S: SaveWriter>(&mut self, save_writer: &mut S) -> Result<(), S::Err> {
        if let Mapper::Mbc3(mbc3) = &mut self.mapper {
            mbc3.save_rtc_state(save_writer)?;
        }

        Ok(())
    }
}
