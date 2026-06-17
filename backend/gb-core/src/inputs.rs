//! Game Boy input handling

use crate::api::GameBoyEmulatorConfig;
use crate::interrupts::InterruptRegisters;
use crate::sm83::InterruptType;
use bincode::{Decode, Encode};
use gb_config::GameBoyInputs;
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct InputState {
    inputs: GameBoyInputs,
    d_pad_selected: bool,
    buttons_selected: bool,
    prev_joyp: u8,
    allow_opposing_directions: bool,
}

impl InputState {
    pub(crate) fn new(config: &GameBoyEmulatorConfig) -> Self {
        Self {
            inputs: GameBoyInputs::default(),
            d_pad_selected: false,
            buttons_selected: false,
            prev_joyp: 0xFF,
            allow_opposing_directions: config.allow_opposing_joypad_directions,
        }
    }

    pub(crate) fn set_inputs(
        &mut self,
        inputs: GameBoyInputs,
        interrupts: &mut InterruptRegisters,
    ) {
        if self.inputs == inputs {
            return;
        }

        self.inputs = inputs;
        self.check_for_joypad_interrupt(interrupts);
    }

    pub(crate) fn write_joyp(&mut self, value: u8, interrupts: &mut InterruptRegisters) {
        self.buttons_selected = !value.bit(5);
        self.d_pad_selected = !value.bit(4);

        self.check_for_joypad_interrupt(interrupts);

        log::trace!("JOYP write: {value:02X}");
    }

    fn check_for_joypad_interrupt(&mut self, interrupts: &mut InterruptRegisters) {
        // Joypad interrupt triggers when any of JOYP bits 0-3 change from 1 to 0
        let new_joyp = self.read_joyp();
        if self.prev_joyp & 0x0F & !new_joyp != 0 {
            interrupts.set_flag(InterruptType::Joypad);
        }
        self.prev_joyp = new_joyp;
    }

    pub(crate) fn read_joyp(&self) -> u8 {
        let inputs = self.inputs.with_allow_opposing_directions(self.allow_opposing_directions);

        let bit_3_inverted =
            (self.buttons_selected && inputs.start) || (self.d_pad_selected && inputs.down);
        let bit_2_inverted =
            (self.buttons_selected && inputs.select) || (self.d_pad_selected && inputs.up);
        let bit_1_inverted =
            (self.buttons_selected && inputs.b) || (self.d_pad_selected && inputs.left);
        let bit_0_inverted =
            (self.buttons_selected && inputs.a) || (self.d_pad_selected && inputs.right);

        0xC0 | (u8::from(!self.buttons_selected) << 5)
            | (u8::from(!self.d_pad_selected) << 4)
            | (u8::from(!bit_3_inverted) << 3)
            | (u8::from(!bit_2_inverted) << 2)
            | (u8::from(!bit_1_inverted) << 1)
            | u8::from(!bit_0_inverted)
    }

    pub fn reload_config(&mut self, config: &GameBoyEmulatorConfig) {
        self.allow_opposing_directions = config.allow_opposing_joypad_directions;
    }
}
