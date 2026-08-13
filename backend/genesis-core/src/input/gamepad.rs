use crate::input::Pins;
use bincode::{Decode, Encode};
use genesis_config::GenesisJoypadState;

#[derive(Debug, Clone, Encode, Decode)]
pub struct ThreeButtonState {
    pub joypad: GenesisJoypadState,
}

impl ThreeButtonState {
    pub fn new(joypad: GenesisJoypadState) -> Self {
        Self { joypad }
    }

    pub fn update_pins(&self, pins: &mut Pins) {
        if pins.th() {
            // B, C, and directional inputs
            pins.input_tr(!self.joypad.c);
            pins.input_tl(!self.joypad.b);
            pins.input_d3(!self.joypad.right);
            pins.input_d2(!self.joypad.left);
            pins.input_d1(!self.joypad.down);
            pins.input_d0(!self.joypad.up);
        } else {
            // A and start (and up/down)
            pins.input_tr(!self.joypad.start);
            pins.input_tl(!self.joypad.a);
            pins.input_d3(false);
            pins.input_d2(false);
            pins.input_d1(!self.joypad.down);
            pins.input_d0(!self.joypad.up);
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SixButtonState {
    pub joypad: GenesisJoypadState,
    th_flip_count: u8,
    flip_reset_counter: u32,
    last_th: bool,
}

impl SixButtonState {
    // Produces roughly the expected timeout value in Joystick Test Program (PD), about 1.58ms
    const FLIP_COUNTER_CYCLES: u32 = 12150;

    pub fn new(joypad: GenesisJoypadState) -> Self {
        Self { joypad, th_flip_count: 0, flip_reset_counter: 0, last_th: true }
    }

    pub fn update_pins(&mut self, pins: &mut Pins) {
        // 6-button controller cycles through 5 different modes whenever TH flips from 0 to 1,
        // resetting after ~1.5ms have passed without such a flip
        let th = pins.th();
        if !self.last_th && th {
            self.th_flip_count = (self.th_flip_count + 1) % 5;
            self.flip_reset_counter = Self::FLIP_COUNTER_CYCLES;
        }
        self.last_th = th;

        // TR and TL are always set the same way as 3-button
        if th {
            pins.input_tr(!self.joypad.c);
            pins.input_tl(!self.joypad.b);
        } else {
            pins.input_tr(!self.joypad.start);
            pins.input_tl(!self.joypad.a);
        }

        match (self.th_flip_count, th) {
            (0..=2 | 4, true) => {
                // 3-button: B, C, and directional inputs
                pins.input_d3(!self.joypad.right);
                pins.input_d2(!self.joypad.left);
                pins.input_d1(!self.joypad.down);
                pins.input_d0(!self.joypad.up);
            }
            (0 | 1 | 4, false) => {
                // 3-button: A and Start (and up/down)
                pins.input_d3(false);
                pins.input_d2(false);
                pins.input_d1(!self.joypad.down);
                pins.input_d0(!self.joypad.up);
            }
            (2, false) => {
                // 6-button: A, Start, and all 0s in the lower bits
                pins.input_data_nibble(0b0000);
            }
            (3, true) => {
                // 6-button: New buttons (and B and C)
                pins.input_d3(!self.joypad.mode);
                pins.input_d2(!self.joypad.x);
                pins.input_d1(!self.joypad.y);
                pins.input_d0(!self.joypad.z);
            }
            (3, false) => {
                // 6-button: A, Start, and all 1s in the lower bits
                pins.input_data_nibble(0b1111);
            }
            _ => panic!("th_flip_count should always be <= 4, was {}", self.th_flip_count),
        }
    }

    pub fn tick(&mut self, m68k_cycles: u32, pins: &mut Pins) {
        if self.flip_reset_counter == 0 {
            return;
        }

        self.flip_reset_counter = self.flip_reset_counter.saturating_sub(m68k_cycles);
        if self.flip_reset_counter == 0 {
            self.th_flip_count = 0;
            self.update_pins(pins);
        }
    }
}
