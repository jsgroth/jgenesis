//! YM2612 envelope generator

use crate::ym2612::phase::PhaseGenerator;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::cmp;

// Envelope updates every third cycle at 53627 Hz
const ENVELOPE_DIVIDER: u8 = 3;

const SSG_ATTENUATION_THRESHOLD: u16 = 0x200;

// Attenuation is 10 bits
pub(super) const ATTENUATION_MASK: u16 = 0x03FF;
pub(super) const MAX_ATTENUATION: u16 = ATTENUATION_MASK;

// From http://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386&start=105
#[rustfmt::skip]
const ATTENUATION_INCREMENTS: &[[u8; 8]; 64] = &[
    [0,0,0,0,0,0,0,0], [0,0,0,0,0,0,0,0], [0,1,0,1,0,1,0,1], [0,1,0,1,0,1,0,1],  // 0-3
    [0,1,0,1,0,1,0,1], [0,1,0,1,0,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,0,1,1,1],  // 4-7
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 8-11
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 12-15
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 16-19
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 20-23
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 24-27
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 28-31
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 32-35
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 36-39
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 40-43
    [0,1,0,1,0,1,0,1], [0,1,0,1,1,1,0,1], [0,1,1,1,0,1,1,1], [0,1,1,1,1,1,1,1],  // 44-47
    [1,1,1,1,1,1,1,1], [1,1,1,2,1,1,1,2], [1,2,1,2,1,2,1,2], [1,2,2,2,1,2,2,2],  // 48-51
    [2,2,2,2,2,2,2,2], [2,2,2,4,2,2,2,4], [2,4,2,4,2,4,2,4], [2,4,4,4,2,4,4,4],  // 52-55
    [4,4,4,4,4,4,4,4], [4,4,4,8,4,4,4,8], [4,8,4,8,4,8,4,8], [4,8,8,8,4,8,8,8],  // 56-59
    [8,8,8,8,8,8,8,8], [8,8,8,8,8,8,8,8], [8,8,8,8,8,8,8,8], [8,8,8,8,8,8,8,8],  // 60-63
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum EnvelopePhase {
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(super) struct EnvelopeGenerator {
    // Register values
    pub(super) attack_rate: u8,
    pub(super) decay_rate: u8,
    pub(super) sustain_rate: u8,
    pub(super) release_rate: u8,
    pub(super) total_level: u8,
    pub(super) sustain_level: u8,
    pub(super) key_scale: u8,
    // Internal state
    phase: EnvelopePhase,
    attenuation: u16,
    pub(super) key_scale_rate: u8,
    cycle_count: u16,
    divider: u8,
    ssg_enabled: bool,
    ssg_attack: bool,
    ssg_alternate: bool,
    ssg_hold: bool,
    ssg_invert_output: bool,
}

impl EnvelopeGenerator {
    pub(super) fn new() -> Self {
        Self {
            attack_rate: 0,
            decay_rate: 0,
            sustain_rate: 0,
            release_rate: 0,
            total_level: 0,
            sustain_level: 0,
            key_scale: 0,
            phase: EnvelopePhase::Release,
            attenuation: MAX_ATTENUATION,
            key_scale_rate: 0,
            cycle_count: 1,
            divider: ENVELOPE_DIVIDER,
            ssg_enabled: false,
            ssg_attack: false,
            ssg_alternate: false,
            ssg_hold: false,
            ssg_invert_output: false,
        }
    }

    #[inline]
    pub(super) fn clock(&mut self, phase_generator: &mut PhaseGenerator) {
        // SSG updates on every clock if enabled
        if self.ssg_enabled {
            self.ssg_clock(phase_generator);
        }

        self.divider -= 1;
        if self.divider == 0 {
            self.divider = ENVELOPE_DIVIDER;
            self.envelope_clock();
        }
    }

    #[inline]
    fn envelope_clock(&mut self) {
        // It's been verified that actual hardware's envelope cycle counter is 12-bit and skips 0 on overflow
        self.cycle_count += 1;
        self.cycle_count = (self.cycle_count & 0xFFF) + (self.cycle_count >> 12);

        // Sustain level applies in increments of 32, with max level special cased to be
        // the max multiple of 32
        let sustain_level = match self.sustain_level {
            15 => (MAX_ATTENUATION >> 5) << 5,
            sl => u16::from(sl) << 5,
        };

        // Progress past attack phase if attenuation is at 0
        if self.phase == EnvelopePhase::Attack && self.attenuation == 0 {
            self.phase = EnvelopePhase::Decay;
        }

        // Progress past decay phase if attenuation is at or past sustain level
        if self.phase == EnvelopePhase::Decay && self.attenuation >= sustain_level {
            self.phase = EnvelopePhase::Sustain;
        }

        let r = match self.phase {
            EnvelopePhase::Attack => self.attack_rate,
            EnvelopePhase::Decay => self.decay_rate,
            EnvelopePhase::Sustain => self.sustain_rate,
            EnvelopePhase::Release => (self.release_rate << 1) | 1,
        };

        let rate = if r == 0 { 0 } else { cmp::min(63, 2 * r + self.key_scale_rate) };

        let update_frequency_shift = 11_u8.saturating_sub(rate >> 2);
        if self.cycle_count & ((1 << update_frequency_shift) - 1) == 0 {
            let increment_idx = (self.cycle_count >> update_frequency_shift) & 7;
            let increment: u16 =
                ATTENUATION_INCREMENTS[rate as usize][increment_idx as usize].into();

            match self.phase {
                EnvelopePhase::Attack => {
                    // Rates of 62 and 63 do nothing during attack phase; during key on they skip
                    // this phase entirely
                    if rate <= 61 {
                        // Formula from http://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386&start=405
                        self.attenuation = self
                            .attenuation
                            .wrapping_add((!self.attenuation).wrapping_mul(increment) >> 4)
                            & ATTENUATION_MASK;
                    }
                }
                EnvelopePhase::Decay | EnvelopePhase::Sustain | EnvelopePhase::Release => {
                    if self.ssg_enabled {
                        // Attenuation increments 4x as fast in SSG-EG mode, but only if current
                        // attenuation is below 0x200
                        if self.attenuation < SSG_ATTENUATION_THRESHOLD {
                            self.attenuation =
                                cmp::min(MAX_ATTENUATION, self.attenuation + 4 * increment);
                        }
                    } else {
                        self.attenuation = cmp::min(MAX_ATTENUATION, self.attenuation + increment);
                    }
                }
            }
        }
    }

    #[inline]
    fn ssg_clock(&mut self, phase_generator: &mut PhaseGenerator) {
        // SSG-EG implementation pretty much entirely based on this:
        // https://gendev.spritesmind.net/forum/viewtopic.php?t=386&start=405

        // SSG-EG updates only apply when attenuation is greater than or equal to 0x200
        if self.attenuation < SSG_ATTENUATION_THRESHOLD {
            return;
        }

        // Alternate flag causes the output to invert after each attack-decay-sustain pass
        if self.ssg_alternate {
            if self.ssg_hold {
                // If holding, the output is always inverted once the hold begins
                self.ssg_invert_output = true;
            } else {
                // If not holding, flip the invert output flag
                self.ssg_invert_output = !self.ssg_invert_output;
            }
        }

        if !self.ssg_alternate && !self.ssg_hold {
            // When not alternating and not holding, the phase counter is held at 0 until
            // attenuation drops below 0x200
            phase_generator.reset();
        }

        if matches!(self.phase, EnvelopePhase::Decay | EnvelopePhase::Sustain) && !self.ssg_hold {
            // If in decay or sustain phase and not holding, start a new attack-decay-sustain pass
            if 2 * self.attack_rate + self.key_scale_rate >= 62 {
                // Skip attack phase
                self.attenuation = 0;
                self.phase = EnvelopePhase::Decay;
            } else {
                self.phase = EnvelopePhase::Attack;
            }
        } else if self.phase == EnvelopePhase::Release
            || (self.phase != EnvelopePhase::Attack && self.ssg_invert_output == self.ssg_attack)
        {
            // If in release phase _or_ in decay/sustain phase with invert output flag matching attack flag,
            // force attenuation to max
            self.attenuation = MAX_ATTENUATION;
        }
    }

    pub(super) fn is_key_on(&self) -> bool {
        self.phase != EnvelopePhase::Release
    }

    pub(super) fn key_on(&mut self) {
        if self.is_key_on() {
            // Key is already down
            return;
        }

        let rate = 2 * self.attack_rate + self.key_scale_rate;

        // Rates of 62 and 63 skip attack phase
        if rate >= 62 {
            self.phase = EnvelopePhase::Decay;
            self.attenuation = 0;
        } else {
            self.phase = EnvelopePhase::Attack;
        }

        self.ssg_invert_output = false;

        log::trace!("State at key on: {self:?}");
    }

    pub(super) fn key_off(&mut self) {
        if self.ssg_enabled
            && self.phase != EnvelopePhase::Release
            && self.ssg_invert_output != self.ssg_attack
        {
            // If SSG-EG is on and output is inverted, keying off applies the inversion to stored
            // attenuation
            self.attenuation =
                SSG_ATTENUATION_THRESHOLD.wrapping_sub(self.attenuation) & ATTENUATION_MASK;
        }

        self.phase = EnvelopePhase::Release;
    }

    pub(super) fn update_key_scale_rate(&mut self, f_number: u16, block: u8) {
        let key_code = super::compute_key_code(f_number, block);
        self.key_scale_rate = key_code >> (3 - self.key_scale);
    }

    pub(super) fn current_attenuation(&self) -> u16 {
        let attenuation = if self.ssg_enabled
            && self.phase != EnvelopePhase::Release
            && self.ssg_invert_output != self.ssg_attack
        {
            // Apply SSG output inversion, which centers around 0x200
            SSG_ATTENUATION_THRESHOLD.wrapping_sub(self.attenuation) & ATTENUATION_MASK
        } else {
            self.attenuation
        };

        let total_level = u16::from(self.total_level) << 3;
        cmp::min(MAX_ATTENUATION, attenuation + total_level)
    }

    pub(super) fn write_ssg_register(&mut self, value: u8) {
        self.ssg_enabled = value.bit(3);
        self.ssg_attack = value.bit(2);
        self.ssg_alternate = value.bit(1);
        self.ssg_hold = value.bit(0);

        log::trace!(
            "SSG-EG register write; enabled={}, attack={}, alternate={}, hold={}",
            self.ssg_enabled,
            self.ssg_attack,
            self.ssg_alternate,
            self.ssg_hold
        );
    }
}

impl Default for EnvelopeGenerator {
    fn default() -> Self {
        Self::new()
    }
}
