//! The wavetable PSG built into the HuC6280

use crate::api;
use bincode::{Decode, Encode};
use dsp::sinc::PerformanceSincResampler;
use jgenesis_common::frontend::AudioOutput;
use jgenesis_common::num::{GetBit, U16Ext};
use std::sync::LazyLock;
use std::{array, mem};

// Roughly 3.58 MHz
pub const PSG_CLOCK_DIVIDER: u64 = 6;
pub const PSG_FREQUENCY: f64 = api::MASTER_CLOCK_FREQUENCY / (PSG_CLOCK_DIVIDER as f64);

const VOLUME_UPDATE_PERIOD_CYCLES: u16 = 256;

// Inverted amplitude lookup table
// 0 is max volume, 30-31 is silence, each step increases attenuation by 1.5 dB
static ATTENUATION_LOOKUP_TABLE: LazyLock<[f64; 32]> = LazyLock::new(|| {
    let mut table = [0.0; 32];
    table[0] = 1.0;

    for i in 1..=29 {
        table[i] = table[i - 1] * 10.0_f64.powf(-1.5 / 20.0);
    }

    table
});

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
            counter: 0x1F * 64,
            counter_reload: 0x1F * 64,
            current_sample: 0x1F,
        }
    }

    fn write_r7(&mut self, value: u8) {
        self.enabled = value.bit(7);

        // Given a frequency value F, LFSR clocks every 64 * !F PSG cycles
        self.counter_reload = 64 * u16::from(!value & 0x1F);
    }

    fn clock(&mut self) {
        // In hardware there seems to be some sort of 6-bit divider; emulate that as the counter
        // being 11-bit instead of 5-bit counter + 6-bit divider
        self.counter = self.counter.wrapping_sub(1) & 0x7FF;
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
    idx: u8,
    on: bool,
    direct_da: bool,
    wave_ram: [u8; 32],
    wave_address: u8,
    frequency: u16,
    counter: u16,
    amplitude: u8,
    l_amplitude: u8,
    r_amplitude: u8,
    current_sample: u8,
    noise: NoiseGenerator,
    latched_attenuation_l: u8,
    latched_attenuation_r: u8,
}

impl PsgChannel {
    fn new(idx: u8) -> Self {
        Self {
            idx,
            on: false,
            direct_da: false,
            wave_ram: array::from_fn(|_| 0),
            wave_address: 0,
            frequency: 0xFFF,
            counter: 0xFFF,
            amplitude: 0,
            l_amplitude: 0,
            r_amplitude: 0,
            current_sample: 0,
            noise: NoiseGenerator::new(),
            latched_attenuation_l: 31,
            latched_attenuation_r: 31,
        }
    }

    fn clock(&mut self, lfo: &mut LowFrequencyOscillator, channel_2_sample: Option<u8>) {
        if self.direct_da {
            return;
        }

        if self.idx == 1 && lfo.enabled() && lfo.triggered {
            // Manual implies that channel 2 is halted while the LFO is enabled and triggered
            return;
        }

        if self.noise.enabled {
            self.noise.clock();
        }

        // 12-bit frequency counter
        self.counter = self.counter.wrapping_sub(1) & 0xFFF;
        if self.counter == 0 {
            // When LFO is enabled, channel 2 frequency modulates channel 1
            let effective_frequency = if self.idx == 0
                && lfo.enabled()
                && let Some(sample) = channel_2_sample
            {
                lfo.modulate_frequency(self.frequency, sample)
            } else {
                self.frequency
            };
            self.counter = effective_frequency;

            // When LFO is enabled, channel 2 frequency is multiplied by LFO frequency
            let increment_wave_address = if self.idx == 1 && lfo.enabled() {
                lfo.clock() == LfoClock::IncrementWaveAddress
            } else {
                true
            };
            if increment_wave_address {
                self.wave_address = (self.wave_address + 1) & 0x1F;
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LfoClock {
    IncrementWaveAddress,
    None,
}

#[derive(Debug, Clone, Encode, Decode)]
struct LowFrequencyOscillator {
    triggered: bool,
    control: u8,
    counter: u8,
    frequency: u8,
}

impl LowFrequencyOscillator {
    fn new() -> Self {
        Self { triggered: false, control: 0, counter: 0xFF, frequency: 0xFF }
    }

    fn enabled(&self) -> bool {
        // LFO is enabled whenever control bits are non-zero (R9 lowest two bits)
        self.control != 0
    }

    fn clock(&mut self) -> LfoClock {
        self.counter = self.counter.wrapping_sub(1);
        if self.counter == 0 {
            self.counter = self.frequency;
            LfoClock::IncrementWaveAddress
        } else {
            LfoClock::None
        }
    }

    fn modulate_frequency(&self, frequency: u16, channel_2_sample: u8) -> u16 {
        debug_assert_ne!(self.control, 0, "modulate_frequency() called when LFO is disabled");

        // Per manual, modulation range is +0x0F (sample 0x1F) to -0x10 (sample 0x00)
        let frequency_delta = i16::from(channel_2_sample) - 0x10;

        // 1 = No shift
        // 2 = Left shift 2
        // 3 = Left shift 4
        let shift = 2 * (self.control - 1);

        frequency.wrapping_add_signed(frequency_delta << shift) & 0xFFF
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum OutputChannel {
    Right,
    Left,
}

impl OutputChannel {
    fn other(self) -> Self {
        match self {
            Self::Right => Self::Left,
            Self::Left => Self::Right,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeUpdateState {
    update_needed: bool,
    active: bool,
    channel: u8,
    output: OutputChannel,
    cycles_till_next_update: u16,
    latched_attenuation: u8,
}

impl VolumeUpdateState {
    fn new() -> Self {
        Self {
            update_needed: false,
            active: false,
            channel: 0,
            output: OutputChannel::Right,
            cycles_till_next_update: VOLUME_UPDATE_PERIOD_CYCLES,
            latched_attenuation: 0,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Huc6280Psg {
    channels: [PsgChannel; 6],
    selected_channel: u8,
    l_main_amplitude: u8,
    r_main_amplitude: u8,
    lfo: LowFrequencyOscillator,
    resampler: PerformanceSincResampler<2>,
    cycles: u64,
    volume: VolumeUpdateState,
}

impl Huc6280Psg {
    pub fn new() -> Self {
        Self {
            channels: array::from_fn(|idx| PsgChannel::new(idx as u8)),
            selected_channel: 0,
            l_main_amplitude: 0,
            r_main_amplitude: 0,
            lfo: LowFrequencyOscillator::new(),
            resampler: PerformanceSincResampler::new(PSG_FREQUENCY, 48000.0),
            cycles: 0,
            volume: VolumeUpdateState::new(),
        }
    }

    pub fn step_to(&mut self, cycles: u64) {
        while self.cycles < cycles {
            self.clock();
            self.cycles += PSG_CLOCK_DIVIDER;
        }
    }

    pub fn clock(&mut self) {
        let mut sample_l = 0.0;
        let mut sample_r = 0.0;

        let channel_2_sample = self.channels[1].on.then_some(self.channels[1].current_sample);

        for channel in &mut self.channels {
            if !channel.on {
                continue;
            }

            channel.clock(&mut self.lfo, channel_2_sample);

            // Per the official manual, total attenuation of 45 dB or higher results in silence
            // Attenuation is in steps of 1.5 dB, so 30 = 45 dB
            if channel.latched_attenuation_l >= 30 && channel.latched_attenuation_r >= 30 {
                // Both of this channel's outputs are silent, skip sample calculations
                continue;
            }

            // Center waveform at 0 for less poppy audio, and divide by 6 for number of channels
            let channel_sample = channel.current_output();
            let channel_sample = (f64::from(channel_sample) - 15.5) / 15.5 / 6.0;

            if channel.latched_attenuation_l < 30 {
                sample_l += channel_sample
                    * ATTENUATION_LOOKUP_TABLE[channel.latched_attenuation_l as usize];
            }

            if channel.latched_attenuation_r < 30 {
                sample_r += channel_sample
                    * ATTENUATION_LOOKUP_TABLE[channel.latched_attenuation_r as usize];
            }
        }

        self.resampler.collect([sample_l, sample_r]);

        if self.volume.active {
            self.progress_volume_update();
        }
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

    fn trigger_volume_update(&mut self) {
        if !self.volume.active {
            // Volume update is not running; start one now
            self.volume.active = true;
        } else {
            // A volume update is already running, but another one is needed after it finishes
            self.volume.update_needed = true;
        }
    }

    fn progress_volume_update(&mut self) {
        // Based on Mednafen's PSG volume update implementation; I don't think this behavior is
        // documented anywhere else, but volume/amplitude updates apparently don't apply immediately
        // Honey in the Sky depends on this to avoid static/buzzing noises caused by rapid volume changes

        self.volume.cycles_till_next_update -= 1;
        if self.volume.cycles_till_next_update == VOLUME_UPDATE_PERIOD_CYCLES - 1 {
            // Latch attenuation on the first cycle of the 256-cycle update period
            if let Some(channel) = self.channels.get(self.volume.channel as usize) {
                // Channel amplitude is 0-31, -1 is 1.5 dB attenuation
                // L/R amplitudes are 0-15, -1 is 3 dB attenuation
                self.volume.latched_attenuation = match self.volume.output {
                    OutputChannel::Right => {
                        (31 - channel.amplitude)
                            + 2 * (15 - channel.r_amplitude)
                            + 2 * (15 - self.r_main_amplitude)
                    }
                    OutputChannel::Left => {
                        (31 - channel.amplitude)
                            + 2 * (15 - channel.l_amplitude)
                            + 2 * (15 - self.l_main_amplitude)
                    }
                };
            }
        }

        if self.volume.cycles_till_next_update != 0 {
            return;
        }
        self.volume.cycles_till_next_update = VOLUME_UPDATE_PERIOD_CYCLES;

        if let Some(channel) = self.channels.get_mut(self.volume.channel as usize) {
            // Apply latched attenuation to channel at the end of the 256-cycle period
            match self.volume.output {
                OutputChannel::Right => {
                    channel.latched_attenuation_r = self.volume.latched_attenuation;
                }
                OutputChannel::Left => {
                    channel.latched_attenuation_l = self.volume.latched_attenuation;
                }
            }
        }

        self.volume.output = self.volume.output.other();
        if self.volume.output == OutputChannel::Right {
            // Per Mednafen the volume update goes through an 8-channel loop, even though there are
            // only 6 channels; the last two iterations do nothing
            self.volume.channel = (self.volume.channel + 1) & 7;
            if self.volume.channel == 0 {
                // End of update loop; start again if an amplitude register was written mid-update
                self.volume.active = mem::take(&mut self.volume.update_needed);
            }
        }
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

                self.trigger_volume_update();
            }
            2 => {
                // R2: Frequency, low bits
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.frequency.set_lsb(value);

                log::trace!("Frequency: {}", channel.frequency);
            }
            3 => {
                // R3: Frequency, high bits
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.frequency.set_msb(value & 0xF);

                log::trace!("Frequency: {}", channel.frequency);
            }
            4 => {
                // R4: Channel on, direct D/A, channel amplitude
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

                self.trigger_volume_update();
            }
            5 => {
                // R5: L/R amplitude
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.l_amplitude = value >> 4;
                channel.r_amplitude = value & 0xF;

                log::trace!("Channel L amplitude: {}", channel.l_amplitude);
                log::trace!("Channel R amplitude: {}", channel.r_amplitude);

                self.trigger_volume_update();
            }
            6 => {
                // R6: Waveform data
                let sample = value & 0x1F;

                let channel = &mut self.channels[self.selected_channel as usize];
                channel.current_sample = sample;

                if !channel.direct_da {
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
                // R8: LFO frequency
                self.lfo.frequency = value;

                log::trace!("LFO frequency: {}", self.lfo.frequency);
            }
            9 => {
                // R9: LFO control
                self.lfo.triggered = value.bit(7);
                self.lfo.control = value & 3;

                if self.lfo.enabled() && self.lfo.triggered {
                    // Manual implies that triggering the LFO resets channel 2 and halts it
                    self.channels[1].wave_address = 0;
                    self.channels[1].current_sample = self.channels[1].wave_ram[0];
                    self.channels[1].counter = self.channels[1].frequency;

                    self.lfo.counter = self.lfo.frequency;
                }

                log::trace!("LFO triggered: {}", self.lfo.triggered);
                log::trace!("LFO control: {}", self.lfo.control);
            }
            10..=15 => {} // Invalid addresses
            _ => unreachable!("value & 0xF is always <= 15"),
        }
    }
}
