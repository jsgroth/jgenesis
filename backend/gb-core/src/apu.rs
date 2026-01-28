//! Game Boy APU (audio processing unit)

pub mod components;
pub mod noise;
pub mod pulse;
mod wavetable;

use crate::HardwareMode;
use crate::api::GameBoyEmulatorConfig;
use crate::apu::noise::NoiseChannel;
use crate::apu::pulse::PulseChannel;
use crate::apu::wavetable::WavetableChannel;
use crate::audio::{GB_APU_FREQUENCY, GameBoyResampler};
use crate::cgb::CpuSpeed;
use crate::timer::GbTimer;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::AudioOutput;
use jgenesis_common::num::GetBit;
use std::array;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct StereoControl {
    pub left_volume: u8,
    pub right_volume: u8,
    pub left_channels: [bool; 4],
    pub right_channels: [bool; 4],
    // Vin functionality is not emulated but some test ROMs depend on these bits being R/W
    vin_bits: u8,
}

impl Default for StereoControl {
    fn default() -> Self {
        Self::new()
    }
}

impl StereoControl {
    #[must_use]
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

    #[must_use]
    pub fn zero() -> Self {
        Self {
            left_volume: 0,
            right_volume: 0,
            vin_bits: 0,
            left_channels: [false; 4],
            right_channels: [false; 4],
        }
    }

    #[must_use]
    pub fn read_volume(&self) -> u8 {
        (self.left_volume << 4) | self.right_volume | self.vin_bits
    }

    pub fn write_volume(&mut self, value: u8) {
        // NR50: Stereo volume controls
        self.left_volume = (value >> 4) & 0x07;
        self.right_volume = value & 0x07;
        self.vin_bits = value & 0x88;

        log::trace!("NR50 write");
        log::trace!("  L volume: {}", self.left_volume);
        log::trace!("  R volume: {}", self.right_volume);
    }

    #[must_use]
    pub fn read_enabled(&self) -> u8 {
        let high_nibble = stereo_channels_to_nibble(self.left_channels);
        let low_nibble = stereo_channels_to_nibble(self.right_channels);
        (high_nibble << 4) | low_nibble
    }

    pub fn write_enabled(&mut self, value: u8) {
        // NR51: Stereo panning controls
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
struct Dac {
    analog_sample: f64,
    output_level: f64,
}

impl Dac {
    fn new() -> Self {
        Self { analog_sample: 0.0, output_level: 0.0 }
    }

    fn digital_to_analog(&mut self, sample: Option<u8>) -> f64 {
        // When the DAC is enabled or disabled, gradually fade in/out over a very short period.
        // Some games depend on this to avoid buzzing due to how they use the wavetable channel, e.g. Cannon Fodder
        // 1/20000th of a second period is based on what SameBoy does; I think in actual hardware this can vary
        // because it's an emergent property of the hardware, not an intentional audio effect
        const FADE_DELTA: f64 = 20000.0 / GB_APU_FREQUENCY;

        match sample {
            Some(sample) => {
                // Convert from digital [0, 15] to analog [-1, +1] but inverted
                //   Digital 0  -> Analog +1
                //   Digital 15 -> Analog -1
                self.analog_sample = (f64::from(15 - sample) - 7.5) / 7.5;

                // Gradually fade in if DAC was just enabled
                self.output_level = (self.output_level + FADE_DELTA).clamp(0.0, 1.0);
            }
            None => {
                // DAC is disabled; gradually fade out, keep current analog sample output
                self.output_level = (self.output_level - FADE_DELTA).clamp(0.0, 1.0);
            }
        }

        self.analog_sample * self.output_level
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Apu {
    hardware_mode: HardwareMode,
    enabled: bool,
    pulse_1: PulseChannel,
    pulse_2: PulseChannel,
    wavetable: WavetableChannel,
    noise: NoiseChannel,
    stereo_control: StereoControl,
    dacs: [Dac; 4],
    frame_sequencer_step: u8,
    previous_div_bit: bool,
    resampler: GameBoyResampler,
}

impl Apu {
    pub fn new(config: GameBoyEmulatorConfig, hardware_mode: HardwareMode) -> Self {
        Self {
            hardware_mode,
            enabled: true,
            pulse_1: PulseChannel::new(),
            pulse_2: PulseChannel::new(),
            wavetable: WavetableChannel::new(hardware_mode),
            noise: NoiseChannel::new(),
            stereo_control: StereoControl::new(),
            dacs: array::from_fn(|_| Dac::new()),
            frame_sequencer_step: 0,
            previous_div_bit: false,
            resampler: GameBoyResampler::new(&config),
        }
    }

    pub fn tick_m_cycle(&mut self, timer: &GbTimer, cpu_speed: CpuSpeed) {
        // In CGB double speed mode, the DIV-APU counter reads DIV bit 5 instead of 4 so that it
        // continues to tick at 512 Hz instead of running twice as fast
        let div_bit_index = match cpu_speed {
            CpuSpeed::Normal => 4,
            CpuSpeed::Double => 5,
        };

        let div_bit = timer.read_div().bit(div_bit_index);
        if self.previous_div_bit && !div_bit {
            // Clock frame sequencer
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
            // If APU is disabled, output constant 0s
            self.resampler.collect_sample(0.0, 0.0);
            return;
        }

        self.pulse_1.tick_m_cycle();
        self.pulse_2.tick_m_cycle();
        self.noise.tick_m_cycle();

        for _ in 0..2 {
            self.wavetable.tick_2mhz();
            self.generate_sample();
        }
    }

    fn generate_sample(&mut self) {
        // Analog samples in range [-1, +1]
        let channel_samples = [
            self.dacs[0].digital_to_analog(self.pulse_1.sample()),
            self.dacs[1].digital_to_analog(self.pulse_2.sample()),
            self.dacs[2].digital_to_analog(self.wavetable.sample()),
            self.dacs[3].digital_to_analog(self.noise.sample()),
        ];

        // Sum channel samples; now in range [-4, +4]
        let mut sample_l = (0..4)
            .map(|i| channel_samples[i] * f64::from(self.stereo_control.left_channels[i]))
            .sum::<f64>();
        let mut sample_r = (0..4)
            .map(|i| channel_samples[i] * f64::from(self.stereo_control.right_channels[i]))
            .sum::<f64>();

        // Apply volume multiplier (1-8); now in range [-32, +32]
        sample_l *= f64::from(self.stereo_control.left_volume + 1);
        sample_r *= f64::from(self.stereo_control.right_volume + 1);

        // Normalize back from [-32, +32] to [-1, +1] range
        // Additionally multiply by 0.5 because otherwise sound is way too loud
        sample_l /= 64.0;
        sample_r /= 64.0;

        self.resampler.collect_sample(sample_l, sample_r);
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

    pub fn read_pcm12(&self) -> u8 {
        let ch1_sample = self.pulse_1.sample().unwrap_or(0);
        let ch2_sample = self.pulse_2.sample().unwrap_or(0);
        ch1_sample | (ch2_sample << 4)
    }

    pub fn read_pcm34(&self) -> u8 {
        let ch3_sample = self.wavetable.sample().unwrap_or(0);
        let ch4_sample = self.noise.sample().unwrap_or(0);
        ch3_sample | (ch4_sample << 4)
    }

    pub fn write_register(&mut self, address: u16, value: u8) {
        log::trace!("APU write register {address:04X} {value:02X}");

        if !self.enabled && address != 0xFF26 && !(0xFF30..0xFF40).contains(&address) {
            // When APU is disabled, writes are only allowed to NR52 and wavetable RAM
            // On DMG, writes to length counters are allowed while the APU is disabled
            if self.hardware_mode == HardwareMode::Dmg {
                match address & 0x7F {
                    0x11 => self.pulse_1.write_register_1(value, false),
                    0x16 => self.pulse_2.write_register_1(value, false),
                    0x1B => self.wavetable.write_register_1(value),
                    0x20 => self.noise.write_register_1(value),
                    _ => {}
                }
            }

            return;
        }

        match address & 0x7F {
            0x10 => self.pulse_1.write_register_0(value),
            0x11 => self.pulse_1.write_register_1(value, self.enabled),
            0x12 => self.pulse_1.write_register_2(value),
            0x13 => self.pulse_1.write_register_3(value),
            0x14 => self.pulse_1.write_register_4(value, self.frame_sequencer_step),
            0x16 => self.pulse_2.write_register_1(value, self.enabled),
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
        // NR52: APU control
        let prev_enabled = self.enabled;
        self.enabled = value.bit(7);

        if prev_enabled && !self.enabled {
            // Reset all channel and register state
            self.pulse_1 = PulseChannel::new();
            self.pulse_2 = PulseChannel::new();
            self.wavetable.reset(self.hardware_mode);
            self.noise = NoiseChannel::new();
            self.stereo_control = StereoControl::zero();
        } else if !prev_enabled && self.enabled {
            // Reset frame sequencer step when APU is re-enabled
            self.frame_sequencer_step = 7;
        }

        log::trace!("NR52 write, APU enabled: {}", self.enabled);
    }

    pub fn drain_samples_into<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        self.resampler.output_samples(audio_output)
    }

    pub fn reload_config(&mut self, config: GameBoyEmulatorConfig) {
        self.resampler.reload_config(&config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
    }
}
