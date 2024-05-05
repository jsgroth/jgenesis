//! Game Boy internal memory

use bincode::{Decode, Encode};
use std::iter;

const MAIN_RAM_LEN: usize = 32 * 1024;
const HRAM_LEN: usize = 127;

type MainRam = [u8; MAIN_RAM_LEN];
type Hram = [u8; HRAM_LEN];

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    main_ram: Box<MainRam>,
    main_ram_bank: u8,
    hram: Box<Hram>,
}

impl Memory {
    pub fn new() -> Self {
        // Randomize RAM contents at power-on
        let main_ram: Vec<u8> = iter::repeat_with(rand::random).take(MAIN_RAM_LEN).collect();

        Self {
            main_ram: main_ram.into_boxed_slice().try_into().unwrap(),
            main_ram_bank: 0,
            hram: vec![0; HRAM_LEN].into_boxed_slice().try_into().unwrap(),
        }
    }

    pub fn read_main_ram(&self, address: u16) -> u8 {
        let ram_addr = map_ram_address(address, self.main_ram_bank);
        self.main_ram[ram_addr as usize]
    }

    pub fn write_main_ram(&mut self, address: u16, value: u8) {
        let ram_addr = map_ram_address(address, self.main_ram_bank);
        self.main_ram[ram_addr as usize] = value;
    }

    pub fn read_hram(&self, address: u16) -> u8 {
        self.hram[(address & 0x7F) as usize]
    }

    pub fn write_hram(&mut self, address: u16, value: u8) {
        self.hram[(address & 0x7F) as usize] = value;
    }

    pub fn read_svbk(&self) -> u8 {
        0xF8 | self.main_ram_bank
    }

    pub fn write_svbk(&mut self, value: u8) {
        self.main_ram_bank = value & 0x07;

        log::trace!("SVBK write: WRAM bank set to {}", self.main_ram_bank);
    }
}

fn map_ram_address(address: u16, ram_bank: u8) -> u16 {
    match address & 0x1FFF {
        address @ 0x0000..=0x0FFF => address,
        0x1000..=0x1FFF => {
            // RAM bank of 0 functions as 1
            let ram_bank = if ram_bank == 0 { 1 } else { u16::from(ram_bank) };
            (ram_bank << 12) | (address & 0x0FFF)
        }
        _ => unreachable!("address & 0x1FFF is always <= 0x1FFF"),
    }
}
