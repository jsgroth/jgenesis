mod envelope;
mod lfo;
mod phase;
mod timer;

use crate::ym2612::envelope::EnvelopeGenerator;
use crate::ym2612::lfo::LowFrequencyOscillator;
use crate::ym2612::phase::PhaseGenerator;
use crate::ym2612::timer::{TimerA, TimerB};
use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;
use std::array;
use std::sync::OnceLock;

const FM_CLOCK_DIVIDER: u8 = 6;
const FM_SAMPLE_DIVIDER: u8 = 24;

// Phase is 10 bits
const PHASE_MASK: u16 = 0x03FF;

// Operator output is signed 14-bit
const OPERATOR_OUTPUT_MIN: i16 = -0x2000;
const OPERATOR_OUTPUT_MAX: i16 = 0x1FFF;

// Group 1 is channels 1-3 (idx 0-2), group 2 is channels 4-6 (idx 3-5)
const GROUP_1_BASE_CHANNEL: usize = 0;
const GROUP_2_BASE_CHANNEL: usize = 3;

fn compute_key_code(f_number: u16, block: u8) -> u8 {
    // Bits 4-2: Block
    // Bit 1: F11
    // Bit 0: (F11 & (F10 | F9 | F8)) | (!F11 & F10 & F9 & F8)
    let f11 = f_number.bit(10);
    let f10 = f_number.bit(9);
    let f9 = f_number.bit(8);
    let f8 = f_number.bit(7);
    (block << 2)
        | (u8::from(f11) << 1)
        | u8::from((f11 && (f10 || f9 || f8)) || (!f11 && f10 && f9 && f8))
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct FmOperator {
    phase: PhaseGenerator,
    envelope: EnvelopeGenerator,
    feedback_level: u8,
    am_enabled: bool,
    current_output: i16,
    last_output: i16,
    // Values used in output calculation that are copied here for convenience
    lfo_counter: u8,
    am_sensitivity: u8,
}

impl FmOperator {
    fn update_frequency(&mut self, f_number: u16, block: u8) {
        self.phase.f_number = f_number;
        self.phase.block = block;
        self.envelope.update_key_scale_rate(f_number, block);
    }

    fn update_key_scale(&mut self, key_scale: u8) {
        self.envelope.key_scale = key_scale;
        self.envelope.update_key_scale_rate(self.phase.f_number, self.phase.block);
    }

    fn key_on_or_off(&mut self, value: bool) {
        if value {
            if !self.envelope.is_key_on() {
                self.phase.reset();
                self.envelope.key_on();
            }
        } else {
            self.envelope.key_off();
        }
    }

    // TODO optimize to avoid floating-point arithmetic
    fn sample_clock(&mut self, modulation_input: u16) -> i16 {
        let feedback = match self.feedback_level {
            0 => 0,
            feedback_level => {
                // Feedback is implemented by summing the last 2 operator outputs, shifting from
                // signed 14-bit to signed 10-bit, and then applying a right shift of (6 - feedback_level).
                // This is equivalent to shifting by (10 - feedback_level).
                let feedback_output = self.current_output + self.last_output;
                ((feedback_output >> (10 - feedback_level)) as u16) & PHASE_MASK
            }
        };

        let phase = (self.phase.current_phase() + modulation_input + feedback) & PHASE_MASK;
        let sine = phase_sin(phase);

        let envelope_attenuation = self.envelope.current_attenuation();
        let attenuation = if self.am_enabled {
            let am_attenuation = lfo::amplitude_modulation(self.lfo_counter, self.am_sensitivity);
            (envelope_attenuation + am_attenuation).clamp(0, envelope::MAX_ATTENUATION)
        } else {
            envelope_attenuation
        };

        // Convert from attenuation in dB to volume in linear units
        let volume = attenuation_to_volume(attenuation);
        let amplitude = sine * volume;

        // Convert volume to a 14-bit signed integer representing a floating-point value between -1 and 1
        let output = (amplitude * f64::from(OPERATOR_OUTPUT_MAX)).round() as i16;
        self.last_output = self.current_output;
        self.current_output = output;

        output
    }
}

#[inline]
fn phase_sin(phase: u16) -> f64 {
    static LOOKUP_TABLE: OnceLock<[f64; 1024]> = OnceLock::new();

    // Phase represents a value from 0 to 2PI on a scale from 0 to 2^10
    let lookup_table = LOOKUP_TABLE
        .get_or_init(|| array::from_fn(|i| (i as f64 / 1024.0 * 2.0 * std::f64::consts::PI).sin()));
    lookup_table[phase as usize]
}

#[inline]
fn attenuation_to_volume(attenuation: u16) -> f64 {
    static LOOKUP_TABLE: OnceLock<[f64; 1024]> = OnceLock::new();

    // Envelope attenuation represents a value from 0dB to 96dB on a scale from 0 to 2^10
    let lookup_table = LOOKUP_TABLE.get_or_init(|| {
        array::from_fn(|i| {
            let decibels = 96.0 * i as f64 / 1024.0;
            10.0_f64.powf(decibels / -20.0)
        })
    });
    lookup_table[attenuation as usize]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum FrequencyMode {
    #[default]
    Single,
    Multiple,
}

#[derive(Debug, Clone, Encode, Decode)]
struct FmChannel {
    operators: [FmOperator; 4],
    mode: FrequencyMode,
    channel_f_number: u16,
    channel_block: u8,
    operator_f_numbers: [u16; 3],
    operator_blocks: [u8; 3],
    algorithm: u8,
    am_sensitivity: u8,
    fm_sensitivity: u8,
    l_output: bool,
    r_output: bool,
    divider: u8,
    current_output: (i16, i16),
}

impl FmChannel {
    fn new() -> Self {
        Self {
            operators: array::from_fn(|_| FmOperator::default()),
            mode: FrequencyMode::Single,
            channel_f_number: 0,
            channel_block: 0,
            operator_f_numbers: [0; 3],
            operator_blocks: [0; 3],
            algorithm: 0,
            am_sensitivity: 0,
            fm_sensitivity: 0,
            l_output: false,
            r_output: false,
            divider: FM_SAMPLE_DIVIDER,
            current_output: (0, 0),
        }
    }

    #[inline]
    fn fm_clock(&mut self, lfo_counter: u8) {
        for operator in &mut self.operators {
            operator.phase.fm_clock(lfo_counter, self.fm_sensitivity);
            operator.envelope.fm_clock();

            operator.lfo_counter = lfo_counter;
            operator.am_sensitivity = self.am_sensitivity;
        }

        if self.divider == 1 {
            self.divider = FM_SAMPLE_DIVIDER;
            self.sample_clock();
        } else {
            self.divider -= 1;
        }
    }

    fn sample_clock(&mut self) {
        let sample = match self.algorithm {
            0 => {
                // O1 -> O2 -> O3 -> O4 -> Output
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));
                let m2 = compute_modulation_input(self.operators[1].sample_clock(m1));
                let m3 = compute_modulation_input(self.operators[2].sample_clock(m2));
                self.operators[3].sample_clock(m3)
            }
            1 => {
                // O1 --|
                //      --> O3 -> O4 -> Output
                // O2 --|
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));
                let m2 = compute_modulation_input(self.operators[1].sample_clock(0));
                let m3 = compute_modulation_input(
                    self.operators[2].sample_clock((m1 + m2) & PHASE_MASK),
                );
                self.operators[3].sample_clock(m3)
            }
            2 => {
                //       O1 --|
                //            --> O4 -> Output
                // O2 -> O3 --|
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));
                let m2 = compute_modulation_input(self.operators[1].sample_clock(0));
                let m3 = compute_modulation_input(self.operators[2].sample_clock(m2));
                self.operators[3].sample_clock((m1 + m3) & PHASE_MASK)
            }
            3 => {
                // O1 -> O2 --|
                //            --> O4 -> Output
                //       O3 --|
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));
                let m2 = compute_modulation_input(self.operators[1].sample_clock(m1));
                let m3 = compute_modulation_input(self.operators[2].sample_clock(0));
                self.operators[3].sample_clock((m2 + m3) & PHASE_MASK)
            }
            4 => {
                // O1 -> O2 --|
                //            --> Output
                // O3 -> O4 --|
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));
                let c1 = self.operators[1].sample_clock(m1);
                let m2 = compute_modulation_input(self.operators[2].sample_clock(0));
                let c2 = self.operators[3].sample_clock(m2);
                (c1 + c2).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            5 => {
                //      --> O2 --|
                //      |        |
                // O1 --|-> O3 ----> Output
                //      |        |
                //      --> O4 --|
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));
                let c1 = self.operators[1].sample_clock(m1);
                let c2 = self.operators[2].sample_clock(m1);
                let c3 = self.operators[3].sample_clock(m1);
                (c1 + c2 + c3).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            6 => {
                // O1 --> O2 --|
                //             |
                //        O3 ----> Output
                //             |
                //        O4 --|
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));
                let c1 = self.operators[1].sample_clock(m1);
                let c2 = self.operators[2].sample_clock(0);
                let c3 = self.operators[3].sample_clock(0);
                (c1 + c2 + c3).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            7 => {
                // O1 --|
                //      |
                // O2 --|
                //      --> Output
                // O3 --|
                //      |
                // O4 --|
                let c1 = self.operators[0].sample_clock(0);
                let c2 = self.operators[1].sample_clock(0);
                let c3 = self.operators[2].sample_clock(0);
                let c4 = self.operators[3].sample_clock(0);
                (c1 + c2 + c3 + c4).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            _ => panic!("invalid algorithm: {}", self.algorithm),
        };

        let sample_l = sample * i16::from(self.l_output);
        let sample_r = sample * i16::from(self.r_output);
        self.current_output = (sample_l, sample_r);
    }

    // Update phase generator F-numbers & blocks after channel-level F-number, block, or frequency mode is updated
    fn update_phase_generators(&mut self) {
        match self.mode {
            FrequencyMode::Single => {
                let f_number = self.channel_f_number;
                let block = self.channel_block;
                for operator in &mut self.operators {
                    operator.update_frequency(f_number, block);
                }
            }
            FrequencyMode::Multiple => {
                for i in 0..3 {
                    let f_number = self.operator_f_numbers[i];
                    let block = self.operator_blocks[i];

                    self.operators[i].update_frequency(f_number, block);
                }

                let last_f_number = self.channel_f_number;
                let last_block = self.channel_block;

                self.operators[3].update_frequency(last_f_number, last_block);
            }
        }
    }
}

impl Default for FmChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn compute_modulation_input(operator_output: i16) -> u16 {
    // Modulation input uses bits 10-1 of the operator output
    ((operator_output as u16) >> 1) & PHASE_MASK
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YmTickEffect {
    None,
    OutputSample,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ym2612 {
    channels: [FmChannel; 6],
    pcm_enabled: bool,
    pcm_sample: u8,
    lfo: LowFrequencyOscillator,
    group_1_register: u8,
    group_2_register: u8,
    clock_divider: u8,
    sample_divider: u8,
    timer_a: TimerA,
    timer_b: TimerB,
}

impl Ym2612 {
    pub fn new() -> Self {
        Self {
            channels: array::from_fn(|_| FmChannel::default()),
            pcm_enabled: false,
            pcm_sample: 0,
            lfo: LowFrequencyOscillator::new(),
            group_1_register: 0,
            group_2_register: 0,
            clock_divider: FM_CLOCK_DIVIDER,
            sample_divider: FM_SAMPLE_DIVIDER,
            timer_a: TimerA::new(),
            timer_b: TimerB::new(),
        }
    }

    // Set the address register for group 1 (system registers + channels 1-3)
    pub fn write_address_1(&mut self, value: u8) {
        self.group_1_register = value;
    }

    // Write to the data port for group 1 (system registers + channels 1-3)
    pub fn write_data_1(&mut self, value: u8) {
        if self.group_1_register != 0x2A {
            log::trace!("G1: Wrote {value:02X} to {:02X}", self.group_1_register);
        }

        let register = self.group_1_register;
        match register {
            0x22 => {
                // LFO configuration register
                let lfo_enabled = value.bit(3);
                self.lfo.set_enabled(lfo_enabled);

                let lfo_frequency = value & 0x07;
                self.lfo.set_frequency(lfo_frequency);

                log::trace!("LFO enabled: {}", lfo_enabled);
                log::trace!("LFO frequency: {}", lfo_frequency);
            }
            0x24 => {
                // Timer A interval bits 9-2
                let interval = (self.timer_a.interval() & 0x0003) | (u32::from(value) << 2);
                self.timer_a.set_interval(interval);

                log::trace!("Timer A interval: {}", interval);
            }
            0x25 => {
                // Timer A interval bits 1-0
                let interval = (self.timer_a.interval() & 0xFFFC) | u32::from(value & 0x03);
                self.timer_a.set_interval(interval);

                log::trace!("Timer A interval: {}", interval);
            }
            0x26 => {
                // Timer B interval
                self.timer_b.set_interval(value.into());

                log::trace!("Timer B interval: {}", self.timer_b.interval());
            }
            0x27 => {
                // Channel 3 mode + timer control
                let mode =
                    if value & 0xC0 != 0 { FrequencyMode::Multiple } else { FrequencyMode::Single };

                // Mode applies only to channel 3
                let channel = &mut self.channels[2];
                channel.mode = mode;
                channel.update_phase_generators();

                self.timer_a.set_enabled(value.bit(0));
                self.timer_b.set_enabled(value.bit(1));
                self.timer_a.set_overflow_flag_enabled(value.bit(2));
                self.timer_b.set_overflow_flag_enabled(value.bit(3));

                if value.bit(4) {
                    self.timer_a.clear_overflow_flag();
                }
                if value.bit(5) {
                    self.timer_b.clear_overflow_flag();
                }

                log::trace!("Channel 3 frequency mode: {mode:?}");
                log::trace!("Timer A state: {:?}", self.timer_a);
                log::trace!("Timer B state: {:?}", self.timer_b);
            }
            0x28 => {
                let base_channel =
                    if value.bit(2) { GROUP_2_BASE_CHANNEL } else { GROUP_1_BASE_CHANNEL };
                let offset = value & 0x03;
                if offset < 3 {
                    let channel_idx = base_channel + (value & 0x03) as usize;
                    let channel = &mut self.channels[channel_idx];
                    channel.operators[0].key_on_or_off(value.bit(4));
                    channel.operators[1].key_on_or_off(value.bit(5));
                    channel.operators[2].key_on_or_off(value.bit(6));
                    channel.operators[3].key_on_or_off(value.bit(7));

                    log::trace!("Key on/off for channel {}: {:02X}", channel_idx + 1, value >> 4);
                }
            }
            0x2A => {
                self.pcm_sample = value;
            }
            0x2B => {
                self.pcm_enabled = value.bit(7);
                log::trace!("PCM enabled: {}", self.pcm_enabled);
            }
            0x30..=0x9F => {
                self.write_operator_level_register(register, value, GROUP_1_BASE_CHANNEL);
            }
            0xA0..=0xBF => {
                self.write_channel_level_register(register, value, GROUP_1_BASE_CHANNEL);
            }
            _ => {}
        }
    }

    // Set the address register for group 2 (channels 4-6)
    pub fn write_address_2(&mut self, value: u8) {
        self.group_2_register = value;
    }

    // Write to the data port for group 2 (channels 4-6)
    pub fn write_data_2(&mut self, value: u8) {
        log::trace!("G2: Wrote {value:02X} to {:02X}", self.group_2_register);

        let register = self.group_2_register;
        match register {
            0x30..=0x9F => {
                self.write_operator_level_register(register, value, GROUP_2_BASE_CHANNEL);
            }
            0xA0..=0xBF => {
                self.write_channel_level_register(register, value, GROUP_2_BASE_CHANNEL);
            }
            _ => {}
        }
    }

    #[allow(clippy::unused_self)]
    pub fn read_register(&self) -> u8 {
        // TODO busy bit
        (u8::from(self.timer_b.overflow_flag()) << 1) | u8::from(self.timer_a.overflow_flag())
    }

    #[inline]
    pub fn tick(&mut self) -> YmTickEffect {
        self.lfo.tick();
        self.timer_a.tick();
        self.timer_b.tick();

        if self.clock_divider == 1 {
            self.clock_divider = FM_CLOCK_DIVIDER;
            self.clock(self.lfo.counter());

            self.sample_divider -= 1;
            if self.sample_divider == 0 {
                self.sample_divider = FM_SAMPLE_DIVIDER;
                return YmTickEffect::OutputSample;
            }
        } else {
            self.clock_divider -= 1;
        }

        YmTickEffect::None
    }

    pub fn sample(&self) -> (f64, f64) {
        let mut sum_l = 0;
        let mut sum_r = 0;
        for channel in &self.channels[0..5] {
            let (sample_l, sample_r) = channel.current_output;
            sum_l += i32::from(sample_l);
            sum_r += i32::from(sample_r);
        }

        let (ch6_sample_l, ch6_sample_r) = if self.pcm_enabled {
            // Convert unsigned 8-bit sample to a signed 14-bit sample
            let pcm_sample = (i16::from(self.pcm_sample) - 128) << 6;
            (pcm_sample, pcm_sample)
        } else {
            self.channels[5].current_output
        };
        sum_l += i32::from(ch6_sample_l);
        sum_r += i32::from(ch6_sample_r);

        // Each channel has a range of [-8192, 8191], so divide the sums by 6*8192 to convert to [-1.0, 1.0]
        (f64::from(sum_l) / 49152.0, f64::from(sum_r) / 49152.0)
    }

    fn write_operator_level_register(&mut self, register: u8, value: u8, base_channel_idx: usize) {
        assert!((0x30..=0x9F).contains(&register));

        let channel_offset = register & 0x03;
        if channel_offset == 3 {
            // Invalid; only 3 channels per group
            return;
        }

        let channel_idx = base_channel_idx + channel_offset as usize;
        // Operator comes from bits 2 and 3 of register, except swapped (01=Operator 3, 10=Operator 2)
        let operator_idx = (((register & 0x08) >> 3) | ((register & 0x04) >> 1)) as usize;

        log::trace!(
            "Writing to operator-level register for channel {} / operator {}",
            channel_idx + 1,
            operator_idx + 1
        );

        let operator = &mut self.channels[channel_idx].operators[operator_idx];
        match register >> 4 {
            0x03 => {
                operator.phase.multiple = value & 0x0F;
                operator.phase.detune = (value >> 4) & 0x07;

                log::trace!(
                    "Multiple={}, detune={}",
                    operator.phase.multiple,
                    operator.phase.detune
                );
            }
            0x04 => {
                operator.envelope.total_level = value & 0x7F;

                log::trace!("Total level={:02X}", operator.envelope.total_level);
            }
            0x05 => {
                operator.envelope.attack_rate = value & 0x1F;
                operator.update_key_scale(value >> 6);

                log::trace!(
                    "Attack rate={}, key scale={}, Rks={}",
                    operator.envelope.attack_rate,
                    operator.envelope.key_scale,
                    operator.envelope.key_scale_rate
                );
            }
            0x06 => {
                operator.envelope.decay_rate = value & 0x1F;
                operator.am_enabled = value.bit(7);

                log::trace!(
                    "Decay rate={}, AM enabled={}",
                    operator.envelope.decay_rate,
                    operator.am_enabled
                );
            }
            0x07 => {
                operator.envelope.sustain_rate = value & 0x1F;

                log::trace!("Sustain rate={}", operator.envelope.sustain_rate);
            }
            0x08 => {
                operator.envelope.release_rate = value & 0x0F;
                operator.envelope.sustain_level = value >> 4;

                log::trace!(
                    "Release rate={}, sustain level={}",
                    operator.envelope.release_rate,
                    operator.envelope.sustain_level
                );
            }
            0x09 => {
                // TODO SSG-EG
            }
            _ => unreachable!("register is in 0x30..=0x9F"),
        }
    }

    fn write_channel_level_register(&mut self, register: u8, value: u8, base_channel_idx: usize) {
        assert!((0xA0..=0xBF).contains(&register));

        match register {
            0xA0..=0xA2 => {
                // F-number low bits
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.channel_f_number = (channel.channel_f_number & 0xFF00) | u16::from(value);
                channel.update_phase_generators();

                log::trace!("Channel {}: F-num={:04X}", channel_idx + 1, channel.channel_f_number);
            }
            0xA4..=0xA6 => {
                // F-number high bits and block
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.channel_f_number =
                    (channel.channel_f_number & 0x00FF) | (u16::from(value & 0x07) << 8);
                channel.channel_block = (value >> 3) & 0x07;
                channel.update_phase_generators();

                log::trace!(
                    "Channel {}: F-num={:04X}, block={}",
                    channel_idx + 1,
                    channel.channel_f_number,
                    channel.channel_block
                );
            }
            0xA8..=0xAA => {
                // Operator-level F-number low bits for channels 3 and 6
                let channel_idx = base_channel_idx + 2;
                let operator_idx = match register {
                    0xA8 => 2,
                    0xA9 => 0,
                    0xAA => 1,
                    _ => unreachable!("nested match expressions"),
                };
                let channel = &mut self.channels[channel_idx];
                channel.operator_f_numbers[operator_idx] =
                    (channel.operator_f_numbers[operator_idx] & 0xFF00) | u16::from(value);
                if channel.mode == FrequencyMode::Multiple {
                    channel.update_phase_generators();
                }

                log::trace!(
                    "Set operator-level frequency for channel {} / operator {}: F-num={:04X}",
                    channel_idx + 1,
                    operator_idx + 1,
                    channel.operator_f_numbers[operator_idx]
                );
            }
            0xAC..=0xAE => {
                // Operator-level F-number high bits and block for channels 3 and 6
                let channel_idx = base_channel_idx + 2;
                let operator_idx = match register {
                    0xAC => 2,
                    0xAD => 0,
                    0xAE => 1,
                    _ => unreachable!("nested match expressions"),
                };
                let channel = &mut self.channels[channel_idx];
                channel.operator_f_numbers[operator_idx] =
                    (channel.operator_f_numbers[operator_idx] & 0x00FF)
                        | (u16::from(value & 0x07) << 8);
                channel.operator_blocks[operator_idx] = (value >> 3) & 0x07;
                if channel.mode == FrequencyMode::Multiple {
                    channel.update_phase_generators();
                }

                log::trace!(
                    "Set operator-level frequency / block for channel {} / operator {}: F-num={:04X}, block={}",
                    channel_idx + 1,
                    operator_idx + 1,
                    channel.operator_f_numbers[operator_idx],
                    channel.operator_blocks[operator_idx]
                );
            }
            0xB0..=0xB2 => {
                // Algorithm and operator 1 feedback level
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.algorithm = value & 0x07;
                channel.operators[0].feedback_level = (value >> 3) & 0x07;

                log::trace!(
                    "Channel {}: Algorithm={}, feedback level={}",
                    channel_idx + 1,
                    channel.algorithm,
                    channel.operators[0].feedback_level
                );
            }
            0xB4..=0xB6 => {
                // Stereo control and LFO sensitivity
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.l_output = value.bit(7);
                channel.r_output = value.bit(6);
                channel.am_sensitivity = (value >> 4) & 0x03;
                channel.fm_sensitivity = value & 0x07;

                log::trace!(
                    "Channel {}: L={}, R={}, AM sensitivity={}, FM sensitivity={}",
                    channel_idx + 1,
                    channel.l_output,
                    channel.r_output,
                    channel.am_sensitivity,
                    channel.fm_sensitivity
                );
            }
            _ => {}
        }
    }

    #[inline]
    fn clock(&mut self, lfo_counter: u8) {
        for channel in &mut self.channels {
            channel.fm_clock(lfo_counter);
        }
    }
}
