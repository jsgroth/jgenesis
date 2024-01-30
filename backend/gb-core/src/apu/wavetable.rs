use crate::apu::components::{TimerTickEffect, WavetableLengthCounter, WavetableTimer};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Encode, Decode)]
pub struct WavetableChannel {
    ram: [u8; 16],
    sample_buffer: u8,
    timer: WavetableTimer,
    length_counter: WavetableLengthCounter,
    volume: u8,
    channel_enabled: bool,
    dac_enabled: bool,
}

impl WavetableChannel {
    pub fn new() -> Self {
        Self {
            ram: [0; 16],
            sample_buffer: 0,
            timer: WavetableTimer::new(),
            length_counter: WavetableLengthCounter::new(),
            volume: 0,
            channel_enabled: false,
            dac_enabled: false,
        }
    }

    pub fn read_ram(&self, address: u16) -> u8 {
        self.ram[(address & 0xF) as usize]
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        self.ram[(address & 0xF) as usize] = value;
    }

    pub fn read_register_0(&self) -> u8 {
        0x7F | (u8::from(self.dac_enabled) << 7)
    }

    pub fn write_register_0(&mut self, value: u8) {
        // NR30: Custom wave DAC enabled
        self.dac_enabled = value.bit(7);

        if !self.dac_enabled {
            self.channel_enabled = false;
        }

        log::trace!("NR30 write, DAC enabled: {}", self.dac_enabled);
    }

    pub fn write_register_1(&mut self, value: u8) {
        // NR31: Custom wave length counter reload
        self.length_counter.load(value);

        log::trace!("NR31 write, length counter: {}", self.length_counter.counter);
    }

    pub fn read_register_2(&self) -> u8 {
        0x9F | (self.volume << 5)
    }

    pub fn write_register_2(&mut self, value: u8) {
        // NR32: Custom wave volume
        self.volume = (value >> 5) & 0x03;

        log::trace!("NR32 write, volume: {}", self.volume);
    }

    pub fn write_register_3(&mut self, value: u8) {
        // NR33: Custom wave frequency low bits
        self.timer.write_frequency_low(value);

        log::trace!("NR33 write, timer frequency: {}", self.timer.frequency());
    }

    pub fn read_register_4(&self) -> u8 {
        0xBF | (u8::from(self.length_counter.enabled) << 6)
    }

    pub fn write_register_4(&mut self, value: u8, frame_sequencer_step: u8) {
        // NR34: Custom wave frequency high bits + length counter enabled + trigger
        self.timer.write_frequency_high(value);
        self.length_counter.set_enabled(
            value.bit(6),
            frame_sequencer_step,
            &mut self.channel_enabled,
        );

        if value.bit(7) {
            // Channel triggered
            self.timer.trigger();
            self.timer.phase = 0;

            self.length_counter.trigger(frame_sequencer_step);

            self.channel_enabled = self.dac_enabled;
        }

        log::trace!("NR34 write");
        log::trace!("  Timer frequency: {}", self.timer.frequency());
        log::trace!("  Length counter enabled: {}", self.length_counter.enabled);
        log::trace!("  Triggered: {}", value.bit(7));
    }

    pub fn tick_m_cycle(&mut self) {
        if self.timer.tick_m_cycle() == TimerTickEffect::Clocked {
            let sample_byte = self.ram[(self.timer.phase >> 1) as usize];

            // First sample in high nibble, second in low nibble
            self.sample_buffer =
                if !self.timer.phase.bit(0) { sample_byte >> 4 } else { sample_byte & 0x0F };
        }
    }

    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock(&mut self.channel_enabled);
    }

    pub fn sample(&self) -> Option<u8> {
        if !self.dac_enabled {
            return None;
        }

        if !self.channel_enabled || self.volume == 0 {
            return Some(0);
        }

        Some(self.sample_buffer >> (self.volume - 1))
    }

    pub fn enabled(&self) -> bool {
        self.channel_enabled
    }

    pub fn reset(&mut self) {
        *self = Self { ram: self.ram, ..Self::new() };
    }
}
