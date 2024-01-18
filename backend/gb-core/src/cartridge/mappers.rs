use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

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
            ram_enabled: true,
            banking_mode: BankingMode::Simple,
        }
    }

    pub fn map_rom_address(&self, address: u16) -> u32 {
        match self.banking_mode {
            BankingMode::Simple => {
                if !address.bit(14) {
                    // $0000-$3FFF is always mapped to the first 16KB of ROM
                    (address & 0x3FFF).into()
                } else {
                    // $4000-$7FFF is mapped to the currently selected ROM bank
                    let rom_bank = if self.rom_bank & 0x1F == 0 { 1 } else { self.rom_bank };
                    ((u32::from(rom_bank) << 14) | u32::from(address & 0x3FFF)) & self.rom_addr_mask
                }
            }
            BankingMode::Complex => todo!("complex MBC1 mode"),
        }
    }

    pub fn map_ram_address(&self, address: u16) -> u32 {
        match self.banking_mode {
            BankingMode::Simple => {
                // RAM is not banked in simple mode; always mapped to the first 8KB of RAM
                (address & 0x1FFF).into()
            }
            BankingMode::Complex => {
                ((u32::from(self.ram_bank) << 13) | u32::from(address & 0x1FFF))
                    & self.ram_addr_mask
            }
        }
    }

    pub fn write_rom_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                self.ram_enabled = value & 0x0A == 0x0A;
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

    pub fn is_ram_enabled(&self) -> bool {
        self.ram_enabled
    }
}
