use smsgg_core::num::GetBit;

const PHASE_DIVIDER: u8 = 144 / 6;

// Shifted F-num values are 17 bits
const SHIFTED_F_NUM_MASK: u32 = 0x1FFFF;

// Phase counter is 20 bits
const PHASE_COUNTER_MASK: u32 = 0xFFFFF;

// Created by adapting the documented detune table into increments of ~0.053Hz
const DETUNE_TABLE: [[u8; 4]; 32] = [
    [0, 0, 1, 2],
    [0, 0, 1, 2],
    [0, 0, 1, 2],
    [0, 0, 1, 2],
    [0, 1, 2, 2],
    [0, 1, 2, 3],
    [0, 1, 2, 3],
    [0, 1, 2, 3],
    [0, 1, 2, 4],
    [0, 1, 3, 4],
    [0, 1, 3, 4],
    [0, 1, 3, 5],
    [0, 2, 4, 5],
    [0, 2, 4, 6],
    [0, 2, 4, 6],
    [0, 2, 5, 7],
    [0, 2, 5, 8],
    [0, 3, 6, 8],
    [0, 3, 6, 9],
    [0, 3, 7, 10],
    [0, 4, 8, 11],
    [0, 4, 8, 12],
    [0, 4, 9, 13],
    [0, 5, 10, 14],
    [0, 5, 11, 16],
    [0, 6, 12, 17],
    [0, 6, 13, 19],
    [0, 7, 14, 20],
    [0, 8, 16, 22],
    [0, 8, 16, 22],
    [0, 8, 16, 22],
    [0, 8, 16, 22],
];

#[derive(Debug, Clone)]
pub(super) struct PhaseGenerator {
    // Register values
    pub(super) f_number: u16,
    pub(super) block: u8,
    pub(super) multiple: u8,
    pub(super) detune: u8,
    // Internal state
    counter: u32,
    current_output: u16,
    last_output: u16,
    divider: u8,
}

impl PhaseGenerator {
    pub(super) fn new() -> Self {
        Self {
            f_number: 0,
            block: 0,
            multiple: 0,
            detune: 0,
            counter: 0,
            current_output: 0,
            last_output: 0,
            divider: PHASE_DIVIDER,
        }
    }

    #[inline]
    pub(super) fn fm_clock(&mut self) {
        if self.divider == 1 {
            self.divider = PHASE_DIVIDER;
            self.clock();
        } else {
            self.divider -= 1;
        }
    }

    pub(super) fn reset(&mut self) {
        self.counter = 0;
    }

    #[inline]
    fn clock(&mut self) {
        let phase_increment = self.compute_phase_increment();
        self.counter = (self.counter + phase_increment) & PHASE_COUNTER_MASK;

        // Phase generator output is the highest 10 bits of the 20-bit phase counter
        self.last_output = self.current_output;
        self.current_output = (self.counter >> 10) as u16;
    }

    fn compute_phase_increment(&self) -> u32 {
        // Apply block/octave multiplier
        let shifted_f_num = match self.block {
            0 => u32::from(self.f_number) >> 1,
            block => u32::from(self.f_number) << (block - 1),
        };

        // Apply detune
        let key_code = super::compute_key_code(self.f_number, self.block);
        let detune_magnitude = self.detune & 0x03;
        let detune_increment_magnitude: u32 =
            DETUNE_TABLE[key_code as usize][detune_magnitude as usize].into();
        let detune_increment = if self.detune.bit(2) {
            (!detune_increment_magnitude).wrapping_add(1)
        } else {
            detune_increment_magnitude
        };

        let detuned_f_num = shifted_f_num.wrapping_add(detune_increment) & SHIFTED_F_NUM_MASK;

        // Apply frequency multiplier
        if self.multiple == 0 {
            detuned_f_num >> 1
        } else {
            (detuned_f_num * u32::from(self.multiple)) & PHASE_COUNTER_MASK
        }
    }

    pub(super) fn current_phase(&self) -> u16 {
        self.current_output
    }

    pub(super) fn last_phase(&self) -> u16 {
        self.last_output
    }
}

impl Default for PhaseGenerator {
    fn default() -> Self {
        Self::new()
    }
}
