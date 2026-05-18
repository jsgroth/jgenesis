//! HuC6260 VCE (video color encoder)

use crate::video::WordByte;
use crate::video::palette::PcePalette;
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedWordArray;
use jgenesis_common::frontend::Color;
use jgenesis_common::num::{GetBit, U16Ext};
use std::iter;

pub const MAX_LINES_PER_FRAME: usize = 263;

pub const CRAM_LEN_WORDS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DotClockDivider {
    #[default]
    Four = 4, // ~5.37 MHz, commonly H256px
    Three = 3, // ~7.13 MHz, commonly H304px to H352px
    Two = 2,   // ~10.69 MHz, commonly H512px (not used by commercial releases)
}

impl DotClockDivider {
    fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Four,
            1 => Self::Three,
            2 | 3 => Self::Two,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    pub fn divide_difference(self, cycles: u64, prev_cycles: u64) -> u64 {
        debug_assert!(cycles >= prev_cycles);

        // This is faster than dividing by u64::from(self) because it avoids div instructions and
        // dot clock divider rarely changes
        match self {
            Self::Four => (cycles >> 2) - (prev_cycles >> 2),
            Self::Three => (cycles / 3) - (prev_cycles / 3),
            Self::Two => (cycles >> 1) - (prev_cycles >> 1),
        }
    }

    pub fn divide(self, cycles: u64) -> u64 {
        // Same as in divide_difference(), avoids div instructions
        match self {
            Self::Four => cycles >> 2,
            Self::Three => cycles / 3,
            Self::Two => cycles >> 1,
        }
    }
}

impl From<DotClockDivider> for u64 {
    fn from(value: DotClockDivider) -> Self {
        match value {
            DotClockDivider::Four => 4,
            DotClockDivider::Three => 3,
            DotClockDivider::Two => 2,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vce {
    cram: BoxedWordArray<CRAM_LEN_WORDS>,
    dot_clock_divider: DotClockDivider,
    extra_line_per_frame: bool,
    greyscale: bool,
    color_table_address: u16,
}

impl Vce {
    pub fn new() -> Self {
        Self {
            cram: BoxedWordArray::new(),
            dot_clock_divider: DotClockDivider::default(),
            extra_line_per_frame: false,
            greyscale: false,
            color_table_address: 0,
        }
    }

    pub fn overscan_color(&self) -> u16 {
        // Sprite color 0
        self.cram[0x100]
    }

    pub fn read_color(&self, color_idx: u16) -> u16 {
        self.cram[(color_idx as usize) & (CRAM_LEN_WORDS - 1)]
    }

    pub fn dot_clock_divider(&self) -> DotClockDivider {
        self.dot_clock_divider
    }

    pub fn lines_per_frame(&self) -> u16 {
        (MAX_LINES_PER_FRAME as u16) - 1 + u16::from(self.extra_line_per_frame)
    }

    pub fn greyscale(&self) -> bool {
        self.greyscale
    }

    // $1FE400: CR (Control register)
    pub fn write_control(&mut self, value: u8) {
        self.dot_clock_divider = DotClockDivider::from_bits(value);
        self.extra_line_per_frame = value.bit(2);
        self.greyscale = value.bit(7);

        log::trace!("CR write: {value:02X}");
        log::trace!("  Dot clock divider: {}", u64::from(self.dot_clock_divider));
        log::trace!("  Lines per frame: {}", if self.extra_line_per_frame { 263 } else { 262 });
        log::trace!("  Monochrome: {}", self.greyscale);
    }

    // $1FE402-$1FE403: CTA (Color table address register)
    pub fn write_color_address(&mut self, value: u8, byte: WordByte) {
        match byte {
            WordByte::Low => self.color_table_address.set_lsb(value),
            WordByte::High => self.color_table_address.set_msb(value & 1),
        }

        log::trace!("CTA {byte:?} write: {value:02X}");
        log::trace!("  Color table address: {:03X}", self.color_table_address);
    }

    // $1FE404-$1FE405: CTR (Color table data read register)
    pub fn read_color_data(&mut self, byte: WordByte) -> u8 {
        log::trace!("CTR {byte:?} read (current address {:03X})", self.color_table_address);

        // Highest 7 bits always read 1
        let color = self.cram[self.color_table_address as usize] | !0x1FF;

        if byte == WordByte::High {
            self.increment_color_table_address();
        }

        byte.get(color)
    }

    // $1FE404-$1FE405: CTW (Color table data write register)
    pub fn write_color_data(&mut self, value: u8, byte: WordByte) {
        log::trace!(
            "CTW {byte:?} write: {value:02X} (current address {:03X})",
            self.color_table_address
        );

        let color = &mut self.cram[self.color_table_address as usize];
        match byte {
            WordByte::Low => color.set_lsb(value),
            WordByte::High => color.set_msb(value & 1),
        }

        if byte == WordByte::High {
            self.increment_color_table_address();
        }
    }

    fn increment_color_table_address(&mut self) {
        self.color_table_address = (self.color_table_address + 1) & (CRAM_LEN_WORDS - 1) as u16;
    }

    pub fn dump_palettes(&self, out: &mut [Color], palette: &PcePalette) {
        for (cram_color, out_color) in
            iter::zip(self.cram.iter().copied(), &mut out[..CRAM_LEN_WORDS])
        {
            let (r, g, b) = palette[(cram_color & 0x1FF) as usize];
            *out_color = Color::rgb(r, g, b);
        }
    }
}
