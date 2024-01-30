use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct LengthCounter<const MAX: u16> {
    pub enabled: bool,
    pub counter: u16,
}

impl<const MAX: u16> LengthCounter<MAX> {
    pub fn new() -> Self {
        Self { enabled: false, counter: MAX }
    }

    pub fn load(&mut self, value: u8) {
        let masked_value = u16::from(value) & (MAX - 1);
        self.counter = MAX - masked_value;
    }

    pub fn trigger(&mut self, frame_sequencer_step: u8) {
        if self.counter == 0 {
            self.counter = MAX;

            // Quirk: Immediately clock if enabled during trigger and this is a length counter cycle
            if self.enabled && !frame_sequencer_step.bit(0) {
                self.counter -= 1;
            }
        }
    }

    pub fn clock(&mut self, channel_enabled: &mut bool) {
        if !self.enabled || self.counter == 0 {
            return;
        }

        self.counter -= 1;
        if self.counter == 0 {
            *channel_enabled = false;
        }
    }

    pub fn set_enabled(
        &mut self,
        enabled: bool,
        frame_sequencer_step: u8,
        channel_enabled: &mut bool,
    ) {
        let prev_enabled = self.enabled;
        self.enabled = enabled;

        // Quirk: Immediately clock if newly enabled and this is a length counter cycle
        if !prev_enabled && self.enabled && !frame_sequencer_step.bit(0) {
            self.clock(channel_enabled);
        }
    }
}

pub type StandardLengthCounter = LengthCounter<64>;
pub type WavetableLengthCounter = LengthCounter<256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum EnvelopeDirection {
    Increasing,
    #[default]
    Decreasing,
}

impl EnvelopeDirection {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Increasing } else { Self::Decreasing }
    }

    fn to_bit(self) -> bool {
        self == Self::Increasing
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct Envelope {
    pub volume: u8,
    enabled: bool,
    period: u8,
    counter: u8,
    direction: EnvelopeDirection,
    initial_volume: u8,
    configured_direction: EnvelopeDirection,
    configured_period: u8,
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            volume: 0,
            enabled: false,
            period: 0,
            counter: 0,
            direction: EnvelopeDirection::default(),
            initial_volume: 0,
            configured_direction: EnvelopeDirection::default(),
            configured_period: 0,
        }
    }

    pub fn read_register(self) -> u8 {
        (self.initial_volume << 4)
            | (u8::from(self.configured_direction.to_bit()) << 3)
            | self.configured_period
    }

    pub fn write_register(&mut self, value: u8) {
        let direction = EnvelopeDirection::from_bit(value.bit(3));

        // "Zombie mode" hardware glitch: If the envelope register is written to with bit 3 set while the
        // current period is 0, immediately increment volume while wrapping around from 15 to 0.
        // Exact behavior seems to vary between hardware revisions, but this implementation seems to
        // work for games that depend on "zombie mode" (e.g. Prehistorik Man)
        if self.enabled && self.period == 0 && direction == EnvelopeDirection::Increasing {
            self.volume = (self.volume + 1) & 0x0F;
        }

        self.initial_volume = value >> 4;
        self.configured_direction = direction;
        self.configured_period = value & 0x07;
    }

    pub fn trigger(&mut self) {
        self.volume = self.initial_volume;
        self.direction = self.configured_direction;
        self.period = self.configured_period;

        self.enabled = true;
        self.counter = self.period;
    }

    pub fn clock(&mut self) {
        if self.period == 0 || !self.enabled {
            return;
        }

        self.counter -= 1;
        if self.counter == 0 {
            self.counter = self.period;

            match (self.direction, self.volume) {
                (EnvelopeDirection::Decreasing, 0) | (EnvelopeDirection::Increasing, 15) => {
                    // Volume cannot decrease past 0 or increase past 15
                    // Disable the envelope until next trigger
                    self.enabled = false;
                }
                (EnvelopeDirection::Decreasing, _) => {
                    self.volume -= 1;
                }
                (EnvelopeDirection::Increasing, _) => {
                    self.volume += 1;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerTickEffect {
    None,
    Clocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct PhaseTimer<const MAX_PHASE: u8, const SPEED_MULTIPLIER: u16> {
    pub phase: u8,
    frequency: u16,
    counter: u16,
    period: u16,
}

impl<const MAX_PHASE: u8, const SPEED_MULTIPLIER: u16> PhaseTimer<MAX_PHASE, SPEED_MULTIPLIER> {
    pub fn new() -> Self {
        // Sanity check that (MAX_PHASE + 1) is a power of 2
        assert_eq!(MAX_PHASE.trailing_ones() + MAX_PHASE.leading_zeros(), u8::BITS);

        Self { phase: 0, counter: 2048, period: 2048, frequency: 0 }
    }

    pub fn frequency(self) -> u16 {
        self.frequency
    }

    pub fn write_frequency_low(&mut self, value: u8) {
        self.write_frequency((self.frequency & 0xFF00) | u16::from(value));
    }

    pub fn write_frequency_high(&mut self, value: u8) {
        self.write_frequency((self.frequency & 0x00FF) | (u16::from(value & 0x07) << 8));
    }

    pub fn write_frequency(&mut self, value: u16) {
        self.frequency = value;
        self.period = 2048 - value;
    }

    pub fn trigger(&mut self) {
        self.counter = self.period;
    }

    pub fn tick_m_cycle(&mut self) -> TimerTickEffect {
        let mut tick_effect = TimerTickEffect::None;

        for _ in 0..SPEED_MULTIPLIER {
            self.counter -= 1;
            if self.counter == 0 {
                self.counter = self.period;
                self.phase = (self.phase + 1) & MAX_PHASE;
                tick_effect = TimerTickEffect::Clocked;
            }
        }

        tick_effect
    }
}

pub type PulseTimer = PhaseTimer<7, 1>;
pub type WavetableTimer = PhaseTimer<31, 2>;
