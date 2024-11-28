//! GBA cartridge code

use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::array;
use std::ops::Deref;

#[derive(Debug, FakeEncode, FakeDecode)]
pub struct Rom(Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for Rom {
    fn default() -> Self {
        Self(vec![].into_boxed_slice())
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Cartridge {
    #[partial_clone(default)]
    pub rom: Rom,
    pub sram: Box<[u8]>,
}

impl Cartridge {
    pub fn new(mut rom: Vec<u8>) -> Self {
        jgenesis_common::mirror_to_power_of_two(&mut rom);

        // TODO figure out actual SRAM size
        let sram = vec![0; 64 * 1024].into_boxed_slice();

        Self { rom: Rom(rom.into_boxed_slice()), sram }
    }

    pub fn read_rom_byte(&self, address: u32) -> u8 {
        self.rom[(address as usize) & (self.rom.len() - 1)]
    }

    pub fn read_rom_halfword(&self, address: u32) -> u16 {
        let rom_addr = (address as usize) & (self.rom.len() - 1) & !1;
        u16::from_le_bytes(array::from_fn(|i| self.rom[rom_addr + i]))
    }

    pub fn read_rom_word(&self, address: u32) -> u32 {
        let rom_addr = (address as usize) & (self.rom.len() - 1) & !3;
        u32::from_le_bytes(array::from_fn(|i| self.rom[rom_addr + i]))
    }

    pub fn read_sram_byte(&self, address: u32) -> u8 {
        self.sram[(address as usize) & (self.sram.len() - 1)]
    }

    pub fn write_sram_byte(&mut self, address: u32, value: u8) {
        self.sram[(address as usize) & (self.sram.len() - 1)] = value;
    }
}
