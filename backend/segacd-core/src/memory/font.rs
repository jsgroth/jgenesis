//! Sega CD font rendering / color calculation registers

use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct FontRegisters {
    color_0: u8,
    color_1: u8,
    font_bits: u16,
}

impl FontRegisters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_color(&self) -> u8 {
        (self.color_1 << 4) | self.color_0
    }

    pub fn write_color(&mut self, color_byte: u8) {
        self.color_0 = color_byte & 0x0F;
        self.color_1 = color_byte >> 4;
    }

    pub fn font_bits(&self) -> u16 {
        self.font_bits
    }

    pub fn write_font_bits(&mut self, font_bits: u16) {
        self.font_bits = font_bits;
    }

    pub fn write_font_bits_msb(&mut self, font_bits_msb: u8) {
        self.font_bits.set_msb(font_bits_msb);
    }

    pub fn write_font_bits_lsb(&mut self, font_bits_lsb: u8) {
        self.font_bits.set_lsb(font_bits_lsb);
    }

    pub fn read_font_data(&self, address: u32) -> u16 {
        // Font data registers are mapped at $FF8050-$FF8057
        // $FF8050-$FF8051: Bit 15-12 data
        // $FF8052-$FF8053: Bit 11-8 data
        // $FF8054-$FF8055: Bit 7-4 data
        // $FF8053-$FF8050: Bit 3-0 data
        let word_idx = (address & 0x07) >> 1;
        let base_font_bit = ((3 - word_idx) << 2) as u8;

        (0..16)
            .map(|i| {
                let font_bit_idx = base_font_bit + (i >> 2);
                let font_color_idx = i & 0x03;

                let bit = if self.font_bits.bit(font_bit_idx) {
                    self.color_1.bit(font_color_idx)
                } else {
                    self.color_0.bit(font_color_idx)
                };

                u16::from(bit) << i
            })
            .reduce(|a, b| a | b)
            .unwrap()
    }
}
