//! Yamaha OPLL FM synthesis sound chip. Used in the YM2413 and the NES VRC7 expansion audio chip
//!
//! This implementation is largely based on reverse engineering work by andete:
//! <https://github.com/andete/ym2413>

use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use std::sync::LazyLock;
use std::{array, cmp};

type FixedPatches = [[u8; 8]; 15];

// Tables from https://www.smspower.org/Development/YM2413ReverseEngineeringNotes2015-03-20
#[rustfmt::skip]
const ENVELOPE_INCREMENT_TABLES: [[u8; 8]; 4] =
    [
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
            }
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
            (-sine.log2() * 256.0).round() as u16
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

fn compute_amplitude(attenuation: u16, sign: Sign) -> i32 {
    let magnitude = exp2_lookup(attenuation);
    match sign {
        Sign::Positive => magnitude.into(),
        Sign::Negative => -i32::from(magnitude),
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Channel {
    fixed_patches: FixedPatches,
    modulator: Operator,
    carrier: Operator,
    settings: ChannelSettings,
    // Used for tom-tom
    modulator_volume_override: Option<u8>,
}

impl Channel {
    fn new(fixed_patches: FixedPatches) -> Self {
        Self {
            fixed_patches,
            modulator: Operator::new(OperatorType::Modulator),
            carrier: Operator::new(OperatorType::Carrier),
            settings: ChannelSettings::default(),
            modulator_volume_override: None,
        }
    }

    fn write_register_1(&mut self, value: u8) {
        self.settings.f_number.set_lsb(value);

        log::trace!("F-number: {:03X}", self.settings.f_number);
    }

    fn write_register_2(&mut self, value: u8) {
        self.settings.f_number.set_msb(value & 0x01);
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
            _ => Instrument::from_patch(self.fixed_patches[(instrument_idx - 1) as usize]),
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

#[derive(Debug, Clone, Default, Encode, Decode)]
struct RhythmSettings {
    snare_drum_volume: u8,
    snare_drum_on: bool,
    tom_tom_volume: u8,
    tom_tom_on: bool,
    top_cymbal_volume: u8,
    top_cymbal_on: bool,
    high_hat_volume: u8,
    high_hat_on: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Opll<const CHANNELS: usize, const RHYTHM: bool> {
    channels: [Channel; CHANNELS],
    rhythm_mode_enabled: bool,
    rhythm_settings: RhythmSettings,
    lfsr: u32,
    am_unit: AmUnit,
    fm_unit: FmUnit,
    selected_register: u8,
    custom_instrument_patch: [u8; 8],
    divider: u8,
    clock_interval: u8,
}

const MAX_CARRIER_OUTPUT: f64 = 255.0;

impl<const CHANNELS: usize, const RHYTHM: bool> Opll<CHANNELS, RHYTHM> {
    fn new(fixed_patches: FixedPatches, clock_interval: u8) -> Self {
        assert_ne!(clock_interval, 0, "OPLL clock interval must be non-zero");

        Self {
            channels: array::from_fn(|_| Channel::new(fixed_patches)),
            rhythm_mode_enabled: false,
            rhythm_settings: RhythmSettings::default(),
            lfsr: 1,
            am_unit: AmUnit::new(),
            fm_unit: FmUnit::new(),
            selected_register: 0,
            custom_instrument_patch: [0; 8],
            divider: clock_interval,
            clock_interval,
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
                let end_idx = if RHYTHM && self.rhythm_mode_enabled { 6 } else { CHANNELS };
                for channel in &mut self.channels[..end_idx] {
                    if channel.settings.instrument == 0 {
                        channel
                            .load_instrument(Instrument::from_patch(self.custom_instrument_patch));
                    }
                }
            }
            0x0E if RHYTHM => {
                self.handle_rhythm_register_write(value);
            }
            register @ 0x10..=0x18 => {
                let channel = register & 0x0F;
                if channel < CHANNELS as u8 {
                    self.channels[channel as usize].write_register_1(value);
                }
            }
            register @ 0x20..=0x28 => {
                let channel = register & 0x0F;
                if channel < CHANNELS as u8 {
                    self.channels[channel as usize].write_register_2(value);
                }
            }
            register @ 0x30..=0x38 => {
                let channel = register & 0x0F;
                if channel < CHANNELS as u8 {
                    self.channels[channel as usize].write_register_3(value);
                }

                if channel < 6 || (RHYTHM && !self.rhythm_mode_enabled) {
                    self.channels[channel as usize].reload_instrument(self.custom_instrument_patch);
                }

                if RHYTHM {
                    // Rhythm volume writes
                    match channel {
                        // No need to special case bass drum volume; it uses channel 6 volume normally
                        7 => {
                            self.rhythm_settings.high_hat_volume = value >> 4;
                            self.rhythm_settings.snare_drum_volume = value & 0x0F;
                        }
                        8 => {
                            let tom_tom_volume = value >> 4;
                            self.rhythm_settings.tom_tom_volume = tom_tom_volume;
                            self.rhythm_settings.top_cymbal_volume = value & 0x0F;

                            if self.rhythm_mode_enabled {
                                self.channels[8].modulator_volume_override =
                                    Some(tom_tom_volume << 3);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_rhythm_register_write(&mut self, value: u8) {
        if !RHYTHM {
            return;
        }

        let rhythm_mode_enabled = value.bit(5);
        if rhythm_mode_enabled != self.rhythm_mode_enabled {
            if rhythm_mode_enabled {
                self.channels[6].load_instrument(Instrument::from_patch(BASS_DRUM_PATCH));
                self.channels[7].load_instrument(Instrument::from_patch(SNARE_DRUM_HIGH_HAT_PATCH));
                self.channels[8].load_instrument(Instrument::from_patch(TOM_TOM_TOP_CYMBAL_PATCH));

                self.channels[8].modulator_volume_override =
                    Some(self.rhythm_settings.tom_tom_volume);
            } else {
                self.channels[6].reload_instrument(self.custom_instrument_patch);
                self.channels[7].reload_instrument(self.custom_instrument_patch);
                self.channels[8].reload_instrument(self.custom_instrument_patch);

                self.channels[8].modulator_volume_override = None;

                // TODO not sure this is right, but it fixes sounds in OutRun
                self.channels[6].set_key_on(false);
                self.channels[7].set_key_on(false);
                self.channels[8].set_key_on(false);
            }
        }
        self.rhythm_mode_enabled = rhythm_mode_enabled;

        log::trace!("  Rhythm mode enabled: {rhythm_mode_enabled}");

        if rhythm_mode_enabled {
            let bass_drum_on = value.bit(4);
            let snare_drum_on = value.bit(3);
            let tom_tom_on = value.bit(2);
            let top_cymbal_on = value.bit(1);
            let high_hat_on = value.bit(0);

            self.channels[6].set_key_on(bass_drum_on);
            self.channels[7].set_key_on(snare_drum_on || high_hat_on);
            self.channels[8].set_key_on(tom_tom_on || top_cymbal_on);

            self.rhythm_settings.snare_drum_on = snare_drum_on;
            self.rhythm_settings.tom_tom_on = tom_tom_on;
            self.rhythm_settings.top_cymbal_on = top_cymbal_on;
            self.rhythm_settings.high_hat_on = high_hat_on;

            log::trace!("  Bass drum on: {}", value.bit(4));
            log::trace!("  Snare drum on: {}", value.bit(3));
            log::trace!("  Tom-tom on: {}", value.bit(2));
            log::trace!("  Top cymbal on: {}", value.bit(1));
            log::trace!("  High hat on: {}", value.bit(0));
        }
    }

    pub fn tick(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = self.clock_interval;
            self.clock();
        }
    }

    fn clock(&mut self) {
        self.am_unit.clock();
        self.fm_unit.clock();
        self.shift_lfsr();

        let am_output = self.am_unit.output();
        let fm_position = self.fm_unit.position;
        for channel in &mut self.channels {
            channel.clock(am_output, fm_position);
        }
    }

    fn shift_lfsr(&mut self) {
        let xor_operand = if self.lfsr.bit(0) {
            // Flip bits 22, 8, 7, and 0
            0x400181
        } else {
            0
        };
        self.lfsr = (self.lfsr >> 1) ^ xor_operand;
    }

    #[must_use]
    pub fn sample(&self) -> f64 {
        let sample = if RHYTHM && self.rhythm_mode_enabled {
            let melodic = self.channels[..6]
                .iter()
                .map(|channel| f64::from(channel.sample()) / MAX_CARRIER_OUTPUT)
                .sum::<f64>();
            let bass_drum = f64::from(self.channels[6].sample()) / MAX_CARRIER_OUTPUT;
            let snare_drum = self.snare_drum_sample();
            let tom_tom = self.tom_tom_sample();
            let top_cymbal = self.top_cymbal_sample();
            let high_hat = self.high_hat_sample();
            melodic + 2.0 * (bass_drum + snare_drum + tom_tom + top_cymbal + high_hat)
        } else {
            self.channels
                .iter()
                .map(|channel| f64::from(channel.sample()) / MAX_CARRIER_OUTPUT)
                .sum::<f64>()
        };

        (sample / CHANNELS as f64).clamp(-1.0, 1.0)
    }

    // Rhythm instrument formulas based on https://github.com/andete/ym2413/blob/master/results/rhythm/rhythm.md

    fn snare_drum_sample(&self) -> f64 {
        let operator = &self.channels[7].carrier;

        if !self.rhythm_settings.snare_drum_on || operator.envelope.attenuation >= ENVELOPE_END {
            return 0.0;
        }

        let phase = operator.phase.counter.bit(18);
        let (sine_attenuation, sign) = match (self.lfsr.bit(0), phase) {
            (false, false) | (true, true) => log_sine_lookup(0),
            (false, true) => (0, Sign::Negative),
            (true, false) => (0, Sign::Positive),
        };

        let total_attenuation = rhythm_attenuation(
            operator.envelope.attenuation,
            self.rhythm_settings.snare_drum_volume,
        );
        let amplitude = compute_amplitude(sine_attenuation + 16 * total_attenuation, sign) >> 4;
        f64::from(amplitude) / MAX_CARRIER_OUTPUT
    }

    fn tom_tom_sample(&self) -> f64 {
        if self.rhythm_settings.tom_tom_on {
            f64::from(self.channels[8].modulator.current_output >> 4) / MAX_CARRIER_OUTPUT
        } else {
            0.0
        }
    }

    fn top_cymbal_sample(&self) -> f64 {
        let operator = &self.channels[8].carrier;

        if !self.rhythm_settings.top_cymbal_on || operator.envelope.attenuation >= ENVELOPE_END {
            return 0.0;
        }

        let sign = if self.top_cymbal_high_hat_phase() { Sign::Positive } else { Sign::Negative };

        let total_attenuation = rhythm_attenuation(
            operator.envelope.attenuation,
            self.rhythm_settings.top_cymbal_volume,
        );
        // Sine attenuation is always 0
        let amplitude = compute_amplitude(16 * total_attenuation, sign) >> 4;
        f64::from(amplitude) / MAX_CARRIER_OUTPUT
    }

    fn high_hat_sample(&self) -> f64 {
        let operator = &self.channels[7].modulator;

        if !self.rhythm_settings.high_hat_on || operator.envelope.attenuation >= ENVELOPE_END {
            return 0.0;
        }

        let phase = match (self.lfsr.bit(0), self.top_cymbal_high_hat_phase()) {
            (false, false) => 0x2D0,
            (false, true) => 0x34,
            (true, false) => 0x234,
            (true, true) => 0xD0,
        };
        let (sine_attenuation, sign) = log_sine_lookup(phase);

        let total_attenuation =
            rhythm_attenuation(operator.envelope.attenuation, self.rhythm_settings.high_hat_volume);
        let amplitude = compute_amplitude(sine_attenuation + 16 * total_attenuation, sign) >> 4;
        f64::from(amplitude) / MAX_CARRIER_OUTPUT
    }

    fn top_cymbal_high_hat_phase(&self) -> bool {
        let c8_phase = self.channels[8].carrier.phase.counter >> 9;
        let m7_phase = self.channels[7].modulator.phase.counter >> 9;

        let c8_3 = c8_phase.bit(3);
        let c8_5 = c8_phase.bit(5);
        let m7_2 = m7_phase.bit(2);
        let m7_3 = m7_phase.bit(3);
        let m7_7 = m7_phase.bit(7);

        (c8_5 ^ c8_3) && (m7_7 ^ m7_2) && (c8_5 ^ m7_3)
    }
}

fn rhythm_attenuation(envelope_attenuation: u8, volume: u8) -> u16 {
    cmp::min(u16::from(MAX_ATTENUATION), u16::from(envelope_attenuation) + u16::from(volume << 3))
}

// YM2413 built-in instrument and rhythm patches from:
//   https://siliconpr0n.org/archive/doku.php?id=vendor:yamaha:opl2#ym2413_instrument_rom
const YM2413_INSTRUMENT_PATCHES: FixedPatches = [
    [0x71, 0x61, 0x1E, 0x17, 0xD0, 0x78, 0x00, 0x17],
    [0x13, 0x41, 0x1A, 0x0D, 0xD8, 0xF7, 0x23, 0x13],
    [0x13, 0x01, 0x99, 0x00, 0xF2, 0xC4, 0x11, 0x23],
    [0x31, 0x61, 0x0E, 0x07, 0xA8, 0x64, 0x70, 0x27],
    [0x32, 0x21, 0x1E, 0x06, 0xE0, 0x76, 0x00, 0x28],
    [0x31, 0x22, 0x16, 0x05, 0xE0, 0x71, 0x00, 0x18],
    [0x21, 0x61, 0x1D, 0x07, 0x82, 0x81, 0x10, 0x07],
    [0x23, 0x21, 0x2D, 0x14, 0xA2, 0x72, 0x00, 0x07],
    [0x61, 0x61, 0x1B, 0x06, 0x64, 0x65, 0x10, 0x17],
    [0x41, 0x61, 0x0B, 0x18, 0x85, 0xF7, 0x71, 0x07],
    [0x13, 0x01, 0x83, 0x11, 0xFA, 0xE4, 0x10, 0x04],
    [0x17, 0xC1, 0x24, 0x07, 0xF8, 0xF8, 0x22, 0x12],
    [0x61, 0x50, 0x0C, 0x05, 0xC2, 0xF5, 0x20, 0x42],
    [0x01, 0x01, 0x55, 0x03, 0xC9, 0x95, 0x03, 0x02],
    [0x61, 0x41, 0x89, 0x03, 0xF1, 0xE4, 0x40, 0x13],
];

const BASS_DRUM_PATCH: [u8; 8] = [0x01, 0x01, 0x18, 0x0F, 0xDF, 0xF8, 0x6A, 0x6D];
const SNARE_DRUM_HIGH_HAT_PATCH: [u8; 8] = [0x01, 0x01, 0x00, 0x00, 0xC8, 0xD8, 0xA7, 0x68];
const TOM_TOM_TOP_CYMBAL_PATCH: [u8; 8] = [0x05, 0x01, 0x00, 0x00, 0xF8, 0xAA, 0x59, 0x55];

// From https://www.nesdev.org/wiki/VRC7_audio#Internal_patch_set
// Indexed into using (instrument # - 1) since 0 is custom instrument
const VRC7_INSTRUMENT_PATCHES: FixedPatches = [
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

pub type Ym2413 = Opll<9, true>;
pub type Vrc7AudioUnit = Opll<6, false>;

#[must_use]
pub fn new_ym2413(clock_interval: u8) -> Ym2413 {
    Ym2413::new(YM2413_INSTRUMENT_PATCHES, clock_interval)
}

#[must_use]
pub fn new_vrc7(clock_interval: u8) -> Vrc7AudioUnit {
    Vrc7AudioUnit::new(VRC7_INSTRUMENT_PATCHES, clock_interval)
}
