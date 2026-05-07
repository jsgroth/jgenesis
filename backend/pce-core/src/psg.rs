//! The wavetable PSG built into the HuC6280

use crate::api;
use bincode::{Decode, Encode};
use dsp::sinc::PerformanceSincResampler;
use jgenesis_common::frontend::AudioOutput;
use jgenesis_common::num::GetBit;
use std::array;
use std::sync::LazyLock;

// Roughly 3.58 MHz
pub const PSG_FREQUENCY: f64 = api::MASTER_CLOCK_FREQUENCY / 6.0;

// 15 is max amplitude, 0 is silence, each step down is -3 dB
static AMPLITUDE_3_DB_LOOKUP_TABLE: LazyLock<[f64; 16]> =
    LazyLock::new(|| generate_amplitude_table(3.0));

// 31 is max amplitude, 0 is silence, each step down is -1.5 dB
static AMPLITUDE_1_5_DB_LOOKUP_TABLE: LazyLock<[f64; 32]> =
    LazyLock::new(|| generate_amplitude_table(1.5));

fn generate_amplitude_table<const LEN: usize>(step_db: f64) -> [f64; LEN] {
    let mut table = [0.0; LEN];
    table[LEN - 1] = 1.0;

    // A decrease of N dB is equal to multiplying by 10^(-N/20)
    let multiplier = 10.0_f64.powf(-step_db / 20.0);

    for i in (1..=LEN - 2).rev() {
        table[i] = table[i + 1] * multiplier;
    }

    table
}

#[derive(Debug, Clone, Encode, Decode)]
struct NoiseGenerator {
    enabled: bool,
    lfsr: u32,
    counter: u16,
    counter_reload: u16,
    current_sample: u8,
}

impl NoiseGenerator {
    // https://web.archive.org/web/20080311065543/http://cgfm2.emuviews.com:80/blog/index.php
    // Noise generator contains an 18-bit LFSR, initialized with only bit 0 set, taps bits 0 + 1 + 11 + 12 + 17

    fn new() -> Self {
        Self {
            enabled: false,
            lfsr: 1,
            counter: 0,
            counter_reload: 0x1F * 64,
            current_sample: 0x1F,
        }
    }

    fn write_r7(&mut self, value: u8) {
        self.enabled = value.bit(7);

        // Given a frequency value F, LFSR clocks every 64 * !F PSG cycles
        // TODO what happens when F is 0x1F, i.e. !F is 0? manual says undefined
        self.counter_reload = 64 * u16::from(!value & 0x1F);
    }

    fn clock(&mut self) {
        self.counter = self.counter.saturating_sub(1);
        if self.counter == 0 {
            self.counter = self.counter_reload;

            // Noise generator always outputs either max sample or min sample based on the shifted-out bit
            self.current_sample = if self.lfsr.bit(0) { 0x1F } else { 0x00 };

            let new_bit = self.lfsr.bit(0)
                ^ self.lfsr.bit(1)
                ^ self.lfsr.bit(11)
                ^ self.lfsr.bit(12)
                ^ self.lfsr.bit(17);
            self.lfsr = (self.lfsr >> 1) | (u32::from(new_bit) << 17);
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct PsgChannel {
    on: bool,
    direct_da: bool,
    wave_ram: [u8; 32],
    wave_address: u8,
    frequency: u32,
    counter: u32,
    amplitude: u8,
    l_amplitude: u8,
    r_amplitude: u8,
    current_sample: u8,
    noise: NoiseGenerator,
}

impl PsgChannel {
    fn new() -> Self {
        Self {
            on: false,
            direct_da: false,
            wave_ram: array::from_fn(|_| 0),
            wave_address: 0,
            frequency: 1,
            counter: 1,
            amplitude: 0,
            l_amplitude: 0,
            r_amplitude: 0,
            current_sample: 0,
            noise: NoiseGenerator::new(),
        }
    }

    fn clock(&mut self) {
        if self.direct_da {
            return;
        }

        if self.noise.enabled {
            self.noise.clock();
        }

        self.counter = self.counter.saturating_sub(1);
        if self.counter == 0 {
            self.wave_address = (self.wave_address + 1) & 0x1F;
            self.counter = self.frequency;
        }

        self.current_sample = self.wave_ram[self.wave_address as usize];
    }

    fn current_output(&self) -> u8 {
        if self.noise.enabled && !self.direct_da {
            self.noise.current_sample
        } else {
            self.current_sample
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Huc6280Psg {
    channels: [PsgChannel; 6],
    selected_channel: u8,
    l_main_amplitude: u8,
    r_main_amplitude: u8,
    lfo_frequency: u32,
    lfo_control: u8,
    resampler: PerformanceSincResampler<2>,
    cycles: u64,
}

impl Huc6280Psg {
    pub fn new() -> Self {
        Self {
            channels: array::from_fn(|_| PsgChannel::new()),
            selected_channel: 0,
            l_main_amplitude: 0,
            r_main_amplitude: 0,
            lfo_frequency: 0,
            lfo_control: 0,
            resampler: PerformanceSincResampler::new(PSG_FREQUENCY, 48000.0),
            cycles: 0,
        }
    }

    pub fn step_to(&mut self, cycles: u64) {
        while self.cycles < cycles {
            self.clock();
            self.cycles += 6;
        }
    }

    pub fn clock(&mut self) {
        let mut sample_l = 0.0;
        let mut sample_r = 0.0;

        for channel in &mut self.channels {
            if !channel.on {
                continue;
            }

            channel.clock();

            // Per the official manual, total attenuation of 45 dB or higher results in silence
            // Calculate in units of 3 dB (so 15 = 45 dB)
            let total_attenuation_l =
                45 - (channel.amplitude >> 1) - channel.l_amplitude - self.l_main_amplitude;
            let total_attenuation_r =
                45 - (channel.amplitude >> 1) - channel.r_amplitude - self.r_main_amplitude;

            if total_attenuation_l >= 15 && total_attenuation_r >= 15 {
                // Both of this channel's outputs are silent, skip sample calculations
                continue;
            }

            // Center waveform at 0 for less poppy audio, and divide by 6 for number of channels
            let channel_sample = channel.current_output();
            let channel_sample = (f64::from(channel_sample) - 15.5) / 15.5
                * AMPLITUDE_1_5_DB_LOOKUP_TABLE[channel.amplitude as usize]
                / 6.0;

            if total_attenuation_l < 15 {
                sample_l +=
                    channel_sample * AMPLITUDE_3_DB_LOOKUP_TABLE[channel.l_amplitude as usize];
            }

            if total_attenuation_r < 15 {
                sample_r +=
                    channel_sample * AMPLITUDE_3_DB_LOOKUP_TABLE[channel.r_amplitude as usize];
            }
        }

        sample_l *= AMPLITUDE_3_DB_LOOKUP_TABLE[self.l_main_amplitude as usize];
        sample_r *= AMPLITUDE_3_DB_LOOKUP_TABLE[self.r_main_amplitude as usize];

        self.resampler.collect([sample_l, sample_r]);
    }

    pub fn drain_output_buffer<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency as f64);
    }

    // $1FE800-$1FE80F: PSG registers
    pub fn write(&mut self, address: u32, value: u8) {
        let address = address & 0xF;

        log::trace!("PSG R{address} write: {value:02X} (ch {})", self.selected_channel);

        if (2..=7).contains(&address) && self.selected_channel >= 6 {
            // Per-channel register with an invalid channel
            return;
        }

        if address == 7 && self.selected_channel < 4 {
            // Invalid; only channels 5 and 6 support noise
            return;
        }

        match address {
            0 => {
                // R0: Channel select
                self.selected_channel = value & 7;

                log::trace!("Selected channel: {}", self.selected_channel);
            }
            1 => {
                // R1: Main amplitude
                self.l_main_amplitude = value >> 4;
                self.r_main_amplitude = value & 0xF;

                log::trace!("L main amplitude: {}", self.l_main_amplitude);
                log::trace!("R main amplitude: {}", self.r_main_amplitude);
            }
            2 => {
                // R2: Frequency, low bits
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.frequency = (channel.frequency & !0xFF) | u32::from(value);

                log::trace!("Frequency: {}", channel.frequency);
            }
            3 => {
                // R3: Frequency, high bits
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.frequency = (channel.frequency & 0xFF) | (u32::from(value & 0xF) << 8);

                log::trace!("Frequency: {}", channel.frequency);
            }
            4 => {
                // R4: Channel on, DDA, channel amplitude
                let channel = &mut self.channels[self.selected_channel as usize];

                let prev_on = channel.on;
                channel.on = value.bit(7);
                channel.direct_da = value.bit(6);
                channel.amplitude = value & 0x1F;

                if !prev_on && channel.on {
                    channel.counter = channel.frequency;
                }

                if channel.direct_da {
                    channel.wave_address = 0;
                }

                log::trace!("Channel on: {}", channel.on);
                log::trace!("Direct D/A: {}", channel.direct_da);
                log::trace!("Channel amplitude: {}", channel.amplitude);
            }
            5 => {
                // R5: L/R amplitude
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.l_amplitude = value >> 4;
                channel.r_amplitude = value & 0xF;

                log::trace!("Channel L amplitude: {}", channel.l_amplitude);
                log::trace!("Channel R amplitude: {}", channel.r_amplitude);
            }
            6 => {
                // R6: Waveform data
                let sample = value & 0x1F;

                let channel = &mut self.channels[self.selected_channel as usize];
                if channel.direct_da {
                    channel.current_sample = sample;
                } else {
                    channel.wave_ram[channel.wave_address as usize] = sample;

                    // TODO is this right? manual suggests increment is based purely on frequency
                    // when CHON=1 and DDA=0
                    if !channel.on {
                        channel.wave_address = (channel.wave_address + 1) & 0x1F;
                    }
                }
            }
            7 => {
                // R7: Noise enable and frequency
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.noise.write_r7(value);

                log::trace!("Noise enabled: {}", channel.noise.enabled);
                log::trace!("Noise frequency: {}", value & 0x1F);
            }
            8 => {
                // LFO frequency
                self.lfo_frequency = value.into();

                log::trace!("LFO frequency: {}", self.lfo_frequency);
            }
            9 => {
                // LFO control
                if value.bit(7) {
                    // TODO trigger LFO
                }

                self.lfo_control = value & 3;

                if self.lfo_control != 0 {
                    log::warn!("LFO enabled");
                }

                log::trace!("LFO triggered: {}", value.bit(7));
                log::trace!("LFO control: {}", self.lfo_control);
            }
            10..=15 => {} // Invalid addresses
            _ => unreachable!("value & 0xF is always <= 15"),
        }
    }
}
