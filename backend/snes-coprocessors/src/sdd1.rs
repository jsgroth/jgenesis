//! S-DD1 coprocessor, a data decompression chip with an algorithm designed for SNES tile data
//!
//! Used by Star Ocean and Street Fighter Alpha 2

mod decompressor;

use crate::common;
use crate::common::{impl_take_set_rom, Rom};
use crate::sdd1::decompressor::Sdd1Decompressor;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::PartialClone;
use std::array;

#[derive(Debug, Clone, Encode, Decode)]
struct Sdd1Mmc {
    bank_c_base_addr: u32,
    bank_d_base_addr: u32,
    bank_e_base_addr: u32,
    bank_f_base_addr: u32,
}

impl Sdd1Mmc {
    fn new() -> Self {
        Self {
            bank_c_base_addr: 0x000000,
            bank_d_base_addr: 0x100000,
            bank_e_base_addr: 0x200000,
            bank_f_base_addr: 0x300000,
        }
    }

    fn map_rom_address(&self, address: u32, rom_len: u32) -> Option<u32> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) => {
                // Banks $00-$3F and $80-$BF are mapped as if the cartridge were LoROM, except
                // with $80-$BF explicitly mirroring $00-$3F
                Some(common::lorom_map_rom_address(address & 0x7FFFFF, rom_len))
            }
            (0xC0..=0xCF, _) => {
                // MMC bank C
                Some(self.bank_c_base_addr | (address & 0x0FFFFF))
            }
            (0xD0..=0xDF, _) => {
                // MMC bank D
                Some(self.bank_d_base_addr | (address & 0x0FFFFF))
            }
            (0xE0..=0xEF, _) => {
                // MMC bank E
                Some(self.bank_e_base_addr | (address & 0x0FFFFF))
            }
            (0xF0..=0xFF, _) => {
                // MMC bank F
                Some(self.bank_f_base_addr | (address & 0x0FFFFF))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Sdd1 {
    #[partial_clone(default)]
    rom: Rom,
    sram: Box<[u8]>,
    mmc: Sdd1Mmc,
    decompressor: Sdd1Decompressor,
    dma_enabled_1: [bool; 8],
    dma_enabled_2: [bool; 8],
    dma_in_progress: bool,
}

impl Sdd1 {
    #[must_use]
    pub fn new(rom: Box<[u8]>, sram: Box<[u8]>) -> Self {
        Self {
            rom: Rom(rom),
            sram,
            mmc: Sdd1Mmc::new(),
            decompressor: Sdd1Decompressor::new(),
            dma_enabled_1: [false; 8],
            dma_enabled_2: [false; 8],
            dma_in_progress: false,
        }
    }

    #[inline]
    #[must_use]
    pub fn read(&mut self, address: u32) -> Option<u8> {
        if self.dma_in_progress {
            // TODO check address?
            return Some(self.decompressor.next_byte(&self.mmc, &self.rom));
        }

        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) | (0x70..=0x73, _)
                if !self.sram.is_empty() =>
            {
                // SRAM
                let sram_addr = (address as usize) & (self.sram.len() - 1);
                Some(self.sram[sram_addr])
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x4800..=0x4807) => {
                // S-DD1 internal registers
                self.read_register(offset)
            }
            _ => {
                // ROM (or possibly open bus)
                self.mmc
                    .map_rom_address(address, self.rom.len() as u32)
                    .and_then(|rom_addr| self.rom.get(rom_addr as usize).copied())
            }
        }
    }

    fn read_register(&self, offset: u32) -> Option<u8> {
        match offset {
            0x4800 => Some(
                (0..8).map(|i| u8::from(self.dma_enabled_1[i]) << i).reduce(|a, b| a | b).unwrap(),
            ),
            0x4801 => Some(
                (0..8).map(|i| u8::from(self.dma_enabled_2[i]) << i).reduce(|a, b| a | b).unwrap(),
            ),
            0x4802 | 0x4803 => Some(0x00),
            0x4804 => Some((self.mmc.bank_c_base_addr >> 20) as u8),
            0x4805 => Some((self.mmc.bank_d_base_addr >> 20) as u8),
            0x4806 => Some((self.mmc.bank_e_base_addr >> 20) as u8),
            0x4807 => Some((self.mmc.bank_f_base_addr >> 20) as u8),
            _ => None,
        }
    }

    #[inline]
    pub fn write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) | (0x70..=0x73, _)
                if !self.sram.is_empty() =>
            {
                // SRAM
                let sram_addr = (address as usize) & (self.sram.len() - 1);
                self.sram[sram_addr] = value;
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x4800..=0x4807) => {
                // S-DD1 internal registers
                self.write_register(offset, value);
            }
            _ => {}
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register(&mut self, offset: u32, value: u8) {
        match offset {
            0x4800 => {
                // DMA enabled 1
                self.dma_enabled_1 = array::from_fn(|i| value.bit(i as u8));
            }
            0x4801 => {
                // DMA enabled 2
                self.dma_enabled_2 = array::from_fn(|i| value.bit(i as u8));
            }
            0x4802 | 0x4803 => {
                // Unknown functionality; Star Ocean writes $00 to these registers,
                // Street Fighter Alpha 2 doesn't touch them
            }
            0x4804 => {
                // Set MMC bank C
                self.mmc.bank_c_base_addr = u32::from(value) << 20;
            }
            0x4805 => {
                // Set MMC bank D
                self.mmc.bank_d_base_addr = u32::from(value) << 20;
            }
            0x4806 => {
                // Set MMC bank E
                self.mmc.bank_e_base_addr = u32::from(value) << 20;
            }
            0x4807 => {
                // Set MMC bank F
                self.mmc.bank_f_base_addr = u32::from(value) << 20;
            }
            _ => {}
        }
    }

    impl_take_set_rom!(rom);

    #[inline]
    #[must_use]
    pub fn sram(&self) -> Option<&[u8]> {
        (!self.sram.is_empty()).then_some(self.sram.as_ref())
    }

    #[inline]
    pub fn notify_dma_start(&mut self, channel: u8, source_address: u32) {
        if !self.dma_enabled_1[channel as usize]
            || !self.dma_enabled_2[channel as usize]
            || self.mmc.map_rom_address(source_address, self.rom.len() as u32).is_none()
        {
            self.dma_in_progress = false;
            return;
        }

        // DMA enabled 2 is automatically cleared during/after DMA
        self.dma_enabled_2[channel as usize] = false;
        self.dma_in_progress = true;

        self.decompressor.init(source_address, &self.mmc, &self.rom);
    }

    #[inline]
    pub fn notify_dma_end(&mut self) {
        self.dma_in_progress = false;
    }
}
