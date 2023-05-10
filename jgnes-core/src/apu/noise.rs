use crate::apu::units::{Envelope, LengthCounter, LengthCounterChannel};
use crate::num::GetBit;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum LfsrMode {
    Bit1Feedback,
    Bit6Feedback,
}

#[derive(Debug, Clone, Encode, Decode)]
struct LinearFeedbackShiftRegister {
    register: u16,
    mode: LfsrMode,
}

impl LinearFeedbackShiftRegister {
    fn new() -> Self {
        Self {
            register: 1,
            mode: LfsrMode::Bit1Feedback,
        }
    }

    fn clock(&mut self) {
        let feedback = match self.mode {
            LfsrMode::Bit1Feedback => (self.register & 0x01) ^ ((self.register & 0x02) >> 1),
            LfsrMode::Bit6Feedback => (self.register & 0x01) ^ ((self.register & 0x40) >> 6),
        };

        self.register = (self.register >> 1) | (feedback << 14);
    }

    fn sample(&self) -> u8 {
        (!self.register & 0x01) as u8
    }
}

const NOISE_PERIOD_LOOKUP_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

#[derive(Debug, Clone, Encode, Decode)]
pub struct NoiseChannel {
    lfsr: LinearFeedbackShiftRegister,
    timer_counter: u16,
    timer_period: u16,
    length_counter: LengthCounter,
    envelope: Envelope,
}

impl NoiseChannel {
    pub fn new() -> Self {
        Self {
            lfsr: LinearFeedbackShiftRegister::new(),
            timer_counter: 0,
            timer_period: 1,
            length_counter: LengthCounter::new(LengthCounterChannel::Noise),
            envelope: Envelope::new(),
        }
    }

    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    pub fn tick_cpu(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period - 1;
            self.lfsr.clock();
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn process_vol_update(&mut self, vol_value: u8) {
        self.envelope.process_vol_update(vol_value);
        self.length_counter.process_vol_update(vol_value);
    }

    pub fn process_lo_update(&mut self, lo_value: u8) {
        self.lfsr.mode = if lo_value.bit(7) {
            LfsrMode::Bit6Feedback
        } else {
            LfsrMode::Bit1Feedback
        };

        self.timer_period = NOISE_PERIOD_LOOKUP_TABLE[(lo_value & 0x0F) as usize];
    }

    pub fn process_hi_update(&mut self, hi_value: u8) {
        self.envelope.process_hi_update();
        self.length_counter.process_hi_update(hi_value);
    }

    pub fn process_snd_chn_update(&mut self, snd_chn_value: u8) {
        self.length_counter.process_snd_chn_update(snd_chn_value);
    }

    pub fn sample(&self) -> u8 {
        if self.length_counter.counter == 0 {
            0
        } else {
            self.lfsr.sample() * self.envelope.volume()
        }
    }

    pub fn length_counter(&self) -> u8 {
        self.length_counter.counter
    }
}
