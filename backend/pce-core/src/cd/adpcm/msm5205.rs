//! MSM5205 ADPCM decoding
//!
//! Based on:
//!   <https://wiki.multimedia.cx/index.php/IMA_ADPCM>
//!   <https://wiki.multimedia.cx/index.php/Dialogic_IMA_ADPCM>

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::sync::LazyLock;

const INDEX_TABLE: &[i8; 8] = &[-1, -1, -1, -1, 2, 4, 6, 8];

const STEP_TABLE_LEN: usize = 49;

// This is extremely sensitive to rounding/truncation behavior, so calculate it the dumb way but
// only once per application lifetime
static STEP_TABLE: LazyLock<[[i16; 8]; STEP_TABLE_LEN]> = LazyLock::new(|| {
    const BASE_STEP_TABLE: &[u16; STEP_TABLE_LEN] = &[
        16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118,
        130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658,
        724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552,
    ];

    array::from_fn(|step_idx| {
        let step = BASE_STEP_TABLE[step_idx];
        array::from_fn(|magnitude| {
            let magnitude = magnitude as u16;

            let delta = step * ((magnitude >> 2) & 1)
                + step / 2 * ((magnitude >> 1) & 1)
                + step / 4 * (magnitude & 1)
                + step / 8;
            delta as i16
        })
    })
});

#[derive(Debug, Clone, Encode, Decode)]
pub struct Msm5205 {
    predictor: u16, // Unsigned 12-bit accumulator
    step_index: i8,
    quantize_output: bool,
}

impl Msm5205 {
    pub fn new(quantize_output: bool) -> Self {
        Self { predictor: 0x800, step_index: 0, quantize_output }
    }

    pub fn decode_start(&mut self) {
        *self = Self::new(self.quantize_output);
    }

    pub fn decode_nibble(&mut self, nibble: u8) {
        let sign = nibble.bit(3);
        let magnitude: u16 = (nibble & 7).into();
        let delta_magnitude = STEP_TABLE[self.step_index as usize][magnitude as usize];
        let delta = if sign { -delta_magnitude } else { delta_magnitude };

        // MSM5205 wraps instead of clamping when accumulator overflows, unlike other IMA ADPCM decoders
        self.predictor = self.predictor.wrapping_add_signed(delta) & 0xFFF;

        self.step_index = (self.step_index + INDEX_TABLE[magnitude as usize])
            .clamp(0, (STEP_TABLE_LEN - 1) as i8);
    }

    // Signed 12-bit
    pub fn current_sample(&self) -> i16 {
        // Per MSM5205 datasheet, its DAC is only 10 bits; mask out the lowest 2 (if accurate
        // quantization is enabled)
        let mut sample_unsigned = self.predictor as i16;
        if self.quantize_output {
            sample_unsigned &= !3;
        }
        sample_unsigned - 0x800
    }

    pub fn set_quantize_output(&mut self, quantize_output: bool) {
        self.quantize_output = quantize_output;
    }
}
