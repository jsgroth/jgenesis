//! YM2612 FM synthesis sound chip, also known as the OPN2
//!
//! This implementation is mostly based on community research documented here:
//! <http://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386>

mod envelope;
mod lfo;
mod phase;
mod timer;

use crate::GenesisEmulatorConfig;
use crate::ym2612::envelope::EnvelopeGenerator;
use crate::ym2612::lfo::LowFrequencyOscillator;
use crate::ym2612::phase::PhaseGenerator;
use crate::ym2612::timer::{TimerA, TimerB, TimerTickEffect};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::sync::LazyLock;

const FM_CLOCK_DIVIDER: u8 = 6;
const FM_SAMPLE_DIVIDER: u8 = 24;

// Phase is 10 bits
const PHASE_MASK: u16 = 0x03FF;
const HALF_PHASE_MASK: u16 = PHASE_MASK >> 1;

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

        // Phase is a 10-bit value that represents a number in the range 0 to 2*PI.
        // Actual hardware splits this into a sign bit and a half-phase value from 0 to PI, computes
        // the amplitude based on the half-phase, and then applies the sign bit at final output
        let sign = phase.bit(9);
        let sine_attenuation = phase_to_attenuation(phase);

        let envelope_attenuation = self.envelope.current_attenuation();
        let envelope_am_attenuation = if self.am_enabled {
            let am_attenuation = lfo::amplitude_modulation(self.lfo_counter, self.am_sensitivity);
            (envelope_attenuation + am_attenuation).clamp(0, envelope::MAX_ATTENUATION)
        } else {
            envelope_attenuation
        };

        // Add phase attenuation (4.8 fixed-point) and envelope/AM attenuation (4.6 fixed-point)
        let total_attenuation = sine_attenuation + (envelope_am_attenuation << 2);

        // Compute final output, adding the sign bit back in
        let amplitude = attenuation_to_amplitude(total_attenuation);
        let output = if sign { -(amplitude as i16) } else { amplitude as i16 };

        self.last_output = self.current_output;
        self.current_output = output;

        output
    }
}

// Logic based on http://gendev.spritesmind.net/forum/viewtopic.php?p=6114#p6114
#[inline]
fn phase_to_attenuation(phase: u16) -> u16 {
    // Actual hardware has a 256-entry quarter-sine table. This is emulated using a half-sine table
    // for simplicity, but the values are calculated the same way
    static LOG_SINE_TABLE: LazyLock<[u16; 512]> = LazyLock::new(|| {
        array::from_fn(|mut i| {
            use std::f64::consts::PI;

            if i.bit(8) {
                // Second quarter-phase
                i = (!i) & 0xFF;
            }

            // The table indices represent numbers in the range 0 to PI/2, but slightly offset in order
            // to avoid computing log2(0)
            let n = ((i << 1) | 1) as f64;
            let sine = (n / 512.0 * PI / 2.0).sin();

            // The table stores attenuation values, but on a log2 scale instead of log10
            let attenuation = -sine.log2();

            // Table contains 12-bit values that represent 4.8 fixed-point
            (attenuation * f64::from(1 << 8)).round() as u16
        })
    });

    LOG_SINE_TABLE[(phase & HALF_PHASE_MASK) as usize]
}

// Logic based on http://gendev.spritesmind.net/forum/viewtopic.php?p=6114#p6114
#[inline]
fn attenuation_to_amplitude(attenuation: u16) -> u16 {
    static POW2_TABLE: LazyLock<[u16; 256]> = LazyLock::new(|| {
        array::from_fn(|i| {
            // This is a lookup table for 2^(-n), where n is a value between 0 and 1
            // Index i represents the number (i + 1)/256
            let n = ((i + 1) as f64) / 256.0;
            let inverse_pow2 = 2.0_f64.powf(-n);

            // Table contains 11-bit values that represent 0.11 fixed-point
            (inverse_pow2 * f64::from(1 << 11)).round() as u16
        })
    });

    // Attenuation is interpreted as a 5.8 fixed-point number on a log2 scale
    let int_part = (attenuation >> 8) & 0x1F;
    if int_part >= 13 {
        // Final result is guaranteed to shift down to 0
        // Int part is applied as a right shift to 13-bit values
        return 0;
    }

    let fract_part = attenuation & 0xFF;
    let fract_pow2 = POW2_TABLE[fract_part as usize];
    (fract_pow2 << 2) >> int_part
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
    pending_ch_f_number_high: u8,
    channel_f_number: u16,
    pending_ch_block: u8,
    channel_block: u8,
    pending_op_f_numbers_high: [u8; 3],
    operator_f_numbers: [u16; 3],
    pending_op_blocks: [u8; 3],
    operator_blocks: [u8; 3],
    algorithm: u8,
    am_sensitivity: u8,
    fm_sensitivity: u8,
    l_output: bool,
    r_output: bool,
    current_output: (i16, i16),
}

impl FmChannel {
    fn new() -> Self {
        Self {
            operators: array::from_fn(|_| FmOperator::default()),
            mode: FrequencyMode::Single,
            pending_ch_f_number_high: 0,
            channel_f_number: 0,
            pending_ch_block: 0,
            channel_block: 0,
            pending_op_f_numbers_high: [0; 3],
            operator_f_numbers: [0; 3],
            pending_op_blocks: [0; 3],
            operator_blocks: [0; 3],
            algorithm: 0,
            am_sensitivity: 0,
            fm_sensitivity: 0,
            l_output: true,
            r_output: true,
            current_output: (0, 0),
        }
    }

    #[inline]
    fn clock(&mut self, lfo_counter: u8) {
        for operator in &mut self.operators {
            operator.phase.clock(lfo_counter, self.fm_sensitivity);
            operator.envelope.clock(&mut operator.phase);

            operator.lfo_counter = lfo_counter;
            operator.am_sensitivity = self.am_sensitivity;
        }

        self.generate_sample();
    }

    fn generate_sample(&mut self) {
        // Operator order is 1 -> 3 -> 2 -> 4, per http://gendev.spritesmind.net/forum/viewtopic.php?p=30063#p30063
        // This affects output of algorithms 0, 1, and 2
        let sample = match self.algorithm {
            0 => {
                // O1 -> O2 -> O3 -> O4 -> Output
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));

                let m2 = compute_modulation_input(self.operators[1].current_output);
                self.operators[1].sample_clock(m1);

                let m3 = compute_modulation_input(self.operators[2].sample_clock(m2));
                self.operators[3].sample_clock(m3)
            }
            1 => {
                // O1 --|
                //      --> O3 -> O4 -> Output
                // O2 --|
                let m1 = compute_modulation_input(self.operators[0].sample_clock(0));

                let m2 = compute_modulation_input(self.operators[1].current_output);
                self.operators[1].sample_clock(0);

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

                let m2 = compute_modulation_input(self.operators[1].current_output);
                self.operators[1].sample_clock(0);

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

// The YM2612 always raises the BUSY line for exactly 32 internal cycles after a register write
const WRITE_BUSY_CYCLES: u8 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum RegisterGroup {
    // Channel 1-3 and global registers
    #[default]
    One,
    // Channel 4-6 registers
    Two,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ym2612 {
    channels: [FmChannel; 6],
    pcm_enabled: bool,
    pcm_sample: u8,
    lfo: LowFrequencyOscillator,
    selected_register: u8,
    selected_register_group: RegisterGroup,
    clock_divider: u8,
    sample_divider: u8,
    busy_cycles_remaining: u8,
    timer_a: TimerA,
    timer_b: TimerB,
    csm_enabled: bool,
    quantize_output: bool,
    emulate_ladder_effect: bool,
}

impl Ym2612 {
    #[must_use]
    pub fn new(config: GenesisEmulatorConfig) -> Self {
        Self {
            channels: array::from_fn(|_| FmChannel::default()),
            pcm_enabled: false,
            pcm_sample: 0,
            lfo: LowFrequencyOscillator::new(),
            selected_register: 0,
            selected_register_group: RegisterGroup::default(),
            clock_divider: FM_CLOCK_DIVIDER,
            sample_divider: FM_SAMPLE_DIVIDER,
            busy_cycles_remaining: 0,
            timer_a: TimerA::new(),
            timer_b: TimerB::new(),
            csm_enabled: false,
            quantize_output: config.quantize_ym2612_output,
            emulate_ladder_effect: config.emulate_ym2612_ladder_effect,
        }
    }

    pub fn reset(&mut self, config: GenesisEmulatorConfig) {
        *self = Self::new(config);
    }

    // Set the address register and set group to 1 (system registers + channels 1-3)
    pub fn write_address_1(&mut self, value: u8) {
        self.selected_register = value;
        self.selected_register_group = RegisterGroup::One;
    }

    // Set the address register and set group to 2 (channels 4-6)
    pub fn write_address_2(&mut self, value: u8) {
        self.selected_register = value;
        self.selected_register_group = RegisterGroup::Two;
    }

    // Write to the data port
    // Whether this is a group 1 or 2 write depends solely on which address register was last written
    pub fn write_data(&mut self, value: u8) {
        match self.selected_register_group {
            RegisterGroup::One => self.write_group_1_register(value),
            RegisterGroup::Two => self.write_group_2_register(value),
        }
    }

    // Write to the data port for group 1 (system registers + channels 1-3)
    fn write_group_1_register(&mut self, value: u8) {
        if self.selected_register != 0x2A {
            log::trace!("G1: Wrote {value:02X} to {:02X}", self.selected_register);
        }

        self.busy_cycles_remaining = WRITE_BUSY_CYCLES;

        let register = self.selected_register;
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
                self.csm_enabled = value & 0xC0 == 0x80;

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
                log::trace!("CSM enabled: {}", self.csm_enabled);
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

    // Write to the data port for group 2 (channels 4-6)
    fn write_group_2_register(&mut self, value: u8) {
        log::trace!("G2: Wrote {value:02X} to {:02X}", self.selected_register);

        self.busy_cycles_remaining = WRITE_BUSY_CYCLES;

        let register = self.selected_register;
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
    #[must_use]
    pub fn read_register(&self) -> u8 {
        (u8::from(self.busy_cycles_remaining != 0) << 7)
            | (u8::from(self.timer_b.overflow_flag()) << 1)
            | u8::from(self.timer_a.overflow_flag())
    }

    #[inline]
    pub fn tick(&mut self, ticks: u32, mut output: impl FnMut((f64, f64))) {
        for _ in 0..ticks {
            self.lfo.tick();

            let timer_a_effect = self.timer_a.tick();
            self.timer_b.tick();

            if self.csm_enabled && timer_a_effect == TimerTickEffect::Overflowed {
                // CSM: Whenever Timer A overflows, instantaneously key on & off all operators in
                // channel 3 that are not already keyed on
                for operator in &mut self.channels[2].operators {
                    if !operator.envelope.is_key_on() {
                        operator.key_on_or_off(true);
                        operator.key_on_or_off(false);
                    }
                }
            }

            self.clock_divider -= 1;
            if self.clock_divider == 0 {
                self.clock_divider = FM_CLOCK_DIVIDER;
                self.busy_cycles_remaining = self.busy_cycles_remaining.saturating_sub(1);

                self.sample_divider -= 1;
                if self.sample_divider == 0 {
                    self.sample_divider = FM_SAMPLE_DIVIDER;
                    self.clock(self.lfo.counter());
                    output(self.sample());
                }
            }
        }
    }

    #[must_use]
    pub fn sample(&self) -> (f64, f64) {
        let quantization_mask = self.quantization_mask();

        let mut sum_l = 0;
        let mut sum_r = 0;
        for (i, channel) in self.channels.iter().enumerate() {
            let (mut sample_l, mut sample_r) = if i == 5 && self.pcm_enabled {
                // Channel 6 is in DAC mode; play PCM sample instead of FM output
                // Convert unsigned 8-bit sample to a signed 14-bit sample
                let pcm_sample = (i16::from(self.pcm_sample) - 128) << 6;
                (pcm_sample, pcm_sample)
            } else {
                channel.current_output
            };

            sample_l &= quantization_mask;
            sample_r &= quantization_mask;

            sample_l = self.apply_ladder_effect(sample_l);
            sample_r = self.apply_ladder_effect(sample_r);

            sum_l += i32::from(sample_l);
            sum_r += i32::from(sample_r);
        }

        // Each channel has a range of [-8192, 8191], so divide the sums by 6*8192 to convert to [-1.0, 1.0]
        (f64::from(sum_l) / 49152.0, f64::from(sum_r) / 49152.0)
    }

    fn quantization_mask(&self) -> i16 {
        if self.quantize_output {
            // Simulate a 9-bit DAC by masking out the lowest 5 bits of the 14-bit channel outputs
            !((1 << 5) - 1)
        } else {
            !0
        }
    }

    fn apply_ladder_effect(&self, sample: i16) -> i16 {
        if !self.emulate_ladder_effect {
            return sample;
        }

        // The "ladder effect" is a distortion in the YM2612 DAC that effectively amplifies
        // low-volume waves (and has little effect on high-volume waves). A number of games depend
        // on it for their music to sound correct.
        //
        // Emulate the distortion by adding -3 to negative samples and +4 to non-negative samples,
        // shifted left by 5 because the distortion occurs in the 9-bit DAC but these are 14-bit samples
        if sample < 0 { sample - (3 << 5) } else { sample + (4 << 5) }
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
                operator.envelope.write_ssg_register(value);
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

                channel.channel_f_number =
                    u16::from_le_bytes([value, channel.pending_ch_f_number_high]);
                channel.channel_block = channel.pending_ch_block;

                channel.update_phase_generators();

                log::trace!("Channel {}: F-num={:04X}", channel_idx + 1, channel.channel_f_number);
            }
            0xA4..=0xA6 => {
                // F-number high bits and block
                // Writes to this register do not take effect until low bits are written
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.pending_ch_f_number_high = value & 7;
                channel.pending_ch_block = (value >> 3) & 7;

                log::trace!(
                    "Channel {}: F-num high bits {}, block {}",
                    channel_idx + 1,
                    channel.pending_ch_f_number_high,
                    channel.pending_ch_block,
                );
            }
            0xA8..=0xAA => {
                // Operator-level F-number low bits for channel 3
                let channel_idx = base_channel_idx + 2;
                let operator_idx = match register {
                    0xA8 => 2,
                    0xA9 => 0,
                    0xAA => 1,
                    _ => unreachable!("nested match expressions"),
                };
                let channel = &mut self.channels[channel_idx];

                let f_num_high = channel.pending_op_f_numbers_high[operator_idx];
                channel.operator_f_numbers[operator_idx] = u16::from_le_bytes([value, f_num_high]);
                channel.operator_blocks[operator_idx] = channel.pending_op_blocks[operator_idx];
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
                // Operator-level F-number high bits and block for channel 3
                // Writes to this register do not take effect until low bits are written
                let channel_idx = base_channel_idx + 2;
                let operator_idx = match register {
                    0xAC => 2,
                    0xAD => 0,
                    0xAE => 1,
                    _ => unreachable!("nested match expressions"),
                };
                let channel = &mut self.channels[channel_idx];
                channel.pending_op_f_numbers_high[operator_idx] = value & 7;
                channel.pending_op_blocks[operator_idx] = (value >> 3) & 7;

                log::trace!(
                    "Set operator-level frequency / block for channel {} / operator {}: F-num high bits {}, block {}",
                    channel_idx + 1,
                    operator_idx + 1,
                    channel.pending_op_f_numbers_high[operator_idx],
                    channel.pending_op_blocks[operator_idx],
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
            channel.clock(lfo_counter);
        }
    }

    pub fn reload_config(&mut self, config: GenesisEmulatorConfig) {
        self.quantize_output = config.quantize_ym2612_output;
        self.emulate_ladder_effect = config.emulate_ym2612_ladder_effect;
    }
}
