//! SH7604 CPU cache
//!
//! This is a 4-way set-associative cache that uses a pseudo-LRU algorithm for cache replacement.
//!
//! Cache replacement is performed when a cached read misses. The cache is write-through, so writes
//! will only update cache if there is a cache hit.
//!
//! WWF Raw (32X) depends on CPU cache emulation because it writes to cartridge ROM addresses and
//! expects to be able to read back the written values from CPU cache.

use crate::bus::BusInterface;
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use std::array;

const CACHE_RAM_LEN_WORDS: usize = 4 * 1024 / 2;

const WAYS: usize = 4;

// Cache lines are 16 bytes and there are 4 ways in each cache line
// 4096 / 16 / 4 = 64
const CACHE_ENTRIES: usize = 64;

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct CacheControlRegister {
    // Specifies which way is accessed when the address array is accessed directly
    pub way: u8,
    pub mode: CacheMode,
    pub disable_data_replacement: bool,
    pub disable_instruction_replacement: bool,
    pub cache_enabled: bool,
}

impl CacheControlRegister {
    fn read(&self) -> u8 {
        (self.way << 6)
            | ((self.mode as u8) << 3)
            | (u8::from(self.disable_data_replacement) << 2)
            | (u8::from(self.disable_instruction_replacement) << 1)
            | u8::from(self.cache_enabled)
    }

    fn write(&mut self, value: u8) {
        self.way = value >> 6;
        self.mode = CacheMode::from_bit(value.bit(3));
        self.disable_data_replacement = value.bit(2);
        self.disable_instruction_replacement = value.bit(1);
        self.cache_enabled = value.bit(0);

        log::trace!("CCR write: {value:02X}");
        log::trace!("  Way specification: {}", self.way);
        log::trace!("  Cache mode: {:?}", self.mode);
        log::trace!("  Cache purged: {}", value.bit(4));
        log::trace!("  Disable data replacement: {}", self.disable_data_replacement);
        log::trace!("  Disable instruction replacement: {}", self.disable_instruction_replacement);
        log::trace!("  Cache enabled: {}", self.cache_enabled);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Way {
    // Tag is address bits 10-28
    tags: [u32; CACHE_ENTRIES],
    valid_bits: u64,
}

impl Way {
    fn new() -> Self {
        Self { tags: array::from_fn(|_| 0), valid_bits: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum CacheMode {
    #[default]
    FourWay = 0,
    TwoWay = 1,
}

impl CacheMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::TwoWay } else { Self::FourWay }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CpuCache {
    // Store cache as u16s because the most common fetches are opcodes which are 16-bit
    ram: Box<[u16; CACHE_RAM_LEN_WORDS]>,
    ways: Box<[Way; WAYS]>,
    lru_bits: Box<[u8; CACHE_ENTRIES]>,
    control: CacheControlRegister,
}

impl CpuCache {
    pub fn new() -> Self {
        Self {
            ram: vec![0; CACHE_RAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            ways: Box::new(array::from_fn(|_| Way::new())),
            lru_bits: Box::new(array::from_fn(|_| 0)),
            control: CacheControlRegister::default(),
        }
    }

    pub fn read_u8(&mut self, address: u32) -> Option<u8> {
        self.cache_read(address, move |cache, way_idx, entry_idx| {
            let address = cache_ram_addr(way_idx, entry_idx) | ((address as usize) & 0xF);
            cache.ram[address >> 1].to_be_bytes()[address & 1]
        })
    }

    #[inline(always)]
    pub fn read_u16(&mut self, address: u32) -> Option<u16> {
        self.cache_read(address, move |cache, way_idx, entry_idx| {
            let address = cache_ram_addr(way_idx, entry_idx) | ((address as usize) & 0xE);
            cache.ram[address >> 1]
        })
    }

    pub fn read_u32(&mut self, address: u32) -> Option<u32> {
        self.cache_read(address, move |cache, way_idx, entry_idx| {
            let address = (cache_ram_addr(way_idx, entry_idx) | ((address as usize) & 0xC)) >> 1;
            let high_word = cache.ram[address];
            let low_word = cache.ram[address + 1];
            (u32::from(high_word) << 16) | u32::from(low_word)
        })
    }

    #[inline(always)]
    fn cache_read<T>(
        &mut self,
        address: u32,
        read_fn: impl FnOnce(&Self, usize, usize) -> T,
    ) -> Option<T> {
        if !self.control.cache_enabled {
            return None;
        }

        let entry_idx = cache_entry_index(address);
        let tag = tag_address(address);

        // Iterate in reverse for slightly better performance when cache is in 2-way mode.
        // Per SH7604 documentation, all 4 ways are checked even in 2-way mode; 2-way mode only
        // changes replacement behavior
        for way_idx in (0..4).rev() {
            if self.ways[way_idx].valid_bits.bit(entry_idx as u8)
                && self.ways[way_idx].tags[entry_idx] == tag
            {
                self.update_lru_bits(way_idx, entry_idx);
                return Some(read_fn(self, way_idx, entry_idx));
            }
        }

        None
    }

    #[inline]
    pub fn should_replace_instruction(&self) -> bool {
        self.control.cache_enabled && !self.control.disable_instruction_replacement
    }

    #[inline]
    pub fn should_replace_data(&self) -> bool {
        self.control.cache_enabled && !self.control.disable_data_replacement
    }

    #[must_use]
    pub fn replace<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u32 {
        let entry_idx = cache_entry_index(address);

        let lru_bits = self.lru_bits[entry_idx];
        let way_idx = match self.control.mode {
            CacheMode::FourWay => {
                usize::from(lru_bits & 0b100110 == 0b000110)
                    | (usize::from(lru_bits & 0b010101 == 0b000001) << 1)
                    | (3 * usize::from(lru_bits & 0b001011 == 0))
            }
            CacheMode::TwoWay => {
                if lru_bits.bit(0) {
                    2
                } else {
                    3
                }
            }
        };

        self.ways[way_idx].tags[entry_idx] = tag_address(address);
        self.ways[way_idx].valid_bits |= 1 << entry_idx;
        self.update_lru_bits(way_idx, entry_idx);

        let longwords = bus.read_cache_line(address & 0x1FFFFFF0);
        let mut ram_addr = cache_ram_addr(way_idx, entry_idx) >> 1;
        for longword in longwords {
            self.ram[ram_addr] = (longword >> 16) as u16;
            self.ram[ram_addr + 1] = longword as u16;
            ram_addr += 2;
        }

        longwords[((address >> 2) & 3) as usize]
    }

    #[inline(always)]
    fn update_lru_bits(&mut self, way_idx: usize, entry_idx: usize) {
        // Bit 5: 0 -> 1
        // Bit 4: 0 -> 2
        // Bit 3: 0 -> 3
        // Bit 2: 1 -> 2
        // Bit 1: 1 -> 3
        // Bit 0: 2 -> 3
        let (and_mask, or_mask) = match way_idx {
            // Clear bits 5-3
            0 => (!0b111000, 0b000000),
            // Clear bits 2-1 and set bit 5
            1 => (!0b000110, 0b100000),
            // Clear bit 0 and set bits 4 and 2
            2 => (!0b000001, 0b010100),
            // Set bits 3, 1, and 0
            3 => (!0b000000, 0b001011),
            _ => panic!("Invalid way index, should be 0-3: {way_idx}"),
        };

        self.lru_bits[entry_idx] &= and_mask;
        self.lru_bits[entry_idx] |= or_mask;
    }

    pub fn write_through_u8(&mut self, address: u32, value: u8) {
        self.cache_write_through(address, move |cache, way_idx, entry_idx| {
            let address = cache_ram_addr(way_idx, entry_idx) | ((address as usize) & 0xF);
            if !address.bit(0) {
                cache.ram[address >> 1].set_msb(value);
            } else {
                cache.ram[address >> 1].set_lsb(value);
            }
        });
    }

    pub fn write_through_u16(&mut self, address: u32, value: u16) {
        self.cache_write_through(address, move |cache, way_idx, entry_idx| {
            let address = cache_ram_addr(way_idx, entry_idx) | ((address as usize) & 0xE);
            cache.ram[address >> 1] = value;
        });
    }

    pub fn write_through_u32(&mut self, address: u32, value: u32) {
        self.cache_write_through(address, move |cache, way_idx, entry_idx| {
            let address = (cache_ram_addr(way_idx, entry_idx) | ((address as usize) & 0xC)) >> 1;
            cache.ram[address] = (value >> 16) as u16;
            cache.ram[address + 1] = value as u16;
        });
    }

    fn cache_write_through(&mut self, address: u32, set_fn: impl FnOnce(&mut Self, usize, usize)) {
        if !self.control.cache_enabled {
            return;
        }

        let entry_idx = cache_entry_index(address);
        let tag = tag_address(address);

        // Iterate in reverse for slightly better performance when cache is in 2-way mode
        for way_idx in (0..4).rev() {
            if self.ways[way_idx].valid_bits.bit(entry_idx as u8)
                && self.ways[way_idx].tags[entry_idx] == tag
            {
                self.update_lru_bits(way_idx, entry_idx);
                set_fn(self, way_idx, entry_idx);
                return;
            }
        }
    }

    // $FFFFFE92: CCR (Cache control register)
    pub fn read_control(&self) -> u8 {
        self.control.read()
    }

    // $FFFFFE92: CCR (Cache control register)
    pub fn write_control(&mut self, value: u8) {
        self.control.write(value);

        if value.bit(4) {
            self.purge_all();
        }
    }

    pub fn purge_all(&mut self) {
        for way in self.ways.iter_mut() {
            way.valid_bits = 0;
        }

        self.lru_bits.fill(0);
    }

    // A29-31 = 010
    pub fn associative_purge(&mut self, address: u32) {
        // Invalidates a single cache line
        let idx = cache_entry_index(address);
        let mask = !(1 << idx);
        for way in self.ways.iter_mut() {
            way.valid_bits &= mask;
        }

        // TODO should associative purge clear the LRU bits?
        self.lru_bits[idx] = 0;
    }

    // A29-31 = 011
    pub fn read_address_array(&self, address: u32) -> u32 {
        let entry_idx = cache_entry_index(address);
        let way_idx = self.control.way as usize;

        (u32::from(self.ways[way_idx].valid_bits.bit(entry_idx as u8)) << 1)
            | (u32::from(self.lru_bits[entry_idx]) << 3)
            | (self.ways[way_idx].tags[entry_idx] << 10)
    }

    // A29-31 = 011
    pub fn write_address_array(&mut self, address: u32, value: u32) {
        let entry_idx = cache_entry_index(address);
        let way_idx = self.control.way as usize;
        let tag = tag_address(address);
        let valid = address.bit(1);
        let lru_bits = ((value >> 3) & 0x3F) as u8;

        if valid {
            self.ways[way_idx].valid_bits |= 1 << entry_idx;
        } else {
            self.ways[way_idx].valid_bits &= !(1 << entry_idx);
        }

        self.ways[way_idx].tags[entry_idx] = tag;
        self.lru_bits[entry_idx] = lru_bits;
    }

    // A29-31 = 110
    pub fn read_data_array_u8(&self, address: u32) -> u8 {
        let word = self.ram[((address >> 1) as usize) & (CACHE_RAM_LEN_WORDS - 1)];
        word.to_be_bytes()[(address & 1) as usize]
    }

    // A29-31 = 110
    pub fn read_data_array_u16(&self, address: u32) -> u16 {
        self.ram[((address >> 1) as usize) & (CACHE_RAM_LEN_WORDS - 1)]
    }

    // A29-31 = 110
    pub fn read_data_array_u32(&self, address: u32) -> u32 {
        let address = ((address >> 1) as usize) & (CACHE_RAM_LEN_WORDS - 1) & !1;
        let high_word = self.ram[address];
        let low_word = self.ram[address + 1];
        (u32::from(high_word) << 16) | u32::from(low_word)
    }

    // A29-31 = 110
    pub fn write_data_array_u8(&mut self, address: u32, value: u8) {
        let word_addr = ((address >> 1) as usize) & (CACHE_RAM_LEN_WORDS - 1);
        if !address.bit(0) {
            self.ram[word_addr].set_msb(value);
        } else {
            self.ram[word_addr].set_lsb(value);
        }
    }

    // A29-31 = 110
    pub fn write_data_array_u16(&mut self, address: u32, value: u16) {
        self.ram[((address >> 1) as usize) & (CACHE_RAM_LEN_WORDS - 1)] = value;
    }

    // A29-31 = 110
    pub fn write_data_array_u32(&mut self, address: u32, value: u32) {
        let address = ((address >> 1) as usize) & (CACHE_RAM_LEN_WORDS - 1) & !1;
        self.ram[address] = (value >> 16) as u16;
        self.ram[address + 1] = value as u16;
    }
}

#[inline(always)]
fn cache_ram_addr(way_idx: usize, entry_idx: usize) -> usize {
    (way_idx << 10) | (entry_idx << 4)
}

#[inline(always)]
fn cache_entry_index(address: u32) -> usize {
    // Cache is indexed using address bits 4-9
    ((address as usize) >> 4) & 0x3F
}

#[inline(always)]
fn tag_address(address: u32) -> u32 {
    // Cache entries are tagged using address bits 10-28
    (address & 0x1FFFFFFF) >> 10
}
