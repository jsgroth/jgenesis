use bincode::{Decode, Encode};
use gb_core::apu::components::{TimerTickEffect, WavetableLengthCounter, WavetableTimer};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum RamMode {
    #[default]
    OneBank = 0,
    TwoBanks = 1,
}

impl RamMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::TwoBanks } else { Self::OneBank }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct WavetableChannel {
    ram: [u128; 2],
    ram_mode: RamMode,
    ram_bank: u8,
    timer: WavetableTimer,
    length_counter: WavetableLengthCounter,
    volume: u8,
    force_volume_75: bool,
    channel_enabled: bool,
    dac_enabled: bool,
}

impl WavetableChannel {
    pub fn new() -> Self {
        Self {
            ram: [0; 2],
            ram_mode: RamMode::default(),
            ram_bank: 0,
            timer: WavetableTimer::new(),
            length_counter: WavetableLengthCounter::new(),
            volume: 0,
            force_volume_75: false,
            channel_enabled: false,
            dac_enabled: false,
        }
    }

    pub fn tick_m_cycle(&mut self) {
        for _ in 0..2 {
            if self.timer.tick() == TimerTickEffect::Clocked {
                let ram_bank = &mut self.ram[self.ram_bank as usize];
                *ram_bank = ram_bank.rotate_right(4);

                if self.timer.phase == 0 && self.ram_mode == RamMode::TwoBanks {
                    self.ram_bank ^= 1;
                }
            }
        }
    }

    pub fn sample(&self) -> Option<u8> {
        if !self.dac_enabled {
            return None;
        }

        if !self.channel_enabled {
            return Some(0);
        }

        let sample = (self.ram[self.ram_bank as usize] & 0xF) as u8;
        Some(if self.force_volume_75 {
            3 * sample / 4
        } else if self.volume == 0 {
            0
        } else {
            sample >> (self.volume - 1)
        })
    }

    pub fn enabled(&self) -> bool {
        self.channel_enabled
    }

    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock(&mut self.channel_enabled);
    }

    pub fn read_ram(&self, address: u32) -> u8 {
        let ram_bank = self.ram[(self.ram_bank ^ 1) as usize];

        let offset = address & 0xF;
        nibble_swap((ram_bank >> (8 * offset)) as u8)
    }

    pub fn write_ram(&mut self, address: u32, value: u8) {
        let ram_bank = &mut self.ram[(self.ram_bank ^ 1) as usize];

        let offset = address & 0xF;
        let shift = 8 * offset;
        let mask = 0xFF_u128 << shift;
        *ram_bank = (*ram_bank & !mask) | (u128::from(nibble_swap(value)) << shift);
    }

    pub fn read_register_0(&self) -> u8 {
        0x1F | (u8::from(self.dac_enabled) << 7)
            | (self.ram_bank << 6)
            | ((self.ram_mode as u8) << 5)
    }

    pub fn write_register_0(&mut self, value: u8) {
        // NR30: Custom wave DAC enabled
        self.dac_enabled = value.bit(7);
        self.ram_bank = (value >> 6) & 1;
        self.ram_mode = RamMode::from_bit(value.bit(5));

        if !self.dac_enabled {
            self.channel_enabled = false;
        }

        log::trace!("NR30 write: {value:02X}");
        log::trace!("  DAC enabled: {}", self.dac_enabled);
        log::trace!("  RAM mode: {:?}", self.ram_mode);
        log::trace!("  RAM bank: {}", self.ram_bank);
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
        self.force_volume_75 = value.bit(7);

        log::trace!("NR32 write: {value:04X}");
        log::trace!("  Volume: {}", self.volume);
        log::trace!("  Force 75% volume: {}", self.force_volume_75);
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

        log::trace!("NR34 write: {value:02X}");
        log::trace!("  Timer frequency: {}", self.timer.frequency());
        log::trace!("  Length counter enabled: {}", self.length_counter.enabled);
        log::trace!("  Triggered: {}", value.bit(7));
    }
}

fn nibble_swap(byte: u8) -> u8 {
    (byte >> 4) | (byte << 4)
}
