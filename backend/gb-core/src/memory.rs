//! Game Boy internal memory

use bincode::{Decode, Encode};

// TODO 32KB for GBC
const MAIN_RAM_LEN: usize = 8 * 1024;
const HRAM_LEN: usize = 127;

type MainRam = [u8; MAIN_RAM_LEN];
type Hram = [u8; HRAM_LEN];

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    main_ram: Box<MainRam>,
    hram: Box<Hram>,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            main_ram: vec![0; MAIN_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            hram: vec![0; HRAM_LEN].into_boxed_slice().try_into().unwrap(),
        }
    }

    pub fn read_main_ram(&self, address: u16) -> u8 {
        // TODO banking for GBC
        self.main_ram[(address & 0x1FFF) as usize]
    }

    pub fn write_main_ram(&mut self, address: u16, value: u8) {
        // TODO banking for GBC
        self.main_ram[(address & 0x1FFF) as usize] = value;
    }

    pub fn read_hram(&self, address: u16) -> u8 {
        self.hram[(address & 0x7F) as usize]
    }

    pub fn write_hram(&mut self, address: u16, value: u8) {
        self.hram[(address & 0x7F) as usize] = value;
    }
}
