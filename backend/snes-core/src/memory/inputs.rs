use crate::api::SnesEmulatorConfig;
use crate::input::{
    SnesController, SnesInputs, SnesJoypadStateExt, SnesMouseState, SuperScopeState,
};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use snes_config::SnesJoypadState;

const AUTO_JOYPAD_DURATION_MCLK: u64 = 4224;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct MouseRegister {
    sensitivity: u8,
    last_x: f64,
    last_y: f64,
    x_direction: bool,
    x_magnitude: u8,
    y_direction: bool,
    y_magnitude: u8,
    left: bool,
    right: bool,
}

impl Default for MouseRegister {
    fn default() -> Self {
        Self {
            sensitivity: 0,
            last_x: 0.0,
            last_y: 0.0,
            x_direction: false,
            x_magnitude: 0,
            y_direction: false,
            y_magnitude: 0,
            left: false,
            right: false,
        }
    }
}

impl MouseRegister {
    fn update(&mut self, mouse: SnesMouseState, config_sensitivity: f64) {
        // According to the manual, 0 is "slow", 1 is "normal", 2 is "fast"
        // Normal and fast speeds have an exponential curve where the reported movement values
        // increase more rapidly at higher speeds, but presumably any half-decent PC mouse already
        // works that way, so instead of trying to emulate that just apply a fixed multiplier to dx/dy values
        const SENSITIVITY_MULTIPLIERS: [f64; 3] = [0.75, 1.0, 1.5];

        fn mouse_axis_to_magnitude(value: f64) -> u8 {
            // Mouse magnitude values are unsigned 7-bit
            // Multiplying by 0.2 is arbitrary, mouse feels way too sensitive without that
            (0.2 * value).abs().round().clamp(0.0, 127.0) as u8
        }

        let sensitivity = config_sensitivity * SENSITIVITY_MULTIPLIERS[self.sensitivity as usize];

        let dx = sensitivity * (mouse.x_position.get() - self.last_x);
        let dy = sensitivity * (mouse.y_position.get() - self.last_y);
        self.last_x = mouse.x_position.get();
        self.last_y = mouse.y_position.get();

        self.left = mouse.left;
        self.right = mouse.right;

        self.x_magnitude = mouse_axis_to_magnitude(dx);
        self.y_magnitude = mouse_axis_to_magnitude(dy);

        // Supposedly the direction bits are sticky and only change when magnitude is non-zero
        if self.x_magnitude != 0 {
            self.x_direction = dx < 0.0; // 0 = right, 1 = left
        }
        if self.y_magnitude != 0 {
            self.y_direction = dy < 0.0; // 0 = down, 1 = up
        }
    }

    fn increment_sensitivity(&mut self) {
        // Sensitivity is always 0-2
        self.sensitivity = (self.sensitivity + 1) % 3;
        log::debug!("SNES mouse sensitivity set to {}", self.sensitivity);
    }

    fn to_register_bits(self) -> u32 {
        fn reverse_bits_u2(value: u8) -> u8 {
            ((value & 1) << 1) | ((value & 2) >> 1)
        }

        fn reverse_bits_u7(value: u8) -> u8 {
            value.reverse_bits() >> 1
        }

        // First 8 bits are always 0
        (u32::from(self.right) << 8)
        | (u32::from(self.left) << 9)
        | (u32::from(reverse_bits_u2(self.sensitivity)) << 10)
        | (1 << 15) // Bits 12-15 are always mouse ID (0, 0, 0, 1)
        | (u32::from(self.y_direction) << 16)
        | (u32::from(reverse_bits_u7(self.y_magnitude)) << 17)
        | (u32::from(self.x_direction) << 24)
        | (u32::from(reverse_bits_u7(self.x_magnitude)) << 25)
    }
}

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

fn register_word_to_u32(word: u16) -> u32 {
    // Controllers with 16 bits always read out constant 1s afterwards
    u32::from(word) | (0xFFFF << 16)
}

#[derive(Debug, Clone, Encode, Decode)]
struct ControllerPort {
    auto_joypad_inputs: u16,
    manual_joypad_inputs: u32,
    mouse_register: MouseRegister,
    mouse_sensitivity: f64,
    super_scope_register: SuperScopeRegister,
    last_strobe_inputs: SnesController,
}

impl ControllerPort {
    fn new(config: &SnesEmulatorConfig) -> Self {
        Self {
            auto_joypad_inputs: 0,
            manual_joypad_inputs: 0,
            mouse_register: MouseRegister::default(),
            mouse_sensitivity: config.mouse_sensitivity,
            super_scope_register: SuperScopeRegister::default(),
            last_strobe_inputs: SnesController::None,
        }
    }

    fn update_strobe(&mut self, strobe: bool, current_inputs: SnesController) {
        if !matches!(current_inputs, SnesController::Mouse(_)) {
            self.mouse_register = MouseRegister::default();
        }

        if !matches!(current_inputs, SnesController::SuperScope(_)) {
            self.super_scope_register = SuperScopeRegister::default();
        }

        match current_inputs {
            SnesController::Gamepad(joypad) => {
                // Gamepads only reset on strobe 1->0 transitions
                if !strobe {
                    self.manual_joypad_inputs = register_word_to_u32(joypad.to_register_word());
                }
            }
            SnesController::Mouse(mouse) => {
                // Not sure the mouse actually works this way, but only update mouse state
                // on strobe 1->0 transitions because of how mouse movement is tracked and latched
                if !strobe {
                    self.mouse_register.update(mouse, self.mouse_sensitivity);
                    self.manual_joypad_inputs = self.mouse_register.to_register_bits();
                }
            }
            SnesController::SuperScope(super_scope) => {
                self.manual_joypad_inputs =
                    register_word_to_u32(self.super_scope_register.to_register_word());

                let last_strobe_state = match self.last_strobe_inputs {
                    SnesController::SuperScope(state) => state,
                    _ => SuperScopeState::default(),
                };

                self.super_scope_register.update(super_scope, last_strobe_state);
            }
            SnesController::None => {
                // All bits read 0 when no controller is connected
                // Some games use this for controller detection (e.g. Donkey Kong Country)
                self.manual_joypad_inputs = 0;
            }
        }

        self.last_strobe_inputs = current_inputs;
    }

    fn next_manual_bit(&mut self, strobe: bool) -> bool {
        if strobe && matches!(self.last_strobe_inputs, SnesController::Mouse(_)) {
            // Reading from the mouse while strobe=1 increments sensitivity
            self.mouse_register.increment_sensitivity();
            return false;
        }

        let next_bit = self.manual_joypad_inputs.bit(0);
        self.manual_joypad_inputs =
            (self.manual_joypad_inputs >> 1) | (u32::from(self.end_of_input_bit()) << 31);
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
            auto_read_inputs = (auto_read_inputs << 1) | u16::from(self.next_manual_bit(false));
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
            p1: ControllerPort::new(config),
            p2: ControllerPort::new(config),
            inputs: SnesInputs::default(),
            allow_opposing_directions: config.allow_opposing_joypad_directions,
        }
    }

    pub fn set_strobe(&mut self, strobe: bool) {
        if strobe != self.strobe {
            self.p1.update_strobe(
                strobe,
                self.inputs.p1.with_allow_opposing_directions(self.allow_opposing_directions),
            );
            self.p2.update_strobe(
                strobe,
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
        self.p1.next_manual_bit(self.strobe)
    }

    pub fn next_manual_p2_bit(&mut self) -> bool {
        self.p2.next_manual_bit(self.strobe)
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
        self.p1.mouse_sensitivity = config.mouse_sensitivity;
        self.p2.mouse_sensitivity = config.mouse_sensitivity;
    }
}
