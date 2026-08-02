use crate::ym2612::{FrequencyMode, RegisterGroup, Ym2612};
use std::array;

pub struct Ym2612DebugView<'a>(&'a Ym2612);

impl Ym2612 {
    #[must_use]
    pub fn debug_view(&self) -> Ym2612DebugView<'_> {
        Ym2612DebugView(self)
    }
}

#[derive(Debug, Clone)]
pub struct LfoState {
    pub enabled: bool,
    pub frequency: u8,
    pub divider: u8,
}

#[derive(Debug, Clone)]
pub struct TimerState {
    pub enabled: bool,
    pub overflow_flag: bool,
    pub overflow_flag_enabled: bool,
    pub interval: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel3FrequencyMode {
    Normal,
    PerOperator,
    Csm,
}

#[derive(Debug, Clone)]
pub struct GlobalRegisters {
    pub dac_channel_enabled: bool,
    pub dac_channel_sample: u8,
    pub lfo: LfoState,
    pub register_address: u8,
    pub group: RegisterGroup,
    pub timer_a: TimerState,
    pub timer_b: TimerState,
    pub channel_3_frequency_mode: Channel3FrequencyMode,
    pub channel_3_f_numbers: [u16; 3],
    pub channel_3_blocks: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct ChannelRegisters {
    pub f_number: u16,
    pub block: u8,
    pub algorithm: u8,
    pub feedback_level: u8,
    pub am_sensitivity: u8,
    pub fm_sensitivity: u8,
    pub l_output: bool,
    pub r_output: bool,
}

#[derive(Debug, Clone)]
pub struct OperatorRegisters {
    pub key_on: bool,
    pub detune: u8,
    pub multiple: u8,
    pub attack_rate: u8,
    pub decay_rate: u8,
    pub sustain_rate: u8,
    pub release_rate: u8,
    pub total_level: u8,
    pub sustain_level: u8,
    pub key_scale_level: u8,
    pub tremolo_enabled: bool,
    pub ssg_enabled: bool,
    pub ssg_attack: bool,
    pub ssg_alternate: bool,
    pub ssg_hold: bool,
}

impl Ym2612DebugView<'_> {
    #[must_use]
    pub fn global_registers(&self) -> GlobalRegisters {
        GlobalRegisters {
            dac_channel_enabled: self.0.dac_channel_enabled,
            dac_channel_sample: self.0.dac_channel_sample,
            lfo: self.0.lfo.to_debug_state(),
            register_address: self.0.selected_register,
            group: self.0.selected_register_group,
            timer_a: self.0.timer_a.to_debug_state(),
            timer_b: self.0.timer_b.to_debug_state(),
            channel_3_frequency_mode: if self.0.csm_enabled {
                Channel3FrequencyMode::Csm
            } else if self.0.channels[2].mode == FrequencyMode::Multiple {
                Channel3FrequencyMode::PerOperator
            } else {
                Channel3FrequencyMode::Normal
            },
            channel_3_f_numbers: self.0.channels[2].operator_f_numbers,
            channel_3_blocks: self.0.channels[2].operator_blocks,
        }
    }

    #[must_use]
    pub fn channel_registers(&self) -> [ChannelRegisters; 6] {
        array::from_fn(|ch| {
            let channel = &self.0.channels[ch];

            ChannelRegisters {
                f_number: channel.channel_f_number,
                block: channel.channel_block,
                algorithm: channel.algorithm,
                feedback_level: channel.feedback_level,
                am_sensitivity: channel.am_sensitivity,
                fm_sensitivity: channel.fm_sensitivity,
                l_output: channel.l_output,
                r_output: channel.r_output,
            }
        })
    }

    #[must_use]
    pub fn operator_registers(&self) -> [[OperatorRegisters; 4]; 6] {
        array::from_fn(|ch| {
            array::from_fn(|operator| {
                let operator = &self.0.channels[ch].operators[operator];

                OperatorRegisters {
                    key_on: operator.envelope.is_key_on(),
                    detune: operator.phase.detune,
                    multiple: operator.phase.multiple,
                    attack_rate: operator.envelope.attack_rate,
                    decay_rate: operator.envelope.decay_rate,
                    sustain_rate: operator.envelope.sustain_rate,
                    release_rate: operator.envelope.release_rate,
                    total_level: operator.envelope.total_level,
                    sustain_level: operator.envelope.sustain_level,
                    key_scale_level: operator.envelope.key_scale,
                    tremolo_enabled: operator.am_enabled,
                    ssg_enabled: operator.envelope.ssg_enabled,
                    ssg_attack: operator.envelope.ssg_attack,
                    ssg_alternate: operator.envelope.ssg_alternate,
                    ssg_hold: operator.envelope.ssg_hold,
                }
            })
        })
    }
}
