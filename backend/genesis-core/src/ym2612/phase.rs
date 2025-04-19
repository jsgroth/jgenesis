//! YM2612 phase generator

use crate::ym2612::lfo;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

// Shifted F-num values are 17 bits
const SHIFTED_F_NUM_MASK: u32 = 0x1FFFF;

// Phase counter is 20 bits
const PHASE_COUNTER_MASK: u32 = 0xFFFFF;

// Created by adapting the documented detune table into increments of ~0.053Hz
#[rustfmt::skip]
const DETUNE_TABLE: &[[u8; 4]; 32] = &[
    [0,  0,  1,  2],  [0,  0,  1,  2],  [0,  0,  1,  2],  [0,  0,  1,  2],  // Block 0
    [0,  1,  2,  2],  [0,  1,  2,  3],  [0,  1,  2,  3],  [0,  1,  2,  3],  // Block 1
    [0,  1,  2,  4],  [0,  1,  3,  4],  [0,  1,  3,  4],  [0,  1,  3,  5],  // Block 2
    [0,  2,  4,  5],  [0,  2,  4,  6],  [0,  2,  4,  6],  [0,  2,  5,  7],  // Block 3
    [0,  2,  5,  8],  [0,  3,  6,  8],  [0,  3,  6,  9],  [0,  3,  7, 10],  // Block 4
    [0,  4,  8, 11],  [0,  4,  8, 12],  [0,  4,  9, 13],  [0,  5, 10, 14],  // Block 5
    [0,  5, 11, 16],  [0,  6, 12, 17],  [0,  6, 13, 19],  [0,  7, 14, 20],  // Block 6
    [0,  8, 16, 22],  [0,  8, 16, 22],  [0,  8, 16, 22],  [0,  8, 16, 22],  // Block 7
];

#[derive(Debug, Clone, Encode, Decode)]
pub(super) struct PhaseGenerator {
    // Register values
    pub(super) f_number: u16,
    pub(super) block: u8,
    pub(super) multiple: u8,
    pub(super) detune: u8,
    // Internal state
    counter: u32,
    current_output: u16,
}

impl PhaseGenerator {
    pub(super) fn new() -> Self {
        Self { f_number: 0, block: 0, multiple: 0, detune: 0, counter: 0, current_output: 0 }
    }

    pub(super) fn reset(&mut self) {
        self.counter = 0;

        log::trace!("State at key on: {self:?}");
    }

    #[inline]
    pub(super) fn clock(&mut self, lfo_counter: u8, fm_sensitivity: u8) {
        let phase_increment = self.compute_phase_increment(lfo_counter, fm_sensitivity);
        self.counter = (self.counter + phase_increment) & PHASE_COUNTER_MASK;

        // Phase generator output is the highest 10 bits of the 20-bit phase counter
        self.current_output = (self.counter >> 10) as u16;
    }

    fn compute_phase_increment(&self, lfo_counter: u8, fm_sensitivity: u8) -> u32 {
        // Vibrato / LFO FM; 12-bit result
        let modulated_f_num = lfo::frequency_modulation(lfo_counter, fm_sensitivity, self.f_number);

        // Apply block/octave multiplier; 17-bit result
        // Right shift by 2 because the LFO FM result is left shifted by 1 relative to input F-num
        let shifted_f_num = (u32::from(modulated_f_num) << self.block) >> 2;

        // Apply detune; 17-bit result
        let key_code = super::compute_key_code(self.f_number, self.block);
        let detune_magnitude = self.detune & 3;
        let detune_increment_magnitude: u32 =
            DETUNE_TABLE[key_code as usize][detune_magnitude as usize].into();
        let detuned_f_num = if self.detune.bit(2) {
            shifted_f_num.wrapping_sub(detune_increment_magnitude) & SHIFTED_F_NUM_MASK
        } else {
            shifted_f_num.wrapping_add(detune_increment_magnitude) & SHIFTED_F_NUM_MASK
        };

        // Apply frequency multiplier; 20-bit result
        match self.multiple {
            0 => detuned_f_num >> 1,
            m => detuned_f_num * u32::from(m),
        }
    }

    pub(super) fn current_phase(&self) -> u16 {
        self.current_output
    }
}

impl Default for PhaseGenerator {
    fn default() -> Self {
        Self::new()
    }
}
