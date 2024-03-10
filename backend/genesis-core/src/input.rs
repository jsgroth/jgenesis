//! Code for handling Genesis controller input I/O registers

use crate::GenesisEmulatorConfig;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{define_controller_inputs, EnumDisplay, EnumFromStr};

define_controller_inputs! {
    button_ident: GenesisButton,
    joypad_ident: GenesisJoypadState,
    inputs_ident: GenesisInputs,
    buttons: [Up, Left, Right, Down, A, B, C, X, Y, Z, Start, Mode],
    inputs: {
        p1: (Player One),
        p2: (Player Two),
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumFromStr, EnumDisplay)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GenesisControllerType {
    ThreeButton,
    #[default]
    SixButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum InputPinDirection {
    #[default]
    Input,
    Output,
}

impl InputPinDirection {
    fn from_ctrl_bit(bit: bool) -> Self {
        if bit { Self::Output } else { Self::Input }
    }

    fn to_ctrl_bit(self) -> bool {
        self == Self::Output
    }

    fn to_data_bit(self, joypad_bit: bool, data_bit: bool) -> bool {
        match self {
            Self::Input => joypad_bit,
            Self::Output => data_bit,
        }
    }
}

// Slightly less than 1.5ms
const FLIP_COUNTER_CYCLES: u32 = 10000;

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct PinDirections {
    last_data_write: u8,
    th_flip_count: u8,
    flip_reset_counter: u32,
    th: InputPinDirection,
    tr: InputPinDirection,
    tl: InputPinDirection,
    right: InputPinDirection,
    left: InputPinDirection,
    down: InputPinDirection,
    up: InputPinDirection,
}

impl PinDirections {
    fn write_ctrl(&mut self, ctrl_byte: u8) {
        self.th = InputPinDirection::from_ctrl_bit(ctrl_byte.bit(6));
        self.tr = InputPinDirection::from_ctrl_bit(ctrl_byte.bit(5));
        self.tl = InputPinDirection::from_ctrl_bit(ctrl_byte.bit(4));
        self.right = InputPinDirection::from_ctrl_bit(ctrl_byte.bit(3));
        self.left = InputPinDirection::from_ctrl_bit(ctrl_byte.bit(2));
        self.down = InputPinDirection::from_ctrl_bit(ctrl_byte.bit(1));
        self.up = InputPinDirection::from_ctrl_bit(ctrl_byte.bit(0));
    }

    fn write_data(&mut self, data_byte: u8, controller_type: GenesisControllerType) {
        let prev_th = self.th.to_data_bit(true, self.last_data_write.bit(6));
        self.last_data_write = data_byte;
        let th = self.th.to_data_bit(true, self.last_data_write.bit(6));

        // 6-button controller cycles through 4 different modes whenever TH flips from 0 to 1,
        // resetting after ~1.5ms have passed without such a flip
        if controller_type == GenesisControllerType::SixButton && !prev_th && th {
            self.th_flip_count = (self.th_flip_count + 1) & 0x03;
            self.flip_reset_counter = FLIP_COUNTER_CYCLES;
        }
    }

    fn to_data_byte(self, joypad_state: GenesisJoypadState) -> u8 {
        let th = self.th.to_data_bit(true, self.last_data_write.bit(6));

        let tr_joypad = if th { !joypad_state.c } else { !joypad_state.start };
        let tl_joypad = if th { !joypad_state.b } else { !joypad_state.a };
        let d3_joypad = match (self.th_flip_count, th) {
            (0..=2, true) => !joypad_state.right,
            (3, true) => !joypad_state.mode,
            (0..=2, false) => false,
            (3, false) => true,
            _ => panic!("th_flip_count should always be <= 3"),
        };
        let d2_joypad = match (self.th_flip_count, th) {
            (0..=2, true) => !joypad_state.left,
            (3, true) => !joypad_state.x,
            (0..=2, false) => false,
            (3, false) => true,
            _ => panic!("th_flip_count should always be <= 3"),
        };
        let d1_joypad = match (self.th_flip_count, th) {
            (0 | 1, _) | (2, true) => !joypad_state.down,
            (3, true) => !joypad_state.y,
            (2, false) => false,
            (3, false) => true,
            _ => panic!("th_flip_count should always be <= 3"),
        };
        let d0_joypad = match (self.th_flip_count, th) {
            (0 | 1, _) | (2, true) => !joypad_state.up,
            (3, true) => !joypad_state.z,
            (2, false) => false,
            (3, false) => true,
            _ => panic!("th_flip_count should always be <= 3"),
        };

        let last_data_write = self.last_data_write;
        (last_data_write & 0x80)
            | (u8::from(th) << 6)
            | (u8::from(self.tr.to_data_bit(tr_joypad, last_data_write.bit(5))) << 5)
            | (u8::from(self.tl.to_data_bit(tl_joypad, last_data_write.bit(4))) << 4)
            | (u8::from(self.right.to_data_bit(d3_joypad, last_data_write.bit(3))) << 3)
            | (u8::from(self.left.to_data_bit(d2_joypad, last_data_write.bit(2))) << 2)
            | (u8::from(self.down.to_data_bit(d1_joypad, last_data_write.bit(1))) << 1)
            | u8::from(self.up.to_data_bit(d0_joypad, last_data_write.bit(0)))
    }

    fn to_ctrl_byte(self) -> u8 {
        (u8::from(self.th.to_ctrl_bit()) << 6)
            | (u8::from(self.tr.to_ctrl_bit()) << 5)
            | (u8::from(self.tl.to_ctrl_bit()) << 4)
            | (u8::from(self.right.to_ctrl_bit()) << 3)
            | (u8::from(self.left.to_ctrl_bit()) << 2)
            | (u8::from(self.down.to_ctrl_bit()) << 1)
            | u8::from(self.up.to_ctrl_bit())
    }

    fn tick(&mut self, m68k_cycles: u32) {
        self.flip_reset_counter = self.flip_reset_counter.saturating_sub(m68k_cycles);
        if self.flip_reset_counter == 0 {
            self.th_flip_count = 0;
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct InputState {
    inputs: GenesisInputs,
    p1_controller_type: GenesisControllerType,
    p2_controller_type: GenesisControllerType,
    p1_pin_directions: PinDirections,
    p2_pin_directions: PinDirections,
}

impl InputState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_inputs(&mut self, inputs: GenesisInputs) {
        self.inputs = inputs;
    }

    pub fn reload_config(&mut self, config: GenesisEmulatorConfig) {
        self.p1_controller_type = config.p1_controller_type;
        self.p2_controller_type = config.p2_controller_type;
    }

    #[must_use]
    pub fn controller_types(&self) -> (GenesisControllerType, GenesisControllerType) {
        (self.p1_controller_type, self.p2_controller_type)
    }

    #[must_use]
    pub fn read_p1_data(&self) -> u8 {
        self.p1_pin_directions.to_data_byte(self.inputs.p1)
    }

    #[must_use]
    pub fn read_p2_data(&self) -> u8 {
        self.p2_pin_directions.to_data_byte(self.inputs.p2)
    }

    pub fn write_p1_data(&mut self, value: u8) {
        self.p1_pin_directions.write_data(value, self.p1_controller_type);
    }

    pub fn write_p2_data(&mut self, value: u8) {
        self.p2_pin_directions.write_data(value, self.p2_controller_type);
    }

    #[must_use]
    pub fn read_p1_ctrl(&self) -> u8 {
        self.p1_pin_directions.to_ctrl_byte()
    }

    #[must_use]
    pub fn read_p2_ctrl(&self) -> u8 {
        self.p2_pin_directions.to_ctrl_byte()
    }

    pub fn write_p1_ctrl(&mut self, value: u8) {
        self.p1_pin_directions.write_ctrl(value);
    }

    pub fn write_p2_ctrl(&mut self, value: u8) {
        self.p2_pin_directions.write_ctrl(value);
    }

    pub fn tick(&mut self, m68k_cycles: u32) {
        self.p1_pin_directions.tick(m68k_cycles);
        self.p2_pin_directions.tick(m68k_cycles);
    }
}
