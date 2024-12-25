//! Game Boy input handling

use crate::interrupts::InterruptRegisters;
use crate::sm83::InterruptType;
use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::MappableInputs;
use jgenesis_common::input::Player;
use jgenesis_common::num::GetBit;

define_controller_inputs! {
    buttons: GameBoyButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        Start -> start,
        Select -> select,
    },
    joypad: GameBoyInputs,
}

impl MappableInputs<GameBoyButton> for GameBoyInputs {
    #[inline]
    fn set_field(&mut self, button: GameBoyButton, _player: Player, pressed: bool) {
        self.set_button(button, pressed);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct InputState {
    inputs: GameBoyInputs,
    d_pad_selected: bool,
    buttons_selected: bool,
    prev_joyp: u8,
}

impl InputState {
    pub(crate) fn new() -> Self {
        Self {
            inputs: GameBoyInputs::default(),
            d_pad_selected: false,
            buttons_selected: false,
            prev_joyp: 0xFF,
        }
    }

    pub(crate) fn set_inputs(&mut self, inputs: GameBoyInputs) {
        self.inputs = inputs;
    }

    pub(crate) fn write_joyp(&mut self, value: u8) {
        self.buttons_selected = !value.bit(5);
        self.d_pad_selected = !value.bit(4);

        log::trace!("JOYP write: {value:02X}");
    }

    pub(crate) fn check_for_joypad_interrupt(
        &mut self,
        interrupt_registers: &mut InterruptRegisters,
    ) {
        // Joypad interrupt triggers when any of JOYP bits 0-3 change from 1 to 0
        let new_joyp = self.read_joyp();
        if self.prev_joyp & 0x0F & !new_joyp != 0 {
            interrupt_registers.set_flag(InterruptType::Joypad);
        }
        self.prev_joyp = new_joyp;
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
