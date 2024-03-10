//! Code for handling Sega Master System / Game Gear controller input I/O registers

use crate::api::SmsRegion;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::define_controller_inputs;

define_controller_inputs! {
    button_ident: SmsGgButton,
    joypad_ident: SmsGgJoypadState,
    inputs_ident: SmsGgInputs,
    buttons: [Up, Left, Right, Down, Button1, Button2],
    console_buttons: [Pause],
    inputs: {
        p1: (Player One),
        p2: (Player Two),
        pause: (Button Pause),
    },
}

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
    region: SmsRegion,
    reset: bool,
}

impl InputState {
    pub fn new(region: SmsRegion) -> Self {
        Self {
            inputs: SmsGgInputs::default(),
            port_a_tr: PinDirection::Input,
            port_a_th: PinDirection::Input,
            port_b_tr: PinDirection::Input,
            port_b_th: PinDirection::Input,
            region,
            reset: false,
        }
    }

    pub fn pause_pressed(&self) -> bool {
        self.inputs.pause
    }

    pub fn set_inputs(&mut self, inputs: SmsGgInputs) {
        self.inputs = inputs;
    }

    pub fn region(&self) -> SmsRegion {
        self.region
    }

    pub fn set_region(&mut self, region: SmsRegion) {
        self.region = region;
    }

    pub fn set_reset(&mut self, reset: bool) {
        self.reset = reset;
    }

    pub fn write_control(&mut self, value: u8) {
        self.port_b_th =
            if value.bit(3) { PinDirection::Input } else { PinDirection::Output(value.bit(7)) };
        self.port_b_tr =
            if value.bit(2) { PinDirection::Input } else { PinDirection::Output(value.bit(6)) };
        self.port_a_th =
            if value.bit(1) { PinDirection::Input } else { PinDirection::Output(value.bit(5)) };
        self.port_a_tr =
            if value.bit(0) { PinDirection::Input } else { PinDirection::Output(value.bit(4)) };
    }

    pub fn port_dc(&self) -> u8 {
        let port_a_tr_bit = u8::from(self.port_a_tr.bit(!self.inputs.p1.button2)) << 5;

        (u8::from(!self.inputs.p2.down) << 7)
            | (u8::from(!self.inputs.p2.up) << 6)
            | port_a_tr_bit
            | (u8::from(!self.inputs.p1.button1) << 4)
            | (u8::from(!self.inputs.p1.right) << 3)
            | (u8::from(!self.inputs.p1.left) << 2)
            | (u8::from(!self.inputs.p1.down) << 1)
            | u8::from(!self.inputs.p1.up)
    }

    pub fn port_dd(&self) -> u8 {
        let port_b_th_bit =
            u8::from(self.region == SmsRegion::International && self.port_b_th.bit(true)) << 7;
        let port_a_th_bit =
            u8::from(self.region == SmsRegion::International && self.port_a_th.bit(true)) << 6;
        let port_b_tr_bit = u8::from(self.port_b_tr.bit(!self.inputs.p2.button2)) << 3;

        port_b_th_bit
            | port_a_th_bit
            | 0x20
            | (u8::from(!self.reset) << 4)
            | port_b_tr_bit
            | (u8::from(!self.inputs.p2.button1) << 2)
            | (u8::from(!self.inputs.p2.right) << 1)
            | u8::from(!self.inputs.p2.left)
    }
}
