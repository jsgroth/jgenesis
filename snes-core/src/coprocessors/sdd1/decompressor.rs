//! Decompression algorithm from:
//! <https://problemkaputt.github.io/fullsnes.htm#snescartsdd1decompressionalgorithm>
//!
//! The algorithm is also described in English here:
//! <https://wiki.superfamicom.org/s-dd1>

use crate::coprocessors::sdd1::Sdd1Mmc;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

// Golomb decoder codeword size, indexed by state
// Higher states use longer codewords because runs are theoretically more likely to end with the MPS
// States 25-32 don't follow the pattern because they're highly adaptable states that are only used
// shortly after initialization (see MPS/LPS evolution tables)
#[rustfmt::skip]
const EVOLUTION_CODE_SIZE: &[u8; 33] = &[
    0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3,
    4, 4, 5, 5, 6, 6, 7, 7, 0, 1, 2, 3, 4, 5, 6, 7
];

// MPS = Most probable symbol
// If a run ends in the MPS, move to a higher state
#[rustfmt::skip]
const EVOLUTION_MPS_NEXT: &[u8; 33] = &[
    25, 2, 3, 4, 5, 6, 7, 8, 9,10,11,12,13,14,15,16,17,
    18,19,20,21,22,23,24,24,26,27,28,29,30,31,32,24
];

// LPS = Least probable symbol
// If a run ends in the LPS, move to a lower state
#[rustfmt::skip]
const EVOLUTION_LPS_NEXT: &[u8; 33] = &[
    25, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9,10,11,12,13,14,15,
    16,17,18,19,20,21,22,23, 1, 2, 4, 8,12,16,18,22
];

#[rustfmt::skip]
const RUN_TABLE: &[u8; 128] = &[
    128, 64, 96, 32, 112, 48, 80, 16, 120, 56, 88, 24, 104, 40, 72, 8,
    124, 60, 92, 28, 108, 44, 76, 12, 116, 52, 84, 20, 100, 36, 68, 4,
    126, 62, 94, 30, 110, 46, 78, 14, 118, 54, 86, 22, 102, 38, 70, 6,
    122, 58, 90, 26, 106, 42, 74, 10, 114, 50, 82, 18,  98, 34, 66, 2,
    127, 63, 95, 31, 111, 47, 79, 15, 119, 55, 87, 23, 103, 39, 71, 7,
    123, 59, 91, 27, 107, 43, 75, 11, 115, 51, 83, 19,  99, 35, 67, 3,
    125, 61, 93, 29, 109, 45, 77, 13, 117, 53, 85, 21, 101, 37, 69, 5,
    121, 57, 89, 25, 105, 41, 73,  9, 113, 49, 81, 17,  97, 33, 65, 1,
];

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Sdd1Decompressor {
    source_addr: u32,
    input: u16,
    plane: u8,
    num_planes: u8,
    y_location: u8,
    valid_bits: i8,
    high_context_bits: u16,
    low_context_bits: u16,
    bit_counter: [u16; 8],
    prev_bits: [u16; 8],
    context_states: [u8; 32],
    context_mps: [u8; 32],
}

impl Sdd1Decompressor {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn init(&mut self, source_address: u32, mmc: &Sdd1Mmc, rom: &[u8]) {
        self.input = read_byte(source_address, mmc, rom).into();
        self.source_addr = source_address + 1;

        self.num_planes = match self.input & 0xC0 {
            // 2bpp tile data
            0x00 => 2,
            // 8bpp tile data
            0x40 => 8,
            // 4bpp tile data
            0x80 => 4,
            // Other data (e.g. Mode 7 graphics)
            0xC0 => 0,
            _ => unreachable!("value & 0xC0 is always one of the above values"),
        };

        // Context is formed using 3 or 4 of the previous 9 bits, with separate contexts for
        // even and odd bitplanes
        let (high_context_bits, low_context_bits) = match self.input & 0x30 {
            // Bits 1, 7, 8, 9
            0x00 => (0x01C0, 0x0001),
            // Bits 1, 8, 9
            0x10 => (0x0180, 0x0001),
            // Bits 1, 7, 8
            0x20 => (0x00C0, 0x0001),
            // Bits 1, 2, 8, 9
            0x30 => (0x0180, 0x0003),
            _ => unreachable!("value & 0x30 is always one of the above values"),
        };
        self.high_context_bits = high_context_bits;
        self.low_context_bits = low_context_bits;

        let next_byte: u16 = read_byte(self.source_addr, mmc, rom).into();
        self.input = (self.input << 11) | (next_byte << 3);
        self.source_addr += 1;

        self.valid_bits = 5;

        self.bit_counter.fill(0);
        self.prev_bits.fill(0);
        self.context_states.fill(0);
        self.context_mps.fill(0);

        self.plane = 0;
        self.y_location = 0;
    }

    pub(super) fn next_byte(&mut self, mmc: &Sdd1Mmc, rom: &[u8]) -> u8 {
        if self.num_planes == 0 {
            // For miscellaneous data, simply output the next 8 bits
            let mut byte = 0;
            for plane in 0..8 {
                byte |= self.get_bit(plane, mmc, rom) << plane;
            }
            return byte;
        }

        if !self.plane.bit(0) {
            // Retrieve the next 16 bits, alternating between the even bitplane and the odd bitplane
            for _ in 0..8 {
                self.get_bit(self.plane, mmc, rom);
                self.get_bit(self.plane + 1, mmc, rom);
            }

            let byte = self.prev_bits[self.plane as usize] & 0xFF;
            self.plane += 1;

            byte as u8
        } else {
            let byte = self.prev_bits[self.plane as usize] & 0xFF;
            self.plane -= 1;

            self.y_location += 1;
            if self.y_location == 8 {
                // Completed a set of 16 bytes; move to the next 2 bitplanes (if 4bpp or 8bpp)
                self.y_location = 0;
                self.plane = (self.plane + 2) & (self.num_planes - 1);
            }

            byte as u8
        }
    }

    fn get_bit(&mut self, plane: u8, mmc: &Sdd1Mmc, rom: &[u8]) -> u8 {
        // Form context from previous bits in the current plane, with separate contexts for odd
        // and even bitplanes
        let mut context = (u16::from(plane) & 0x01) << 4;
        context |= (self.prev_bits[plane as usize] & self.high_context_bits) >> 5;
        context |= self.prev_bits[plane as usize] & self.low_context_bits;

        let p_bit = self.get_probable_bit(context, mmc, rom);
        self.prev_bits[plane as usize] = (self.prev_bits[plane as usize] << 1) | u16::from(p_bit);

        p_bit
    }

    fn get_probable_bit(&mut self, context: u16, mmc: &Sdd1Mmc, rom: &[u8]) -> u8 {
        let state = self.context_states[context as usize];
        let code_size = EVOLUTION_CODE_SIZE[state as usize];

        if self.bit_counter[code_size as usize] & 0x7F == 0 {
            self.bit_counter[code_size as usize] = self.get_codeword(code_size, mmc, rom);
        }

        let mut p_bit = self.context_mps[context as usize];
        self.bit_counter[code_size as usize] -= 1;

        if self.bit_counter[code_size as usize] == 0x00 {
            // Run ends in the LPS
            self.context_states[context as usize] = EVOLUTION_LPS_NEXT[state as usize];
            p_bit ^= 0x01;

            if state < 2 {
                // MPS can only change while in state 0 or 1
                self.context_mps[context as usize] = p_bit;
            }
        } else if self.bit_counter[code_size as usize] == 0x80 {
            // Run ends in the MPS
            self.context_states[context as usize] = EVOLUTION_MPS_NEXT[state as usize];
        }

        p_bit
    }

    fn get_codeword(&mut self, code_size: u8, mmc: &Sdd1Mmc, rom: &[u8]) -> u16 {
        if self.valid_bits == 0 {
            // Read next input byte
            self.input |= u16::from(read_byte(self.source_addr, mmc, rom));
            self.source_addr += 1;
            self.valid_bits = 8;
        }

        self.input <<= 1;
        self.valid_bits -= 1;

        if !self.input.bit(15) {
            // 0 indicates a run of MPSs of length 2^N, where N is the codeword size
            return 0x80 + (1 << code_size);
        }

        // 1 indicates a run of MPSs that ends with the LPS, where the following N bits determine
        // the run length

        let run_table_idx = ((self.input >> 8) & 0x7F) | (0x7F >> code_size);
        self.input <<= code_size;
        self.valid_bits -= code_size as i8;
        if self.valid_bits < 0 {
            let next_byte: u16 = read_byte(self.source_addr, mmc, rom).into();
            self.input |= next_byte << (-self.valid_bits);
            self.source_addr += 1;
            self.valid_bits += 8;
        }

        RUN_TABLE[run_table_idx as usize].into()
    }
}

fn read_byte(address: u32, mmc: &Sdd1Mmc, rom: &[u8]) -> u8 {
    mmc
        .map_rom_address(address, rom.len() as u32)
        .and_then(|rom_addr| rom.get(rom_addr as usize).copied())
        .unwrap_or_else(|| {
            log::error!("Encountered an invalid ROM address mapping in S-DD1 decompressor ({address:06X}); something has likely gone horribly wrong");
            0
        })
}
