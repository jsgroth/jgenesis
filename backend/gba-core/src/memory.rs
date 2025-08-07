//! GBA internal memory

use crate::api::{GbaEmulatorConfig, GbaLoadError};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::num::GetBit;
use std::array;

const BIOS_ROM_LEN: usize = 16 * 1024;
const IWRAM_LEN: usize = 32 * 1024;
const EWRAM_LEN: usize = 256 * 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct MemoryControl {
    pub sram_wait: u64,
    pub cartridge_n_wait: [u64; 3],
    pub cartridge_s_wait: [u64; 3],
    pub prefetch_enabled: bool,
    pub raw_value: u16,
}

impl MemoryControl {
    const SRAM_WAIT: [u64; 4] = [4, 3, 2, 8];
    const CARTRIDGE_N_WAIT: [u64; 4] = [4, 3, 2, 8];
    const CARTRIDGE_0_S_WAIT: [u64; 2] = [2, 1];
    const CARTRIDGE_1_S_WAIT: [u64; 2] = [4, 1];
    const CARTRIDGE_2_S_WAIT: [u64; 2] = [8, 1];

    pub fn new() -> Self {
        Self {
            sram_wait: Self::SRAM_WAIT[0],
            cartridge_n_wait: array::from_fn(|_| Self::CARTRIDGE_N_WAIT[0]),
            cartridge_s_wait: [
                Self::CARTRIDGE_0_S_WAIT[0],
                Self::CARTRIDGE_1_S_WAIT[0],
                Self::CARTRIDGE_2_S_WAIT[0],
            ],
            prefetch_enabled: false,
            raw_value: 0x0000,
        }
    }

    pub fn read(&self) -> u16 {
        self.raw_value
    }

    pub fn write(&mut self, value: u16) {
        self.sram_wait = Self::SRAM_WAIT[(value & 3) as usize];
        self.cartridge_n_wait =
            array::from_fn(|i| Self::CARTRIDGE_N_WAIT[((value >> (2 + 3 * i)) & 3) as usize]);
        self.cartridge_s_wait = [
            Self::CARTRIDGE_0_S_WAIT[usize::from(value.bit(4))],
            Self::CARTRIDGE_1_S_WAIT[usize::from(value.bit(7))],
            Self::CARTRIDGE_2_S_WAIT[usize::from(value.bit(10))],
        ];
        self.prefetch_enabled = value.bit(14);

        self.raw_value = value;

        log::debug!("WAITCNT write: {value:04X}");
        log::debug!("  SRAM wait states: {}", self.sram_wait);
        log::debug!("  Cartridge 0 N wait states: {}", self.cartridge_n_wait[0]);
        log::debug!("  Cartridge 0 S wait states: {}", self.cartridge_s_wait[0]);
        log::debug!("  Cartridge 1 N wait states: {}", self.cartridge_n_wait[1]);
        log::debug!("  Cartridge 1 S wait states: {}", self.cartridge_s_wait[1]);
        log::debug!("  Cartridge 2 N wait states: {}", self.cartridge_n_wait[2]);
        log::debug!("  Cartridge 2 S wait states: {}", self.cartridge_s_wait[2]);
        log::debug!("  Cartridge ROM prefetch enabled: {}", self.prefetch_enabled);
    }

    pub fn rom_n_wait_states(&self, address: u32) -> u64 {
        self.cartridge_n_wait[wait_states_area(address)]
    }

    pub fn rom_s_wait_states(&self, address: u32) -> u64 {
        self.cartridge_s_wait[wait_states_area(address)]
    }
}

fn wait_states_area(address: u32) -> usize {
    // Area 0: $08000000-$09FFFFFF
    // Area 1: $0A000000-$0BFFFFFF
    // Area 2: $0C000000-$0DFFFFFF
    assert!((0x08000000..0x0E000000).contains(&address), "{address:08X}");
    ((address >> 25) & 3) as usize
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    bios_rom: BoxedByteArray<BIOS_ROM_LEN>,
    iwram: BoxedByteArray<IWRAM_LEN>,
    ewram: BoxedByteArray<EWRAM_LEN>,
    memory_control: MemoryControl,
    post_boot: bool,
}

impl Memory {
    pub fn new(bios_rom: Vec<u8>, config: GbaEmulatorConfig) -> Result<Self, GbaLoadError> {
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
            memory_control: MemoryControl::new(),
            post_boot: config.skip_bios_animation,
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

    pub fn control(&self) -> &MemoryControl {
        &self.memory_control
    }

    // $4000204: WAITCNT (Waitstate control)
    pub fn read_waitcnt(&self) -> u16 {
        self.memory_control.read()
    }

    // $4000204: WAITCNT (Waitstate control)
    pub fn write_waitcnt(&mut self, value: u16) {
        self.memory_control.write(value);
    }

    // $4000300: POSTFLG (Post boot flag)
    pub fn read_postflg(&self) -> u8 {
        self.post_boot.into()
    }

    // $4000300: POSTFLG (Post boot flag)
    pub fn write_postflg(&mut self, value: u8) {
        self.post_boot = value.bit(0);
        log::trace!("POSTFLG write {value:02X}");
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
