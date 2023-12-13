//! SPC7110 decompressor
//!
//! Algorithm and tables from:
//! <https://problemkaputt.github.io/fullsnes.htm#snescartspc7110decompressionalgorithm>

use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext, U24Ext};
use std::{array, mem};

#[rustfmt::skip]
const EVOLUTION_PROBABILITY: &[u8; 53] = &[
    90,37,17, 8, 3, 1,90,63,44,32,23,17,12, 9, 7, 5, 4, 3, 2,
    90,72,58,46,38,31,25,21,17,14,11, 9, 8, 7, 5, 4, 4, 3, 2,
    2 ,88,77,67,59,52,46,41,37,86,79,71,65,60,55
];

#[rustfmt::skip]
const EVOLUTION_NEXT_LPS: &[u8; 53] = &[
    1 , 6, 8,10,12,15, 7,19,21,22,23,25,26,28,29,31,32,34,35,
    20,39,40,42,44,45,46,25,26,26,27,28,29,30,31,33,33,34,35,
    36,39,47,48,49,50,51,44,45,47,47,48,49,50,51
];

#[rustfmt::skip]
const EVOLUTION_NEXT_MPS: &[u8; 53] = &[
    1 , 2, 3, 4, 5, 5, 7, 8, 9,10,11,12,13,14,15,16,17,18, 5,
    20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,
    5 ,40,41,42,43,44,45,46,24,48,49,50,51,52,43
];

// Values at 0-2 don't matter; only indices 3-14 are used
const MODE_2_CONTEXT_TABLE: &[u8; 15] = &[0, 0, 0, 15, 17, 19, 21, 23, 25, 25, 25, 25, 25, 27, 29];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum DecompressionMode {
    // Miscellaneous data
    #[default]
    Zero,
    // 2bpp graphical data
    One,
    // 4bpp graphical data
    Two,
}

impl DecompressionMode {
    fn bpp(self) -> u32 {
        match self {
            Self::Zero => 1,
            Self::One => 2,
            Self::Two => 3,
        }
    }
}

impl DecompressionMode {
    fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => Self::Zero,
            0x01 => Self::One,
            0x02 => Self::Two,
            _ => {
                log::warn!("Unexpected SPC7110 decompression mode, defaulting to 0: {byte:02X}");
                Self::Zero
            }
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct DecompressionState {
    initialized: bool,
    mode: DecompressionMode,
    source: u32,
    out: u32,
    decoded: u8,
    in_count: u8,
    buffer_index: u8,
    a: u8,
    b: u8,
    c: u8,
    context: u8,
    top: u8,
    input: u16,
    plane1: u8,
    plane_buffer: [u8; 16],
    pixel_order: [u8; 16],
    real_order: [u8; 16],
    context_index: [u8; 32],
    context_invert: [u8; 32],
}

impl DecompressionState {
    fn input_msb(&self) -> u8 {
        (self.input >> 8) as u8
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Spc7110Decompressor {
    pub rom_directory_base: u32,
    pub rom_directory_index: u8,
    pub target_offset: u16,
    pub length_counter: u16,
    pub skip_enabled: bool,
    state: DecompressionState,
}

impl Spc7110Decompressor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_rom_directory_base_low(&mut self, value: u8) {
        self.rom_directory_base.set_low_byte(value);
    }

    pub fn write_rom_directory_base_mid(&mut self, value: u8) {
        self.rom_directory_base.set_mid_byte(value);
    }

    pub fn write_rom_directory_base_high(&mut self, value: u8) {
        self.rom_directory_base.set_high_byte(value);
    }

    pub fn write_target_offset_low(&mut self, value: u8) {
        self.target_offset.set_lsb(value);
    }

    pub fn write_target_offset_high(&mut self, value: u8, data_rom: &[u8]) {
        self.target_offset.set_msb(value);

        // Writing offset MSB initializes decompressor
        self.initialize(data_rom);
    }

    pub fn write_length_counter_low(&mut self, value: u8) {
        self.length_counter.set_lsb(value);
    }

    pub fn write_length_counter_high(&mut self, value: u8) {
        self.length_counter.set_msb(value);
    }

    pub fn write_mode(&mut self, value: u8) {
        // Supposedly, $02 causes the decompressor to skip <offset> rows of pixels after initialization,
        // while $00 causes it to ignore the target offset
        self.skip_enabled = value == 0x02;
    }

    pub fn read_mode(&self) -> u8 {
        if self.skip_enabled { 0x02 } else { 0x00 }
    }

    pub fn read_status(&self) -> u8 {
        u8::from(self.state.initialized) << 7
    }

    pub fn next_byte(&mut self, data_rom: &[u8]) -> u8 {
        if !self.state.initialized {
            return 0;
        }

        self.length_counter = self.length_counter.wrapping_sub(1);

        match self.state.mode {
            DecompressionMode::Zero => self.next_byte_mode_0(data_rom),
            DecompressionMode::One => self.next_byte_mode_1(data_rom),
            DecompressionMode::Two => self.next_byte_mode_2(data_rom),
        }
    }

    fn next_byte_mode_0(&mut self, data_rom: &[u8]) -> u8 {
        // Decompress 8 bits and output them
        self.state.decoded = 0;
        for ctx_offset in [0, 1, 3, 7] {
            self.state.context = ctx_offset + self.state.decoded;
            self.decompress_bit(data_rom);
        }

        self.state.out = (self.state.out << 4)
            ^ (((self.state.out >> 12) ^ u32::from(self.state.decoded)) & 0xF);

        self.state.decoded = 0;
        for ctx_offset in [0, 1, 3, 7] {
            self.state.context = 15 + ctx_offset + self.state.decoded;
            self.decompress_bit(data_rom);
        }

        self.state.out = (self.state.out << 4)
            ^ (((self.state.out >> 12) ^ u32::from(self.state.decoded)) & 0xF);

        self.state.out as u8
    }

    fn next_byte_mode_1(&mut self, data_rom: &[u8]) -> u8 {
        let byte = if !self.state.buffer_index.bit(0) {
            // Decompress the next 16 bits
            for _ in 0..8 {
                self.state.a = ((self.state.out >> 2) & 0x03) as u8;
                self.state.b = ((self.state.out >> 14) & 0x03) as u8;

                self.state.decoded = 0;
                self.state.context = get_context(self.state.a, self.state.b, self.state.c);
                self.decompress_bit(data_rom);

                self.state.context = 2 * self.state.context + 5 + self.state.decoded;
                self.decompress_bit(data_rom);

                self.adjust_pixel_order(2);
            }

            // Deinterleave into 2bpp bitplanes and return bitplane 0
            let (plane1, plane0) = deinterleave_bits(self.state.out);
            self.state.plane1 = plane1 as u8;

            plane0 as u8
        } else {
            // Return bitplane 1 from the last call
            self.state.plane1
        };

        self.state.buffer_index = self.state.buffer_index.wrapping_add(1);

        byte
    }

    fn next_byte_mode_2(&mut self, data_rom: &[u8]) -> u8 {
        let byte = if self.state.buffer_index & 0x11 == 0 {
            // Decompress the next 32 bits
            for _ in 0..8 {
                self.state.a = (self.state.out & 0xF) as u8;
                self.state.b = ((self.state.out >> 28) & 0xF) as u8;

                self.state.decoded = 0;
                self.state.context = 0;
                self.decompress_bit(data_rom);

                self.state.context = self.state.decoded + 1;
                self.decompress_bit(data_rom);

                self.state.context = if self.state.context == 2 {
                    self.state.decoded + 11
                } else {
                    get_context(self.state.a, self.state.b, self.state.c)
                        + 3
                        + 5 * self.state.decoded
                };
                self.decompress_bit(data_rom);

                self.state.context =
                    MODE_2_CONTEXT_TABLE[self.state.context as usize] + (self.state.decoded & 0x01);
                self.decompress_bit(data_rom);

                self.adjust_pixel_order(4);
            }

            // Deinterleave into 4bpp bitplanes and return bitplane 0
            // This is designed for SNES tile data, so $00-$0F alternate between planes 0 and 1
            // and $10-$1F alternate between planes 2 and 3
            let (even_bits, odd_bits) = deinterleave_bits(self.state.out);
            let (plane2, plane0) = deinterleave_bits(odd_bits.into());
            let (plane3, plane1) = deinterleave_bits(even_bits.into());
            self.state.plane1 = plane1 as u8;
            self.state.plane_buffer[(self.state.buffer_index & 0xF) as usize] = plane2 as u8;
            self.state.plane_buffer[((self.state.buffer_index + 1) & 0xF) as usize] = plane3 as u8;

            plane0 as u8
        } else if self.state.buffer_index & 0x10 == 0 {
            // Return bitplane 1 from the last call
            self.state.plane1
        } else {
            // Return bitplane 2 or 3 from the call 16-17 bytes ago
            self.state.plane_buffer[(self.state.buffer_index & 0xF) as usize]
        };

        self.state.buffer_index = self.state.buffer_index.wrapping_add(1);

        byte
    }

    fn initialize(&mut self, data_rom: &[u8]) {
        self.state.initialized = true;

        // Directory entries are 4 bytes
        // Byte 0 contains the decompression mode (0/1/2)
        // Bytes 1-3 contain the data ROM address, in big endian (unlike everything else in this chip)
        let directory_addr = self.rom_directory_base + 4 * u32::from(self.rom_directory_index);
        self.state.mode = DecompressionMode::from_byte(rom_get(data_rom, directory_addr));
        self.state.source = u32::from_be_bytes([
            0,
            rom_get(data_rom, directory_addr + 1),
            rom_get(data_rom, directory_addr + 2),
            rom_get(data_rom, directory_addr + 3),
        ]);

        self.state.buffer_index = 0;
        self.state.out = 0;
        self.state.top = 255;
        self.state.c = 0;

        let input_msb = rom_get(data_rom, self.state.source);
        self.state.input = u16::from_be_bytes([input_msb, 0]);
        self.state.source += 1;
        self.state.in_count = 0;

        self.state.pixel_order = array::from_fn(|i| i as u8);
        self.state.context_index.fill(0);
        self.state.context_invert.fill(0);

        if self.skip_enabled {
            // Skip the next N rows of pixels, where N = target offset
            // Not sure this is right, but not multiplying by bpp causes graphical glitches in
            // Super Power League 4
            let skip_bytes = self.state.mode.bpp() * u32::from(self.target_offset);
            for _ in 0..skip_bytes {
                self.next_byte(data_rom);
            }

            self.target_offset = 0;
        }
    }

    fn decompress_bit(&mut self, data_rom: &[u8]) {
        let context = self.state.context as usize;

        self.state.decoded = (self.state.decoded << 1) | self.state.context_invert[context];

        let evolution = self.state.context_index[context] as usize;
        self.state.top -= EVOLUTION_PROBABILITY[evolution];

        if self.state.input_msb() > self.state.top {
            // Output LPS, and possibly swap LPS and MPS for this context
            let input_msb = self.state.input_msb() - 1 - self.state.top;
            self.state.input = (self.state.input & 0x00FF) | (u16::from(input_msb) << 8);

            self.state.top = EVOLUTION_PROBABILITY[evolution] - 1;
            if self.state.top > 79 {
                self.state.context_invert[context] ^= 1;
            }

            self.state.decoded ^= 1;

            self.state.context_index[context] = EVOLUTION_NEXT_LPS[evolution];
        } else {
            // Output MPS
            if self.state.top <= 126 {
                self.state.context_index[context] = EVOLUTION_NEXT_MPS[evolution];
            }
        }

        while self.state.top <= 126 {
            if self.state.in_count == 0 {
                let input_lsb = rom_get(data_rom, self.state.source);
                self.state.input = (self.state.input & 0xFF00) | u16::from(input_lsb);
                self.state.source += 1;
                self.state.in_count = 8;
            }

            self.state.top = (self.state.top << 1) | 1;
            self.state.input <<= 1;
            self.state.in_count -= 1;
        }
    }

    fn adjust_pixel_order(&mut self, bpp: u8) {
        let mut x = self.state.a;
        for m in 0.. {
            mem::swap(&mut x, &mut self.state.pixel_order[m]);
            if x == self.state.a {
                break;
            }
        }

        for m in 0..1 << bpp {
            self.state.real_order[m] = self.state.pixel_order[m];
        }

        x = self.state.c;
        for m in 0.. {
            mem::swap(&mut x, &mut self.state.real_order[m]);
            if x == self.state.c {
                break;
            }
        }

        x = self.state.b;
        for m in 0.. {
            mem::swap(&mut x, &mut self.state.real_order[m]);
            if x == self.state.b {
                break;
            }
        }

        x = self.state.a;
        for m in 0.. {
            mem::swap(&mut x, &mut self.state.real_order[m]);
            if x == self.state.a {
                break;
            }
        }

        self.state.out =
            (self.state.out << bpp) + u32::from(self.state.real_order[self.state.decoded as usize]);

        self.state.c = self.state.b;
    }
}

fn rom_get(data_rom: &[u8], address: u32) -> u8 {
    data_rom.get(address as usize).copied().unwrap_or(0)
}

fn get_context(a: u8, b: u8, c: u8) -> u8 {
    if a == b && b == c {
        0
    } else if a == b {
        1
    } else if b == c {
        2
    } else if a == c {
        3
    } else {
        4
    }
}

// Based on:
// https://stackoverflow.com/questions/4909263/how-to-efficiently-de-interleave-bits-inverse-morton
// Returns even bits in the first return value, odd bits in the second
fn deinterleave_bits(n: u32) -> (u16, u16) {
    let mut n: u64 = n.into();

    n = (n & 0x0000_0000_5555_5555) | ((n << 31) & 0x5555_5555_0000_0000);
    n = (n | (n >> 1)) & 0x3333_3333_3333_3333;
    n = (n | (n >> 2)) & 0x0F0F_0F0F_0F0F_0F0F;
    n = (n | (n >> 4)) & 0x00FF_00FF_00FF_00FF;
    n = (n | (n >> 8)) & 0x0000_FFFF_0000_FFFF;

    (n as u16, (n >> 32) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deinterleave() {
        let (even, odd) = deinterleave_bits(0xFFFFFFFF);
        assert_eq!(even, 0xFFFF);
        assert_eq!(odd, 0xFFFF);

        let (even, odd) = deinterleave_bits(0x5555AAAA);
        assert_eq!(even, 0xFF00);
        assert_eq!(odd, 0x00FF);

        let (even, odd) = deinterleave_bits(0x12345678);
        assert_eq!(even, 0x46EC);
        assert_eq!(odd, 0x1416);
    }

    const DECOMPRESSED: &[u8] =
        "Test123.ABCDABCDAAAAAAAAaaaabbbbccccdddd7654321076543210.Test123".as_bytes();

    const MODE_0_COMPRESSED: &[u8; 45] = &[
        0x68, 0x91, 0x36, 0x15, 0xF8, 0xBF, 0x42, 0x35, 0x2F, 0x67, 0x3D, 0xB7, 0xAA, 0x05, 0xB4,
        0xF7, 0x70, 0x7A, 0x26, 0x20, 0xEA, 0x58, 0x2C, 0x09, 0x61, 0x00, 0xC5, 0x00, 0x8C, 0x6F,
        0xFF, 0xD1, 0x42, 0x9D, 0xEE, 0x7F, 0x72, 0x87, 0xDF, 0xD6, 0x5F, 0x92, 0x65, 0x00, 0x00,
    ];

    const MODE_1_COMPRESSED: &[u8; 47] = &[
        0x4B, 0xF6, 0x80, 0x1E, 0x3A, 0x4C, 0x42, 0x6C, 0xDA, 0x16, 0x0F, 0xC6, 0x44, 0xED, 0x64,
        0x10, 0x77, 0xAF, 0x50, 0x00, 0x05, 0xC0, 0x01, 0x27, 0x22, 0xB0, 0x83, 0x51, 0x05, 0x32,
        0x4A, 0x1E, 0x74, 0x93, 0x08, 0x76, 0x07, 0xE5, 0x32, 0x12, 0xB4, 0x99, 0x9E, 0x55, 0xA3,
        0xF8, 0x00,
    ];

    const MODE_2_COMPRESSED: &[u8; 52] = &[
        0x13, 0xB3, 0x27, 0xA6, 0xF4, 0x5C, 0xD8, 0xED, 0x6C, 0x6D, 0xF8, 0x76, 0x80, 0xA7, 0x87,
        0x20, 0x39, 0x4B, 0x37, 0x1A, 0xCC, 0x3F, 0xE4, 0x3D, 0xBE, 0x65, 0x2D, 0x89, 0x7E, 0x0B,
        0x0A, 0xD3, 0x46, 0xD5, 0x0C, 0x1F, 0xD3, 0x81, 0xF3, 0xAD, 0xDD, 0xE8, 0x5C, 0xC0, 0xBD,
        0x62, 0xAA, 0xCB, 0xF8, 0xB5, 0x38, 0x00,
    ];

    fn perform_decompression(rom: &[u8]) -> Vec<u8> {
        let mut decompressor = Spc7110Decompressor::new();
        decompressor.rom_directory_base = 0;
        decompressor.rom_directory_index = 0x50 / 4;
        decompressor.length_counter = 64;
        decompressor.target_offset = 0;

        let mut decompressed = Vec::with_capacity(64);
        decompressor.initialize(rom);
        for _ in 0..64 {
            decompressed.push(decompressor.next_byte(rom));
        }

        assert_eq!(decompressor.length_counter, 0);

        decompressed
    }

    #[test]
    fn mode_0() {
        let mut rom = vec![0_u8; 0x100];
        rom[..MODE_0_COMPRESSED.len()].copy_from_slice(MODE_0_COMPRESSED);
        rom[0x50..0x54].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        let decompressed = perform_decompression(&rom);

        assert_eq!(decompressed.as_slice(), DECOMPRESSED);
    }

    #[test]
    fn mode_1() {
        let mut rom = vec![0_u8; 0x100];
        rom[..MODE_1_COMPRESSED.len()].copy_from_slice(MODE_1_COMPRESSED);
        rom[0x50..0x54].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        let decompressed = perform_decompression(&rom);

        assert_eq!(decompressed.as_slice(), DECOMPRESSED);
    }

    #[test]
    fn mode_2() {
        let mut rom = vec![0_u8; 0x100];
        rom[..MODE_2_COMPRESSED.len()].copy_from_slice(MODE_2_COMPRESSED);
        rom[0x50..0x54].copy_from_slice(&[0x02, 0x00, 0x00, 0x00]);

        let decompressed = perform_decompression(&rom);

        assert_eq!(decompressed.as_slice(), DECOMPRESSED);
    }
}
