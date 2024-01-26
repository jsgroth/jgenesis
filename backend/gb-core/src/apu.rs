//! Game Boy APU (audio processing unit)

mod components;
mod noise;
mod pulse;
mod wavetable;

use crate::apu::noise::NoiseChannel;
use crate::apu::pulse::PulseChannel;
use crate::apu::wavetable::WavetableChannel;
use crate::audio::GameBoyResampler;
use crate::speed::CpuSpeed;
use crate::timer::GbTimer;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::AudioOutput;
use jgenesis_common::num::GetBit;
use std::array;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct StereoControl {
    left_volume: u8,
    right_volume: u8,
    vin_bits: u8,
    left_channels: [bool; 4],
    right_channels: [bool; 4],
}

impl StereoControl {
    pub fn new() -> Self {
        // Initial state from https://gbdev.io/pandocs/Power_Up_Sequence.html#hardware-registers
        Self {
            left_volume: 7,
            right_volume: 7,
            vin_bits: 0,
            left_channels: [true; 4],
            right_channels: [true, true, false, false],
        }
    }

    pub fn zero() -> Self {
        Self {
            left_volume: 0,
            right_volume: 0,
            vin_bits: 0,
            left_channels: [false; 4],
            right_channels: [false; 4],
        }
    }

    pub fn read_volume(&self) -> u8 {
        (self.left_volume << 4) | self.right_volume | self.vin_bits
    }

    pub fn write_volume(&mut self, value: u8) {
        self.left_volume = (value >> 4) & 0x07;
        self.right_volume = value & 0x07;
        self.vin_bits = value & 0x88;

        log::trace!("NR50 write");
        log::trace!("  L volume: {}", self.left_volume);
        log::trace!("  R volume: {}", self.right_volume);
    }

    pub fn read_enabled(&self) -> u8 {
        let high_nibble = stereo_channels_to_nibble(self.left_channels);
        let low_nibble = stereo_channels_to_nibble(self.right_channels);
        (high_nibble << 4) | low_nibble
    }

    pub fn write_enabled(&mut self, value: u8) {
        self.left_channels = array::from_fn(|i| value.bit(4 + i as u8));
        self.right_channels = array::from_fn(|i| value.bit(i as u8));

        log::trace!("NR51 write");
        log::trace!("  L enabled: {:?}", self.left_channels);
        log::trace!("  R enabled: {:?}", self.right_channels);
    }
}

fn stereo_channels_to_nibble(channels: [bool; 4]) -> u8 {
    channels.into_iter().enumerate().map(|(i, b)| u8::from(b) << i).reduce(|a, b| a | b).unwrap()
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Apu {
    enabled: bool,
    pulse_1: PulseChannel,
    pulse_2: PulseChannel,
    wavetable: WavetableChannel,
    noise: NoiseChannel,
    stereo_control: StereoControl,
    frame_sequencer_step: u8,
    previous_div_bit: bool,
    resampler: GameBoyResampler,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            enabled: false,
            pulse_1: PulseChannel::new(),
            pulse_2: PulseChannel::new(),
            wavetable: WavetableChannel::new(),
            noise: NoiseChannel::new(),
            stereo_control: StereoControl::new(),
            frame_sequencer_step: 0,
            previous_div_bit: false,
            resampler: GameBoyResampler::new(),
        }
    }

    pub fn tick_m_cycle(&mut self, timer: &GbTimer, cpu_speed: CpuSpeed) {
        let div_bit_index = match cpu_speed {
            CpuSpeed::Normal => 4,
            CpuSpeed::Double => 5,
        };

        let div_bit = timer.read_div().bit(div_bit_index);
        if self.previous_div_bit && !div_bit {
            self.frame_sequencer_step = (self.frame_sequencer_step + 1) & 7;

            if self.enabled {
                if !self.frame_sequencer_step.bit(0) {
                    self.clock_length_counters();
                }

                if self.frame_sequencer_step == 7 {
                    self.clock_envelopes();
                }

                if self.frame_sequencer_step == 2 || self.frame_sequencer_step == 6 {
                    self.pulse_1.clock_sweep();
                }
            }
        }
        self.previous_div_bit = div_bit;

        if !self.enabled {
            self.resampler.collect_sample(0.0, 0.0);
            return;
        }

        self.pulse_1.tick_m_cycle();
        self.pulse_2.tick_m_cycle();
        self.wavetable.tick_m_cycle();
        self.noise.tick_m_cycle();

        self.generate_sample();
    }

    fn generate_sample(&mut self) {
        // Sample values in the range [-15, +15]
        let channel_1_sample = digital_to_analog(self.pulse_1.sample());
        let channel_2_sample = digital_to_analog(self.pulse_2.sample());
        let channel_3_sample = digital_to_analog(self.wavetable.sample());
        let channel_4_sample = digital_to_analog(self.noise.sample());

        let mut sample_l = 0;
        let mut sample_r = 0;

        sample_l += i32::from(self.stereo_control.left_channels[0]) * channel_1_sample;
        sample_r += i32::from(self.stereo_control.right_channels[0]) * channel_1_sample;

        sample_l += i32::from(self.stereo_control.left_channels[1]) * channel_2_sample;
        sample_r += i32::from(self.stereo_control.right_channels[1]) * channel_2_sample;

        sample_l += i32::from(self.stereo_control.left_channels[2]) * channel_3_sample;
        sample_r += i32::from(self.stereo_control.right_channels[2]) * channel_3_sample;

        sample_l += i32::from(self.stereo_control.left_channels[3]) * channel_4_sample;
        sample_r += i32::from(self.stereo_control.right_channels[3]) * channel_4_sample;

        // L/R samples are in the range [-60, +60] after adding in all 4 channels
        // Volume multiplier is 1-8, so after this they will be in the range [-480, +480]
        sample_l *= i32::from(self.stereo_control.left_volume + 1);
        sample_r *= i32::from(self.stereo_control.right_volume + 1);

        // Convert to floating point and store
        // Multiply by 0.5 because otherwise the sound will be way too loud
        let sample_l_f64 = f64::from(sample_l) / 960.0;
        let sample_r_f64 = f64::from(sample_r) / 960.0;
        self.resampler.collect_sample(sample_l_f64, sample_r_f64);
    }

    fn clock_length_counters(&mut self) {
        self.pulse_1.clock_length_counter();
        self.pulse_2.clock_length_counter();
        self.wavetable.clock_length_counter();
        self.noise.clock_length_counter();
    }

    fn clock_envelopes(&mut self) {
        self.pulse_1.clock_envelope();
        self.pulse_2.clock_envelope();
        self.noise.clock_envelope();
    }

    pub fn read_register(&self, address: u16) -> u8 {
        log::trace!("APU read register {address:04X}");

        match address & 0x7F {
            0x10 => self.pulse_1.read_register_0(),
            0x11 => self.pulse_1.read_register_1(),
            0x12 => self.pulse_1.read_register_2(),
            0x14 => self.pulse_1.read_register_4(),
            0x16 => self.pulse_2.read_register_1(),
            0x17 => self.pulse_2.read_register_2(),
            0x19 => self.pulse_2.read_register_4(),
            0x1A => self.wavetable.read_register_0(),
            0x1C => self.wavetable.read_register_2(),
            0x1E => self.wavetable.read_register_4(),
            0x21 => self.noise.read_register_2(),
            0x22 => self.noise.read_register_3(),
            0x23 => self.noise.read_register_4(),
            0x24 => self.stereo_control.read_volume(),
            0x25 => self.stereo_control.read_enabled(),
            0x26 => self.read_nr52(),
            0x30..=0x3F => self.wavetable.read_ram(address),
            _ => 0xFF,
        }
    }

    fn read_nr52(&self) -> u8 {
        0x70 | (u8::from(self.enabled) << 7)
            | (u8::from(self.noise.enabled()) << 3)
            | (u8::from(self.wavetable.enabled()) << 2)
            | (u8::from(self.pulse_2.enabled()) << 1)
            | u8::from(self.pulse_1.enabled())
    }

    pub fn write_register(&mut self, address: u16, value: u8) {
        log::trace!("APU write register {address:04X} {value:02X}");

        if !self.enabled && address != 0xFF26 && !(0xFF30..0xFF40).contains(&address) {
            // When APU is disabled, writes are only allowed to NR52 and wavetable RAM
            return;
        }

        match address & 0x7F {
            0x10 => self.pulse_1.write_register_0(value),
            0x11 => self.pulse_1.write_register_1(value),
            0x12 => self.pulse_1.write_register_2(value),
            0x13 => self.pulse_1.write_register_3(value),
            0x14 => self.pulse_1.write_register_4(value, self.frame_sequencer_step),
            0x16 => self.pulse_2.write_register_1(value),
            0x17 => self.pulse_2.write_register_2(value),
            0x18 => self.pulse_2.write_register_3(value),
            0x19 => self.pulse_2.write_register_4(value, self.frame_sequencer_step),
            0x1A => self.wavetable.write_register_0(value),
            0x1B => self.wavetable.write_register_1(value),
            0x1C => self.wavetable.write_register_2(value),
            0x1D => self.wavetable.write_register_3(value),
            0x1E => self.wavetable.write_register_4(value, self.frame_sequencer_step),
            0x20 => self.noise.write_register_1(value),
            0x21 => self.noise.write_register_2(value),
            0x22 => self.noise.write_register_3(value),
            0x23 => self.noise.write_register_4(value, self.frame_sequencer_step),
            0x24 => self.stereo_control.write_volume(value),
            0x25 => self.stereo_control.write_enabled(value),
            0x26 => self.write_nr52(value),
            0x30..=0x3F => self.wavetable.write_ram(address, value),
            _ => {}
        }
    }

    fn write_nr52(&mut self, value: u8) {
        let prev_enabled = self.enabled;
        self.enabled = value.bit(7);

        if prev_enabled && !self.enabled {
            // Reset all channel and register state
            self.pulse_1 = PulseChannel::new();
            self.pulse_2 = PulseChannel::new();
            self.wavetable.reset();
            self.noise = NoiseChannel::new();
            self.stereo_control = StereoControl::zero();
        } else if !prev_enabled && self.enabled {
            // Reset frame sequencer step
            self.frame_sequencer_step = 7;
        }

        log::trace!("NR52 write, APU enabled: {}", self.enabled);
    }

    pub fn queued_sample_count(&self) -> u32 {
        self.resampler.output_buffer_len() as u32
    }

    pub fn drain_samples_into<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        self.resampler.output_samples(audio_output)
    }
}

fn digital_to_analog(sample: Option<u8>) -> i32 {
    let Some(sample) = sample else { return 0 };

    // Map 0 to -15 and 15 to +15
    (2 * i32::from(sample)) - 15
}
