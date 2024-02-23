//! SPC7110 coprocessor, a data decompression chip (optionally with an Epson RTC-4513 real-time clock chip)
//!
//! Used by Tengai Makyou Zero, Momotarou Dentetsu Happy, and Super Power League 4

mod decompressor;
mod registers;
mod rtc;

use crate::common::{impl_take_set_rom, Rom};
use crate::spc7110::decompressor::Spc7110Decompressor;
use crate::spc7110::registers::Registers;
use crate::spc7110::rtc::Rtc4513;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_common::num::{U16Ext, U24Ext};
use jgenesis_proc_macros::PartialClone;

// All 3 SPC7110 game images have a 1MB program ROM followed by a data ROM
const DATA_ROM_START: usize = 0x100000;

// All 3 SPC7110 games have 8KB of SRAM
const SRAM_LEN: usize = 8 * 1024;

type Sram = [u8; SRAM_LEN];

#[derive(Debug, Clone, PartialClone, Encode, Decode)]
pub struct Spc7110 {
    #[partial_clone(default)]
    rom: Rom,
    sram: Box<Sram>,
    registers: Registers,
    decompressor: Spc7110Decompressor,
    rtc: Option<Rtc4513>,
}

impl Spc7110 {
    #[allow(clippy::boxed_local)]
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new<S: SaveWriter>(
        rom: Box<[u8]>,
        initial_sram: Box<[u8]>,
        save_writer: &mut S,
    ) -> Self {
        let sram = if initial_sram.len() == SRAM_LEN {
            initial_sram.try_into().unwrap()
        } else {
            vec![0; SRAM_LEN].into_boxed_slice().try_into().unwrap()
        };

        // Chipset byte of $F9 indicates SPC7110 + RTC-4513 (only used by Tengai Makyou Zero)
        let has_rtc = rom[0xFFD6] == 0xF9;
        let rtc =
            has_rtc.then(|| save_writer.load_serialized("rtc").ok().unwrap_or_else(Rtc4513::new));

        log::info!("SPC7110 has RTC-4513: {}", rtc.is_some());

        Self {
            rom: Rom(rom),
            sram,
            registers: Registers::new(),
            decompressor: Spc7110Decompressor::new(),
            rtc,
        }
    }

    #[inline]
    #[must_use]
    pub fn read(&mut self, address: u32) -> Option<u8> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x4800..=0x4842) => {
                // SPC7110 internal registers
                self.read_register(address)
            }
            (0x00..=0x0F | 0x80..=0x8F, 0x8000..=0xFFFF) | (0xC0..=0xCF, _) => {
                // Program ROM (1MB)
                let rom_addr = address & 0x0FFFFF;
                Some(self.rom[rom_addr as usize])
            }
            (0x40..=0x4F, _) => {
                // Expansion ROM (1MB) if applicable, otherwise mirror program ROM
                // This is needed for the fan translated version of Tengai Makyou Zero which stores
                // an additional 1MB of ROM at the end of the 7MB file and expects it to be mapped
                // to banks $40-$4F
                Some(if self.rom.len() >= 0x700000 {
                    self.rom[0x600000 | (address & 0xFFFFF) as usize]
                } else {
                    self.rom[(address & 0xFFFFF) as usize]
                })
            }
            (0xD0..=0xDF, _) => {
                // Data ROM bank D
                let rom_addr = (address & 0x0FFFFF) | (u32::from(self.registers.rom_bank_d) << 20);
                self.rom[DATA_ROM_START..].get(rom_addr as usize).copied()
            }
            (0xE0..=0xEF, _) => {
                // Data ROM bank E
                let rom_addr = (address & 0x0FFFFF) | (u32::from(self.registers.rom_bank_e) << 20);
                self.rom[DATA_ROM_START..].get(rom_addr as usize).copied()
            }
            (0xF0..=0xFF, _) => {
                // Data ROM bank F
                let rom_addr = (address & 0x0FFFFF) | (u32::from(self.registers.rom_bank_f) << 20);
                self.rom[DATA_ROM_START..].get(rom_addr as usize).copied()
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // SRAM (always 8KB)
                self.registers.sram_enabled.then(|| {
                    let sram_addr = address & 0x1FFF;
                    self.sram[sram_addr as usize]
                })
            }
            (0x50, _) => {
                // Alternate mapping for internal register $4800 (read next decompressed byte)
                Some(self.decompressor.next_byte(&self.rom[DATA_ROM_START..]))
            }
            _ => None,
        }
    }

    #[inline]
    pub fn write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x4800..=0x4842) => {
                // SPC7110 internal registers
                self.write_register(address, value);
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // SRAM (always 8KB)
                if self.registers.sram_enabled {
                    let sram_addr = address & 0x1FFF;
                    self.sram[sram_addr as usize] = value;
                }
            }
            _ => {}
        }
    }

    impl_take_set_rom!(rom);

    #[inline]
    #[must_use]
    pub fn sram(&self) -> &[u8] {
        self.sram.as_ref()
    }

    #[inline]
    #[must_use]
    pub fn rtc(&self) -> Option<&Rtc4513> {
        self.rtc.as_ref()
    }

    #[allow(clippy::match_same_arms)]
    fn read_register(&mut self, address: u32) -> Option<u8> {
        log::trace!("SPC7110 register read {:04X}", address & 0xFFFF);

        match address & 0xFFFF {
            0x4800 => Some(self.decompressor.next_byte(&self.rom[DATA_ROM_START..])),
            0x4801 => Some(self.decompressor.rom_directory_base.low_byte()),
            0x4802 => Some(self.decompressor.rom_directory_base.mid_byte()),
            0x4803 => Some(self.decompressor.rom_directory_base.high_byte()),
            0x4804 => Some(self.decompressor.rom_directory_index),
            0x4805 => Some(self.decompressor.target_offset.lsb()),
            0x4806 => Some(self.decompressor.target_offset.msb()),
            // $4807: Unknown functionality, reportedly DMA channel?
            0x4807 => Some(0x00),
            // $4808: Unknown functionality, reportedly "C r/w option"?
            0x4808 => Some(0x00),
            0x480B => Some(self.decompressor.read_mode()),
            0x480C => Some(self.decompressor.read_status()),
            0x4809 => Some(self.decompressor.length_counter.lsb()),
            0x480A => Some(self.decompressor.length_counter.msb()),
            0x4810 => Some(self.registers.read_direct_data_rom_r4810(&self.rom[DATA_ROM_START..])),
            0x4811 => Some(self.registers.direct_data_rom_base.low_byte()),
            0x4812 => Some(self.registers.direct_data_rom_base.mid_byte()),
            0x4813 => Some(self.registers.direct_data_rom_base.high_byte()),
            0x4814 => Some(self.registers.direct_data_rom_offset.lsb()),
            0x4815 => Some(self.registers.direct_data_rom_offset.msb()),
            0x4816 => Some(self.registers.direct_data_rom_step.lsb()),
            0x4817 => Some(self.registers.direct_data_rom_step.msb()),
            // $4818: Direct data ROM mode; reading from this register has unknown functionality
            0x4818 => Some(0x00),
            0x481A => Some(self.registers.read_direct_data_rom_r481a(&self.rom[DATA_ROM_START..])),
            0x4820..=0x4823 => Some(self.registers.read_dividend(address)),
            0x4824 => Some(self.registers.math.multiplier.lsb()),
            0x4825 => Some(self.registers.math.multiplier.msb()),
            0x4826 => Some(self.registers.math.divisor.lsb()),
            0x4827 => Some(self.registers.math.divisor.msb()),
            0x4828..=0x482B => Some(self.registers.read_math_result(address)),
            0x482C => Some(self.registers.math.remainder.lsb()),
            0x482D => Some(self.registers.math.remainder.msb()),
            // $482E: Reportedly either "reset" or signed/unsigned toggle; games seem to only write $00
            0x482E => Some(0x00),
            // $482F: Multiplication/division unit status; hardcoded to 0 (ready)
            0x482F => Some(0x00),
            0x4830 => Some(self.registers.read_sram_enabled()),
            0x4831 => Some(self.registers.rom_bank_d),
            0x4832 => Some(self.registers.rom_bank_e),
            0x4833 => Some(self.registers.rom_bank_f),
            0x4834 => Some(self.registers.sram_bank),
            0x4840 => self.rtc.as_ref().map(Rtc4513::read_chip_select),
            0x4841 => self.rtc.as_mut().map(Rtc4513::read_data_port),
            0x4842 => self.rtc.is_some().then_some(rtc::STATUS_BYTE),
            _ => None,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register(&mut self, address: u32, value: u8) {
        log::trace!("SPC7110 register write {:04X} {value:02X}", address & 0xFFFF);

        match address & 0xFFFF {
            0x4801 => self.decompressor.write_rom_directory_base_low(value),
            0x4802 => self.decompressor.write_rom_directory_base_mid(value),
            0x4803 => self.decompressor.write_rom_directory_base_high(value),
            0x4804 => self.decompressor.rom_directory_index = value,
            0x4805 => self.decompressor.write_target_offset_low(value),
            0x4806 => {
                self.decompressor.write_target_offset_high(value, &self.rom[DATA_ROM_START..]);
            }
            // $4807: Unknown functionality, reportedly DMA channel?
            0x4807 => {}
            // $4808: Unknown functionality, reportedly "C r/w option"?
            0x4808 => {}
            0x4809 => self.decompressor.write_length_counter_low(value),
            0x480A => self.decompressor.write_length_counter_high(value),
            0x480B => self.decompressor.write_mode(value),
            0x4811 => self.registers.write_direct_data_rom_base_low(value),
            0x4812 => self.registers.write_direct_data_rom_base_mid(value),
            0x4813 => self.registers.write_direct_data_rom_base_high(value),
            0x4814 => self.registers.write_direct_data_rom_offset_low(value),
            0x4815 => self.registers.write_direct_data_rom_offset_high(value),
            0x4816 => self.registers.write_direct_data_rom_step_low(value),
            0x4817 => self.registers.write_direct_data_rom_step_high(value),
            0x4818 => self.registers.write_direct_data_rom_mode(value),
            0x4820..=0x4823 => self.registers.write_dividend(address, value),
            0x4824 => self.registers.write_multiplier_low(value),
            0x4825 => self.registers.write_multiplier_high(value),
            0x4826 => self.registers.write_divisor_low(value),
            0x4827 => self.registers.write_divisor_high(value),
            0x482E => self.registers.write_math_mode(value),
            0x4830 => self.registers.write_sram_enabled(value),
            0x4831 => self.registers.rom_bank_d = value,
            0x4832 => self.registers.rom_bank_e = value,
            0x4833 => self.registers.rom_bank_f = value,
            0x4834 => self.registers.sram_bank = value,
            0x4840 => {
                if let Some(rtc) = &mut self.rtc {
                    rtc.write_chip_select(value);
                }
            }
            0x4841 => {
                if let Some(rtc) = &mut self.rtc {
                    rtc.write_data_port(value);
                }
            }
            _ => {}
        }
    }
}
