//! GBA internal memory

use crate::api::GbaInitializationError;
use crate::cartridge::Cartridge;
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_proc_macros::PartialClone;
use std::array;

// BIOS ROM; 16KB
const BIOS_LEN: usize = 16 * 1024;

// External working RAM; 256KB, slow (accesses incur waitstates)
const EWRAM_LEN: usize = 256 * 1024;

// Internal working RAM; 32KB, fast (no waitstates on access)
const IWRAM_LEN: usize = 32 * 1024;

fn read_wram_byte<const LEN: usize>(wram: &[u8; LEN], address: u32) -> u8 {
    wram[(address as usize) & (LEN - 1)]
}

fn read_wram_halfword<const LEN: usize>(wram: &[u8; LEN], address: u32) -> u16 {
    let wram_addr = (address as usize) & (LEN - 1) & !1;
    u16::from_le_bytes([wram[wram_addr], wram[wram_addr + 1]])
}

fn read_wram_word<const LEN: usize>(wram: &[u8; LEN], address: u32) -> u32 {
    let wram_addr = (address as usize) & (LEN - 1) & !3;
    u32::from_le_bytes(array::from_fn(|i| wram[wram_addr + i]))
}

fn write_wram_byte<const LEN: usize>(wram: &mut [u8; LEN], address: u32, value: u8) {
    wram[(address as usize) & (LEN - 1)] = value;
}

fn write_wram_halfword<const LEN: usize>(wram: &mut [u8; LEN], address: u32, value: u16) {
    let wram_addr = (address as usize) & (LEN - 1) & !1;
    wram[wram_addr..wram_addr + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_wram_word<const LEN: usize>(wram: &mut [u8; LEN], address: u32, value: u32) {
    let wram_addr = (address as usize) & (LEN - 1) & !3;
    wram[wram_addr..wram_addr + 4].copy_from_slice(&value.to_le_bytes());
}

macro_rules! impl_read_byte {
    ($name:ident, $wram:ident) => {
        pub fn $name(&self, address: u32) -> u8 {
            read_wram_byte(&self.$wram, address)
        }
    };
}

macro_rules! impl_read_halfword {
    ($name:ident, $wram:ident) => {
        pub fn $name(&self, address: u32) -> u16 {
            read_wram_halfword(&self.$wram, address)
        }
    };
}

macro_rules! impl_read_word {
    ($name:ident, $wram:ident) => {
        pub fn $name(&self, address: u32) -> u32 {
            read_wram_word(&self.$wram, address)
        }
    };
}

macro_rules! impl_write_byte {
    ($name:ident, $wram:ident) => {
        pub fn $name(&mut self, address: u32, value: u8) {
            write_wram_byte(&mut self.$wram, address, value);
        }
    };
}

macro_rules! impl_write_halfword {
    ($name:ident, $wram:ident) => {
        pub fn $name(&mut self, address: u32, value: u16) {
            write_wram_halfword(&mut self.$wram, address, value);
        }
    };
}

macro_rules! impl_write_word {
    ($name:ident, $wram:ident) => {
        pub fn $name(&mut self, address: u32, value: u32) {
            write_wram_word(&mut self.$wram, address, value);
        }
    };
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Memory {
    #[partial_clone(partial)]
    pub cartridge: Cartridge,
    pub bios: BoxedByteArray<BIOS_LEN>,
    pub ewram: BoxedByteArray<EWRAM_LEN>,
    pub iwram: BoxedByteArray<IWRAM_LEN>,
}

impl Memory {
    pub fn new(cartridge: Cartridge, bios_rom: Vec<u8>) -> Result<Self, GbaInitializationError> {
        if bios_rom.len() != BIOS_LEN {
            return Err(GbaInitializationError::InvalidBiosRom { length: bios_rom.len() });
        }

        let bios_rom: Box<[u8; BIOS_LEN]> = bios_rom.into_boxed_slice().try_into().unwrap();

        Ok(Self {
            cartridge,
            bios: bios_rom.into(),
            ewram: BoxedByteArray::new(),
            iwram: BoxedByteArray::new(),
        })
    }

    impl_read_byte!(read_bios_byte, bios);
    impl_read_halfword!(read_bios_halfword, bios);
    impl_read_word!(read_bios_word, bios);

    impl_read_byte!(read_ewram_byte, ewram);
    impl_read_halfword!(read_ewram_halfword, ewram);
    impl_read_word!(read_ewram_word, ewram);

    impl_read_byte!(read_iwram_byte, iwram);
    impl_read_halfword!(read_iwram_halfword, iwram);
    impl_read_word!(read_iwram_word, iwram);

    impl_write_byte!(write_ewram_byte, ewram);
    impl_write_halfword!(write_ewram_halfword, ewram);
    impl_write_word!(write_ewram_word, ewram);

    impl_write_byte!(write_iwram_byte, iwram);
    impl_write_halfword!(write_iwram_halfword, iwram);
    impl_write_word!(write_iwram_word, iwram);
}
