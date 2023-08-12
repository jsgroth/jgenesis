mod metadata;

use crate::num::GetBit;
use crc::Crc;
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
    has_battery: bool,
    rom_bank_0: u32,
    rom_bank_1: u32,
    rom_bank_2: u32,
    ram_mapped: bool,
    ram_bank: u32,
    ram_dirty: bool,
}

// Most cartridges with RAM only had 8KB, but up to 32KB was supported, and the header contains
// no information on RAM size (or even whether RAM is present)
const CARTRIDGE_RAM_SIZE: usize = 32 * 1024;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

impl Cartridge {
    fn new(rom: Vec<u8>, initial_ram: Option<Vec<u8>>) -> Self {
        let mapper = Mapper::detect_from_rom(&rom);
        log::info!("Detected mapper {mapper:?} from ROM header");

        let mut crc_digest = CRC.digest();
        crc_digest.update(&rom);
        let checksum = crc_digest.finalize();
        log::info!("ROM CRC32: {checksum:08X}");

        let has_battery = metadata::has_battery_backup(checksum);
        log::info!("Cartridge has battery-backed RAM: {has_battery}");

        let ram = match initial_ram {
            Some(ram) if ram.len() == CARTRIDGE_RAM_SIZE => {
                log::info!("Successfully loaded cartridge SRAM");
                ram
            }
            _ => vec![0; CARTRIDGE_RAM_SIZE],
        };

        Self {
            rom,
            ram,
            mapper,
            has_battery,
            rom_bank_0: 0,
            rom_bank_1: 1,
            rom_bank_2: 2,
            ram_mapped: false,
            ram_bank: 0,
            ram_dirty: false,
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
            let ram_addr = match self.mapper {
                Mapper::Sega => (self.ram_bank << 14) | u32::from(address & 0x3FFF),
                Mapper::Codemasters => (address & 0x1FFF).into(),
            };
            self.ram[ram_addr as usize] = value;

            self.ram_dirty = true;
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
    pub fn new(rom: Vec<u8>, initial_cartridge_ram: Option<Vec<u8>>) -> Self {
        Self {
            cartridge: Cartridge::new(rom, initial_cartridge_ram),
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
                    self.cartridge.write_ram(address, value);
                } else {
                    self.cartridge.rom_bank_2 = value.into();
                }
            }
            _ => {}
        }
    }

    pub fn cartridge_ram(&self) -> &[u8] {
        &self.cartridge.ram
    }

    pub fn cartridge_has_battery(&self) -> bool {
        self.cartridge.has_battery
    }

    pub fn cartridge_ram_dirty(&self) -> bool {
        self.cartridge.ram_dirty
    }

    pub fn clear_cartridge_ram_dirty(&mut self) {
        self.cartridge.ram_dirty = false;
    }
}
