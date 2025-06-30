//! GBA internal memory

use crate::api::GbaLoadError;
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;

const BIOS_ROM_LEN: usize = 16 * 1024;
const IWRAM_LEN: usize = 32 * 1024;
const EWRAM_LEN: usize = 256 * 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    bios_rom: BoxedByteArray<BIOS_ROM_LEN>,
    iwram: BoxedByteArray<IWRAM_LEN>,
    ewram: BoxedByteArray<EWRAM_LEN>,
}

impl Memory {
    pub fn new(bios_rom: Vec<u8>) -> Result<Self, GbaLoadError> {
        if bios_rom.len() != BIOS_ROM_LEN {
            return Err(GbaLoadError::InvalidBiosLength {
                expected: BIOS_ROM_LEN,
                actual: bios_rom.len(),
            });
        }

        let bios_rom: Box<[u8; BIOS_ROM_LEN]> = bios_rom.into_boxed_slice().try_into().unwrap();

        Ok(Self {
            bios_rom: bios_rom.into(),
            iwram: BoxedByteArray::new(),
            ewram: BoxedByteArray::new(),
        })
    }

    pub fn clone_bios_rom(&mut self) -> Vec<u8> {
        self.bios_rom.to_vec()
    }
}
