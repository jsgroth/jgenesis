//! Code for handling Sega Master System / Game Gear controller input I/O registers

use crate::SmsGgEmulatorConfig;
use crate::vdp::Vdp;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use smsgg_config::{SmsGgInputs, SmsGgRegion};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PinDirection {
    Input,
    Output(bool),
}

impl PinDirection {
    fn bit(self, joypad_value: bool) -> bool {
        match self {
            Self::Input => joypad_value,
            Self::Output(output_value) => output_value,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    inputs: SmsGgInputs,
    port_a_tr: PinDirection,
    port_a_th: PinDirection,
    port_b_tr: PinDirection,
    port_b_th: PinDirection,
    region: SmsGgRegion,
    reset: bool,
    allow_opposing_directions: bool,
}

impl InputState {
    pub fn new(region: SmsGgRegion, config: &SmsGgEmulatorConfig) -> Self {
        Self {
            inputs: SmsGgInputs::default(),
            port_a_tr: PinDirection::Input,
            port_a_th: PinDirection::Input,
            port_b_tr: PinDirection::Input,
            port_b_th: PinDirection::Input,
            region,
            reset: false,
            allow_opposing_directions: config.allow_opposing_joypad_directions,
        }
    }

    pub fn pause_pressed(&self) -> bool {
        self.inputs.pause
    }

    pub fn set_inputs(&mut self, inputs: SmsGgInputs) {
        self.inputs = inputs;
    }

    pub fn region(&self) -> SmsGgRegion {
        self.region
    }

    pub fn reload_config(&mut self, region: SmsGgRegion, config: &SmsGgEmulatorConfig) {
        self.region = region;
        self.allow_opposing_directions = config.allow_opposing_joypad_directions;
    }

    pub fn set_reset(&mut self, reset: bool) {
        self.reset = reset;
    }

    pub fn write_control(&mut self, value: u8, vdp: &mut Vdp) {
        log::debug!("I/O control write {value:02X}");

        let prev_a_th = self.port_a_th != PinDirection::Output(false);
        let prev_b_th = self.port_b_th != PinDirection::Output(false);

        self.port_b_th =
            if value.bit(3) { PinDirection::Input } else { PinDirection::Output(value.bit(7)) };
        self.port_b_tr =
            if value.bit(2) { PinDirection::Input } else { PinDirection::Output(value.bit(6)) };
        self.port_a_th =
            if value.bit(1) { PinDirection::Input } else { PinDirection::Output(value.bit(5)) };
        self.port_a_tr =
            if value.bit(0) { PinDirection::Input } else { PinDirection::Output(value.bit(4)) };

        if (!prev_a_th && self.port_a_th != PinDirection::Output(false))
            || (!prev_b_th && self.port_b_th != PinDirection::Output(false))
        {
            vdp.latch_h_counter_on_th_change();
        }
    }

    pub fn port_dc(&self) -> u8 {
        let p1_inputs =
            self.inputs.p1.with_allow_opposing_directions(self.allow_opposing_directions);
        let p2_inputs =
            self.inputs.p2.with_allow_opposing_directions(self.allow_opposing_directions);

        let port_a_tr_bit = u8::from(self.port_a_tr.bit(!p1_inputs.button2)) << 5;

        (u8::from(!p2_inputs.down) << 7)
            | (u8::from(!p2_inputs.up) << 6)
            | port_a_tr_bit
            | (u8::from(!p1_inputs.button1) << 4)
            | (u8::from(!p1_inputs.right) << 3)
            | (u8::from(!p1_inputs.left) << 2)
            | (u8::from(!p1_inputs.down) << 1)
            | u8::from(!p1_inputs.up)
    }

    pub fn port_dd(&self) -> u8 {
        let p2_inputs =
            self.inputs.p2.with_allow_opposing_directions(self.allow_opposing_directions);

        let port_b_th_bit =
            u8::from(self.region == SmsGgRegion::International && self.port_b_th.bit(true)) << 7;
        let port_a_th_bit =
            u8::from(self.region == SmsGgRegion::International && self.port_a_th.bit(true)) << 6;
        let port_b_tr_bit = u8::from(self.port_b_tr.bit(!p2_inputs.button2)) << 3;

        port_b_th_bit
            | port_a_th_bit
            | (1 << 5)
            | (u8::from(!self.reset) << 4)
            | port_b_tr_bit
            | (u8::from(!p2_inputs.button1) << 2)
            | (u8::from(!p2_inputs.right) << 1)
            | u8::from(!p2_inputs.left)
    }
}
