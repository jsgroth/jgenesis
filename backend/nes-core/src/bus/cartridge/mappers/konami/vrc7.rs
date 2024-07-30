//! Code for Konami's VRC7 board (iNES mapper 85).
//!
//! This board has a full-blown FM synthesizer chip as expansion audio, and as such the vast
//! majority of this module is dedicated to audio chip emulation. The mapper excluding audio is
//! a bit less complicated than MMC3.
//!
//! The audio chip implementation is almost entirely lifted from the YM2413 implementation in my
//! Sega Master System emulator:
//! <https://github.com/jsgroth/jgenesis/blob/master/smsgg-core/src/ym2413.rs>

use crate::bus;
use crate::bus::cartridge::mappers::konami::irq::VrcIrqCounter;
use crate::bus::cartridge::mappers::{
    konami, BankSizeKb, ChrType, NametableMirroring, PpuMapResult,
};
use crate::bus::cartridge::{HasBasicPpuMapping, MapperImpl};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::sync::LazyLock;
use std::{array, cmp};

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

// Tables from https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-03-20
const ENVELOPE_INCREMENT_TABLES: [[u8; 8]; 4] = [
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
];

// Numbers are multiplied by 2 here - need to divide by 2 after multiplying
const MULTIPLIER_TABLE: [u32; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

// Numbers for key_scale_level=3; need to be shifted down for key_scale_level=1 or 2
const KEY_SCALE_TABLE: [u8; 16] =
    [0, 48, 64, 74, 80, 86, 90, 94, 96, 100, 102, 104, 106, 108, 110, 112];

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct OperatorSettings {
    tremolo: bool,
    vibrato: bool,
    sustained_tone: bool,
    key_scale_rate: bool,
    key_scale_level: u8,
    multiple: u8,
    wave_rectification: bool,
    attack_rate: u8,
    decay_rate: u8,
    sustain_level: u8,
    release_rate: u8,
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct ChannelSettings {
    block: u8,
    f_number: u16,
    sustain: bool,
    instrument: u8,
    volume: u8,
    modulator_feedback_level: u8,
    modulator_total_level: u8,
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct PhaseGenerator {
    counter: u32,
}

const PHASE_COUNTER_MASK: u32 = (1 << 19) - 1;
const PHASE_MASK: u32 = (1 << 10) - 1;

impl PhaseGenerator {
    #[inline]
    fn clock(&mut self, block: u8, f_number: u16, multiple: u8, fm_position: u8, vibrato: bool) {
        let fm_shift = if vibrato { compute_fm_shift(fm_position, f_number) } else { 0 };

        let phase_shift = (((2 * u32::from(f_number) + fm_shift as u32)
            * MULTIPLIER_TABLE[multiple as usize])
            << block)
            >> 2;
        self.counter = self.counter.wrapping_add(phase_shift) & PHASE_COUNTER_MASK;
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum EnvelopePhase {
    Damp,
    Attack,
    Decay,
    Sustain,
    #[default]
    Release,
}

#[derive(Debug, Clone, Encode, Decode)]
struct EnvelopeGenerator {
    operator_type: OperatorType,
    key_on: bool,
    attenuation: u8,
    phase: EnvelopePhase,
    global_counter: u32,
}

const MAX_ATTENUATION: u8 = 127;

impl EnvelopeGenerator {
    fn new(operator_type: OperatorType) -> Self {
        Self {
            operator_type,
            key_on: false,
            attenuation: MAX_ATTENUATION,
            phase: EnvelopePhase::Release,
            global_counter: 0,
        }
    }

    fn set_key_on(&mut self, key_on: bool, sustained_tone: bool) {
        if !self.key_on && key_on {
            self.phase = EnvelopePhase::Damp;
        } else if self.key_on && !key_on {
            self.phase = match (self.operator_type, sustained_tone) {
                (OperatorType::Carrier, _) | (OperatorType::Modulator, false) => {
                    EnvelopePhase::Release
                }
                (OperatorType::Modulator, true) => EnvelopePhase::Sustain,
            };
        }
        self.key_on = key_on;
    }

    fn clock(
        &mut self,
        operator: OperatorSettings,
        channel: ChannelSettings,
        phase_generator: &mut PhaseGenerator,
        modulator: Option<&mut Operator>,
    ) {
        self.global_counter = self.global_counter.wrapping_add(1);

        let sustain_level = operator.sustain_level << 3;
        let rks = compute_rks(channel.block, channel.f_number, operator.key_scale_rate);

        if self.phase == EnvelopePhase::Damp
            && self.attenuation >= ENVELOPE_END
            && self.operator_type == OperatorType::Carrier
        {
            if 4 * operator.attack_rate + rks >= 60 {
                // Skip attack phase if rate is 60-63
                self.attenuation = 0;
                self.phase = EnvelopePhase::Decay;
            } else {
                self.phase = EnvelopePhase::Attack;
            };
            phase_generator.counter = 0;

            if let Some(modulator) = modulator {
                let modulator_rks =
                    compute_rks(channel.block, channel.f_number, modulator.settings.key_scale_rate);
                if 4 * modulator.settings.attack_rate + modulator_rks >= 60 {
                    modulator.envelope.attenuation = 0;
                    modulator.envelope.phase = EnvelopePhase::Decay;
                } else {
                    modulator.envelope.phase = EnvelopePhase::Attack;
                }
                modulator.phase.counter = 0;
            }
        }

        if self.phase == EnvelopePhase::Attack && self.attenuation == 0 {
            self.phase = EnvelopePhase::Decay;
        }

        if self.phase == EnvelopePhase::Decay && self.attenuation >= sustain_level {
            self.phase = EnvelopePhase::Sustain;
        }

        let r = match self.phase {
            EnvelopePhase::Damp => 12,
            EnvelopePhase::Attack => operator.attack_rate,
            EnvelopePhase::Decay => operator.decay_rate,
            EnvelopePhase::Sustain => {
                if operator.sustained_tone {
                    0
                } else {
                    operator.release_rate
                }
            }
            EnvelopePhase::Release => {
                if channel.sustain {
                    5
                } else if !operator.sustained_tone {
                    7
                } else {
                    operator.release_rate
                }
            }
        };

        let rate = if r == 0 { 0 } else { cmp::min(63, 4 * r + rks) };

        // Envelope behaviors from:
        // https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-03-20
        // https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-03-27
        match self.phase {
            EnvelopePhase::Attack => {
                match rate {
                    0..=3 | 60..=63 => {
                        // Do nothing
                    }
                    4..=47 => {
                        let shift = 13 - (rate >> 2);
                        let mask = ((1 << shift) - 1) & !0x03;
                        if self.global_counter & mask == 0 {
                            let table_idx = (rate & 0x03) as usize;
                            let increment_idx = ((self.global_counter >> shift) & 0x07) as usize;
                            let increment = ENVELOPE_INCREMENT_TABLES[table_idx][increment_idx];
                            if increment == 1 {
                                self.attenuation -= (self.attenuation >> 4) + 1;
                            }
                        }
                    }
                    48..=59 => {
                        let table_idx = (rate & 0x03) as usize;
                        let increment_idx = ((self.global_counter >> 1) & 0x06) as usize;
                        let increment = ENVELOPE_INCREMENT_TABLES[table_idx][increment_idx];
                        let shift = 16 - (rate >> 2) - increment;
                        self.attenuation -= (self.attenuation >> shift) + 1;
                    }
                    _ => panic!("rate must be <= 63"),
                }
            }
            EnvelopePhase::Damp
            | EnvelopePhase::Decay
            | EnvelopePhase::Sustain
            | EnvelopePhase::Release => {
                match rate {
                    0..=3 => {
                        // Do nothing
                    }
                    4..=51 => {
                        let shift = 13 - (rate >> 2);
                        if self.global_counter & ((1 << shift) - 1) == 0 {
                            let table_idx = (rate & 0x03) as usize;
                            let increment_idx = ((self.global_counter >> shift) & 0x07) as usize;
                            let increment = ENVELOPE_INCREMENT_TABLES[table_idx][increment_idx];
                            self.attenuation =
                                cmp::min(MAX_ATTENUATION, self.attenuation + increment);
                        }
                    }
                    52..=55 => {
                        // Rates 52-55 increment every clock, and each pair of increments gets
                        // repeated once before moving on to the next pair
                        let table_idx = (rate & 0x03) as usize;
                        let increment_idx = (((self.global_counter >> 1) & 0x06)
                            | (self.global_counter & 0x01))
                            as usize;
                        let increment = ENVELOPE_INCREMENT_TABLES[table_idx][increment_idx];
                        self.attenuation = cmp::min(MAX_ATTENUATION, self.attenuation + increment);
                    }
                    56..=59 => {
                        // Rates 56-59 increment every clock, only use even columns from the table,
                        // and increment by 1 higher than what's in the table
                        let table_idx = (rate & 0x03) as usize;
                        let increment_idx = ((self.global_counter >> 1) & 0x06) as usize;
                        let increment = ENVELOPE_INCREMENT_TABLES[table_idx][increment_idx] + 1;
                        self.attenuation = cmp::min(MAX_ATTENUATION, self.attenuation + increment);
                    }
                    60..=63 => {
                        // Always increment by 2
                        self.attenuation = cmp::min(MAX_ATTENUATION, self.attenuation + 2);
                    }
                    _ => panic!("rate should always be <= 63"),
                }
            }
        }
    }
}

fn compute_rks(block: u8, f_number: u16, key_scale_rate: bool) -> u8 {
    ((block << 1) | u8::from(f_number.bit(8))) >> (2 * u8::from(!key_scale_rate))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum OperatorType {
    Modulator,
    Carrier,
}

#[derive(Debug, Clone, Encode, Decode)]
struct Operator {
    settings: OperatorSettings,
    phase: PhaseGenerator,
    envelope: EnvelopeGenerator,
    current_output: i32,
    prev_output: i32,
}

// Operators start outputting 0 once attenuation is >= 124 (out of 127)
const ENVELOPE_END: u8 = 124;

impl Operator {
    fn new(operator_type: OperatorType) -> Self {
        Self {
            settings: OperatorSettings::default(),
            phase: PhaseGenerator::default(),
            envelope: EnvelopeGenerator::new(operator_type),
            current_output: 0,
            prev_output: 0,
        }
    }

    fn set_key_on(&mut self, key_on: bool) {
        self.envelope.set_key_on(key_on, self.settings.sustained_tone);
    }

    fn clock(
        &mut self,
        channel: ChannelSettings,
        modulation_input: u32,
        base_attenuation: u8,
        am_output: u8,
        fm_position: u8,
        modulator: Option<&mut Operator>,
    ) -> i32 {
        let block = channel.block;
        let f_number = channel.f_number;

        self.phase.clock(
            block,
            f_number,
            self.settings.multiple,
            fm_position,
            self.settings.vibrato,
        );
        self.envelope.clock(self.settings, channel, &mut self.phase, modulator);

        if self.envelope.attenuation >= ENVELOPE_END {
            self.prev_output = self.current_output;
            self.current_output = 0;
            return 0;
        }

        // Phase counter is 19 bits, log-sin table is a 10-bit loookup
        let adjusted_phase = (self.phase.counter >> 9).wrapping_add(modulation_input) & PHASE_MASK;
        let (sine_attenuation, sign) = log_sine_lookup(adjusted_phase);

        let key_scale_level = self.settings.key_scale_level;
        let key_scale_attenuation = if key_scale_level != 0 {
            KEY_SCALE_TABLE[(f_number >> 5) as usize].saturating_sub((7 - block) << 4)
                >> (3 - key_scale_level)
        } else {
            0
        };

        let am_attenuation = if self.settings.tremolo { am_output } else { 0 };

        let total_attenuation = cmp::min(
            u16::from(MAX_ATTENUATION),
            u16::from(base_attenuation)
                + u16::from(key_scale_attenuation)
                + u16::from(self.envelope.attenuation)
                + u16::from(am_attenuation),
        );
        let amplitude_magnitude = exp2_lookup(sine_attenuation + 16 * total_attenuation);

        let amplitude = match (sign, self.settings.wave_rectification) {
            (Sign::Positive, _) => i32::from(amplitude_magnitude),
            (Sign::Negative, false) => -i32::from(amplitude_magnitude),
            (Sign::Negative, true) => 0,
        };

        self.prev_output = self.current_output;
        self.current_output = amplitude;
        amplitude
    }
}

fn compute_fm_shift(fm_position: u8, f_number: u16) -> i16 {
    // Based on https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-12-01
    let f_num_high_bits = f_number >> 6;
    let magnitude = match fm_position & 0x03 {
        0 => 0,
        1 | 3 => (f_num_high_bits >> 1) as i16,
        2 => f_num_high_bits as i16,
        _ => unreachable!("value & 0x03 is always <= 3"),
    };
    let sign = if fm_position.bit(3) { -1 } else { 1 };
    sign * magnitude
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    Positive,
    Negative,
}

// Returns the *attenuation* for the given phase, in log2 decibels units
//   log-sin[i] = -log2(sin((i + 0.5) / 256 * PI/2)) * 256
// Output range is 0..=2137
// Source: https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-04-09
fn log_sine_lookup(phase: u32) -> (u16, Sign) {
    static LOOKUP_TABLE: LazyLock<[(u16, Sign); 1024]> = LazyLock::new(|| {
        let quarter_table: [u16; 256] = array::from_fn(|i| {
            let sine = ((i as f64 + 0.5) / 256.0 * std::f64::consts::PI / 2.0).sin();
            (-1.0 * sine.log2() * 256.0).round() as u16
        });

        array::from_fn(|i| match i {
            0..=255 => (quarter_table[i], Sign::Positive),
            256..=511 => (quarter_table[255 - (i & 0xFF)], Sign::Positive),
            512..=767 => (quarter_table[i & 0xFF], Sign::Negative),
            768..=1023 => (quarter_table[255 - (i & 0xFF)], Sign::Negative),
            _ => unreachable!("array::from_fn with array of size 1024"),
        })
    });

    LOOKUP_TABLE[phase as usize]
}

// Returns a 12-bit unsigned amplitude, assuming the input is an attenuation in log2 decibels units
// Output range is 0..=4084
// Source: https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-04-09
#[allow(clippy::items_after_statements)]
fn exp2_lookup(attenuation: u16) -> u16 {
    let [attenuation_lsb, attenuation_msb] = attenuation.to_le_bytes();

    if attenuation_msb >= 16 {
        return 0;
    }

    static LOOKUP_TABLE: LazyLock<[u16; 256]> = LazyLock::new(|| {
        array::from_fn(|i| (2.0_f64.powf((255 - i) as f64 / 256.0) * 1024.0).round() as u16 - 1024)
    });

    ((LOOKUP_TABLE[attenuation_lsb as usize] + 1024) << 1) >> attenuation_msb
}

#[derive(Debug, Clone, Encode, Decode)]
struct Channel {
    modulator: Operator,
    carrier: Operator,
    settings: ChannelSettings,
    // Used for tom-tom
    modulator_volume_override: Option<u8>,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            modulator: Operator::new(OperatorType::Modulator),
            carrier: Operator::new(OperatorType::Carrier),
            settings: ChannelSettings::default(),
            modulator_volume_override: None,
        }
    }
}

impl Channel {
    fn write_register_1(&mut self, value: u8) {
        self.settings.f_number = (self.settings.f_number & 0xFF00) | u16::from(value);

        log::trace!("F-number: {:03X}", self.settings.f_number);
    }

    fn write_register_2(&mut self, value: u8) {
        self.settings.f_number = (self.settings.f_number & 0x00FF) | (u16::from(value & 0x01) << 8);
        self.settings.block = (value >> 1) & 0x07;
        self.settings.sustain = value.bit(5);

        log::trace!(
            "F-number: {:03X}, Block: {}, Channel Sustain: {}",
            self.settings.f_number,
            self.settings.block,
            self.settings.sustain
        );

        self.set_key_on(value.bit(4));
    }

    fn write_register_3(&mut self, value: u8) {
        self.settings.volume = value & 0x0F;
        self.settings.instrument = value >> 4;

        log::trace!(
            "Volume: {:02X}, Instrument: {}",
            self.settings.volume,
            self.settings.instrument
        );
    }

    fn set_key_on(&mut self, key_on: bool) {
        if self.modulator.envelope.key_on != key_on {
            log::trace!("State at key on ({key_on}): {self:?}");
        }

        self.modulator.set_key_on(key_on);
        self.carrier.set_key_on(key_on);
    }

    fn reload_instrument(&mut self, custom_instrument_patch: [u8; 8]) {
        let instrument_idx = self.settings.instrument;
        let instrument = match instrument_idx {
            0 => Instrument::from_patch(custom_instrument_patch),
            _ => Instrument::from_patch(ROM_PATCHES[(instrument_idx - 1) as usize]),
        };

        self.load_instrument(instrument);
    }

    fn load_instrument(&mut self, instrument: Instrument) {
        self.modulator.settings = instrument.modulator;
        self.carrier.settings = instrument.carrier;
        self.settings.modulator_feedback_level = instrument.modulator_feedback_level;
        self.settings.modulator_total_level = instrument.modulator_total_level;
    }

    fn clock(&mut self, am_output: u8, fm_position: u8) {
        let modulation_feedback = match self.settings.modulator_feedback_level {
            0 => 0,
            feedback_level => {
                (self.modulator.prev_output + self.modulator.current_output) >> (9 - feedback_level)
            }
        };
        let modulator_base_attenuation =
            self.modulator_volume_override.unwrap_or(self.settings.modulator_total_level << 1);
        let modulator_output = self.modulator.clock(
            self.settings,
            modulation_feedback as u32,
            modulator_base_attenuation,
            am_output,
            fm_position,
            None,
        );

        self.carrier.clock(
            self.settings,
            modulator_output as u32,
            self.settings.volume << 3,
            am_output,
            fm_position,
            Some(&mut self.modulator),
        );
    }

    fn sample(&self) -> i32 {
        self.carrier.current_output >> 4
    }
}

struct Instrument {
    modulator: OperatorSettings,
    carrier: OperatorSettings,
    modulator_feedback_level: u8,
    modulator_total_level: u8,
}

impl Instrument {
    fn from_patch(patch: [u8; 8]) -> Self {
        Self {
            modulator: OperatorSettings {
                tremolo: patch[0].bit(7),
                vibrato: patch[0].bit(6),
                sustained_tone: patch[0].bit(5),
                key_scale_rate: patch[0].bit(4),
                key_scale_level: patch[2] >> 6,
                multiple: patch[0] & 0x0F,
                wave_rectification: patch[3].bit(3),
                attack_rate: patch[4] >> 4,
                decay_rate: patch[4] & 0x0F,
                sustain_level: patch[6] >> 4,
                release_rate: patch[6] & 0x0F,
            },
            carrier: OperatorSettings {
                tremolo: patch[1].bit(7),
                vibrato: patch[1].bit(6),
                sustained_tone: patch[1].bit(5),
                key_scale_rate: patch[1].bit(4),
                key_scale_level: patch[3] >> 6,
                multiple: patch[1] & 0x0F,
                wave_rectification: patch[3].bit(4),
                attack_rate: patch[5] >> 4,
                decay_rate: patch[5] & 0x0F,
                sustain_level: patch[7] >> 4,
                release_rate: patch[7] & 0x0F,
            },
            modulator_feedback_level: patch[3] & 0x07,
            modulator_total_level: patch[2] & 0x3F,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct AmUnit {
    position: u8,
    divider: u8,
}

const AM_DIVIDER: u8 = 64;
const AM_POSITIONS: u8 = 210;

impl AmUnit {
    fn new() -> Self {
        Self { position: 0, divider: AM_DIVIDER }
    }

    fn clock(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = AM_DIVIDER;
            self.position = (self.position + 1) % AM_POSITIONS;
        }
    }

    fn output(&self) -> u8 {
        // Based on https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-11-28
        match self.position {
            0..=2 => 0,
            3..=109 => (self.position - 3) >> 3,
            110..=209 => 12 - ((self.position - 110) >> 3),
            _ => panic!("AM position must be <= 209"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct FmUnit {
    position: u8,
    divider: u16,
}

const FM_DIVIDER: u16 = 1024;
const FM_POSITIONS: u8 = 8;

impl FmUnit {
    fn new() -> Self {
        Self { position: 0, divider: FM_DIVIDER }
    }

    fn clock(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = FM_DIVIDER;
            self.position = (self.position + 1) % FM_POSITIONS;
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vrc7AudioUnit {
    enabled: bool,
    channels: [Channel; 6],
    am_unit: AmUnit,
    fm_unit: FmUnit,
    selected_register: u8,
    custom_instrument_patch: [u8; 8],
    divider: u8,
}

// VRC7 has its own oscillator, but the frequency is almost an exact division of the NES CPU clock speed
const AUDIO_DIVIDER: u8 = 36;

const MAX_CARRIER_OUTPUT: f64 = 255.0;

impl Vrc7AudioUnit {
    pub fn new() -> Self {
        Self {
            enabled: false,
            channels: array::from_fn(|_| Channel::default()),
            am_unit: AmUnit::new(),
            fm_unit: FmUnit::new(),
            selected_register: 0,
            custom_instrument_patch: [0; 8],
            divider: AUDIO_DIVIDER,
        }
    }

    pub fn select_register(&mut self, register: u8) {
        self.selected_register = register;
    }

    pub fn write_data(&mut self, value: u8) {
        log::trace!("Write to register {:02X}: {value:02X}", self.selected_register);

        match self.selected_register {
            register @ 0x00..=0x07 => {
                self.custom_instrument_patch[register as usize] = value;

                // Immediately reload any channels using custom instrument
                for channel in &mut self.channels {
                    if channel.settings.instrument == 0 {
                        channel
                            .load_instrument(Instrument::from_patch(self.custom_instrument_patch));
                    }
                }
            }
            register @ 0x10..=0x15 => {
                let channel = register & 0x0F;
                self.channels[channel as usize].write_register_1(value);
            }
            register @ 0x20..=0x25 => {
                let channel = register & 0x0F;
                self.channels[channel as usize].write_register_2(value);
            }
            register @ 0x30..=0x35 => {
                let channel = register & 0x0F;
                self.channels[channel as usize].write_register_3(value);
                self.channels[channel as usize].reload_instrument(self.custom_instrument_patch);
            }
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = AUDIO_DIVIDER;

            self.am_unit.clock();
            self.fm_unit.clock();

            let am_output = self.am_unit.output();
            let fm_position = self.fm_unit.position;
            for channel in &mut self.channels {
                channel.clock(am_output, fm_position);
            }
        }
    }

    pub fn sample(&self) -> f64 {
        let sample = self
            .channels
            .iter()
            .map(|channel| f64::from(channel.sample()) / MAX_CARRIER_OUTPUT)
            .sum::<f64>();

        (sample / 6.0).clamp(-1.0, 1.0)
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
    Unknown,
}

impl Vrc7 {
    pub(crate) fn new(sub_mapper_number: u8, chr_type: ChrType) -> Self {
        let variant = match sub_mapper_number {
            1 => Variant::Vrc7b,
            2 => Variant::Vrc7a,
            0 => Variant::Unknown,
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
                (Variant::Vrc7a | Variant::Unknown, 0x8010)
                | (Variant::Vrc7b | Variant::Unknown, 0x8008) => {
                    self.data.prg_bank_1 = value & 0x3F;
                }
                (_, 0x9000) => {
                    self.data.prg_bank_2 = value & 0x3F;
                }
                (Variant::Vrc7a | Variant::Unknown, 0x9010) => {
                    self.data.audio.select_register(value);
                }
                (Variant::Vrc7a | Variant::Unknown, 0x9030) => {
                    if self.data.audio.enabled {
                        self.data.audio.write_data(value);
                    }
                }
                (_, 0xA000..=0xD010) => {
                    let address_mask = match self.data.variant {
                        Variant::Vrc7a => 0x0010,
                        Variant::Vrc7b => 0x0008,
                        Variant::Unknown => 0x0018,
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
                (Variant::Vrc7a | Variant::Unknown, 0xE010)
                | (Variant::Vrc7b | Variant::Unknown, 0xE008) => {
                    self.data.irq.set_reload_value(value);
                }
                (_, 0xF000) => {
                    self.data.irq.set_control(value);
                }
                (Variant::Vrc7a | Variant::Unknown, 0xF010)
                | (Variant::Vrc7b | Variant::Unknown, 0xF008) => {
                    self.data.irq.acknowledge();
                }
                _ => {}
            },
        }
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.irq.tick_cpu();
        self.data.audio.tick();
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
        let clamped_sample = amplified_sample.clamp(-1.0, 1.0);

        mixed_apu_sample - clamped_sample
    }
}

impl HasBasicPpuMapping for MapperImpl<Vrc7> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        konami::map_ppu_address(
            address,
            &self.data.chr_banks,
            self.data.chr_type,
            self.data.nametable_mirroring,
        )
    }
}
