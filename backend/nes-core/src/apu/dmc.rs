//! The APU's delta modulation channel (DMC) which can play back delta-encoded PCM samples read
//! from memory, one byte at a time.
//!
//! Channel output values are between 0 and 127 (inclusive).
//!
//! This channel can optionally generate IRQs when the current sample has been completely read
//! from memory.

use crate::bus::CpuBus;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::GetBit;

// From https://www.nesdev.org/wiki/APU_DMC
const NTSC_PERIOD_LOOKUP_TABLE: [u16; 16] =
    [428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54];
const PAL_PERIOD_LOOKUP_TABLE: [u16; 16] =
    [398, 354, 316, 298, 276, 236, 210, 198, 176, 148, 132, 118, 98, 78, 66, 50];

#[derive(Debug, Clone, Encode, Decode)]
struct DmcOutputUnit {
    output_level: u8,
    shift_register: u8,
    bits_remaining: u8,
    silence_flag: bool,
}

impl DmcOutputUnit {
    fn new() -> Self {
        Self { output_level: 0, shift_register: 0, bits_remaining: 8, silence_flag: true }
    }

    fn clock(&mut self, sample_buffer: &mut Option<u8>) {
        if !self.silence_flag {
            let new_output_level = if self.shift_register.bit(0) {
                self.output_level + 2
            } else {
                self.output_level.wrapping_sub(2)
            };
            if new_output_level < 128 {
                self.output_level = new_output_level;
            }
        }

        self.shift_register >>= 1;
        self.bits_remaining -= 1;

        if self.bits_remaining == 0 {
            self.bits_remaining = 8;
            match sample_buffer.take() {
                Some(sample_bits) => {
                    self.shift_register = sample_bits;
                    self.silence_flag = false;
                }
                None => {
                    self.silence_flag = true;
                }
            }
        }
    }

    fn sample(&self) -> u8 {
        self.output_level
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DeltaModulationChannel {
    enabled: bool,
    timer_counter: u16,
    timer_period: u16,
    sample_buffer: Option<u8>,
    output_unit: DmcOutputUnit,
    sample_address: u16,
    current_sample_address: u16,
    sample_length: u16,
    sample_bytes_remaining: u16,
    loop_flag: bool,
    irq_enabled: bool,
    interrupt_flag: bool,
    dma_initial_load: bool,
    dma_start_latency: u8,
    period_lookup_table: [u16; 16],
}

impl DeltaModulationChannel {
    pub fn new(timing_mode: TimingMode) -> Self {
        let period_lookup_table = match timing_mode {
            TimingMode::Ntsc => NTSC_PERIOD_LOOKUP_TABLE,
            TimingMode::Pal => PAL_PERIOD_LOOKUP_TABLE,
        };

        Self {
            enabled: false,
            timer_counter: period_lookup_table[0] - 1,
            timer_period: period_lookup_table[0],
            sample_buffer: None,
            output_unit: DmcOutputUnit::new(),
            sample_address: 0xC000,
            current_sample_address: 0xC000,
            sample_length: 1,
            sample_bytes_remaining: 0,
            loop_flag: false,
            irq_enabled: false,
            interrupt_flag: false,
            dma_initial_load: false,
            dma_start_latency: 0,
            period_lookup_table,
        }
    }

    pub fn process_dmc_freq_update(&mut self, dmc_freq_value: u8) {
        self.irq_enabled = dmc_freq_value.bit(7);
        self.loop_flag = dmc_freq_value.bit(6);
        self.timer_period = self.period_lookup_table[(dmc_freq_value & 0x0F) as usize];

        if !self.irq_enabled {
            self.interrupt_flag = false;
        }
    }

    pub fn process_dmc_raw_update(&mut self, dmc_raw_value: u8) {
        self.output_unit.output_level = dmc_raw_value & 0x7F;
    }

    pub fn process_dmc_start_update(&mut self, dmc_start_value: u8) {
        self.sample_address = 0xC000 | (u16::from(dmc_start_value) << 6);
    }

    pub fn process_dmc_len_update(&mut self, dmc_len_value: u8) {
        self.sample_length = (u16::from(dmc_len_value) << 4) + 1;
    }

    pub fn process_snd_chn_update(&mut self, snd_chn_value: u8, frame_counter_ticks: u16) {
        log::trace!("SND_CHN write: {snd_chn_value:02X} (enabled = {})", snd_chn_value.bit(4));

        self.interrupt_flag = false;

        self.enabled = snd_chn_value.bit(4);
        if self.enabled && self.sample_bytes_remaining == 0 {
            self.restart();
            self.dma_initial_load = true;
            // Load DMAs begin in 3 cycles (if SND_CHN written on put) or 4 cycles (if written on get)
            self.dma_start_latency = 2 + (frame_counter_ticks & 1) as u8;
        } else if !self.enabled {
            self.sample_bytes_remaining = 0;
            self.sample_buffer = None;
        }
    }

    fn restart(&mut self) {
        self.current_sample_address = self.sample_address;
        self.sample_bytes_remaining = self.sample_length;
    }

    pub fn needs_dma(&self) -> bool {
        self.enabled
            && self.sample_bytes_remaining != 0
            && self.sample_buffer.is_none()
            && self.dma_start_latency == 0
    }

    pub fn dma_initial_load(&self) -> bool {
        self.dma_initial_load
    }

    pub fn dma_read(&mut self, bus: &mut CpuBus<'_>, halted_cpu_address: u16) {
        log::trace!(
            "DMA read from address {:04X}, remaining {}",
            self.current_sample_address,
            self.sample_bytes_remaining
        );

        self.sample_buffer =
            Some(bus.dmc_dma_read(self.current_sample_address, halted_cpu_address));

        // Sample address overflows to $8000 rather than $C000
        self.current_sample_address = 0x8000 | self.current_sample_address.wrapping_add(1);
        self.sample_bytes_remaining -= 1;

        if self.sample_bytes_remaining == 0 {
            if self.loop_flag {
                self.restart();
            } else if self.irq_enabled {
                self.interrupt_flag = true;
            }
        }
    }

    pub fn tick_cpu(&mut self) {
        if self.timer_counter == 0 {
            self.clock();
            self.timer_counter = self.timer_period - 1;
        } else {
            self.timer_counter -= 1;
        }

        self.dma_start_latency = self.dma_start_latency.saturating_sub(1);
    }

    fn clock(&mut self) {
        let sample_buffer_was_full = self.sample_buffer.is_some();
        self.output_unit.clock(&mut self.sample_buffer);

        if self.enabled
            && sample_buffer_was_full
            && self.sample_buffer.is_none()
            && self.sample_bytes_remaining != 0
        {
            self.dma_initial_load = false;
            // Reload DMAs begin in 2 cycles (if emptied on get) or 3 cycles (if emptied on put)
            self.dma_start_latency = 2;
        }
    }

    pub fn sample(&self) -> u8 {
        self.output_unit.sample()
    }

    pub fn sample_bytes_remaining(&self) -> u16 {
        self.sample_bytes_remaining
    }

    pub fn interrupt_flag(&self) -> bool {
        self.interrupt_flag
    }

    pub fn reset(&mut self) {
        self.output_unit.output_level &= 0x01;
    }
}
