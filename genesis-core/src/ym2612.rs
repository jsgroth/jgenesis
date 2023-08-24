mod envelope;
mod phase;

use crate::ym2612::envelope::EnvelopeGenerator;
use crate::ym2612::phase::PhaseGenerator;
use smsgg_core::num::GetBit;
use std::array;

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

#[derive(Debug, Clone, Default)]
struct FmOperator {
    phase: PhaseGenerator,
    envelope: EnvelopeGenerator,
    feedback_level: u8,
}

impl FmOperator {
    fn update_frequency(&mut self, f_number: u16, block: u8) {
        self.phase.f_number = f_number;
        self.phase.block = block;
        self.envelope.update_key_scale_rate(f_number, block);
    }

    fn update_key_scale(&mut self, key_scale: u8) {
        self.envelope.key_scale = key_scale;
        self.envelope
            .update_key_scale_rate(self.phase.f_number, self.phase.block);
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

    // TODO optimize to avoid floating-point arithmetic and expensive sin/exp operations
    fn sample(&self, modulation_input: u16) -> i16 {
        let feedback = match self.feedback_level {
            0 => 0,
            feedback_level => {
                ((self.phase.current_phase() + self.phase.last_phase()) >> (7 - feedback_level))
                    & PHASE_MASK
            }
        };

        // Phase represents a value from 0 to 2PI on a scale from 0 to 2^10
        // TODO LFO FM
        let phase = (self.phase.current_phase() + modulation_input + feedback) & PHASE_MASK;
        let sine =
            (f64::from(phase) / f64::from(PHASE_MASK + 1) * 2.0 * std::f64::consts::PI).sin();

        // Envelope attenuation represents a value from 0dB to 96dB on a scale from 0 to 2^10
        let envelope_attenuation = self.envelope.current_attenuation();
        let attenuation_db =
            96.0 * f64::from(envelope_attenuation) / f64::from(envelope::MAX_ATTENUATION + 1);

        // Convert from attenuation in dB to volume in linear units
        let volume = 10.0_f64.powf(attenuation_db / -20.0);
        let amplitude = sine * volume;
        // TODO LFO AM

        // Convert volume to a 14-bit signed integer representing a floating-point value between -1 and 1
        if amplitude >= 0.0 {
            (amplitude * f64::from(0x1FFF)).round() as i16
        } else {
            (amplitude * f64::from(0x2000)).round() as i16
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FrequencyMode {
    #[default]
    Single,
    Multiple,
}

#[derive(Debug, Clone, Default)]
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
}

impl FmChannel {
    #[inline]
    fn fm_clock(&mut self) {
        for operator in &mut self.operators {
            operator.phase.fm_clock();
            operator.envelope.fm_clock();
        }
    }

    fn sample(&self) -> (i16, i16) {
        let sample = match self.algorithm {
            0 => {
                let m1 = compute_modulation_input(self.operators[0].sample(0));
                let m2 = compute_modulation_input(self.operators[1].sample(m1));
                let m3 = compute_modulation_input(self.operators[2].sample(m2));
                self.operators[3].sample(m3)
            }
            1 => {
                let m1 = compute_modulation_input(self.operators[0].sample(0));
                let m2 = compute_modulation_input(self.operators[1].sample(0));
                let m3 = compute_modulation_input(self.operators[2].sample((m1 + m2) & PHASE_MASK));
                self.operators[3].sample(m3)
            }
            2 => {
                let m1 = compute_modulation_input(self.operators[0].sample(0));
                let m2 = compute_modulation_input(self.operators[1].sample(0));
                let m3 = compute_modulation_input(self.operators[2].sample(m2));
                self.operators[3].sample((m1 + m3) & PHASE_MASK)
            }
            3 => {
                let m1 = compute_modulation_input(self.operators[0].sample(0));
                let m2 = compute_modulation_input(self.operators[1].sample(m1));
                let m3 = compute_modulation_input(self.operators[2].sample(0));
                self.operators[3].sample((m2 + m3) & PHASE_MASK)
            }
            4 => {
                let m1 = compute_modulation_input(self.operators[0].sample(0));
                let c1 = self.operators[1].sample(m1);
                let m2 = compute_modulation_input(self.operators[2].sample(0));
                let c2 = self.operators[3].sample(m2);
                (c1 + c2).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            5 => {
                let m1 = compute_modulation_input(self.operators[0].sample(0));
                let c1 = self.operators[1].sample(m1);
                let c2 = self.operators[2].sample(m1);
                let c3 = self.operators[3].sample(m1);
                (c1 + c2 + c3).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            6 => {
                let m1 = compute_modulation_input(self.operators[0].sample(0));
                let c1 = self.operators[1].sample(m1);
                let c2 = self.operators[2].sample(0);
                let c3 = self.operators[3].sample(0);
                (c1 + c2 + c3).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            7 => {
                let c1 = self.operators[0].sample(0);
                let c2 = self.operators[1].sample(0);
                let c3 = self.operators[2].sample(0);
                let c4 = self.operators[3].sample(0);
                (c1 + c2 + c3 + c4).clamp(OPERATOR_OUTPUT_MIN, OPERATOR_OUTPUT_MAX)
            }
            _ => panic!("invalid algorithm: {}", self.algorithm),
        };

        let sample_l = sample * i16::from(self.l_output);
        let sample_r = sample * i16::from(self.r_output);
        (sample_l, sample_r)
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

fn compute_modulation_input(operator_output: i16) -> u16 {
    // Modulation input uses bits 10-1 of the operator output
    ((operator_output as u16) >> 1) & PHASE_MASK
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YmTickEffect {
    None,
    OutputSample,
}

#[derive(Debug, Clone)]
pub struct Ym2612 {
    channels: [FmChannel; 6],
    pcm_enabled: bool,
    pcm_sample: u8,
    lfo_enabled: bool,
    lfo_frequency: u8,
    group_1_register: u8,
    group_2_register: u8,
    clock_divider: u8,
    sample_divider: u8,
}

impl Ym2612 {
    pub fn new() -> Self {
        Self {
            channels: array::from_fn(|_| FmChannel::default()),
            pcm_enabled: false,
            pcm_sample: 0,
            lfo_enabled: false,
            lfo_frequency: 0,
            group_1_register: 0,
            group_2_register: 0,
            clock_divider: FM_CLOCK_DIVIDER,
            sample_divider: FM_SAMPLE_DIVIDER,
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
                self.lfo_enabled = value.bit(3);
                self.lfo_frequency = value & 0x07;

                log::trace!("LFO enabled: {}", self.lfo_enabled);
                log::trace!("LFO frequency: {}", self.lfo_frequency);
            }
            // TODO timer registers: $24-$27
            0x27 => {
                let mode = if value & 0xC0 != 0 {
                    FrequencyMode::Multiple
                } else {
                    FrequencyMode::Single
                };

                // Mode applies to both channel 3 and channel 6
                self.channels[2].mode = mode;
                self.channels[5].mode = mode;

                self.channels[2].update_phase_generators();
                self.channels[5].update_phase_generators();

                log::trace!("Channel 3/6 frequency mode: {mode:?}");
            }
            0x28 => {
                let base_channel = if value.bit(2) {
                    GROUP_2_BASE_CHANNEL
                } else {
                    GROUP_1_BASE_CHANNEL
                };
                let offset = value & 0x03;
                if offset < 3 {
                    let channel_idx = base_channel + (value & 0x03) as usize;
                    let channel = &mut self.channels[channel_idx];
                    channel.operators[0].key_on_or_off(value.bit(4));
                    channel.operators[1].key_on_or_off(value.bit(5));
                    channel.operators[2].key_on_or_off(value.bit(6));
                    channel.operators[3].key_on_or_off(value.bit(7));

                    log::trace!(
                        "Key on/off for channel {}: {:02X}",
                        channel_idx + 1,
                        value >> 4
                    );
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
        // TODO busy bit, maybe timer overflow bits
        0x00
    }

    #[inline]
    pub fn tick(&mut self) -> YmTickEffect {
        if self.clock_divider == 1 {
            self.clock_divider = FM_CLOCK_DIVIDER;
            self.clock();

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
        for channel in &self.channels[..5] {
            let (sample_l, sample_r) = channel.sample();
            sum_l += i32::from(sample_l);
            sum_r += i32::from(sample_r);
        }

        let (ch6_sample_l, ch6_sample_r) = if self.pcm_enabled {
            // Convert unsigned 8-bit sample to a signed 14-bit sample
            let pcm_sample = (i16::from(self.pcm_sample) - 128) << 6;
            (pcm_sample, pcm_sample)
        } else {
            self.channels[5].sample()
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
                operator.envelope.am_enabled = value.bit(7);

                log::trace!(
                    "Decay rate={}, AM enabled={}",
                    operator.envelope.decay_rate,
                    operator.envelope.am_enabled
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

                log::trace!(
                    "Channel {}: F-num={:04X}",
                    channel_idx + 1,
                    channel.channel_f_number
                );
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
    fn clock(&mut self) {
        for channel in &mut self.channels {
            channel.fm_clock();
        }
    }
}
