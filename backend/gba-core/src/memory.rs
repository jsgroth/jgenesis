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
    pub waitcnt: u16,
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
            waitcnt: 0,
        })
    }

    pub fn read_bios_rom(&self, address: u32) -> u32 {
        let word_addr = (address as usize) & (BIOS_ROM_LEN - 1) & !3;
        u32::from_le_bytes(self.bios_rom[word_addr..word_addr + 4].try_into().unwrap())
    }

    pub fn read_iwram_byte(&self, address: u32) -> u8 {
        read_byte(&self.iwram, address)
    }

    pub fn read_iwram_halfword(&self, address: u32) -> u16 {
        read_halfword(&self.iwram, address)
    }

    pub fn read_iwram_word(&self, address: u32) -> u32 {
        read_word(&self.iwram, address)
    }

    pub fn read_ewram_byte(&self, address: u32) -> u8 {
        read_byte(&self.ewram, address)
    }

    pub fn read_ewram_halfword(&self, address: u32) -> u16 {
        read_halfword(&self.ewram, address)
    }

    pub fn read_ewram_word(&self, address: u32) -> u32 {
        read_word(&self.ewram, address)
    }

    pub fn write_iwram_byte(&mut self, address: u32, value: u8) {
        write_byte(&mut self.iwram, address, value);
    }

    pub fn write_iwram_halfword(&mut self, address: u32, value: u16) {
        write_halfword(&mut self.iwram, address, value);
    }

    pub fn write_iwram_word(&mut self, address: u32, value: u32) {
        write_word(&mut self.iwram, address, value);
    }

    pub fn write_ewram_byte(&mut self, address: u32, value: u8) {
        write_byte(&mut self.ewram, address, value);
    }

    pub fn write_ewram_halfword(&mut self, address: u32, value: u16) {
        write_halfword(&mut self.ewram, address, value);
    }

    pub fn write_ewram_word(&mut self, address: u32, value: u32) {
        write_word(&mut self.ewram, address, value);
    }

    pub fn clone_bios_rom(&mut self) -> Vec<u8> {
        self.bios_rom.to_vec()
    }
}

fn read_byte<const LEN: usize>(memory: &[u8; LEN], address: u32) -> u8 {
    let memory_addr = (address as usize) & (LEN - 1);
    memory[memory_addr]
}

fn read_halfword<const LEN: usize>(memory: &[u8; LEN], address: u32) -> u16 {
    let memory_addr = (address as usize) & (LEN - 1) & !1;
    u16::from_le_bytes(memory[memory_addr..memory_addr + 2].try_into().unwrap())
}

fn read_word<const LEN: usize>(memory: &[u8; LEN], address: u32) -> u32 {
    let memory_addr = (address as usize) & (LEN - 1) & !3;
    u32::from_le_bytes(memory[memory_addr..memory_addr + 4].try_into().unwrap())
}

fn write_byte<const LEN: usize>(memory: &mut [u8; LEN], address: u32, value: u8) {
    let memory_addr = (address as usize) & (LEN - 1);
    memory[memory_addr] = value;
}

fn write_halfword<const LEN: usize>(memory: &mut [u8; LEN], address: u32, value: u16) {
    let memory_addr = (address as usize) & (LEN - 1) & !1;
    memory[memory_addr..memory_addr + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_word<const LEN: usize>(memory: &mut [u8; LEN], address: u32, value: u32) {
    let memory_addr = (address as usize) & (LEN - 1) & !3;
    memory[memory_addr..memory_addr + 4].copy_from_slice(&value.to_le_bytes());
}
