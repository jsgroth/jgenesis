use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

const CODE_CACHE_RAM_LEN: usize = 512;

type CodeCacheRam = [u8; CODE_CACHE_RAM_LEN];

#[derive(Debug, Clone, Encode, Decode)]
pub struct CodeCache {
    ram: Box<CodeCacheRam>,
    cbr: u16,
    cached_lines: u32,
}

impl CodeCache {
    pub fn new() -> Self {
        Self {
            ram: vec![0; CODE_CACHE_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            cbr: 0,
            cached_lines: 0,
        }
    }

    pub fn pc_is_cacheable(&self, address: u16) -> bool {
        if self.cbr < 0xFE00 {
            (self.cbr..self.cbr + 512).contains(&address)
        } else {
            address >= self.cbr || address < self.cbr.wrapping_add(512)
        }
    }

    pub fn get(&self, address: u16) -> Option<u8> {
        let cache_addr = address.wrapping_sub(self.cbr);
        let cache_line_bit = (cache_addr >> 4) as u8;
        self.cached_lines.bit(cache_line_bit).then_some(self.ram[cache_addr as usize])
    }

    pub fn set(&mut self, address: u16, value: u8) {
        let cache_addr = address.wrapping_sub(self.cbr);
        self.ram[cache_addr as usize] = value;

        if address & 0xF == 0xF {
            // Cache line is set when the last address of a 16-byte block is written
            self.set_cache_line(cache_addr);
        }
    }

    fn set_cache_line(&mut self, cache_addr: u16) {
        let cache_line = 1 << (cache_addr >> 4);
        self.cached_lines |= cache_line;
    }

    pub fn read_ram(&self, address: u16) -> u8 {
        self.ram[(address & 0x1FF) as usize]
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        let cache_addr = address & 0x1FF;
        self.ram[cache_addr as usize] = value;

        if address & 0xF == 0xF {
            // Cache line is set when the last address of a 16-byte block is written
            self.set_cache_line(cache_addr);
        }
    }

    pub fn update_cbr(&mut self, cbr: u16) {
        // Changing CBR via a CACHE or LJMP instruction clears all cache lines
        self.cbr = cbr;
        self.cached_lines = 0;

        log::trace!("Set CBR to {cbr:04X}");
    }

    pub fn full_clear(&mut self) {
        self.cbr = 0;
        self.cached_lines = 0;
    }

    pub fn cbr(&self) -> u16 {
        self.cbr
    }
}
