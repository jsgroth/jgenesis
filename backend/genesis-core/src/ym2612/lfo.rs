//! YM2612 low frequency oscillator (LFO)

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

// LFO counter is 7 bits
const LFO_COUNTER_MASK: u8 = 0x7F;

const LFO_DIVIDERS: [u8; 8] = [
    108, // 3.85 Hz
    77,  // 5.40 Hz
    71,  // 5.86 Hz
    67,  // 6.21 Hz
    62,  // 6.71 Hz
    44,  // 9.46 Hz
    8,   // 52.02 Hz
    5,   // 83.23 Hz
];

// Adapted from http://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386&start=480
// Values are for the highest bit of F-number
const FM_INCREMENT_TABLE: &[[u16; 8]; 8] = &[
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 4, 4, 4, 4],
    [0, 0, 0, 4, 4, 4, 8, 8],
    [0, 0, 4, 4, 8, 8, 12, 12],
    [0, 0, 4, 8, 8, 8, 12, 16],
    [0, 0, 8, 12, 16, 16, 20, 24],
    [0, 0, 16, 24, 32, 32, 40, 48],
    [0, 0, 32, 48, 64, 64, 80, 96],
];

#[derive(Debug, Clone, Encode, Decode)]
pub struct LowFrequencyOscillator {
    enabled: bool,
    counter: u8,
    divider: u8,
    frequency: u8,
}

impl LowFrequencyOscillator {
    pub fn new() -> Self {
        Self { enabled: false, counter: 0, divider: 0, frequency: LFO_DIVIDERS[0] }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.counter = 0;
        }
    }

    pub fn set_frequency(&mut self, frequency: u8) {
        self.frequency = LFO_DIVIDERS[frequency as usize];
    }

    pub fn counter(&self) -> u8 {
        self.counter
    }

    #[inline]
    pub fn tick(&mut self) {
        // TODO is this the correct way to handle LFO frequency changes?
        self.divider += 1;
        if self.divider >= self.frequency {
            self.divider = 0;

            if self.enabled {
                self.counter = (self.counter + 1) & LFO_COUNTER_MASK;
            }
        }
    }
}

// Returns the modulated F-number, as a 12-bit value (left shifted 1 from input F-num)
pub fn frequency_modulation(lfo_counter: u8, fm_sensitivity: u8, f_number: u16) -> u16 {
    if fm_sensitivity == 0 {
        return f_number << 1;
    }

    let fm_table_idx = if lfo_counter.bit(5) {
        // Max to zero
        (0x1F - (lfo_counter & 0x1F)) >> 2
    } else {
        // Zero to max
        (lfo_counter & 0x1F) >> 2
    };

    // Compute total increment from the highest 7 bits of F-number; the lower 4 bits never add any
    // increment
    let raw_increment = FM_INCREMENT_TABLE[fm_sensitivity as usize][fm_table_idx as usize];
    let fm_increment = (4..11)
        .map(|i| {
            let bit = (f_number >> i) & 1;
            bit * (raw_increment >> (10 - i))
        })
        .sum::<u16>();

    if lfo_counter.bit(6) {
        // Negative half of wave
        (f_number << 1).wrapping_sub(fm_increment) & 0xFFF
    } else {
        // Positive half of wave
        (f_number << 1).wrapping_add(fm_increment) & 0xFFF
    }
}

// Returns a value in envelope attenuation units (10 bits representing 0-96dB)
pub fn amplitude_modulation(lfo_counter: u8, am_sensitivity: u8) -> u16 {
    let am_attenuation = if lfo_counter.bit(6) {
        // Attenuation increases from 0dB to 11.8dB
        lfo_counter & 0x3F
    } else {
        // Attenuation decreases from 11.8dB to 0dB
        0x3F - lfo_counter
    };

    // Shift left 1 because LFO counter bits 5-0 correspond to bits 6-1 in envelope attenuation scale
    // Max AM attenuation should be 11.8125dB
    let am_attenuation: u16 = (am_attenuation << 1).into();

    match am_sensitivity {
        0 => 0,
        1 => am_attenuation >> 3,
        2 => am_attenuation >> 1,
        3 => am_attenuation,
        _ => panic!("invalid AM sensitivity value: {am_sensitivity}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_dividers() {
        for (freq, divider) in LFO_DIVIDERS.into_iter().enumerate() {
            let mut lfo = LowFrequencyOscillator::new();
            lfo.set_enabled(true);
            lfo.set_frequency(freq as u8);

            for i in 0..4 {
                for tick in 0..divider - 1 {
                    lfo.tick();
                    assert_eq!(
                        i,
                        lfo.counter(),
                        "LFO counter should be {i} after {} ticks with divider {divider}",
                        tick + 1
                    );
                }

                lfo.tick();
                assert_eq!(
                    i + 1,
                    lfo.counter(),
                    "LFO counter should be {} after {divider} ticks (frequency {freq})",
                    i + 1
                );
            }
        }
    }
}
