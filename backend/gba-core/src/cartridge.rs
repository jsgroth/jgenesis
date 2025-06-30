//! GBA cartridge / game pak code

use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::mem;
use std::ops::Deref;

#[derive(Debug, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

impl Default for Rom {
    fn default() -> Self {
        Self(vec![].into_boxed_slice())
    }
}

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
}

impl Cartridge {
    pub fn new(mut rom: Vec<u8>) -> Self {
        jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);

        Self { rom: Rom(rom.into_boxed_slice()) }
    }

    pub fn read_rom_byte(&self, address: u32) -> u8 {
        let rom_addr = (address as usize) & (self.rom.len() - 1);
        self.rom[rom_addr]
    }

    pub fn read_rom_halfword(&self, address: u32) -> u16 {
        let rom_addr = (address as usize) & (self.rom.len() - 1) & !1;
        u16::from_le_bytes(self.rom[rom_addr..rom_addr + 2].try_into().unwrap())
    }

    pub fn read_rom_word(&self, address: u32) -> u32 {
        let rom_addr = (address as usize) & (self.rom.len() - 1) & !3;
        u32::from_le_bytes(self.rom[rom_addr..rom_addr + 4].try_into().unwrap())
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom.0).into_vec()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom = mem::take(&mut other.rom);
    }
}
