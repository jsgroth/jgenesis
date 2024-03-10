use crate::input::{SnesInputDevice, SnesInputs, SnesJoypadState, SuperScopeState};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::mem;

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
        (u16::from(self.fire) << 15)
            | (u16::from(self.cursor) << 14)
            | (u16::from(self.turbo) << 13)
            | (u16::from(self.pause) << 12)
            | (u16::from(self.offscreen) << 9)
            | 0x00FF
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    auto_read_cycles_remaining: u64,
    auto_joypad_p1_inputs: u16,
    auto_joypad_p2_inputs: u16,
    strobe: bool,
    manual_joypad_p1_inputs: u16,
    manual_joypad_p2_inputs: u16,
    current_inputs: SnesInputs,
    last_strobe_inputs: SnesInputs,
    super_scope_register: SuperScopeRegister,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            auto_read_cycles_remaining: 0,
            auto_joypad_p1_inputs: SnesJoypadState::default().to_register_word(),
            auto_joypad_p2_inputs: SnesJoypadState::default().to_register_word(),
            strobe: false,
            manual_joypad_p1_inputs: SnesJoypadState::default().to_register_word(),
            manual_joypad_p2_inputs: SnesJoypadState::default().to_register_word(),
            current_inputs: SnesInputs::default(),
            last_strobe_inputs: SnesInputs::default(),
            super_scope_register: SuperScopeRegister::default(),
        }
    }

    pub fn set_strobe(&mut self, strobe: bool) {
        if !self.strobe && strobe {
            self.manual_joypad_p1_inputs = self.current_inputs.p1.to_register_word();
            self.manual_joypad_p2_inputs = match self.current_inputs.p2 {
                SnesInputDevice::Controller(joypad_state) => {
                    self.super_scope_register = SuperScopeRegister::default();

                    joypad_state.to_register_word()
                }
                SnesInputDevice::SuperScope(super_scope_state) => {
                    // Read out the bits before updating them; otherwise the SNES will read Fire=1 on the frame before
                    // the PPU latches H/V
                    let word = self.super_scope_register.to_register_word();

                    let last_strobe_state = match self.last_strobe_inputs.p2 {
                        SnesInputDevice::SuperScope(last_state) => last_state,
                        SnesInputDevice::Controller(_) => SuperScopeState::default(),
                    };
                    self.super_scope_register.update(super_scope_state, last_strobe_state);

                    word
                }
            };

            self.last_strobe_inputs = self.current_inputs;
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
        let bit = self.manual_joypad_p1_inputs.bit(15);
        self.manual_joypad_p1_inputs = (self.manual_joypad_p1_inputs << 1) | 0x0001;
        bit
    }

    pub fn next_manual_p2_bit(&mut self) -> bool {
        let bit = self.manual_joypad_p2_inputs.bit(15);
        self.manual_joypad_p2_inputs = (self.manual_joypad_p2_inputs << 1) | 0x0001;
        bit
    }

    pub fn start_auto_joypad_read(&mut self) {
        self.auto_read_cycles_remaining = AUTO_JOYPAD_DURATION_MCLK;
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64, inputs: SnesInputs) {
        self.current_inputs = inputs;

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

            // Drain the manual joypad read registers into the auto joypad read registers
            // Donkey Kong Country depends on the manual joypad read registers reading out 1s after
            // auto joypad read finishes
            self.auto_joypad_p1_inputs = mem::replace(&mut self.manual_joypad_p1_inputs, !0);
            self.auto_joypad_p2_inputs = mem::replace(&mut self.manual_joypad_p2_inputs, !0);
        }
    }

    pub fn hv_latch(&self) -> Option<(u16, u16)> {
        // Super Scope latches the PPU at H=X+40, V=Y+1 when Fire or Cursor is set
        (self.super_scope_register.fire || self.super_scope_register.cursor)
            .then(|| {
                let (x, y) = self.super_scope_register.position?;
                Some((x + 40, y + 1))
            })
            .flatten()
    }
}
