use crate::api::SnesEmulatorConfig;
use crate::input::{SnesController, SnesInputs, SnesJoypadStateExt, SuperScopeState};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use snes_config::SnesJoypadState;

const AUTO_JOYPAD_DURATION_MCLK: u64 = 4224;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct SuperScopeRegister {
    fire: bool,
    cursor: bool,
    pause: bool,
    turbo: bool,
    offscreen: bool,
    position: Option<(u16, u16)>,
}

impl Default for SuperScopeRegister {
    fn default() -> Self {
        Self {
            fire: false,
            cursor: false,
            pause: false,
            turbo: false,
            offscreen: true,
            position: None,
        }
    }
}

impl SuperScopeRegister {
    fn update(&mut self, current_state: SuperScopeState, last_strobe_state: SuperScopeState) {
        if current_state.fire && !last_strobe_state.fire {
            // Turbo bit updates only when fire is pressed
            self.turbo = current_state.turbo;
        }

        // If turbo is off, fire bit is only set for one strobe after the button is pressed
        self.fire = if current_state.turbo {
            current_state.fire
        } else {
            current_state.fire && !last_strobe_state.fire
        };

        // Cursor bit is always set to button press state
        self.cursor = current_state.cursor;

        // Pause is only set for one strobe after the button is pressed
        self.pause = current_state.pause && !last_strobe_state.pause;

        // Offscreen bit is only updated when fire or cursor bit is set
        if self.fire || self.cursor {
            self.offscreen = current_state.position.is_none();
        }

        // Position is only used for latching, which only occurs when fire or cursor bit is set
        self.position = current_state.position;
    }

    fn to_register_word(self) -> u16 {
        u16::from(self.fire)
            | (u16::from(self.cursor) << 1)
            | (u16::from(self.turbo) << 2)
            | (u16::from(self.pause) << 3)
            | (u16::from(self.offscreen) << 6)
            | 0xFF00 // ID bits, always 1
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct ControllerPort {
    auto_joypad_inputs: u16,
    manual_joypad_inputs: u16,
    super_scope_register: SuperScopeRegister,
    last_strobe_inputs: SnesController,
}

impl ControllerPort {
    fn new() -> Self {
        Self {
            auto_joypad_inputs: 0,
            manual_joypad_inputs: 0,
            super_scope_register: SuperScopeRegister::default(),
            last_strobe_inputs: SnesController::None,
        }
    }

    fn strobe(&mut self, current_inputs: SnesController) {
        // Instead of explicitly clearing Super Scope state for non-Super Scope controllers, clear
        // it at the beginning of every strobe and let Super Scope set it again
        let mut super_scope_register = self.super_scope_register;
        self.super_scope_register = SuperScopeRegister::default();

        self.manual_joypad_inputs = match current_inputs {
            SnesController::Gamepad(joypad) => joypad.to_register_word(),
            SnesController::SuperScope(super_scope) => {
                let word = super_scope_register.to_register_word();

                let last_strobe_state = match self.last_strobe_inputs {
                    SnesController::SuperScope(state) => state,
                    _ => SuperScopeState::default(),
                };

                super_scope_register.update(super_scope, last_strobe_state);
                self.super_scope_register = super_scope_register;

                word
            }
            SnesController::None => {
                // All bits read 0 when no controller is connected
                // Some games use this for controller detection (e.g. Donkey Kong Country)
                0
            }
        };

        self.last_strobe_inputs = current_inputs;
    }

    fn next_manual_bit(&mut self) -> bool {
        let next_bit = self.manual_joypad_inputs.bit(0);
        self.manual_joypad_inputs =
            (self.manual_joypad_inputs >> 1) | (u16::from(self.end_of_input_bit()) << 15);
        next_bit
    }

    fn end_of_input_bit(&self) -> bool {
        // Reading past end of inputs produces 1s if a controller is connected, 0s if not
        !matches!(self.last_strobe_inputs, SnesController::None)
    }

    fn do_auto_read(&mut self) -> u16 {
        // Auto joypad read always reads out 16 bits serially, earliest in most significant bits
        let mut auto_read_inputs = 0;
        for _ in 0..16 {
            auto_read_inputs = (auto_read_inputs << 1) | u16::from(self.next_manual_bit());
        }
        auto_read_inputs
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    auto_read_cycles_remaining: u64,
    auto_joypad_p1_inputs: u16,
    auto_joypad_p2_inputs: u16,
    strobe: bool,
    p1: ControllerPort,
    p2: ControllerPort,
    inputs: SnesInputs,
    allow_opposing_directions: bool,
}

impl InputState {
    pub fn new(config: &SnesEmulatorConfig) -> Self {
        Self {
            auto_read_cycles_remaining: 0,
            auto_joypad_p1_inputs: SnesJoypadState::default().to_register_word(),
            auto_joypad_p2_inputs: SnesJoypadState::default().to_register_word(),
            strobe: false,
            p1: ControllerPort::new(),
            p2: ControllerPort::new(),
            inputs: SnesInputs::default(),
            allow_opposing_directions: config.allow_opposing_joypad_directions,
        }
    }

    pub fn set_strobe(&mut self, strobe: bool) {
        if !self.strobe && strobe {
            self.p1.strobe(
                self.inputs.p1.with_allow_opposing_directions(self.allow_opposing_directions),
            );
            self.p2.strobe(
                self.inputs.p2.with_allow_opposing_directions(self.allow_opposing_directions),
            );
        }

        self.strobe = strobe;
    }

    pub fn auto_joypad_read_in_progress(&self) -> bool {
        self.auto_read_cycles_remaining != 0
    }

    pub fn auto_joypad_p1_inputs(&self) -> u16 {
        self.auto_joypad_p1_inputs
    }

    pub fn auto_joypad_p2_inputs(&self) -> u16 {
        self.auto_joypad_p2_inputs
    }

    pub fn next_manual_p1_bit(&mut self) -> bool {
        self.p1.next_manual_bit()
    }

    pub fn next_manual_p2_bit(&mut self) -> bool {
        self.p2.next_manual_bit()
    }

    pub fn start_auto_joypad_read(&mut self) {
        self.auto_read_cycles_remaining = AUTO_JOYPAD_DURATION_MCLK;
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64, inputs: SnesInputs) {
        self.inputs = inputs;

        if self.auto_read_cycles_remaining != 0 {
            self.progress_auto_joypad_read(master_cycles_elapsed);
        }
    }

    fn progress_auto_joypad_read(&mut self, master_cycles_elapsed: u64) {
        self.auto_read_cycles_remaining =
            self.auto_read_cycles_remaining.saturating_sub(master_cycles_elapsed);

        if self.auto_read_cycles_remaining == 0 {
            // Auto joypad read strobes the joypad while reading inputs; this populates the manual
            // joypad read registers
            self.set_strobe(true);
            self.set_strobe(false);

            self.auto_joypad_p1_inputs = self.p1.do_auto_read();
            self.auto_joypad_p2_inputs = self.p2.do_auto_read();
        }
    }

    pub fn hv_latch(&self) -> Option<(u16, u16)> {
        // Super Scope can only trigger HV latching behavior when plugged into port 2
        let super_scope_register = self.p2.super_scope_register;

        // Super Scope latches the PPU at H=X+40, V=Y+1 when Fire or Cursor is set
        (super_scope_register.fire || super_scope_register.cursor)
            .then(|| super_scope_register.position.map(|(x, y)| (x + 40, y + 1)))
            .flatten()
    }

    pub fn reload_config(&mut self, config: &SnesEmulatorConfig) {
        self.allow_opposing_directions = config.allow_opposing_joypad_directions;
    }
}
