pub mod mbc3;

use crate::cartridge::mappers::mbc3::Mbc3Rtc;
use crate::cartridge::HasBasicRamMapping;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_common::num::{GetBit, U16Ext};

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
enum BankingMode {
    Simple,
    Complex,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Mbc1 {
    rom_bank: u8,
    rom_addr_mask: u32,
    ram_bank: u8,
    ram_addr_mask: u32,
    ram_enabled: bool,
    banking_mode: BankingMode,
}

impl Mbc1 {
    pub fn new(rom_len: u32, ram_len: u32) -> Self {
        Self {
            rom_bank: 0,
            rom_addr_mask: rom_len - 1,
            ram_bank: 0,
            ram_addr_mask: ram_len.saturating_sub(1),
            ram_enabled: false,
            banking_mode: BankingMode::Simple,
        }
    }

    pub fn map_rom_address(&self, address: u16) -> u32 {
        if !address.bit(14) {
            // $0000-$3FFF mapping depends on banking mode
            match self.banking_mode {
                BankingMode::Simple => {
                    // $0000-$3FFF is always mapped to the first 16KB of ROM
                    (address & 0x3FFF).into()
                }
                BankingMode::Complex => {
                    // $0000-$3FFF uses the highest 2 bits of ROM bank
                    let rom_bank = self.rom_bank & 0x60;
                    ((u32::from(rom_bank) << 14) | u32::from(address & 0x3FFF)) & self.rom_addr_mask
                }
            }
        } else {
            // $4000-$7FFF is mapped to the currently selected ROM bank
            let rom_bank = if self.rom_bank & 0x1F == 0 { 1 } else { self.rom_bank };
            ((u32::from(rom_bank) << 14) | u32::from(address & 0x3FFF)) & self.rom_addr_mask
        }
    }

    pub fn write_rom_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                self.ram_enabled = value & 0x0F == 0x0A;
            }
            0x2000..=0x3FFF => {
                self.rom_bank = (self.rom_bank & 0xE0) | (value & 0x1F);
            }
            0x4000..=0x5FFF => {
                self.rom_bank = (self.rom_bank & 0x1F) | ((value & 0x03) << 5);
                self.ram_bank = value & 0x03;
            }
            0x6000..=0x7FFF => {
                self.banking_mode =
                    if value.bit(0) { BankingMode::Complex } else { BankingMode::Simple };
            }
            _ => panic!("Invalid cartridge address: {address:04X}"),
        }
    }
}

impl HasBasicRamMapping for Mbc1 {
    fn map_ram_address(&self, address: u16) -> Option<u32> {
        if !self.ram_enabled {
            return None;
        }

        let ram_addr = match self.banking_mode {
            BankingMode::Simple => {
                // RAM is not banked in simple mode; always mapped to the first 8KB of RAM
                (address & 0x1FFF).into()
            }
            BankingMode::Complex => {
                ((u32::from(self.ram_bank) << 13) | u32::from(address & 0x1FFF))
                    & self.ram_addr_mask
            }
        };

        Some(ram_addr)
    }
}

// Every MBC2 cartridge has 512x4 bits of RAM
pub const MBC2_RAM_LEN: usize = 512;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Mbc2 {
    rom_bank: u8,
    rom_addr_mask: u32,
    ram: Box<[u8; MBC2_RAM_LEN]>,
    ram_enabled: bool,
}

impl Mbc2 {
    pub fn new(rom_len: u32, initial_ram: Vec<u8>) -> Self {
        let ram =
            if initial_ram.len() == MBC2_RAM_LEN { initial_ram } else { vec![0; MBC2_RAM_LEN] };

        Self {
            rom_bank: 0,
            rom_addr_mask: rom_len - 1,
            ram: ram.into_boxed_slice().try_into().unwrap(),
            ram_enabled: false,
        }
    }

    pub fn map_rom_address(&self, address: u16) -> u32 {
        basic_map_rom_address(address, self.rom_bank.into(), false, self.rom_addr_mask)
    }

    pub fn read_ram(&self, address: u16) -> u8 {
        if !self.ram_enabled {
            return 0xFF;
        }

        // MBC2 RAM is nibble-sized
        self.ram[(address & 0x1FF) as usize] & 0x0F
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        if !self.ram_enabled {
            return;
        }

        // MBC2 RAM is nibble-sized
        self.ram[(address & 0x1FF) as usize] = value & 0x0F;
    }

    pub fn ram(&self) -> &[u8] {
        self.ram.as_ref()
    }

    pub fn write_rom_address(&mut self, address: u16, value: u8) {
        // MBC2 only has two registers, both mapped to $0000-$3FFF
        if !(0x0000..0x4000).contains(&address) {
            return;
        }

        // Address bit 8 determines whether this sets RAM enabled (clear) or ROM bank (set)
        if !address.bit(8) {
            // Set RAM enabled
            self.ram_enabled = value == 0x0A;
        } else {
            // Set ROM bank
            self.rom_bank = value & 0x0F;
        };
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Mbc3 {
    rom_bank: u8,
    rom_addr_mask: u32,
    ram_bank: u8,
    ram_addr_mask: u32,
    ram_enabled: bool,
    rtc: Option<Mbc3Rtc>,
}

impl Mbc3 {
    pub fn new(rom_len: u32, ram_len: u32, rtc: Option<Mbc3Rtc>) -> Self {
        Self {
            rom_bank: 0,
            rom_addr_mask: rom_len - 1,
            ram_bank: 0,
            ram_addr_mask: if ram_len != 0 { ram_len - 1 } else { 0 },
            ram_enabled: false,
            rtc,
        }
    }

    pub fn map_rom_address(&self, address: u16) -> u32 {
        basic_map_rom_address(address, self.rom_bank.into(), false, self.rom_addr_mask)
    }

    pub fn write_rom_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                // RAM/RTC enabled
                self.ram_enabled = value & 0x0F == 0x0A;
            }
            0x2000..=0x3FFF => {
                // ROM bank
                self.rom_bank = value & 0x7F;
            }
            0x4000..=0x5FFF => {
                // RAM bank number / RTC register select
                self.ram_bank = value & 0x0F;
            }
            0x6000..=0x7FFF => {
                // RTC latch
                if let Some(rtc) = &mut self.rtc {
                    rtc.write_latch(value);
                }
            }
            0x8000..=0xFFFF => panic!("Invalid cartridge address: {address:06X}"),
        }
    }

    pub fn read_ram(&self, address: u16, sram: &[u8]) -> u8 {
        match self.ram_bank {
            0x00..=0x03 => {
                // SRAM
                basic_map_ram_address(
                    self.ram_enabled,
                    address,
                    self.ram_bank.into(),
                    self.ram_addr_mask,
                )
                .map_or(0xFF, |ram_addr| sram[ram_addr as usize])
            }
            0x08..=0x0C => {
                // RTC registers
                self.rtc.as_ref().map_or(0xFF, |rtc| rtc.read_register(self.ram_bank))
            }
            _ => 0xFF,
        }
    }

    pub fn write_ram(&mut self, address: u16, value: u8, sram: &mut [u8]) {
        match self.ram_bank {
            0x00..=0x03 => {
                // SRAM
                let ram_addr = basic_map_ram_address(
                    self.ram_enabled,
                    address,
                    self.ram_bank.into(),
                    self.ram_addr_mask,
                );
                if let Some(ram_addr) = ram_addr {
                    sram[ram_addr as usize] = value;
                }
            }
            0x08..=0x0C => {
                // RTC registers
                if let Some(rtc) = &mut self.rtc {
                    rtc.write_register(self.ram_bank, value);
                }
            }
            _ => {}
        }
    }

    pub fn update_rtc_time(&mut self) {
        if let Some(rtc) = &mut self.rtc {
            rtc.update_time();
        }
    }

    pub fn save_rtc_state<S: SaveWriter>(&self, save_writer: &mut S) -> Result<(), S::Err> {
        if let Some(rtc) = &self.rtc {
            save_writer.persist_serialized("rtc", rtc)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Mbc5 {
    rom_bank: u16,
    rom_addr_mask: u32,
    ram_bank: u8,
    ram_addr_mask: u32,
    ram_enabled: bool,
}

impl Mbc5 {
    pub fn new(rom_len: u32, ram_len: u32) -> Self {
        Self {
            rom_bank: 0,
            rom_addr_mask: rom_len - 1,
            ram_bank: 0,
            ram_addr_mask: if ram_len != 0 { ram_len - 1 } else { 0 },
            ram_enabled: false,
        }
    }

    pub fn map_rom_address(&self, address: u16) -> u32 {
        basic_map_rom_address(address, self.rom_bank.into(), true, self.rom_addr_mask)
    }

    pub fn write_rom_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                // RAM enabled
                self.ram_enabled = value & 0x0F == 0x0A;
            }
            0x2000..=0x2FFF => {
                // Low 8 bits of ROM bank
                self.rom_bank.set_lsb(value);
            }
            0x3000..=0x3FFF => {
                // Highest bit of ROM bank
                self.rom_bank.set_msb(value & 0x01);
            }
            0x4000..=0x5FFF => {
                // RAM bank
                self.ram_bank = value & 0x0F;
            }
            0x6000..=0x7FFF => {}
            _ => panic!("Invalid cartridge address: {address:06X}"),
        }
    }
}

impl HasBasicRamMapping for Mbc5 {
    fn map_ram_address(&self, address: u16) -> Option<u32> {
        basic_map_ram_address(self.ram_enabled, address, self.ram_bank.into(), self.ram_addr_mask)
    }
}

// MBC2 / MBC3 / MBC5
fn basic_map_rom_address(
    address: u16,
    rom_bank: u32,
    allow_bank_0: bool,
    rom_addr_mask: u32,
) -> u32 {
    if address < 0x4000 {
        // First 16KB of ROM
        address.into()
    } else {
        // 16KB ROM bank
        let rom_bank = if !allow_bank_0 && rom_bank == 0 { 1 } else { rom_bank };
        ((rom_bank << 14) | u32::from(address & 0x3FFF)) & rom_addr_mask
    }
}

// MBC3 / MBC5
fn basic_map_ram_address(
    ram_enabled: bool,
    address: u16,
    ram_bank: u32,
    ram_addr_mask: u32,
) -> Option<u32> {
    if !ram_enabled {
        return None;
    }

    Some(((ram_bank << 13) | u32::from(address & 0x1FFF)) & ram_addr_mask)
}
