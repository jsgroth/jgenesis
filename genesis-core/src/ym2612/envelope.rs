use std::cmp;

const ENVELOPE_DIVIDER: u8 = 72;

// Attenuation is 10 bits
pub(super) const ATTENUATION_MASK: u16 = 0x03FF;
pub(super) const MAX_ATTENUATION: u16 = ATTENUATION_MASK;

// From http://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386&start=105
const ATTENUATION_INCREMENTS: [[u8; 8]; 64] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 2, 1, 1, 1, 2],
    [1, 2, 1, 2, 1, 2, 1, 2],
    [1, 2, 2, 2, 1, 2, 2, 2],
    [2, 2, 2, 2, 2, 2, 2, 2],
    [2, 2, 2, 4, 2, 2, 2, 4],
    [2, 4, 2, 4, 2, 4, 2, 4],
    [2, 4, 4, 4, 2, 4, 4, 4],
    [4, 4, 4, 4, 4, 4, 4, 4],
    [4, 4, 4, 8, 4, 4, 4, 8],
    [4, 8, 4, 8, 4, 8, 4, 8],
    [4, 8, 8, 8, 4, 8, 8, 8],
    [8, 8, 8, 8, 8, 8, 8, 8],
    [8, 8, 8, 8, 8, 8, 8, 8],
    [8, 8, 8, 8, 8, 8, 8, 8],
    [8, 8, 8, 8, 8, 8, 8, 8],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvelopePhase {
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone)]
pub(super) struct EnvelopeGenerator {
    // Register values
    pub(super) attack_rate: u8,
    pub(super) decay_rate: u8,
    pub(super) sustain_rate: u8,
    pub(super) release_rate: u8,
    pub(super) total_level: u8,
    pub(super) sustain_level: u8,
    pub(super) am_enabled: bool,
    pub(super) key_scale: u8,
    // Internal state
    phase: EnvelopePhase,
    attenuation: u16,
    pub(super) key_scale_rate: u8,
    cycle_count: u32,
    divider: u8,
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
            am_enabled: false,
            key_scale: 0,
            phase: EnvelopePhase::Release,
            attenuation: MAX_ATTENUATION,
            key_scale_rate: 0,
            cycle_count: 0,
            divider: ENVELOPE_DIVIDER,
        }
    }

    #[inline]
    pub(super) fn fm_clock(&mut self) {
        if self.divider == 1 {
            self.divider = ENVELOPE_DIVIDER;
            self.envelope_clock();
        } else {
            self.divider -= 1;
        }
    }

    #[inline]
    fn envelope_clock(&mut self) {
        self.cycle_count = self.cycle_count.wrapping_add(1);

        // Sustain level applies in increments of 32, with max level special cased to be
        // max attenuation
        let sustain_level = if self.sustain_level == 15 {
            MAX_ATTENUATION
        } else {
            u16::from(self.sustain_level) << 5
        };

        // Skip decay phase if attenuation is already past sustain level
        if self.phase == EnvelopePhase::Decay && self.attenuation >= sustain_level {
            self.phase = EnvelopePhase::Sustain;
        }

        let r = match self.phase {
            EnvelopePhase::Attack => self.attack_rate,
            EnvelopePhase::Decay => self.decay_rate,
            EnvelopePhase::Sustain => self.sustain_rate,
            EnvelopePhase::Release => (self.release_rate << 1) | 0x01,
        };

        let rate = cmp::min(63, 2 * r + self.key_scale_rate);

        let update_frequency_shift = ((63 - rate) >> 2).saturating_sub(4);
        if self.cycle_count % (1 << update_frequency_shift) == 0 {
            let increment_idx = (self.cycle_count >> update_frequency_shift) & 0x07;
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
                        if self.attenuation == 0 {
                            self.phase = EnvelopePhase::Decay;
                        }
                    }
                }
                EnvelopePhase::Decay => {
                    self.attenuation = cmp::min(sustain_level, self.attenuation + increment);

                    if self.attenuation == sustain_level {
                        self.phase = EnvelopePhase::Sustain;
                    }
                }
                EnvelopePhase::Sustain | EnvelopePhase::Release => {
                    self.attenuation = cmp::min(MAX_ATTENUATION, self.attenuation + increment);
                }
            }
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
            self.attenuation = MAX_ATTENUATION;
        }
    }

    pub(super) fn key_off(&mut self) {
        self.phase = EnvelopePhase::Release;
    }

    pub(super) fn update_key_scale_rate(&mut self, f_number: u16, block: u8) {
        let key_code = super::compute_key_code(f_number, block);
        self.key_scale_rate = key_code >> (3 - self.key_scale);
    }

    pub(super) fn current_attenuation(&self) -> u16 {
        let total_level = u16::from(self.total_level) << 3;
        cmp::min(MAX_ATTENUATION, self.attenuation + total_level)
    }
}

impl Default for EnvelopeGenerator {
    fn default() -> Self {
        Self::new()
    }
}
