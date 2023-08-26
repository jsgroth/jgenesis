use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

// LFO counter is 7 bits
const LFO_COUNTER_MASK: u8 = 0x7F;

// TODO figure out if these numbers are remotely correct
const LFO_DIVIDERS: [u16; 8] = [
    15704, // 3.816Hz
    11241, // 5.331Hz
    10382, // 5.772Hz
    9812,  // 6.108Hz
    9084,  // 6.597Hz
    6490,  // 9.233Hz
    1299,  // 46.119Hz
    866,   // 69.226Hz
];

// Adapted from http://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386&start=480
// Values are for the highest bit of F-number
const FM_INCREMENT_TABLE: [[u16; 8]; 8] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 2, 2, 2, 2],
    [0, 0, 0, 2, 2, 2, 4, 4],
    [0, 0, 2, 2, 4, 4, 6, 6],
    [0, 0, 2, 4, 4, 4, 6, 8],
    [0, 0, 4, 6, 8, 8, 10, 12],
    [0, 0, 8, 12, 16, 16, 20, 24],
    [0, 0, 16, 24, 32, 32, 40, 48],
];

#[derive(Debug, Clone, Encode, Decode)]
pub struct LowFrequencyOscillator {
    enabled: bool,
    counter: u8,
    divider: u16,
    divider_reload: u16,
}

impl LowFrequencyOscillator {
    pub fn new() -> Self {
        Self {
            enabled: false,
            counter: 0,
            divider: LFO_DIVIDERS[0],
            divider_reload: LFO_DIVIDERS[0],
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.counter = 0;
        }
    }

    pub fn set_frequency(&mut self, frequency: u8) {
        self.divider_reload = LFO_DIVIDERS[frequency as usize];
    }

    pub fn counter(&self) -> u8 {
        self.counter
    }

    #[inline]
    pub fn tick(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = self.divider_reload;

            if self.enabled {
                self.counter = (self.counter + 1) & LFO_COUNTER_MASK;
            }
        }
    }
}

// Returns the modulated F-number
pub fn frequency_modulation(lfo_counter: u8, fm_sensitivity: u8, f_number: u16) -> u16 {
    if fm_sensitivity == 0 {
        return f_number;
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
    let fm_increment = (4..11)
        .map(|i| {
            if f_number.bit(i) {
                FM_INCREMENT_TABLE[fm_sensitivity as usize][fm_table_idx as usize] >> (10 - i)
            } else {
                0
            }
        })
        .sum::<u16>();

    if lfo_counter.bit(6) {
        // Negative half of wave
        f_number.wrapping_sub(fm_increment) & 0x7FF
    } else {
        // Positive half of wave
        f_number.wrapping_add(fm_increment) & 0x7FF
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
