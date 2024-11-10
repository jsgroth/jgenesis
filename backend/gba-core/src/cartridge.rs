use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::array;
use std::ops::Deref;

#[derive(Debug, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

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
    rom: Rom,
}

impl Cartridge {
    pub fn new(rom: Vec<u8>) -> Self {
        // TODO extend ROM to next highest power of 2 len
        Self { rom: Rom(rom.into_boxed_slice()) }
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
}
