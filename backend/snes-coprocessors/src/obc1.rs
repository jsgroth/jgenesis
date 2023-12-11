//! OBC1, an OBJ controller chip that is designed to enable updating the OAM sprite table more efficiently
//!
//! Used by 1 game, Metal Combat: Falcon's Revenge

use crate::common;
use crate::common::{impl_take_set_rom, Rom};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::PartialClone;

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Obc1 {
    #[partial_clone(default)]
    rom: Rom,
    sram: Box<[u8]>,
    oam_base: u16,
    oam_index: u16,
    register_7: u8,
}

const OAM_BASE_0: u16 = 0x1C00;
const OAM_BASE_1: u16 = 0x1800;

impl Obc1 {
    #[must_use]
    pub fn new(rom: Box<[u8]>, sram: Box<[u8]>) -> Self {
        Self { rom: Rom(rom), sram, oam_base: OAM_BASE_0, oam_index: 0, register_7: 0 }
    }

    #[inline]
    #[must_use]
    pub fn read(&self, address: u32) -> Option<u8> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x7FF0..=0x7FF7) => {
                // OBC1 internal ports
                Some(self.read_obc1_port(address))
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) | (0x40..=0x6F | 0xC0..=0xFF, _) => {
                // ROM
                let rom_addr = common::lorom_map_rom_address(address, self.rom.len() as u32);
                Some(self.rom[rom_addr as usize])
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FEF | 0x7FF8..=0x7FFF)
            | (0x70..=0x7D, 0x6000..=0x7FFF) => {
                // SRAM (8KB)
                let sram_addr = address & 0x1FFF;
                Some(self.sram[sram_addr as usize])
            }
            _ => None,
        }
    }

    fn read_obc1_port(&self, address: u32) -> u8 {
        log::trace!("OBC1 register read: {}", address & 7);

        match address & 7 {
            port @ 0..=3 => {
                // OAM lower bytes
                let sram_addr = self.oam_base + (self.oam_index << 2) + port as u16;
                self.sram[sram_addr as usize]
            }
            4 => {
                // OAM upper bits
                let sram_addr = self.oam_base + 0x200 + (self.oam_index >> 2);
                self.sram[sram_addr as usize]
            }
            5 => {
                // OAM base in SRAM (bit 0; 0=$7C00, 1=$7800)
                u8::from(self.oam_base == OAM_BASE_1)
            }
            6 => {
                // OAM index
                self.oam_index as u8
            }
            7 => {
                // Unknown (SRAM vs. I/O ports?)
                self.register_7
            }
            _ => unreachable!("value & 7 is always <= 7"),
        }
    }

    #[inline]
    pub fn write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x7FF0..=0x7FF7) => {
                // OBC1 internal ports
                self.write_obc1_port(address, value);
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FEF | 0x7FF8..=0x7FFF)
            | (0x70..=0x7D, 0x6000..=0x7FFF) => {
                // SRAM (8KB)
                let sram_addr = address & 0x1FFF;
                self.sram[sram_addr as usize] = value;
            }
            _ => {}
        }
    }

    fn write_obc1_port(&mut self, address: u32, value: u8) {
        log::trace!("OBC1 register write: {} {value:02X}", address & 7);

        match address & 7 {
            port @ 0..=3 => {
                // OAM lower bytes
                let sram_addr = self.oam_base + (self.oam_index << 2) + port as u16;
                self.sram[sram_addr as usize] = value;
            }
            4 => {
                // OAM upper bits
                // Only set the 2 bits for the specified OAM index
                let sram_addr = (self.oam_base + 0x200 + (self.oam_index >> 2)) as usize;
                let shift = 2 * (self.oam_index & 0x03);
                self.sram[sram_addr] =
                    (self.sram[sram_addr] & !(0x03 << shift)) | ((value & 0x03) << shift);
            }
            5 => {
                // OAM base in SRAM (bit 0; 0=$7C00, 1=$7800)
                self.oam_base = if value.bit(0) { OAM_BASE_1 } else { OAM_BASE_0 };
            }
            6 => {
                // OAM index (0-127)
                self.oam_index = u16::from(value & 0x7F);
            }
            7 => {
                // Unknown (SRAM vs. I/O ports?)
                self.register_7 = value;
            }
            _ => unreachable!("value & 7 is always <= 7"),
        }
    }

    #[inline]
    #[must_use]
    pub fn sram(&self) -> &[u8] {
        self.sram.as_ref()
    }

    impl_take_set_rom!(rom);
}
