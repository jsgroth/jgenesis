mod metadata;

use crate::memory::eeprom::{X24C01Chip, X24C02Chip, X24C08Chip, X24C16Chip};
use crate::memory::external::metadata::{EepromMetadata, EepromType};
use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;
use std::mem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub(crate) enum RamType {
    SixteenBit,
    EightBitOddAddress,
    EightBitEvenAddress,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Ram {
    ram: Vec<u8>,
    address_mask: u32,
    ram_type: RamType,
    persistent: bool,
    dirty: bool,
    start_address: u32,
    end_address: u32,
}

impl Ram {
    pub(crate) fn from_rom_header(rom: &[u8], initial_ram: &mut Option<Vec<u8>>) -> Option<Self> {
        let ram_header_bytes = &rom[0x1B0..0x1BC];

        // RAM header should always start with ASCII "RA" followed by $F8 and $20
        if ram_header_bytes[..4] != [b'R', b'A', 0xF8, 0x20] {
            return None;
        }

        // Third byte indicates RAM type and whether or not it is persistent memory
        let (ram_type, persistent) = match ram_header_bytes[2] {
            0xA0 => (RamType::SixteenBit, false),
            0xB0 => (RamType::EightBitEvenAddress, false),
            0xB8 => (RamType::EightBitOddAddress, false),
            0xE0 => (RamType::SixteenBit, true),
            0xF0 => (RamType::EightBitEvenAddress, true),
            0xF8 => (RamType::EightBitOddAddress, true),
            _ => {
                return None;
            }
        };

        // Next 8 bytes indicate start and end addresses
        let start_address = u32::from_be_bytes([
            ram_header_bytes[4],
            ram_header_bytes[5],
            ram_header_bytes[6],
            ram_header_bytes[7],
        ]);
        let end_address = u32::from_be_bytes([
            ram_header_bytes[8],
            ram_header_bytes[9],
            ram_header_bytes[10],
            ram_header_bytes[11],
        ]);

        log::info!(
            "RAM header information: type={ram_type:?}, persistent={persistent}, start_address={start_address:06X}, end_address={end_address:06X}"
        );

        let ram_len = if ram_type == RamType::SixteenBit {
            end_address - start_address + 1
        } else {
            (end_address - start_address) / 2 + 1
        };

        let ram = match initial_ram.take() {
            Some(ram) if ram.len() as u32 == ram_len => ram,
            _ => vec![0; ram_len as usize],
        };

        // TODO support RAM persistence
        Some(Self {
            ram,
            address_mask: ram_len - 1,
            ram_type,
            persistent,
            dirty: false,
            start_address,
            end_address,
        })
    }

    fn map_address(&self, address: u32) -> Option<u32> {
        if !(self.start_address..=self.end_address).contains(&address) {
            return None;
        }

        match (self.ram_type, address.bit(0)) {
            (RamType::SixteenBit, _) => Some(address & self.address_mask),
            (RamType::EightBitOddAddress, false) | (RamType::EightBitEvenAddress, true) => None,
            (RamType::EightBitEvenAddress, false) | (RamType::EightBitOddAddress, true) => {
                Some((address >> 1) & self.address_mask)
            }
        }
    }

    fn read_byte(&self, address: u32) -> Option<u8> {
        self.map_address(address).map(|address| self.ram[address as usize])
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        if let Some(address) = self.map_address(address) {
            self.ram[address as usize] = value;
            self.dirty = true;
        }
    }

    fn read_word(&self, address: u32) -> Option<u16> {
        let msb = self.read_byte(address);
        let lsb = self.read_byte(address.wrapping_add(1));
        if msb.is_none() && lsb.is_none() {
            None
        } else {
            Some(u16::from_be_bytes([msb.unwrap_or(0x00), lsb.unwrap_or(0x00)]))
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        let msb_address = self.map_address(address);
        let lsb_address = self.map_address(address.wrapping_add(1));

        let [msb, lsb] = value.to_be_bytes();
        if let Some(msb_address) = msb_address {
            self.ram[msb_address as usize] = msb;
            self.dirty = true;
        }
        if let Some(lsb_address) = lsb_address {
            self.ram[lsb_address as usize] = lsb;
            self.dirty = true;
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Eeprom {
    X24C01(X24C01Chip),
    X24C02(X24C02Chip),
    X24C08(X24C08Chip),
    X24C16(X24C16Chip),
}

macro_rules! match_each_eeprom {
    ($eeprom:expr, $chip:ident => $match_arm:expr) => {
        match $eeprom {
            Eeprom::X24C01($chip) => $match_arm,
            Eeprom::X24C02($chip) => $match_arm,
            Eeprom::X24C08($chip) => $match_arm,
            Eeprom::X24C16($chip) => $match_arm,
        }
    };
}

impl Eeprom {
    fn handle_read(&self) -> bool {
        match_each_eeprom!(self, chip => chip.handle_read())
    }

    fn handle_data_write(&mut self, data: bool) {
        match_each_eeprom!(self, chip => chip.handle_data_write(data));
    }

    fn handle_clock_write(&mut self, clock: bool) {
        match_each_eeprom!(self, chip => chip.handle_clock_write(clock));
    }

    fn handle_dual_write(&mut self, data: bool, clock: bool) {
        match_each_eeprom!(self, chip => chip.handle_dual_write(data, clock));
    }

    fn get_and_clear_dirty_bit(&mut self) -> bool {
        match_each_eeprom!(self, chip => chip.get_and_clear_dirty_bit())
    }

    fn get_memory(&self) -> &[u8] {
        match_each_eeprom!(self, chip => chip.get_memory())
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) enum ExternalMemory {
    None,
    Ram(Ram),
    Eeprom {
        chip: Box<Eeprom>,
        sda_in_addr: u32,
        sda_in_bit: u8,
        sda_out_addr: u32,
        sda_out_bit: u8,
        scl_addr: u32,
        scl_bit: u8,
    },
}

impl ExternalMemory {
    pub(crate) fn from_rom(rom: &[u8], mut initial_ram: Option<Vec<u8>>) -> Self {
        if let Some(ram) = Ram::from_rom_header(rom, &mut initial_ram) {
            return Self::Ram(ram);
        }

        if let Some(eeprom_metadata) = metadata::eeprom(rom) {
            log::info!("EEPROM metadata: {eeprom_metadata:X?}");
            return new_eeprom(rom, initial_ram, eeprom_metadata);
        }

        Self::None
    }

    pub(crate) fn read_byte(&self, address: u32) -> Option<u8> {
        match self {
            Self::None => None,
            Self::Ram(ram) => ram.read_byte(address),
            Self::Eeprom { chip, sda_out_addr, sda_out_bit, .. } => {
                (*sda_out_addr == address).then(|| u8::from(chip.handle_read()) << *sda_out_bit)
            }
        }
    }

    pub(crate) fn read_word(&self, address: u32) -> Option<u16> {
        match self {
            Self::None => None,
            Self::Ram(ram) => ram.read_word(address),
            &Self::Eeprom { sda_out_addr, .. } => {
                if address == sda_out_addr {
                    // TODO shift left 8?
                    self.read_byte(address).map(u16::from)
                } else if address + 1 == sda_out_addr {
                    self.read_byte(address + 1).map(u16::from)
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn write_byte(&mut self, address: u32, value: u8) {
        match self {
            Self::None => {}
            Self::Ram(ram) => {
                ram.write_byte(address, value);
            }
            Self::Eeprom { chip, sda_in_addr, sda_in_bit, scl_addr, scl_bit, .. } => {
                if address == *sda_in_addr && address == *scl_addr {
                    chip.handle_dual_write(value.bit(*sda_in_bit), value.bit(*scl_bit));
                } else if address == *sda_in_addr {
                    chip.handle_data_write(value.bit(*sda_in_bit));
                } else if address == *scl_addr {
                    chip.handle_clock_write(value.bit(*scl_bit));
                }
            }
        }
    }

    pub(crate) fn write_word(&mut self, address: u32, value: u16) {
        match self {
            Self::None => {}
            Self::Ram(ram) => {
                ram.write_word(address, value);
            }
            Self::Eeprom { .. } => {
                self.write_byte(address, value as u8);
            }
        }
    }

    pub(crate) fn take_if_persistent(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::None => None,
            Self::Ram(ram) => ram.persistent.then(|| mem::take(&mut ram.ram)),
            Self::Eeprom { chip, .. } => Some(chip.get_memory().to_vec()),
        }
    }

    pub(crate) fn get_memory(&self) -> &[u8] {
        const EMPTY_SLICE: &[u8] = &[];

        match self {
            Self::None => EMPTY_SLICE,
            Self::Ram(ram) => &ram.ram,
            Self::Eeprom { chip, .. } => chip.get_memory(),
        }
    }

    pub(crate) fn is_persistent(&self) -> bool {
        match self {
            Self::None => false,
            Self::Ram(ram) => ram.persistent,
            Self::Eeprom { .. } => true,
        }
    }

    pub(crate) fn get_and_clear_dirty_bit(&mut self) -> bool {
        match self {
            Self::None => false,
            Self::Ram(ram) => {
                let dirty = ram.dirty;
                ram.dirty = false;
                dirty
            }
            Self::Eeprom { chip, .. } => chip.get_and_clear_dirty_bit(),
        }
    }
}

fn new_eeprom(
    rom: &[u8],
    initial_ram: Option<Vec<u8>>,
    metadata: EepromMetadata,
) -> ExternalMemory {
    let chip = Box::new(match metadata.eeprom_type {
        EepromType::X24C01 => Eeprom::X24C01(X24C01Chip::new(initial_ram.as_ref())),
        EepromType::X24C02 => Eeprom::X24C02(X24C02Chip::new(initial_ram.as_ref())),
        EepromType::X24C08 => {
            if metadata::is_micro_machines_2(rom) && initial_ram.is_none() {
                // Micro Machines 2 supposedly requires EEPROM to be initialized with the string
                // "PETETEST01234567" repeatedly or it won't use chip
                let ram = "PETETEST01234567".bytes().cycle().take(1024).collect::<Vec<_>>();
                Eeprom::X24C08(X24C08Chip::new(Some(ram.as_ref())))
            } else {
                Eeprom::X24C08(X24C08Chip::new(initial_ram.as_ref()))
            }
        }
        EepromType::X24C16 => Eeprom::X24C16(X24C16Chip::new(initial_ram.as_ref())),
    });

    ExternalMemory::Eeprom {
        chip,
        sda_in_addr: metadata.sda_in_addr,
        sda_in_bit: metadata.sda_in_bit,
        sda_out_addr: metadata.sda_out_addr,
        sda_out_bit: metadata.sda_out_bit,
        scl_addr: metadata.scl_addr,
        scl_bit: metadata.scl_bit,
    }
}
