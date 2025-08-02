//! PSG channel 3 (wavetable / custom wave)

use bincode::{Decode, Encode};
use gb_core::apu::components::{TimerTickEffect, WavetableLengthCounter, WavetableTimer};
use jgenesis_common::define_bit_enum;
use jgenesis_common::num::GetBit;
use std::array;

const WAVE_RAM_BANK_LEN: usize = 16;

define_bit_enum!(WaveRamSizeBanks, [One, Two]);

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct WavetableChannel {
    wave_ram: [[u8; WAVE_RAM_BANK_LEN]; 2],
    sample_buffer: u8,
    wave_ram_size: WaveRamSizeBanks,
    selected_bank: bool,
    timer: WavetableTimer,
    length_counter: WavetableLengthCounter,
    volume: u8,
    force_volume_75: bool,
    enabled: bool,
    active: bool,
}

impl WavetableChannel {
    pub fn new() -> Self {
        Self {
            wave_ram: array::from_fn(|_| array::from_fn(|_| 0)),
            sample_buffer: 0,
            wave_ram_size: WaveRamSizeBanks::default(),
            selected_bank: false,
            timer: WavetableTimer::new(),
            length_counter: WavetableLengthCounter::new(),
            volume: 0,
            force_volume_75: false,
            enabled: false,
            active: false,
        }
    }

    pub fn tick_1mhz(&mut self) {
        if !self.active {
            return;
        }

        if self.timer.tick_m_cycle() == TimerTickEffect::Clocked {
            if self.wave_ram_size == WaveRamSizeBanks::Two && self.timer.phase == 0 {
                self.selected_bank = !self.selected_bank;
            }

            self.sample_buffer =
                self.wave_ram[usize::from(self.selected_bank)][(self.timer.phase >> 1) as usize];
        }
    }

    pub fn clock_length_counter(&mut self) {
        let prev_active = self.active;
        self.length_counter.clock(&mut self.active);

        if prev_active && !self.active {
            // Explicitly clear sample buffer when length counter disables channel
            self.sample_buffer = 0;
        }
    }

    pub fn sample(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        let sample = if !self.timer.phase.bit(0) {
            self.sample_buffer >> 4
        } else {
            self.sample_buffer & 0xF
        };

        if self.force_volume_75 {
            (3 * sample) >> 2
        } else if self.volume == 0 {
            0
        } else {
            sample >> (self.volume - 1)
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn write_nr30(&mut self, value: u8) {
        self.wave_ram_size = WaveRamSizeBanks::from_bit(value.bit(5));
        self.selected_bank = value.bit(6);
        self.enabled = value.bit(7);

        self.active &= self.enabled;

        log::trace!("NR30 write: {value:02X}");
        log::trace!("  Wave RAM banks: {:?}", self.wave_ram_size);
        log::trace!("  Playback wave RAM bank: {}", u8::from(self.selected_bank));
        log::trace!("  Channel enabled: {}", self.enabled);
    }

    pub fn read_nr30(&self) -> u8 {
        0x1F | ((self.wave_ram_size as u8) << 5)
            | (u8::from(self.selected_bank) << 6)
            | (u8::from(self.enabled) << 7)
    }

    pub fn write_nr31(&mut self, value: u8) {
        self.length_counter.load(value);

        log::trace!("NR31 write: {value:02X} (length counter load)");
    }

    pub fn write_nr32(&mut self, value: u8) {
        self.volume = (value >> 5) & 3;
        self.force_volume_75 = value.bit(7);

        log::trace!("NR32 write: {value:02X}");
        log::trace!("  Volume: {}", ["0%", "100%", "50%", "25%"][self.volume as usize]);
        log::trace!("  Force volume to 75%: {}", self.volume);
    }

    pub fn read_nr32(&self) -> u8 {
        0x1F | (self.volume << 5) | (u8::from(self.force_volume_75) << 7)
    }

    pub fn write_nr33(&mut self, value: u8) {
        self.timer.write_frequency_low(value);

        log::trace!("NR33 write: {value:02X}");
        log::trace!("  Timer frequency: {}", self.timer.frequency());
    }

    pub fn read_nr34(&self) -> u8 {
        0xBF | (u8::from(self.length_counter.enabled) << 6)
    }

    pub fn write_nr34(&mut self, value: u8, frame_sequencer_step: u8) {
        self.timer.write_frequency_high(value);
        self.length_counter.set_enabled(value.bit(4), frame_sequencer_step, &mut self.active);

        if value.bit(7) {
            // Channel triggered
            self.timer.trigger();
            self.timer.phase = 0;

            self.length_counter.trigger(frame_sequencer_step);

            self.active = self.enabled;
        }

        log::trace!("NR34 write: {value:02X}");
        log::trace!("  Timer frequency: {}", self.timer.frequency());
        log::trace!("  Length counter enabled: {}", self.length_counter.enabled);
        log::trace!("  Triggered: {}", value.bit(7));
    }

    pub fn read_wave_ram(&self, address: u32) -> u8 {
        // Reads always go to the inactive bank
        let wave_ram_addr = (address & 0xF) as usize;
        let bank: usize = (!self.selected_bank).into();
        self.wave_ram[bank][wave_ram_addr]
    }

    pub fn write_wave_ram(&mut self, address: u32, value: u8) {
        // Writes always go to the inactive bank
        let wave_ram_addr = (address & 0xF) as usize;
        let bank: usize = (!self.selected_bank).into();
        self.wave_ram[bank][wave_ram_addr] = value;
    }
}
