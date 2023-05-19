//! Code for Konami's VRC7 board (iNES mapper 85).

use crate::bus;
use crate::bus::cartridge::mappers::konami::irq::VrcIrqCounter;
use crate::bus::cartridge::mappers::{
    konami, BankSizeKb, ChrType, NametableMirroring, PpuMapResult,
};
use crate::bus::cartridge::MapperImpl;
use crate::num::GetBit;
use bincode::{Decode, Encode};
use std::cmp;
use std::ops::{Add, Sub};

const MULTIPLIER_LOOKUP_TABLE: [f64; 16] = [
    0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 10.0, 12.0, 12.0, 15.0, 15.0,
];

// From https://www.nesdev.org/wiki/VRC7_audio#Internal_patch_set
// Indexed into using (instrument # - 1) since 0 is custom instrument
const ROM_PATCHES: [[u8; 8]; 15] = [
    // $01: Buzzy bell
    [0x03, 0x21, 0x05, 0x06, 0xE8, 0x81, 0x42, 0x27],
    // $02: Guitar
    [0x13, 0x41, 0x14, 0x0D, 0xD8, 0xF6, 0x23, 0x12],
    // $02: Wurly
    [0x11, 0x11, 0x08, 0x08, 0xFA, 0xB2, 0x20, 0x12],
    // $04: Flute
    [0x31, 0x61, 0x0C, 0x07, 0xA8, 0x64, 0x61, 0x27],
    // $05: Clarinet
    [0x32, 0x21, 0x1E, 0x06, 0xE1, 0x76, 0x01, 0x28],
    // $06: Synth
    [0x02, 0x01, 0x06, 0x00, 0xA3, 0xE2, 0xF4, 0xF4],
    // $07: Trumpet
    [0x21, 0x61, 0x1D, 0x07, 0x82, 0x81, 0x11, 0x07],
    // $08: Organ
    [0x23, 0x21, 0x22, 0x17, 0xA2, 0x72, 0x01, 0x17],
    // $09: Bells
    [0x35, 0x11, 0x25, 0x00, 0x40, 0x73, 0x72, 0x01],
    // $0A: Vibes
    [0xB5, 0x01, 0x0F, 0x0F, 0xA8, 0xA5, 0x51, 0x02],
    // $0B: Vibraphone
    [0x17, 0xC1, 0x24, 0x07, 0xF8, 0xF8, 0x22, 0x12],
    // $0C: Tutti
    [0x71, 0x23, 0x11, 0x06, 0x65, 0x74, 0x18, 0x16],
    // $0D: Fretless
    [0x01, 0x02, 0xD3, 0x05, 0xC9, 0x95, 0x03, 0x02],
    // $0E: Synth bass
    [0x61, 0x63, 0x0C, 0x00, 0x94, 0xC0, 0x33, 0xF6],
    // $0F: Sweep
    [0x21, 0x72, 0x0D, 0x00, 0xC1, 0xD5, 0x56, 0x06],
];

const KEY_SCALE_ATTENUATION_LOOKUP_TABLE: [f64; 16] = [
    0.0, 18.0, 24.0, 27.75, 30.0, 32.25, 33.75, 35.25, 36.0, 37.5, 38.25, 39.0, 39.75, 40.5, 41.25,
    42.0,
];

// 2^20
const MODULATION_BASE: f64 = 1048576.0;

// 2^17
const WAVE_PHASE_SCALE: f64 = 131072.0;

// 2^18 - 1
const PHASE_MASK: u32 = 0x0003FFFF;

// 2^23 - 1
const ENVELOPE_COUNTER_MASK: u32 = 0x007FFFFF;

// The audio chip has an external oscillator that is almost exactly 2x the NES CPU clock speed, and
// the oscillator clocks the chip every 72 cycles, so clocking the chip every 36 CPU cycles is a
// close enough approximation
const AUDIO_DIVIDER: u8 = 36;

const EPSILON: f64 = 1e-9;

// 2^23 / 48
const ENVELOPE_SCALE: f64 = 174762.66666666666;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Encode, Decode)]
struct Decibels(f64);

impl Decibels {
    const MAX_ATTENUATION: Self = Self(48.0);

    fn from_linear(linear: f64) -> Self {
        Self(-20.0 * linear.log10())
    }

    fn to_linear(self) -> f64 {
        10.0_f64.powf(self.0 / -20.0)
    }
}

impl Add for Decibels {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Decibels {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum FmSynthWaveform {
    Sine,
    ClippedHalfSine,
}

#[derive(Debug, Clone, Encode, Decode)]
struct ModulatorPatch {
    tremolo: bool,
    vibrato: bool,
    sustain_enabled: bool,
    key_rate_scaling: bool,
    multiplier: f64,
    key_level_scaling: u8,
    output_level: u8,
    waveform: FmSynthWaveform,
    feedback_level: u8,
    attack: u8,
    decay: u8,
    sustain: u8,
    release: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
struct CarrierPatch {
    tremolo: bool,
    vibrato: bool,
    sustain_enabled: bool,
    key_rate_scaling: bool,
    multiplier: f64,
    key_level_scaling: u8,
    waveform: FmSynthWaveform,
    attack: u8,
    decay: u8,
    sustain: u8,
    release: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
struct FmSynthPatch {
    modulator: ModulatorPatch,
    carrier: CarrierPatch,
}

impl FmSynthPatch {
    fn from_bytes(bytes: [u8; 8]) -> Self {
        Self {
            modulator: ModulatorPatch {
                tremolo: bytes[0].bit(7),
                vibrato: bytes[0].bit(6),
                sustain_enabled: bytes[0].bit(5),
                key_rate_scaling: bytes[0].bit(4),
                multiplier: MULTIPLIER_LOOKUP_TABLE[(bytes[0] & 0x0F) as usize],
                key_level_scaling: bytes[2] >> 6,
                output_level: bytes[2] & 0x3F,
                waveform: if bytes[3].bit(3) {
                    FmSynthWaveform::ClippedHalfSine
                } else {
                    FmSynthWaveform::Sine
                },
                feedback_level: bytes[3] & 0x07,
                attack: bytes[4] >> 4,
                decay: bytes[4] & 0x0F,
                sustain: bytes[6] >> 4,
                release: bytes[6] & 0x0F,
            },
            carrier: CarrierPatch {
                tremolo: bytes[1].bit(7),
                vibrato: bytes[1].bit(6),
                sustain_enabled: bytes[1].bit(5),
                key_rate_scaling: bytes[1].bit(4),
                multiplier: MULTIPLIER_LOOKUP_TABLE[(bytes[1] & 0x0F) as usize],
                key_level_scaling: bytes[3] >> 6,
                waveform: if bytes[3].bit(4) {
                    FmSynthWaveform::ClippedHalfSine
                } else {
                    FmSynthWaveform::Sine
                },
                attack: bytes[5] >> 4,
                decay: bytes[5] & 0x0F,
                sustain: bytes[7] >> 4,
                release: bytes[7] & 0x0F,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Instrument {
    Custom,
    Fixed(u8),
}

#[derive(Debug, Clone, Encode, Decode)]
struct ChannelControl {
    frequency: u16,
    sustain: bool,
    key_on: bool,
    octave: u8,
    instrument: Instrument,
    attenuation: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum EnvelopeState {
    Attack,
    Decay,
    Sustain,
    Release,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum OperatorType {
    Modulator,
    Carrier,
}

#[derive(Debug, Clone, Encode, Decode)]
struct EnvelopeGenerator {
    counter: u32,
    state: EnvelopeState,
    operator_type: OperatorType,
    sustain_enabled: bool,
    key_rate_scaling: bool,
    attack: u8,
    decay: u8,
    sustain: u8,
    release: u8,
}

impl EnvelopeGenerator {
    fn new() -> Self {
        Self {
            counter: 0,
            state: EnvelopeState::Attack,
            operator_type: OperatorType::Carrier,
            sustain_enabled: false,
            key_rate_scaling: false,
            attack: 0,
            decay: 0,
            sustain: 0,
            release: 0,
        }
    }

    fn clock(&mut self, frequency: u16, octave: u8, channel_sustain_on: bool) {
        let freq_rate = (octave << 1) | (frequency >> 8) as u8;
        let key_scale_offset = if self.key_rate_scaling {
            freq_rate
        } else {
            freq_rate >> 2
        };

        let rate = match self.state {
            EnvelopeState::Attack => self.attack,
            EnvelopeState::Decay => self.decay,
            EnvelopeState::Sustain => {
                if !self.sustain_enabled {
                    self.release
                } else {
                    0
                }
            }
            EnvelopeState::Release => {
                if channel_sustain_on {
                    5
                } else if !self.sustain_enabled {
                    7
                } else {
                    self.release
                }
            }
            EnvelopeState::Idle => 0,
        };

        if rate == 0 {
            return;
        }

        let scaled_rate = (rate << 2) + key_scale_offset;
        let shift = cmp::min(15, scaled_rate >> 2);
        let base = u32::from(scaled_rate & 0x03);

        match self.state {
            EnvelopeState::Attack => {
                self.counter += (12 * (base + 4)) << shift;
                if self.counter > ENVELOPE_COUNTER_MASK {
                    self.counter = 0;
                    self.state = EnvelopeState::Decay;
                }
            }
            EnvelopeState::Decay => {
                self.counter += (base + 4) << shift.saturating_sub(1);

                let sustain_level = 3 * u32::from(self.sustain) * (1 << 23) / 48;
                if self.counter >= sustain_level {
                    self.counter = sustain_level;
                    self.state = EnvelopeState::Sustain;
                }
            }
            EnvelopeState::Release => {
                self.counter += (base + 4) << shift.saturating_sub(1);

                if self.counter >= 1 << 23 {
                    self.counter = 1 << 23;
                    self.state = EnvelopeState::Idle;
                }
            }
            EnvelopeState::Sustain | EnvelopeState::Idle => {}
        }
    }

    fn key_off(&mut self) {
        if self.state == EnvelopeState::Attack {
            self.counter = self.output().0.round() as u32;
        }

        if self.operator_type == OperatorType::Carrier {
            self.state = EnvelopeState::Release;
        }
    }

    fn output(&self) -> Decibels {
        match self.state {
            EnvelopeState::Attack => {
                let volume =
                    Decibels(48.0 * f64::from(self.counter).ln() / f64::from(1 << 23).ln());
                let attenuation = Decibels(48.0) - volume;
                attenuation
            }
            EnvelopeState::Decay | EnvelopeState::Sustain | EnvelopeState::Release => {
                Decibels(f64::from(self.counter) / ENVELOPE_SCALE)
            }
            EnvelopeState::Idle => Decibels::MAX_ATTENUATION,
        }
    }
}

trait WaveGeneratorBehavior: Copy {
    fn adjust_phase(self, phase: u32, current_modulator_output: i32) -> u32;

    fn base_attenuation(self, channel_attenuation: u8) -> Decibels;
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct ModulatorWaveBehavior {
    feedback_level: u8,
    output_level: u8,
}

impl WaveGeneratorBehavior for ModulatorWaveBehavior {
    fn adjust_phase(self, phase: u32, current_modulator_output: i32) -> u32 {
        match self.feedback_level {
            0 => phase,
            _ => {
                ((phase as i32 + (current_modulator_output >> (8 - self.feedback_level))) as u32)
                    & PHASE_MASK
            }
        }
    }

    fn base_attenuation(self, _channel_attenuation: u8) -> Decibels {
        Decibels(0.75 * f64::from(self.output_level))
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct CarrierWaveBehavior;

impl WaveGeneratorBehavior for CarrierWaveBehavior {
    fn adjust_phase(self, phase: u32, current_modulator_output: i32) -> u32 {
        ((phase as i32 + current_modulator_output) as u32) & PHASE_MASK
    }

    fn base_attenuation(self, channel_attenuation: u8) -> Decibels {
        Decibels(3.0 * f64::from(channel_attenuation))
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct WaveGenerator<WaveType> {
    phase_counter: u32,
    adjusted_phase: u32,
    tremolo: bool,
    vibrato: bool,
    waveform: FmSynthWaveform,
    freq_multiplier: f64,
    key_scale_level: u8,
    envelope: EnvelopeGenerator,
    current_output: i32,
    behavior: WaveType,
}

impl<WaveType: WaveGeneratorBehavior> WaveGenerator<WaveType> {
    fn new(behavior: WaveType) -> Self {
        Self {
            phase_counter: 0,
            adjusted_phase: 0,
            tremolo: false,
            vibrato: false,
            waveform: FmSynthWaveform::Sine,
            freq_multiplier: 0.0,
            key_scale_level: 0,
            envelope: EnvelopeGenerator::new(),
            current_output: 0,
            behavior,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn clock(
        &mut self,
        frequency: u16,
        octave: u8,
        channel_sustain_on: bool,
        channel_attenuation: u8,
        am_output: Decibels,
        fm_output: f64,
        modulator_output: i32,
    ) {
        // Frequency
        let fm_multiplier = if self.vibrato { fm_output } else { 1.0 };
        // TODO figure out why dividing by 2 here is necessary and fix the actual bug
        let delta = f64::from(frequency)
            * f64::from(1_u32 << octave)
            * self.freq_multiplier
            * fm_multiplier
            / 2.0;
        self.phase_counter = (self.phase_counter + delta.round() as u32) & PHASE_MASK;
        self.adjusted_phase = self
            .behavior
            .adjust_phase(self.phase_counter, modulator_output);

        // Clock envelope before computing amplitude
        self.envelope.clock(frequency, octave, channel_sustain_on);

        // Amplitude
        // Compute sine using the last 17 bits of phase
        let sin_output = (std::f64::consts::PI * f64::from(self.adjusted_phase & 0x0001FFFF)
            / WAVE_PHASE_SCALE)
            .sin();
        let sin_output_db = Decibels::from_linear(sin_output);

        let base_attenuation = self.behavior.base_attenuation(channel_attenuation);

        let key_scale_attenuation = if self.key_scale_level != 0 {
            let freq_high_bits = frequency >> 5;
            let attenuation = KEY_SCALE_ATTENUATION_LOOKUP_TABLE[freq_high_bits as usize];
            let attenuation = attenuation - 6.0 * f64::from(7 - octave);
            if attenuation <= EPSILON {
                0.0
            } else {
                attenuation / 2.0_f64.powi(3 - i32::from(self.key_scale_level))
            }
        } else {
            0.0
        };
        let key_scale_attenuation = Decibels(key_scale_attenuation);

        let am_additive = if self.tremolo {
            am_output
        } else {
            Decibels(0.0)
        };

        let output_db = sin_output_db
            + base_attenuation
            + key_scale_attenuation
            + self.envelope.output()
            + am_additive;
        // TODO I don't think this conversion is right
        let output_linear = Decibels::from(output_db).to_linear();
        // Clamp to [0, 1]
        let current_output_linear = if output_linear < EPSILON {
            0.0
        } else if output_linear > 1.0 {
            1.0
        } else {
            output_linear
        };

        let scaled_output = (current_output_linear * 2.0_f64.powi(20)).round() as i32;

        let negative_half = self.adjusted_phase.bit(17);
        let negated_output = match (negative_half, self.waveform) {
            (false, _) => scaled_output,
            (true, FmSynthWaveform::Sine) => -scaled_output,
            (true, FmSynthWaveform::ClippedHalfSine) => 0,
        };

        self.current_output = (self.current_output + negated_output) / 2;
    }
}

type Modulator = WaveGenerator<ModulatorWaveBehavior>;
type Carrier = WaveGenerator<CarrierWaveBehavior>;

#[derive(Debug, Clone, Encode, Decode)]
struct AmplitudeModulationUnit {
    // 20-bit counter
    counter: u32,
}

impl AmplitudeModulationUnit {
    const RATE: u32 = 78;

    fn clock(&mut self) {
        self.counter = (self.counter + Self::RATE) & 0x000FFFFF;
    }

    fn output(&self) -> Decibels {
        Decibels((1.0 + scaled_sin(self.counter.into(), MODULATION_BASE)) * 0.6)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct FrequencyModulationUnit {
    // 20-bit counter
    counter: u32,
}

impl FrequencyModulationUnit {
    const RATE: u32 = 105;

    fn clock(&mut self) {
        self.counter = (self.counter + Self::RATE) & 0x000FFFFF;
    }

    fn output(&self) -> f64 {
        2.0_f64.powf(13.75 / 1200.0 * scaled_sin(self.counter.into(), MODULATION_BASE))
    }
}

fn scaled_sin(value: f64, base: f64) -> f64 {
    (2.0 * std::f64::consts::PI * value / base).sin()
}

#[derive(Debug, Clone, Encode, Decode)]
struct FmSynthChannel {
    modulator: Modulator,
    carrier: Carrier,
    control: ChannelControl,
}

impl FmSynthChannel {
    fn new() -> Self {
        Self {
            modulator: Modulator::new(ModulatorWaveBehavior {
                output_level: 0,
                feedback_level: 0,
            }),
            carrier: Carrier::new(CarrierWaveBehavior),
            control: ChannelControl {
                frequency: 0,
                sustain: false,
                key_on: false,
                octave: 0,
                instrument: Instrument::Custom,
                attenuation: 0,
            },
        }
    }

    fn handle_register_1_write(&mut self, value: u8) {
        // All 8 bits are the lowest 8 bits of frequency
        self.control.frequency = (self.control.frequency & 0xFF00) | u16::from(value);
    }

    fn handle_register_2_write(&mut self, value: u8, custom_instrument: [u8; 8]) {
        // Bits 7-6: Unused
        // Bit 5: Sustain
        // Bit 4: Key on/off
        // Bits 3-1: Octave
        // Bit 0: Highest bit of frequency
        self.control.sustain = value.bit(5);
        self.control.octave = (value & 0x0E) >> 1;
        self.control.frequency = (self.control.frequency & 0x00FF) | (u16::from(value & 0x01) << 8);

        let key_on = value.bit(4);
        match (self.control.key_on, key_on) {
            (false, true) => {
                let patch = match self.control.instrument {
                    Instrument::Custom => FmSynthPatch::from_bytes(custom_instrument),
                    Instrument::Fixed(index) => {
                        FmSynthPatch::from_bytes(ROM_PATCHES[index as usize])
                    }
                };

                self.modulator.envelope = EnvelopeGenerator {
                    counter: 0,
                    state: EnvelopeState::Attack,
                    operator_type: OperatorType::Modulator,
                    sustain_enabled: patch.modulator.sustain_enabled,
                    key_rate_scaling: patch.modulator.key_rate_scaling,
                    attack: patch.modulator.attack,
                    decay: patch.modulator.decay,
                    sustain: patch.modulator.sustain,
                    release: patch.modulator.release,
                };
                self.carrier.envelope = EnvelopeGenerator {
                    counter: 0,
                    state: EnvelopeState::Attack,
                    operator_type: OperatorType::Carrier,
                    sustain_enabled: patch.carrier.sustain_enabled,
                    key_rate_scaling: patch.carrier.key_rate_scaling,
                    attack: patch.carrier.attack,
                    decay: patch.carrier.decay,
                    sustain: patch.carrier.sustain,
                    release: patch.carrier.release,
                };

                self.modulator.waveform = patch.modulator.waveform;
                self.modulator.tremolo = patch.modulator.tremolo;
                self.modulator.vibrato = patch.modulator.vibrato;
                self.modulator.freq_multiplier = patch.modulator.multiplier;
                self.modulator.key_scale_level = patch.modulator.key_level_scaling;
                self.modulator.behavior.output_level = patch.modulator.output_level;
                self.modulator.behavior.feedback_level = patch.modulator.feedback_level;

                self.carrier.waveform = patch.carrier.waveform;
                self.carrier.tremolo = patch.carrier.tremolo;
                self.carrier.vibrato = patch.carrier.vibrato;
                self.carrier.freq_multiplier = patch.carrier.multiplier;
                self.carrier.key_scale_level = patch.carrier.key_level_scaling;
            }
            (true, false) => {
                self.modulator.envelope.key_off();
                self.carrier.envelope.key_off();
            }
            (true, true) | (false, false) => {}
        }
        self.control.key_on = key_on;
    }

    fn handle_register_3_write(&mut self, value: u8) {
        // Bits 7-4: Instrument
        // Bits 3-0: Attenuation
        let instrument_index = value >> 4;
        self.control.instrument = match instrument_index {
            0 => Instrument::Custom,
            _ => Instrument::Fixed(instrument_index - 1),
        };
        self.control.attenuation = value & 0x0F;
    }

    fn clock(&mut self, am_output: Decibels, fm_output: f64) {
        self.modulator.clock(
            self.control.frequency,
            self.control.octave,
            self.control.sustain,
            self.control.attenuation,
            am_output,
            fm_output,
            self.modulator.current_output,
        );
        self.carrier.clock(
            self.control.frequency,
            self.control.octave,
            self.control.sustain,
            self.control.attenuation,
            am_output,
            fm_output,
            self.modulator.current_output,
        );
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Vrc7AudioUnit {
    enabled: bool,
    channels: [FmSynthChannel; 6],
    am: AmplitudeModulationUnit,
    fm: FrequencyModulationUnit,
    selected_register: u8,
    custom_instrument_patch: [u8; 8],
    current_output: f64,
    divider: u8,
}

impl Vrc7AudioUnit {
    fn new() -> Self {
        Self {
            enabled: false,
            channels: [(); 6].map(|_| FmSynthChannel::new()),
            am: AmplitudeModulationUnit { counter: 0 },
            fm: FrequencyModulationUnit { counter: 0 },
            selected_register: 0,
            custom_instrument_patch: [0; 8],
            current_output: 0.0,
            divider: AUDIO_DIVIDER,
        }
    }

    fn handle_register_write(&mut self, value: u8) {
        match self.selected_register {
            0x00..=0x07 => {
                self.custom_instrument_patch[self.selected_register as usize] = value;
            }
            0x10..=0x15 => {
                let channel_index = (self.selected_register & 0x07) as usize;
                self.channels[channel_index].handle_register_1_write(value);
            }
            0x20..=0x25 => {
                let channel_index = (self.selected_register & 0x07) as usize;
                self.channels[channel_index]
                    .handle_register_2_write(value, self.custom_instrument_patch);
            }
            0x30..=0x35 => {
                let channel_index = (self.selected_register & 0x07) as usize;
                self.channels[channel_index].handle_register_3_write(value);
            }
            _ => {}
        }
    }

    fn clock(&mut self) {
        if !self.enabled {
            return;
        }

        self.am.clock();
        self.fm.clock();

        let am_output = self.am.output();
        let fm_output = self.fm.output();

        for channel in &mut self.channels {
            channel.clock(am_output, fm_output);
        }

        let output_20_bit = self
            .channels
            .iter()
            .map(|channel| f64::from(channel.carrier.current_output))
            .sum::<f64>()
            / 6.0;
        self.current_output = output_20_bit / f64::from(1 << 20);
    }

    fn tick_cpu(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.clock();
            self.divider = AUDIO_DIVIDER;
        }
    }

    fn sample(&self) -> f64 {
        self.current_output
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Vrc7 {
    variant: Variant,
    prg_bank_0: u8,
    prg_bank_1: u8,
    prg_bank_2: u8,
    chr_banks: [u8; 8],
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
    irq: VrcIrqCounter,
    ram_enabled: bool,
    audio: Vrc7AudioUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Variant {
    Vrc7a,
    Vrc7b,
}

impl Vrc7 {
    pub(crate) fn new(sub_mapper_number: u8, chr_type: ChrType) -> Self {
        let variant = match sub_mapper_number {
            0 | 1 => Variant::Vrc7b,
            2 => Variant::Vrc7a,
            _ => panic!("invalid VRC7 sub mapper: {sub_mapper_number}"),
        };

        log::info!("VRC7 variant: {variant:?}");

        Self {
            variant,
            prg_bank_0: 0,
            prg_bank_1: 0,
            prg_bank_2: 0,
            chr_banks: [0; 8],
            chr_type,
            nametable_mirroring: NametableMirroring::Vertical,
            irq: VrcIrqCounter::new(),
            ram_enabled: false,
            audio: Vrc7AudioUnit::new(),
        }
    }
}

impl MapperImpl<Vrc7> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => bus::cpu_open_bus(address),
            0x6000..=0x7FFF => {
                if self.data.ram_enabled {
                    self.cartridge.get_prg_ram((address & 0x1FFF).into())
                } else {
                    bus::cpu_open_bus(address)
                }
            }
            0x8000..=0x9FFF => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_0, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xA000..=0xBFFF => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_1, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xC000..=0xDFFF => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_2, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xE000..=0xFFFF => {
                let prg_rom_addr = BankSizeKb::Eight
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {
                if self.data.ram_enabled {
                    self.cartridge.set_prg_ram((address & 0x1FFF).into(), value);
                }
            }
            0x8000..=0xFFFF => match (self.data.variant, address) {
                (_, 0x8000) => {
                    self.data.prg_bank_0 = value & 0x3F;
                }
                (Variant::Vrc7a, 0x8010) | (Variant::Vrc7b, 0x8008) => {
                    self.data.prg_bank_1 = value & 0x3F;
                }
                (_, 0x9000) => {
                    self.data.prg_bank_2 = value & 0x3F;
                }
                (Variant::Vrc7a, 0x9010) => {
                    if self.data.audio.enabled {
                        self.data.audio.selected_register = value;
                    }
                }
                (Variant::Vrc7a, 0x9030) => {
                    if self.data.audio.enabled {
                        self.data.audio.handle_register_write(value);
                    }
                }
                (_, 0xA000..=0xD010) => {
                    let address_mask = match self.data.variant {
                        Variant::Vrc7a => 0x0010,
                        Variant::Vrc7b => 0x0008,
                    };
                    let chr_bank_index =
                        2 * ((address - 0xA000) / 0x1000) + u16::from(address & address_mask != 0);
                    self.data.chr_banks[chr_bank_index as usize] = value;
                }
                (_, 0xE000) => {
                    self.data.nametable_mirroring = match value & 0x03 {
                        0x00 => NametableMirroring::Vertical,
                        0x01 => NametableMirroring::Horizontal,
                        0x02 => NametableMirroring::SingleScreenBank0,
                        0x03 => NametableMirroring::SingleScreenBank1,
                        _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                    };
                    self.data.ram_enabled = value.bit(7);

                    self.data.audio.enabled = !value.bit(6);
                    if !self.data.audio.enabled {
                        // Clear all audio state when audio is disabled
                        self.data.audio = Vrc7AudioUnit::new();
                    }
                }
                (Variant::Vrc7a, 0xE010) | (Variant::Vrc7b, 0xE008) => {
                    self.data.irq.set_reload_value(value);
                }
                (_, 0xF000) => {
                    self.data.irq.set_control(value);
                }
                (Variant::Vrc7a, 0xF010) | (Variant::Vrc7b, 0xF008) => {
                    self.data.irq.acknowledge();
                }
                _ => {}
            },
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        konami::map_ppu_address(
            address,
            &self.data.chr_banks,
            self.data.chr_type,
            self.data.nametable_mirroring,
        )
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.irq.tick_cpu();
        self.data.audio.tick_cpu();
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.irq.interrupt_flag()
    }

    pub(crate) fn sample_audio(&self, mixed_apu_sample: f64) -> f64 {
        if !self.data.audio.enabled {
            return mixed_apu_sample;
        }

        let vrc7_sample = self.data.audio.sample();

        // Amplify the VRC7 samples by ~4dB because otherwise this chip is very quiet
        let amplified_sample = vrc7_sample * 1.5848931924611136;
        let clamped_sample = if amplified_sample < -1.0 {
            -1.0
        } else if amplified_sample > 1.0 {
            1.0
        } else {
            amplified_sample
        };

        mixed_apu_sample - clamped_sample
    }
}
