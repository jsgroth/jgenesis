//! Code for handling Genesis controller input I/O registers

use crate::GenesisEmulatorConfig;
use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{EnumAll, EnumDisplay};

define_controller_inputs! {
    buttons: GenesisButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        C -> c,
        X -> x,
        Y -> y,
        Z -> z,
        Start -> start,
        Mode -> mode,
    },
    joypad: GenesisJoypadState,
    inputs: GenesisInputs {
        players: {
            p1: Player::One,
            p2: Player::Two,
        },
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GenesisControllerType {
    ThreeButton,
    #[default]
    SixButton,
    None,
}

// Slightly less than 1.5ms
const FLIP_COUNTER_CYCLES: u32 = 10000;

const TH_BIT: u8 = 6;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct PinDirections {
    last_data_write: u8,
    last_ctrl_write: u8,
    th_flip_count: u8,
    flip_reset_counter: u32,
    cycles_until_th_high: u32,
    controller_th: bool,
}

impl Default for PinDirections {
    fn default() -> Self {
        Self {
            last_data_write: 0,
            last_ctrl_write: 0,
            th_flip_count: 0,
            flip_reset_counter: 0,
            cycles_until_th_high: 0,
            // Some games will freeze at boot if controller TH doesn't default to 1
            controller_th: true,
        }
    }
}

impl PinDirections {
    fn write_ctrl(&mut self, ctrl_byte: u8, controller_type: GenesisControllerType) {
        self.last_ctrl_write = ctrl_byte;
        self.maybe_set_th(controller_type);

        // When TH is set to input, the controller's TH should get pulled high, but only after a
        // short delay.
        // Micro Machines depends on it getting pulled high within ~70 68K CPU cycles, while
        // Trouble Shooter depends on it _not_ getting pulled high until after ~15 68K CPU cycles.
        self.cycles_until_th_high = if !ctrl_byte.bit(TH_BIT) { 30 } else { 0 };
    }

    fn write_data(&mut self, data_byte: u8, controller_type: GenesisControllerType) {
        self.last_data_write = data_byte;
        self.maybe_set_th(controller_type);
    }

    fn maybe_set_th(&mut self, controller_type: GenesisControllerType) {
        if !self.last_ctrl_write.bit(TH_BIT) {
            // TH bit is set to input; writes won't take effect until it's changed back to output
            return;
        }

        let th = self.last_data_write.bit(TH_BIT);

        // 6-button controller cycles through 4 different modes whenever TH flips from 0 to 1,
        // resetting after ~1.5ms have passed without such a flip
        if controller_type == GenesisControllerType::SixButton && !self.controller_th && th {
            self.th_flip_count = (self.th_flip_count + 1) % 4;
            self.flip_reset_counter = FLIP_COUNTER_CYCLES;
        }
        self.controller_th = th;
    }

    fn to_data_byte(self, joypad_state: GenesisJoypadState) -> u8 {
        let mut controller_byte = match (self.th_flip_count, self.controller_th) {
            (0..=2, true) => {
                // 3-button: B, C, and directional inputs
                (u8::from(!joypad_state.c) << 5)
                    | (u8::from(!joypad_state.b) << 4)
                    | (u8::from(!joypad_state.right) << 3)
                    | (u8::from(!joypad_state.left) << 2)
                    | (u8::from(!joypad_state.down) << 1)
                    | u8::from(!joypad_state.up)
            }
            (0..=1, false) => {
                // 3-button: A and Start (and up/down)
                (u8::from(!joypad_state.start) << 5)
                    | (u8::from(!joypad_state.a) << 4)
                    | (u8::from(!joypad_state.down) << 1)
                    | u8::from(!joypad_state.up)
            }
            (3, true) => {
                // 6-button: New buttons (and B and C)
                (u8::from(!joypad_state.c) << 5)
                    | (u8::from(!joypad_state.b) << 4)
                    | (u8::from(!joypad_state.mode) << 3)
                    | (u8::from(!joypad_state.x) << 2)
                    | (u8::from(!joypad_state.y) << 1)
                    | u8::from(!joypad_state.z)
            }
            (2, false) => {
                // 6-button: A, Start, and all 0s in the lower bits
                (u8::from(!joypad_state.start) << 5) | (u8::from(!joypad_state.a) << 4)
            }
            (3, false) => {
                // 6-button: A, Start, and all 1s in the lower bits
                (u8::from(!joypad_state.start) << 5) | (u8::from(!joypad_state.a) << 4) | 0b00001111
            }
            _ => panic!("th_flip_count should always be <= 3, was {}", self.th_flip_count),
        };
        controller_byte |= u8::from(self.controller_th) << 6;

        // Only bits set to input come from the controller (corresponding bit in CTRL = 0)
        controller_byte &= !self.last_ctrl_write;

        // Bit 7 always comes from the last data write
        let outputs_byte = self.last_data_write & (self.last_ctrl_write | 0x80);

        controller_byte | outputs_byte
    }

    fn to_ctrl_byte(self) -> u8 {
        self.last_ctrl_write
    }

    fn tick(&mut self, m68k_cycles: u32) {
        self.flip_reset_counter = self.flip_reset_counter.saturating_sub(m68k_cycles);
        if self.flip_reset_counter == 0 {
            self.th_flip_count = 0;
        }

        if self.cycles_until_th_high != 0 {
            self.cycles_until_th_high = self.cycles_until_th_high.saturating_sub(m68k_cycles);
            if self.cycles_until_th_high == 0 {
                self.controller_th = true;
            }
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    inputs: GenesisInputs,
    p1_controller_type: GenesisControllerType,
    p2_controller_type: GenesisControllerType,
    p1_pin_directions: PinDirections,
    p2_pin_directions: PinDirections,
}

// All 1s signals to games that nothing is connected to the port
const DATA_NO_CONTROLLER: u8 = 0xFF;

impl InputState {
    #[must_use]
    pub fn new(
        p1_controller_type: GenesisControllerType,
        p2_controller_type: GenesisControllerType,
    ) -> Self {
        Self {
            inputs: GenesisInputs::default(),
            p1_controller_type,
            p2_controller_type,
            p1_pin_directions: PinDirections::default(),
            p2_pin_directions: PinDirections::default(),
        }
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
        if self.p1_controller_type == GenesisControllerType::None {
            return DATA_NO_CONTROLLER;
        }

        self.p1_pin_directions.to_data_byte(self.inputs.p1)
    }

    #[must_use]
    pub fn read_p2_data(&self) -> u8 {
        if self.p2_controller_type == GenesisControllerType::None {
            return DATA_NO_CONTROLLER;
        }

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
        self.p1_pin_directions.write_ctrl(value, self.p1_controller_type);
    }

    pub fn write_p2_ctrl(&mut self, value: u8) {
        self.p2_pin_directions.write_ctrl(value, self.p2_controller_type);
    }

    pub fn tick(&mut self, m68k_cycles: u32) {
        self.p1_pin_directions.tick(m68k_cycles);
        self.p2_pin_directions.tick(m68k_cycles);
    }
}
