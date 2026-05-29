//! Code for handling Genesis controller input I/O registers

use bincode::{Decode, Encode};
use genesis_config::{GenesisController, GenesisInputs, GenesisJoypadState};
use jgenesis_common::num::GetBit;

// Produces roughly the expected timeout value in Joystick Test Program (PD)
const FLIP_COUNTER_CYCLES: u32 = 12150;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct Pins {
    // 0 = input (from controller), 1 = output (to controller)
    directions: u8,
    // Current pin states
    pins: u8,
    // If non-zero, 68K cycles remaining until TH is pulled high due to being set as input
    cycles_until_th_high: u32,
    // Used for external interrupts; not yet implemented except as an R/W bit
    ctrl_bit_7: bool,
}

macro_rules! impl_set_input_pin {
    ($name:ident, $bit:ident) => {
        fn $name(&mut self, pin: bool) {
            if !self.directions.bit(Self::$bit) {
                self.pins = (self.pins & !(1 << Self::$bit)) | (u8::from(pin) << Self::$bit);
            }
        }
    };
}

impl Pins {
    const TH: u8 = 6;
    const TR: u8 = 5;
    const TL: u8 = 4;
    const D3: u8 = 3;
    const D2: u8 = 2;
    const D1: u8 = 1;
    const D0: u8 = 0;

    fn new() -> Self {
        // TH must be initialized to 1 or some games will freeze at boot
        Self { directions: 0, pins: 0xFF, cycles_until_th_high: 0, ctrl_bit_7: false }
    }

    fn read_ctrl(self) -> u8 {
        (self.directions & !(1 << 7)) | (u8::from(self.ctrl_bit_7) << 7)
    }

    fn write_ctrl(&mut self, value: u8, state: &mut ControllerState) {
        // DATA bit 7 always reads the last DATA write, so pretend it's always an output pin
        self.directions = value | (1 << 7);
        self.ctrl_bit_7 = value.bit(7);
        state.update_pins(self);

        if !self.directions.bit(Self::TH) {
            // Gamepads don't drive the TH pin, so when TH is set to input, it should get pulled
            // high after a short delay.
            // Micro Machines depends on it getting pulled high within ~70 68K CPU cycles, while
            // Trouble Shooter depends on it _not_ getting pulled high until after ~15 68K CPU cycles.
            // TODO some devices do drive TH, e.g. lightguns
            if self.cycles_until_th_high == 0 {
                self.cycles_until_th_high = 30;
            }
        } else {
            // TH is set to output
            self.cycles_until_th_high = 0;
        }
    }

    fn th(self) -> bool {
        self.pins.bit(Self::TH)
    }

    impl_set_input_pin!(input_th, TH);
    impl_set_input_pin!(input_tr, TR);
    impl_set_input_pin!(input_tl, TL);
    impl_set_input_pin!(input_d3, D3);
    impl_set_input_pin!(input_d2, D2);
    impl_set_input_pin!(input_d1, D1);
    impl_set_input_pin!(input_d0, D0);

    fn output(&mut self, pins: u8, state: &mut ControllerState) {
        self.pins = (self.pins & !self.directions) | (pins & self.directions);
        state.update_pins(self);
    }

    fn tick(&mut self, m68k_cycles: u32, state: &mut ControllerState) {
        if self.cycles_until_th_high == 0 {
            return;
        }

        self.cycles_until_th_high = self.cycles_until_th_high.saturating_sub(m68k_cycles);
        if self.cycles_until_th_high == 0 {
            self.pins |= 1 << Self::TH;
            state.update_pins(self);
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct ThreeButtonState {
    joypad: GenesisJoypadState,
}

impl ThreeButtonState {
    fn new(joypad: GenesisJoypadState) -> Self {
        Self { joypad }
    }

    fn update_pins(&self, pins: &mut Pins) {
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
struct SixButtonState {
    joypad: GenesisJoypadState,
    th_flip_count: u8,
    flip_reset_counter: u32,
    last_th: bool,
}

impl SixButtonState {
    fn new(joypad: GenesisJoypadState) -> Self {
        Self { joypad, th_flip_count: 0, flip_reset_counter: 0, last_th: true }
    }

    fn update_pins(&mut self, pins: &mut Pins) {
        // 6-button controller cycles through 5 different modes whenever TH flips from 0 to 1,
        // resetting after ~1.5ms have passed without such a flip
        let th = pins.th();
        if !self.last_th && th {
            self.th_flip_count = (self.th_flip_count + 1) % 5;
            self.flip_reset_counter = FLIP_COUNTER_CYCLES;
        }
        self.last_th = th;

        match (self.th_flip_count, th) {
            (0..=2 | 4, true) => {
                // 3-button: B, C, and directional inputs
                pins.input_tr(!self.joypad.c);
                pins.input_tl(!self.joypad.b);
                pins.input_d3(!self.joypad.right);
                pins.input_d2(!self.joypad.left);
                pins.input_d1(!self.joypad.down);
                pins.input_d0(!self.joypad.up);
            }
            (0 | 1 | 4, false) => {
                // 3-button: A and Start (and up/down)
                pins.input_tr(!self.joypad.start);
                pins.input_tl(!self.joypad.a);
                pins.input_d3(false);
                pins.input_d2(false);
                pins.input_d1(!self.joypad.down);
                pins.input_d0(!self.joypad.up);
            }
            (2, false) => {
                // 6-button: A, Start, and all 0s in the lower bits
                // Used by games for 6-button controller detection
                pins.input_tr(!self.joypad.start);
                pins.input_tl(!self.joypad.a);
                pins.input_d3(false);
                pins.input_d2(false);
                pins.input_d1(false);
                pins.input_d0(false);
            }
            (3, true) => {
                // 6-button: New buttons (and B and C)
                pins.input_tr(!self.joypad.c);
                pins.input_tl(!self.joypad.b);
                pins.input_d3(!self.joypad.mode);
                pins.input_d2(!self.joypad.x);
                pins.input_d1(!self.joypad.y);
                pins.input_d0(!self.joypad.z);
            }
            (3, false) => {
                // 6-button: A, Start, and all 1s in the lower bits
                pins.input_tr(!self.joypad.start);
                pins.input_tl(!self.joypad.a);
                pins.input_d3(true);
                pins.input_d2(true);
                pins.input_d1(true);
                pins.input_d0(true);
            }
            _ => panic!("th_flip_count should always be <= 4, was {}", self.th_flip_count),
        }
    }

    fn tick(&mut self, m68k_cycles: u32, pins: &mut Pins) {
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

fn update_pins_no_controller(pins: &mut Pins) {
    // All 1s signals to games that nothing is connected to the controller port
    pins.input_th(true);
    pins.input_tr(true);
    pins.input_tl(true);
    pins.input_d3(true);
    pins.input_d2(true);
    pins.input_d1(true);
    pins.input_d0(true);
}

#[derive(Debug, Clone, Encode, Decode)]
enum ControllerState {
    ThreeButton(ThreeButtonState),
    SixButton(SixButtonState),
    None,
}

impl ControllerState {
    fn new(controller: GenesisController) -> Self {
        match controller {
            GenesisController::ThreeButton(joypad) => {
                Self::ThreeButton(ThreeButtonState::new(joypad))
            }
            GenesisController::SixButton(joypad) => Self::SixButton(SixButtonState::new(joypad)),
            GenesisController::None => Self::None,
        }
    }

    fn update_inputs(&mut self, controller: GenesisController) {
        match (self, controller) {
            (Self::ThreeButton(state), GenesisController::ThreeButton(joypad)) => {
                state.joypad = joypad;
            }
            (Self::SixButton(state), GenesisController::SixButton(joypad)) => {
                state.joypad = joypad;
            }
            (Self::None, GenesisController::None) => {}
            // Controller type changed; reset state
            (state, controller) => {
                *state = Self::new(controller);
            }
        }
    }

    fn update_pins(&mut self, pins: &mut Pins) {
        match self {
            Self::ThreeButton(state) => state.update_pins(pins),
            Self::SixButton(state) => state.update_pins(pins),
            Self::None => update_pins_no_controller(pins),
        }
    }

    fn tick(&mut self, m68k_cycles: u32, pins: &mut Pins) {
        if let Self::SixButton(state) = self {
            state.tick(m68k_cycles, pins);
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    inputs: GenesisInputs,
    p1_state: ControllerState,
    p2_state: ControllerState,
    p1_pins: Pins,
    p2_pins: Pins,
}

impl InputState {
    #[must_use]
    pub fn new() -> Self {
        let inputs = GenesisInputs::default();

        Self {
            inputs,
            p1_state: ControllerState::new(inputs.p1),
            p2_state: ControllerState::new(inputs.p2),
            p1_pins: Pins::new(),
            p2_pins: Pins::new(),
        }
    }

    pub fn set_inputs(&mut self, inputs: GenesisInputs) {
        if inputs == self.inputs {
            return;
        }

        self.p1_state.update_inputs(inputs.p1);
        self.p1_state.update_pins(&mut self.p1_pins);

        self.p2_state.update_inputs(inputs.p2);
        self.p2_state.update_pins(&mut self.p2_pins);

        self.inputs = inputs;
    }

    #[must_use]
    pub fn read_p1_data(&self) -> u8 {
        self.p1_pins.pins
    }

    #[must_use]
    pub fn read_p2_data(&self) -> u8 {
        self.p2_pins.pins
    }

    pub fn write_p1_data(&mut self, value: u8) {
        self.p1_pins.output(value, &mut self.p1_state);
    }

    pub fn write_p2_data(&mut self, value: u8) {
        self.p2_pins.output(value, &mut self.p2_state);
    }

    #[must_use]
    pub fn read_p1_ctrl(&self) -> u8 {
        self.p1_pins.read_ctrl()
    }

    #[must_use]
    pub fn read_p2_ctrl(&self) -> u8 {
        self.p2_pins.read_ctrl()
    }

    pub fn write_p1_ctrl(&mut self, value: u8) {
        self.p1_pins.write_ctrl(value, &mut self.p1_state);
    }

    pub fn write_p2_ctrl(&mut self, value: u8) {
        self.p2_pins.write_ctrl(value, &mut self.p2_state);
    }

    pub fn tick(&mut self, m68k_cycles: u32) {
        self.p1_state.tick(m68k_cycles, &mut self.p1_pins);
        self.p1_pins.tick(m68k_cycles, &mut self.p1_state);

        self.p2_state.tick(m68k_cycles, &mut self.p2_pins);
        self.p2_pins.tick(m68k_cycles, &mut self.p2_state);
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}
