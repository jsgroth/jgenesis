use crate::apu::components::{Envelope, StandardLengthCounter};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum LfsrWidthBits {
    Seven,
    #[default]
    Fifteen,
}

impl LfsrWidthBits {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Seven } else { Self::Fifteen }
    }

    fn to_bit(self) -> bool {
        self == Self::Seven
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct NoiseChannel {
    counter: u32,
    clock_divider: u8,
    clock_shift: u8,
    lfsr: u16,
    lfsr_width: LfsrWidthBits,
    length_counter: StandardLengthCounter,
    envelope: Envelope,
    channel_enabled: bool,
    dac_enabled: bool,
}

impl NoiseChannel {
    pub fn new() -> Self {
        Self {
            counter: 2,
            clock_divider: 0,
            clock_shift: 0,
            lfsr: 0,
            lfsr_width: LfsrWidthBits::default(),
            length_counter: StandardLengthCounter::new(),
            envelope: Envelope::new(),
            channel_enabled: false,
            dac_enabled: false,
        }
    }

    pub fn write_register_1(&mut self, value: u8) {
        // NR41: Noise length counter reload
        self.length_counter.load(value);

        log::trace!("NR41 write, length counter: {}", self.length_counter.counter);
    }

    pub fn read_register_2(&self) -> u8 {
        self.envelope.read_register()
    }

    pub fn write_register_2(&mut self, value: u8) {
        // NR42: Noise envelope control
        self.envelope.write_register(value);
        self.dac_enabled = value & 0xF8 != 0;

        if !self.dac_enabled {
            self.channel_enabled = false;
        }

        log::trace!("NR42 write");
        log::trace!("  Envelope: {:?}", self.envelope);
        log::trace!("  DAC enabled: {}", self.dac_enabled);
    }

    pub fn read_register_3(&self) -> u8 {
        (self.clock_shift << 4) | (u8::from(self.lfsr_width.to_bit()) << 3) | self.clock_divider
    }

    pub fn write_register_3(&mut self, value: u8) {
        // NR43: Noise frequency + LFSR size (7-bit vs. 15-bit)
        self.clock_shift = value >> 4;
        self.lfsr_width = LfsrWidthBits::from_bit(value.bit(3));
        self.clock_divider = value & 0x07;

        log::trace!("NR43 write");
        log::trace!("  Shift: {}", self.clock_shift);
        log::trace!("  LFSR bits: {:?}", self.lfsr_width);
        log::trace!("  Divider code: {}", self.clock_divider);
    }

    pub fn read_register_4(&self) -> u8 {
        0xBF | (u8::from(self.length_counter.enabled) << 6)
    }

    pub fn write_register_4(&mut self, value: u8, frame_sequencer_step: u8) {
        // NR44: Noise length counter enabled + trigger
        self.length_counter.set_enabled(
            value.bit(6),
            frame_sequencer_step,
            &mut self.channel_enabled,
        );

        if value.bit(7) {
            // Channel triggered
            self.length_counter.trigger(frame_sequencer_step);
            self.envelope.trigger();
            self.lfsr = 0x7FFF;
            self.counter = compute_clock_period(self.clock_divider, self.clock_shift);

            self.channel_enabled = self.dac_enabled;
        }

        log::trace!("NR44 write");
        log::trace!("  Length counter enabled: {}", self.length_counter.enabled);
        log::trace!("  Triggered: {}", value.bit(7));
    }

    pub fn tick_m_cycle(&mut self) {
        self.counter -= 1;
        if self.counter == 0 {
            self.counter = compute_clock_period(self.clock_divider, self.clock_shift);

            let new_bit = self.lfsr.bit(0) ^ self.lfsr.bit(1);
            self.lfsr = (self.lfsr >> 1) | (u16::from(new_bit) << 14);

            if self.lfsr_width == LfsrWidthBits::Seven {
                self.lfsr = (self.lfsr & !(1 << 6)) | (u16::from(new_bit) << 6);
            }
        }
    }

    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock(&mut self.channel_enabled);
    }

    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub fn sample(&self) -> Option<u8> {
        if !self.dac_enabled {
            return None;
        }

        if !self.channel_enabled {
            return Some(0);
        }

        Some(u8::from(!self.lfsr.bit(0)) * self.envelope.volume)
    }

    pub fn enabled(&self) -> bool {
        self.channel_enabled
    }
}

fn compute_clock_period(divider: u8, shift: u8) -> u32 {
    let base_divisor = if divider == 0 { 8 } else { 16 * u32::from(divider) };
    (base_divisor << shift) / 4
}
