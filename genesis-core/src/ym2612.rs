use smsgg_core::num::GetBit;

#[derive(Debug, Clone, Default)]
struct PhaseGenerator {
    f_number: u16,
    block: u8,
    multiple: u8,
    detune: u8,
}

#[derive(Debug, Clone, Default)]
struct EnvelopeGenerator {
    attack_rate: u8,
    decay_rate: u8,
    sustain_rate: u8,
    release_rate: u8,
    total_level: u8,
    sustain_level: u8,
    am_enabled: bool,
    key_scale: u8,
}

#[derive(Debug, Clone, Default)]
struct FmOperator {
    phase: PhaseGenerator,
    envelope: EnvelopeGenerator,
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
    feedback_level: u8,
    am_sensitivity: u8,
    fm_sensitivity: u8,
    l_output: bool,
    r_output: bool,
}

impl FmChannel {
    // Update phase generator F-numbers & blocks after channel-level F-number, block, or frequency mode is updated
    fn update_phase_generators(&mut self) {
        match self.mode {
            FrequencyMode::Single => {
                let f_number = self.channel_f_number;
                let block = self.channel_block;
                for operator in &mut self.operators {
                    operator.phase.f_number = f_number;
                    operator.phase.block = block;
                }
            }
            FrequencyMode::Multiple => {
                for i in 0..3 {
                    let f_number = self.operator_f_numbers[i];
                    let block = self.operator_blocks[i];

                    let operator = &mut self.operators[i];
                    operator.phase.f_number = f_number;
                    operator.phase.block = block;
                }

                let last_f_number = self.channel_f_number;
                let last_block = self.channel_block;

                let last_operator = &mut self.operators[3];
                last_operator.phase.f_number = last_f_number;
                last_operator.phase.block = last_block;
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Ym2612 {
    channels: [FmChannel; 6],
    lfo_enabled: bool,
    lfo_frequency: u8,
    group_1_register: u8,
    group_2_register: u8,
}

// Group 1 is channels 1-3 (idx 0-2), group 2 is channels 4-6 (idx 3-5)
const GROUP_1_BASE_CHANNEL: usize = 0;
const GROUP_2_BASE_CHANNEL: usize = 3;

impl Ym2612 {
    pub fn new() -> Self {
        Self::default()
    }

    // Set the address register for group 1 (system registers + channels 1-3)
    pub fn write_address_1(&mut self, value: u8) {
        self.group_1_register = value;
    }

    // Write to the data port for group 1 (system registers + channels 1-3)
    pub fn write_data_1(&mut self, value: u8) {
        let register = self.group_1_register;
        match register {
            0x22 => {
                // LFO configuration register
                self.lfo_enabled = value.bit(3);
                self.lfo_frequency = value & 0x07;
            }
            // TODO timer registers: $24-$27
            // TODO key on/off: $28
            // TODO PCM data register: $2A
            // TODO PCM channel enabled: $2B bit 7
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
    #[allow(clippy::unused_self)]
    pub fn write_data_2(&mut self, value: u8) {
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

    fn write_operator_level_register(&mut self, register: u8, value: u8, base_channel_idx: usize) {
        assert!((0x30..=0x9F).contains(&register));

        let channel_offset = register & 0x03;
        if channel_offset == 4 {
            // Invalid; only 3 channels per group
            return;
        }

        let channel_idx = base_channel_idx + channel_offset as usize;
        // Operator comes from bits 2 and 3 of register, except swapped (01=Operator 3, 10=Operator 2)
        let operator_idx = (((register & 0x08) >> 3) | ((register & 0x04) >> 1)) as usize;

        let operator = &mut self.channels[channel_idx].operators[operator_idx];
        match register >> 4 {
            0x03 => {
                operator.phase.multiple = value & 0x0F;
                operator.phase.detune = (value >> 4) & 0x07;
            }
            0x04 => {
                operator.envelope.total_level = value & 0x7F;
            }
            0x05 => {
                operator.envelope.attack_rate = value & 0x1F;
                operator.envelope.key_scale = value >> 6;
            }
            0x06 => {
                operator.envelope.decay_rate = value & 0x1F;
                operator.envelope.am_enabled = value.bit(7);
            }
            0x07 => {
                operator.envelope.sustain_rate = value & 0x1F;
            }
            0x08 => {
                operator.envelope.release_rate = value & 0x0F;
                operator.envelope.sustain_level = value >> 4;
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
            0xA0 | 0xA1 | 0xA2 => {
                // F-number low bits
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.channel_f_number = (channel.channel_f_number & 0xFF00) | u16::from(value);
                channel.update_phase_generators();
            }
            0xA4 | 0xA5 | 0xA6 => {
                // F-number high bits and block
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.channel_f_number =
                    (channel.channel_f_number & 0x00FF) | (u16::from(value & 0x07) << 8);
                channel.channel_block = (value >> 3) & 0x07;
                channel.update_phase_generators();
            }
            0xA8 | 0xA9 | 0xAA => {
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
            }
            0xAC | 0xAD | 0xAE => {
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
            }
            0xB0 | 0xB1 | 0xB2 => {
                // Algorithm and operator 1 feedback level
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.algorithm = value & 0x07;
                channel.feedback_level = (value >> 3) & 0x07;
            }
            0xB4 | 0xB5 | 0xB6 => {
                // Stereo control and LFO sensitivity
                let channel_idx = base_channel_idx + (register & 0x03) as usize;
                let channel = &mut self.channels[channel_idx];
                channel.l_output = value.bit(7);
                channel.r_output = value.bit(6);
                channel.am_sensitivity = (value >> 4) & 0x03;
                channel.fm_sensitivity = value & 0x07;
            }
            _ => {}
        }
    }
}
