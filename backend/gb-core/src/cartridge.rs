//! Game Boy cartridge loading and mapping

use crate::api::GameBoyLoadError;
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::ops::Deref;

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
}

impl Cartridge {
    pub fn create(rom: Box<[u8]>) -> Result<Self, GameBoyLoadError> {
        // TODO actual cartridge loading
        Ok(Self { rom: Rom(rom) })
    }

    pub fn read(&self, address: u16) -> u8 {
        // TODO MBCs
        self.rom[(address & 0x7FFF) as usize]
    }
}
