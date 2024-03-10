//! Game Boy input handling

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::define_controller_inputs;

define_controller_inputs! {
    button_ident: GameBoyButton,
    joypad_ident: GameBoyInputs,
    buttons: [Up, Left, Right, Down, A, B, Start, Select],
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct InputState {
    inputs: GameBoyInputs,
    d_pad_selected: bool,
    buttons_selected: bool,
}

impl InputState {
    pub(crate) fn new() -> Self {
        Self { inputs: GameBoyInputs::default(), d_pad_selected: false, buttons_selected: false }
    }

    pub(crate) fn set_inputs(&mut self, inputs: GameBoyInputs) {
        self.inputs = inputs;
    }

    pub(crate) fn write_joyp(&mut self, value: u8) {
        self.buttons_selected = !value.bit(5);
        self.d_pad_selected = !value.bit(4);

        log::trace!("JOYP write: {value:02X}");
    }

    pub(crate) fn read_joyp(&self) -> u8 {
        let bit_3_inverted = (self.buttons_selected && self.inputs.start)
            || (self.d_pad_selected && self.inputs.down);
        let bit_2_inverted = (self.buttons_selected && self.inputs.select)
            || (self.d_pad_selected && self.inputs.up);
        let bit_1_inverted =
            (self.buttons_selected && self.inputs.b) || (self.d_pad_selected && self.inputs.left);
        let bit_0_inverted =
            (self.buttons_selected && self.inputs.a) || (self.d_pad_selected && self.inputs.right);

        0xC0 | (u8::from(!self.buttons_selected) << 5)
            | (u8::from(!self.buttons_selected) << 4)
            | (u8::from(!bit_3_inverted) << 3)
            | (u8::from(!bit_2_inverted) << 2)
            | (u8::from(!bit_1_inverted) << 1)
            | u8::from(!bit_0_inverted)
    }
}
