//! Code for handling Genesis controller input I/O registers

use crate::GenesisEmulatorConfig;
use bincode::{Decode, Encode};
use genesis_config::{GenesisController, GenesisInputs, GenesisJoypadState, Xe1apJoypadState};
use jgenesis_common::num::GetBit;
use std::cmp;

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
    // Produces roughly the expected timeout value in Joystick Test Program (PD), about 1.58ms
    const FLIP_COUNTER_CYCLES: u32 = 12150;

    fn new(joypad: GenesisJoypadState) -> Self {
        Self { joypad, th_flip_count: 0, flip_reset_counter: 0, last_th: true }
    }

    fn update_pins(&mut self, pins: &mut Pins) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Xe1apTransferState {
    Idle,
    Active,
}

#[derive(Debug, Clone, Encode, Decode)]
struct Xe1apState {
    joypad: Xe1apJoypadState,
    transfer_state: Xe1apTransferState,
    transfer_counter: u8,
    transfer_ack: bool,
    transfer_cycles_remaining: u32,
    last_th: bool,
}

impl Xe1apState {
    // Timings based on: https://archive.org/details/micomBASIC_1990-10/ (pages 79-80)
    // When connected to the Genesis, TR is ACK and TL is L/H
    // These timings are based on the fastest transfer speed
    //
    // Each pair of nibbles is transferred in a 4-step pattern:
    //   A: ACK=0, L/H=0 (game reads first nibble here)
    //   B: ACK=1, L/H=1
    //   C: ACK=0, L/H=1 (game reads second nibble here)
    //   D: ACK=1, L/H=0
    //
    // L/H appears to change shortly after ACK 0->1 transitions; this is not emulated
    //
    // Transfers seem to begin in step D after a TH 1->0 transition
    const TRANSFER_A_CYCLES: u32 = 92; // Roughly 12 μs
    const TRANSFER_B_CYCLES: u32 = 30; // Roughly 4 μs
    const TRANSFER_C_CYCLES: u32 = 92; // Roughly 12 μs
    const TRANSFER_D_CYCLES: u32 = 168; // Roughly 22 μs

    fn new(joypad: Xe1apJoypadState) -> Self {
        Self {
            joypad,
            transfer_state: Xe1apTransferState::Idle,
            transfer_counter: 0,
            transfer_ack: true,
            transfer_cycles_remaining: 0,
            last_th: true,
        }
    }

    fn update_pins(&mut self, pins: &mut Pins) {
        let th = pins.th();
        if self.last_th && !th {
            // TH 1->0 transition begins a new transfer
            self.transfer_state = Xe1apTransferState::Active;
            self.transfer_counter = 0;
            self.transfer_ack = true;
            self.transfer_cycles_remaining = Self::TRANSFER_D_CYCLES;
        }
        self.last_th = th;

        pins.input_tl(self.transfer_counter.bit(0));
        pins.input_tr(self.transfer_ack);

        match self.transfer_state {
            Xe1apTransferState::Idle => {
                pins.input_data_nibble(0b1111);
            }
            Xe1apTransferState::Active => {
                // Only update D3-D0 pins when TR=0
                if !self.transfer_ack {
                    self.update_data_pins(pins);
                }
            }
        }

        log::debug!(
            "XE-1AP pins update: data={:04b}, TL={}, TR={}, counter={}",
            pins.pins & 0x0F,
            u8::from(pins.pins.bit(Pins::TL)),
            u8::from(pins.pins.bit(Pins::TR)),
            self.transfer_counter
        );
    }

    #[allow(clippy::match_same_arms)]
    fn update_data_pins(&self, pins: &mut Pins) {
        match self.transfer_counter {
            0 => {
                // E1, E2, Start, Select
                pins.input_d3(!self.joypad.e1);
                pins.input_d2(!self.joypad.e2);
                pins.input_d1(!self.joypad.start);
                pins.input_d0(!self.joypad.select);
            }
            1 => {
                // A|A', B|B', C, D
                pins.input_d3(!(self.joypad.a || self.joypad.ap));
                pins.input_d2(!(self.joypad.b || self.joypad.bp));
                pins.input_d1(!self.joypad.c);
                pins.input_d0(!self.joypad.d);
            }
            2 => {
                // Analog stick X, high nibble
                pins.input_data_nibble(self.joypad.analog_x >> 4);
            }
            3 => {
                // Analog stick Y, high nibble
                pins.input_data_nibble(self.joypad.analog_y >> 4);
            }
            4 => {
                // Always 0s
                pins.input_data_nibble(0b0000);
            }
            5 => {
                // Analog slider, high nibble
                pins.input_data_nibble(self.joypad.slider >> 4);
            }
            6 => {
                // Analog stick X, low nibble
                pins.input_data_nibble(self.joypad.analog_x & 0x0F);
            }
            7 => {
                // Analog stick Y, low nibble
                pins.input_data_nibble(self.joypad.analog_y & 0x0F);
            }
            8 => {
                // Always 0s
                pins.input_data_nibble(0b0000);
            }
            9 => {
                // Analog slider, low nibble
                pins.input_data_nibble(self.joypad.slider & 0x0F);
            }
            10 => {
                // Always 1s
                pins.input_data_nibble(0b1111);
            }
            11 => {
                // A, B, A', B'
                pins.input_d3(!self.joypad.a);
                pins.input_d2(!self.joypad.b);
                pins.input_d1(!self.joypad.ap);
                pins.input_d0(!self.joypad.bp);
            }
            _ => panic!(
                "XE-1AP transfer counter should always be <= 11, was {}",
                self.transfer_counter
            ),
        }
    }

    fn tick(&mut self, mut m68k_cycles: u32, pins: &mut Pins) {
        if self.transfer_state == Xe1apTransferState::Idle {
            return;
        }

        while m68k_cycles != 0 {
            match self.transfer_state {
                Xe1apTransferState::Idle => return,
                Xe1apTransferState::Active => {
                    let elapsed = cmp::min(m68k_cycles, self.transfer_cycles_remaining);
                    m68k_cycles -= elapsed;
                    self.transfer_cycles_remaining -= elapsed;
                    if self.transfer_cycles_remaining != 0 {
                        return;
                    }

                    self.transfer_cycles_remaining =
                        match (self.transfer_ack, self.transfer_counter.bit(0)) {
                            (true, false) => Self::TRANSFER_A_CYCLES,
                            (false, false) => Self::TRANSFER_B_CYCLES,
                            (true, true) => Self::TRANSFER_C_CYCLES,
                            (false, true) => Self::TRANSFER_D_CYCLES,
                        };

                    self.transfer_ack = !self.transfer_ack;
                    if self.transfer_ack {
                        if self.transfer_counter < 11 {
                            self.transfer_counter += 1;
                        } else {
                            // Transfer has ended
                            self.transfer_state = Xe1apTransferState::Idle;
                            self.transfer_counter = 0;
                            self.transfer_ack = true;
                        }
                    }

                    self.update_pins(pins);
                }
            }
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
            Self::Xe1ap(state) => state.update_pins(pins),
            Self::None => update_pins_no_controller(pins),
        }
    }

    fn tick(&mut self, m68k_cycles: u32, pins: &mut Pins) {
        match self {
            Self::SixButton(state) => state.tick(m68k_cycles, pins),
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
            Self::Xe1ap(_) | Self::None => {}
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
        log::debug!("P1 DATA read");
        self.p1_pins.pins
    }

    #[must_use]
    pub fn read_p2_data(&self) -> u8 {
        log::debug!("P2 DATA read");
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
