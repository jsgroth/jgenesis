use crate::num::GetBit;
use std::ops::RangeInclusive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mapper {
    #[default]
    Sega,
    Codemasters,
}

const CODEMASTERS_CHECKSUM_ADDR: usize = 0x7FE6;
const SEGA_HEADER_ADDR_RANGE: RangeInclusive<usize> = 0x7FF0..=0x7FFF;

impl Mapper {
    // Codemasters ROMs have a 16-bit checksum at $7FE6 which is the sum of all 16-bit words in the ROM
    // except for the words in the Sega header. If summing all of the words matches the word at
    // $7FE6, assume this is a Codemasters ROM
    fn detect_from_rom(rom: &[u8]) -> Self {
        if rom.len() < 32 * 1024 {
            // No real ROMs should be less than 32KB, but regardless this isn't a Codemasters ROM
            return Self::Sega;
        }

        let expected_checksum = u16::from_le_bytes([
            rom[CODEMASTERS_CHECKSUM_ADDR],
            rom[CODEMASTERS_CHECKSUM_ADDR + 1],
        ]);
        let mut checksum = 0_u16;
        for address in (0..rom.len()).step_by(2) {
            if !SEGA_HEADER_ADDR_RANGE.contains(&address) {
                let word = u16::from_le_bytes([rom[address], rom[address + 1]]);
                checksum = checksum.wrapping_add(word);
            }
        }

        if checksum == expected_checksum {
            log::info!("Codemasters-style ROM checksum is {checksum:04X}, matches word at {CODEMASTERS_CHECKSUM_ADDR:04X}; assuming this is a Codemasters ROM");
            Self::Codemasters
        } else {
            Self::Sega
        }
    }
}

#[derive(Debug, Clone)]
struct Cartridge {
    rom: Vec<u8>,
    ram: Vec<u8>,
    mapper: Mapper,
    rom_bank_0: u32,
    rom_bank_1: u32,
    rom_bank_2: u32,
    ram_mapped: bool,
    ram_bank: u32,
}

const CARTRIDGE_RAM_SIZE: usize = 32 * 1024;

impl Cartridge {
    fn from_rom(rom: Vec<u8>) -> Self {
        let mapper = Mapper::detect_from_rom(&rom);
        log::info!("Detected mapper {mapper:?} from ROM header");

        Self {
            rom,
            ram: vec![0; CARTRIDGE_RAM_SIZE],
            mapper,
            rom_bank_0: 0,
            rom_bank_1: 1,
            rom_bank_2: 2,
            ram_mapped: false,
            ram_bank: 0,
        }
    }

    fn read_rom_address(&self, address: u32) -> u8 {
        let wrapped_addr = (address as usize) & (self.rom.len() - 1);
        self.rom[wrapped_addr]
    }

    fn read(&self, address: u16) -> u8 {
        match (self.mapper, address) {
            (Mapper::Sega, 0x0000..=0x03FF) => self.rom[address as usize],
            (Mapper::Sega, 0x0400..=0x3FFF) | (Mapper::Codemasters, 0x0000..=0x3FFF) => {
                let rom_addr = (self.rom_bank_0 << 14) | u32::from(address);
                self.read_rom_address(rom_addr)
            }
            (_, 0x4000..=0x7FFF) => {
                let rom_addr = (self.rom_bank_1 << 14) | u32::from(address & 0x3FFF);
                self.read_rom_address(rom_addr)
            }
            (Mapper::Sega, 0x8000..=0xBFFF) => {
                if self.ram_mapped {
                    let ram_addr = (self.ram_bank << 14) | u32::from(address & 0x3FFF);
                    self.ram[ram_addr as usize]
                } else {
                    let rom_addr = (self.rom_bank_2 << 14) | u32::from(address & 0x3FFF);
                    self.read_rom_address(rom_addr)
                }
            }
            (Mapper::Codemasters, 0x8000..=0x9FFF) => {
                let rom_addr = (self.rom_bank_2 << 14) | u32::from(address & 0x3FFF);
                self.read_rom_address(rom_addr)
            }
            (Mapper::Codemasters, 0xA000..=0xBFFF) => {
                if self.ram_mapped {
                    // Codemasters cartridges with RAM don't support banking; they only have 8KB
                    self.ram[(address & 0x1FFF) as usize]
                } else {
                    let rom_addr = (self.rom_bank_2 << 14) | u32::from(address & 0x3FFF);
                    self.read_rom_address(rom_addr)
                }
            }
            _ => panic!("0xC000..=0xFFFF should never be read from cartridge"),
        }
    }

    fn write_ram(&mut self, address: u16, value: u8) {
        if self.ram_mapped {
            let ram_addr = (self.ram_bank << 14) | u32::from(address & 0x3FFF);
            self.ram[ram_addr as usize] = value;
        }
    }
}

const SYSTEM_RAM_SIZE: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct Memory {
    cartridge: Cartridge,
    ram: [u8; SYSTEM_RAM_SIZE],
}

impl Memory {
    pub fn new(rom: Vec<u8>) -> Self {
        Self {
            cartridge: Cartridge::from_rom(rom),
            ram: [0; SYSTEM_RAM_SIZE],
        }
    }

    pub fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0xBFFF => self.cartridge.read(address),
            0xC000..=0xFFFF => {
                let ram_addr = address & 0x1FFF;
                self.ram[ram_addr as usize]
            }
        }
    }

    pub fn write(&mut self, address: u16, value: u8) {
        if address >= 0xC000 {
            let ram_addr = address & 0x1FFF;
            self.ram[ram_addr as usize] = value;
        }

        match (self.cartridge.mapper, address) {
            (Mapper::Sega, 0x8000..=0xBFFF) => {
                self.cartridge.write_ram(address, value);
            }
            (Mapper::Sega, 0xFFFC) => {
                log::trace!("RAM flags set to {value:02X}");
                self.cartridge.ram_mapped = value.bit(3);
                self.cartridge.ram_bank = value.bit(2).into();
            }
            (Mapper::Sega, 0xFFFD) | (Mapper::Codemasters, 0x0000..=0x3FFF) => {
                log::trace!("ROM bank 0 set to {value:02X}");
                self.cartridge.rom_bank_0 = value.into();
            }
            (Mapper::Sega, 0xFFFE) => {
                log::trace!("ROM bank 1 set to {value:02X}");
                self.cartridge.rom_bank_1 = value.into();
            }
            (Mapper::Sega, 0xFFFF) | (Mapper::Codemasters, 0x8000..=0x9FFF) => {
                log::trace!("ROM bank 2 set to {value:02X}");
                self.cartridge.rom_bank_2 = value.into();
            }
            (Mapper::Codemasters, 0x4000..=0x7FFF) => {
                log::trace!("ROM bank 1 set to {value:02X}");
                self.cartridge.rom_bank_1 = value.into();
                self.cartridge.ram_mapped = value.bit(7);
            }
            (Mapper::Codemasters, 0xA000..=0xBFFF) => {
                if self.cartridge.ram_mapped {
                    self.cartridge.ram[(address & 0x1FFF) as usize] = value;
                } else {
                    self.cartridge.rom_bank_2 = value.into();
                }
            }
            _ => {}
        }
    }
}
