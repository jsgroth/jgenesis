//! Code for handling Genesis controller input I/O registers

mod gamepad;
mod mouse;
mod xe1ap;

use crate::input::gamepad::{SixButtonState, ThreeButtonState};
use crate::input::mouse::MegaMouseState;
use crate::input::xe1ap::Xe1apState;
use bincode::{Decode, Encode};
use genesis_config::{GenesisController, GenesisEmulatorConfig, GenesisInputs};
use jgenesis_common::num::GetBit;

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

    fn write_ctrl(&mut self, value: u8, last_data_write: u8, state: &mut ControllerState) {
        // DATA bit 7 always reads the last DATA write, so pretend it's always an output pin
        self.directions = value | (1 << 7);
        self.ctrl_bit_7 = value.bit(7);
        self.output(last_data_write, state);

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

    fn tr(self) -> bool {
        self.pins.bit(Self::TR)
    }

    fn tl(self) -> bool {
        self.pins.bit(Self::TL)
    }

    impl_set_input_pin!(input_th, TH);
    impl_set_input_pin!(input_tr, TR);
    impl_set_input_pin!(input_tl, TL);
    impl_set_input_pin!(input_d3, D3);
    impl_set_input_pin!(input_d2, D2);
    impl_set_input_pin!(input_d1, D1);
    impl_set_input_pin!(input_d0, D0);

    fn input_data_nibble(&mut self, pins: u8) {
        let low_nibble = (self.pins & self.directions) | (pins & !self.directions);
        self.pins = (self.pins & !0x0F) | (low_nibble & 0x0F);
    }

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

fn update_pins_no_controller(pins: &mut Pins) {
    // All 1s signals to games that nothing is connected to the controller port
    pins.input_th(true);
    pins.input_tr(true);
    pins.input_tl(true);
    pins.input_data_nibble(0b1111);
}

#[derive(Debug, Clone, Encode, Decode)]
enum ControllerState {
    ThreeButton(ThreeButtonState),
    SixButton(SixButtonState),
    MegaMouse(MegaMouseState),
    Xe1ap(Xe1apState),
    None,
}

impl ControllerState {
    fn new(controller: GenesisController) -> Self {
        log::debug!("Creating new controller state for type {:?}", controller.controller_type());

        match controller {
            GenesisController::ThreeButton(joypad) => {
                Self::ThreeButton(ThreeButtonState::new(joypad))
            }
            GenesisController::SixButton(joypad) => Self::SixButton(SixButtonState::new(joypad)),
            GenesisController::MegaMouse(joypad) => Self::MegaMouse(MegaMouseState::new(joypad)),
            GenesisController::Xe1ap(joypad) => Self::Xe1ap(Xe1apState::new(joypad)),
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
            (Self::MegaMouse(state), GenesisController::MegaMouse(joypad)) => {
                state.joypad = joypad;
            }
            (Self::Xe1ap(state), GenesisController::Xe1ap(joypad)) => {
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
            Self::MegaMouse(state) => state.update_pins(pins),
            Self::Xe1ap(state) => state.update_pins(pins),
            Self::None => update_pins_no_controller(pins),
        }
    }

    fn tick(&mut self, m68k_cycles: u32, pins: &mut Pins) {
        match self {
            Self::SixButton(state) => state.tick(m68k_cycles, pins),
            Self::MegaMouse(state) => state.tick(m68k_cycles, pins),
            Self::Xe1ap(state) => state.tick(m68k_cycles, pins),
            Self::ThreeButton(_) | Self::None => {}
        }
    }
}

trait GenesisControllerExt {
    fn with_auto_3_button(self, auto_3_button: bool) -> Self;

    fn with_allow_opposing_directions(self, allow_opposing_directions: bool) -> Self;
}

impl GenesisControllerExt for GenesisController {
    fn with_auto_3_button(self, auto_3_button: bool) -> Self {
        match self {
            Self::SixButton(joypad) if auto_3_button => Self::ThreeButton(joypad),
            _ => self,
        }
    }

    fn with_allow_opposing_directions(mut self, allow_opposing_directions: bool) -> Self {
        if allow_opposing_directions {
            return self;
        }

        match &mut self {
            Self::ThreeButton(joypad) | Self::SixButton(joypad) => {
                *joypad = joypad.with_allow_opposing_directions(allow_opposing_directions);
            }
            Self::MegaMouse(_) | Self::Xe1ap(_) | Self::None => {}
        }

        self
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    inputs: GenesisInputs,
    allow_opposing_joypad_directions: bool,
    auto_3_button_mode: bool,
    six_button_incompatible_game: bool,
    p1_state: ControllerState,
    p2_state: ControllerState,
    p1_pins: Pins,
    p2_pins: Pins,
    ext_pins: Pins,
    p1_last_data_write: u8,
    p2_last_data_write: u8,
    ext_last_data_write: u8,
    // Serial transfer is not emulated, but these registers are R/W when nothing is connected
    p1_tx_data: u8,
    p2_tx_data: u8,
    ext_tx_data: u8,
}

impl InputState {
    #[must_use]
    pub fn new(config: &GenesisEmulatorConfig, six_button_incompatible_game: bool) -> Self {
        if six_button_incompatible_game && config.auto_3_button_mode {
            log::info!(
                "Game is known to be incompatible with 6-button controller; forcing 3-button mode"
            );
        }

        let mut input_state = Self {
            inputs: GenesisInputs::default(),
            allow_opposing_joypad_directions: config.allow_opposing_joypad_directions,
            auto_3_button_mode: config.auto_3_button_mode,
            six_button_incompatible_game,
            p1_state: ControllerState::new(GenesisController::None),
            p2_state: ControllerState::new(GenesisController::None),
            p1_pins: Pins::new(),
            p2_pins: Pins::new(),
            ext_pins: Pins::new(),
            p1_last_data_write: 0xFF,
            p2_last_data_write: 0xFF,
            ext_last_data_write: 0xFF,
            p1_tx_data: 0xFF,
            p2_tx_data: 0xFF,
            ext_tx_data: 0xFF,
        };

        input_state.update_state_and_pins();
        input_state
    }

    pub fn set_inputs(&mut self, inputs: GenesisInputs) {
        if inputs == self.inputs {
            return;
        }

        self.inputs = inputs;
        self.update_state_and_pins();
    }

    fn update_state_and_pins(&mut self) {
        let auto_3_button = self.six_button_incompatible_game && self.auto_3_button_mode;

        let p1_inputs = self
            .inputs
            .p1
            .with_auto_3_button(auto_3_button)
            .with_allow_opposing_directions(self.allow_opposing_joypad_directions);
        self.p1_state.update_inputs(p1_inputs);
        self.p1_state.update_pins(&mut self.p1_pins);

        let p2_inputs = self
            .inputs
            .p2
            .with_auto_3_button(auto_3_button)
            .with_allow_opposing_directions(self.allow_opposing_joypad_directions);
        self.p2_state.update_inputs(p2_inputs);
        self.p2_state.update_pins(&mut self.p2_pins);
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        macro_rules! update_fields_if_changed {
            ($first:ident $(, $rest:ident)* $(,)?) => {
                {
                    if config.$first == self.$first $(&& config.$rest == self.$rest)* {
                        return;
                    }

                    self.$first = config.$first;
                    $(self.$rest = config.$rest;)*

                    self.update_state_and_pins();
                }
            }
        }

        update_fields_if_changed!(allow_opposing_joypad_directions, auto_3_button_mode);
    }

    #[must_use]
    pub fn read_p1_data(&self) -> u8 {
        log::debug!("P1 DATA read: {:02X}", self.p1_pins.pins);
        self.p1_pins.pins
    }

    #[must_use]
    pub fn read_p2_data(&self) -> u8 {
        log::debug!("P2 DATA read: {:02X}", self.p2_pins.pins);
        self.p2_pins.pins
    }

    #[must_use]
    pub fn read_ext_data(&self) -> u8 {
        self.ext_pins.pins
    }

    pub fn write_p1_data(&mut self, value: u8) {
        log::debug!("P1 DATA write: {value:02X}");
        self.p1_last_data_write = value;
        self.p1_pins.output(value, &mut self.p1_state);
    }

    pub fn write_p2_data(&mut self, value: u8) {
        log::debug!("P2 DATA write: {value:02X}");
        self.p2_last_data_write = value;
        self.p2_pins.output(value, &mut self.p2_state);
    }

    pub fn write_ext_data(&mut self, value: u8) {
        self.ext_last_data_write = value;
        self.ext_pins.output(value, &mut ControllerState::None);
    }

    #[must_use]
    pub fn read_p1_ctrl(&self) -> u8 {
        self.p1_pins.read_ctrl()
    }

    #[must_use]
    pub fn read_p2_ctrl(&self) -> u8 {
        self.p2_pins.read_ctrl()
    }

    #[must_use]
    pub fn read_ext_ctrl(&self) -> u8 {
        self.ext_pins.read_ctrl()
    }

    pub fn write_p1_ctrl(&mut self, value: u8) {
        log::debug!("P1 CTRL write: {value:02X}");
        self.p1_pins.write_ctrl(value, self.p1_last_data_write, &mut self.p1_state);
    }

    pub fn write_p2_ctrl(&mut self, value: u8) {
        log::debug!("P2 CTRL write: {value:02X}");
        self.p2_pins.write_ctrl(value, self.p2_last_data_write, &mut self.p2_state);
    }

    pub fn write_ext_ctrl(&mut self, value: u8) {
        self.ext_pins.write_ctrl(value, self.ext_last_data_write, &mut ControllerState::None);
    }

    #[must_use]
    pub fn read_p1_tx_data(&self) -> u8 {
        self.p1_tx_data
    }

    #[must_use]
    pub fn read_p2_tx_data(&self) -> u8 {
        self.p2_tx_data
    }

    #[must_use]
    pub fn read_ext_tx_data(&self) -> u8 {
        self.ext_tx_data
    }

    pub fn write_p1_tx_data(&mut self, value: u8) {
        self.p1_tx_data = value;
    }

    pub fn write_p2_tx_data(&mut self, value: u8) {
        self.p2_tx_data = value;
    }

    pub fn write_ext_tx_data(&mut self, value: u8) {
        self.ext_tx_data = value;
    }

    pub fn tick(&mut self, m68k_cycles: u32) {
        self.p1_state.tick(m68k_cycles, &mut self.p1_pins);
        self.p1_pins.tick(m68k_cycles, &mut self.p1_state);

        self.p2_state.tick(m68k_cycles, &mut self.p2_pins);
        self.p2_pins.tick(m68k_cycles, &mut self.p2_state);
    }
}
