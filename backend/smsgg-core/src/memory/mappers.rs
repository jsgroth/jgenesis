use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::MatchEachVariantMacro;
use std::ops::RangeInclusive;

const CODEMASTERS_CHECKSUM_ADDR: usize = 0x7FE6;
const SEGA_HEADER_ADDR_RANGE: RangeInclusive<usize> = 0x7FF0..=0x7FFF;

macro_rules! invalid_cartridge_address {
    ($address:expr) => {
        panic!("Invalid cartridge address {:04X}, must be $0000-$BFFF", $address)
    };
}

fn read_16kb_banked(bytes: &[u8], address: u16, bank: u32) -> u8 {
    let rom_addr = (bank << 14) | u32::from(address & 0x3FFF);
    read_wrapped(bytes, rom_addr)
}

fn read_wrapped(bytes: &[u8], address: u32) -> u8 {
    let wrapped_addr = (address as usize) & (bytes.len() - 1);
    bytes[wrapped_addr]
}

fn write_16kb_banked(bytes: &mut [u8], address: u16, bank: u32, value: u8) {
    let rom_addr = (bank << 14) | u32::from(address & 0x3FFF);
    write_wrapped(bytes, rom_addr, value);
}

fn write_wrapped(bytes: &mut [u8], address: u32, value: u8) {
    let wrapped_addr = (address as usize) & (bytes.len() - 1);
    bytes[wrapped_addr] = value;
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct SegaMapper {
    rom_banks: [u32; 3],
    ram_bank: u32,
    ram_enabled: bool,
}

impl SegaMapper {
    pub fn new() -> Self {
        Self { rom_banks: [0, 1, 2], ram_bank: 0, ram_enabled: false }
    }

    pub fn read(&self, address: u16, rom: &[u8], ram: &[u8]) -> u8 {
        match address {
            0x0000..=0x03FF => {
                // First 1KB of ROM
                read_wrapped(rom, address.into())
            }
            0x0400..=0x7FFF => {
                // Mappable ROM banks 0/1
                let bank = self.rom_banks[(address / 0x4000) as usize];
                read_16kb_banked(rom, address, bank)
            }
            0x8000..=0xBFFF => {
                // Mappable ROM bank 2 or RAM bank
                if self.ram_enabled {
                    read_16kb_banked(ram, address, self.ram_bank)
                } else {
                    read_16kb_banked(rom, address, self.rom_banks[2])
                }
            }
            0xC000..=0xFFFF => invalid_cartridge_address!(address),
        }
    }

    pub fn write(&mut self, address: u16, value: u8, ram: &mut [u8], ram_dirty: &mut bool) {
        match address {
            0x8000..=0xBFFF => {
                // RAM bank (if mapped)
                if self.ram_enabled {
                    write_16kb_banked(ram, address, self.ram_bank, value);
                    *ram_dirty = true;
                }
            }
            0xFFFC => {
                // Control / RAM banking
                self.ram_bank = value.bit(2).into();
                self.ram_enabled = value.bit(3);
                // TODO Bank shifting and RAM enabled for $C000-$FFFF; not used by any known software
                log::debug!(
                    "$FFFC write: {value:02X} (RAM bank = {}, RAM enabled = {})",
                    self.ram_bank,
                    self.ram_enabled
                );
            }
            0xFFFD..=0xFFFF => {
                // ROM banking
                let bank_idx = address - 0xFFFD;
                self.rom_banks[bank_idx as usize] = value.into();
                log::debug!("${address:02X} write: {value:02X} (ROM bank {bank_idx})");
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct CodemastersMapper {
    rom_banks: [u32; 3],
    ram_enabled: bool,
}

impl CodemastersMapper {
    pub fn new() -> Self {
        Self { rom_banks: [0, 1, 2], ram_enabled: false }
    }

    pub fn read(&self, address: u16, rom: &[u8], ram: &[u8]) -> u8 {
        match address {
            0x0000..=0x9FFF => {
                // ROM banks (16KB)
                let bank = self.rom_banks[(address / 0x4000) as usize];
                read_16kb_banked(rom, address, bank)
            }
            0xA000..=0xBFFF => {
                // RAM or upper half of ROM bank 2
                if self.ram_enabled {
                    // No Codemasters game has more than 8KB of RAM
                    read_wrapped(ram, (address & 0x1FFF).into())
                } else {
                    read_16kb_banked(rom, address, self.rom_banks[2])
                }
            }
            0xC000..=0xFFFF => invalid_cartridge_address!(address),
        }
    }

    pub fn write(&mut self, address: u16, value: u8, ram: &mut [u8], ram_dirty: &mut bool) {
        match address {
            0x0000..=0x3FFF => {
                // ROM bank 0
                self.rom_banks[0] = value.into();
                log::trace!("${address:04X} write: {value:02X} (ROM bank 0)");
            }
            0x4000..=0x7FFF => {
                // ROM bank 1 and RAM enabled
                self.rom_banks[1] = value.into();
                self.ram_enabled = value.bit(7);
                log::trace!(
                    "${address:04X} write: {value:02X} (ROM bank 1, RAM enabled = {})",
                    self.ram_enabled
                );
            }
            0x8000..=0xBFFF => {
                // ROM bank 2 and RAM (in upper half if enabled)
                if self.ram_enabled && address >= 0xA000 {
                    write_wrapped(ram, (address & 0x1FFF).into(), value);
                    *ram_dirty = true;
                } else {
                    self.rom_banks[2] = value.into();
                    log::trace!("${address:04X} write: {value:02X} (ROM bank 2)");
                }
            }
            0xC000..=0xFFFF => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, MatchEachVariantMacro)]
pub enum Mapper {
    Sega(SegaMapper),
    Codemasters(CodemastersMapper),
}

impl Mapper {
    // Codemasters ROMs have a 16-bit checksum at $7FE6 which is the sum of all 16-bit words in the ROM
    // except for the words in the Sega header. If summing all of the words matches the word at
    // $7FE6, assume this is a Codemasters ROM
    pub fn detect_from_rom(rom: &[u8]) -> Self {
        if rom.len() < 32 * 1024 {
            // No real ROMs should be less than 32KB, but regardless this isn't a Codemasters ROM
            return Self::Sega(SegaMapper::new());
        }

        let expected_checksum = u16::from_le_bytes([
            rom[CODEMASTERS_CHECKSUM_ADDR],
            rom[CODEMASTERS_CHECKSUM_ADDR + 1],
        ]);
        let mut checksum = 0_u16;
        for address in (0..rom.len() & !1).step_by(2) {
            if !SEGA_HEADER_ADDR_RANGE.contains(&address) {
                let word = u16::from_le_bytes([rom[address], rom[address + 1]]);
                checksum = checksum.wrapping_add(word);
            }
        }

        if checksum == expected_checksum {
            log::info!(
                "Codemasters-style ROM checksum is {checksum:04X}, matches word at {CODEMASTERS_CHECKSUM_ADDR:04X}; assuming this is a Codemasters ROM"
            );
            Self::Codemasters(CodemastersMapper::new())
        } else {
            Self::Sega(SegaMapper::new())
        }
    }

    pub fn read(&self, address: u16, rom: &[u8], ram: &[u8]) -> u8 {
        match_each_variant!(self, mapper => mapper.read(address, rom, ram))
    }

    pub fn write(&mut self, address: u16, value: u8, ram: &mut [u8], ram_dirty: &mut bool) {
        match_each_variant!(self, mapper => mapper.write(address, value, ram, ram_dirty));
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Sega(_) => "Sega",
            Self::Codemasters(_) => "Codemasters",
        }
    }
}
