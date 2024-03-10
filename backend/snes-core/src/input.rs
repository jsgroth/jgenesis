use bincode::{Decode, Encode};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::define_controller_inputs;

define_controller_inputs! {
    button_ident: SnesControllerButton,
    joypad_ident: SnesJoypadState,
    buttons: [Up, Left, Right, Down, A, B, X, Y, L, R, Start, Select],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperScopeButton {
    Fire,
    Cursor,
    Pause,
    TurboToggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnesButton {
    Controller(SnesControllerButton),
    SuperScope(SuperScopeButton),
}

impl SnesJoypadState {
    pub(crate) fn to_register_word(self) -> u16 {
        (u16::from(self.b) << 15)
            | (u16::from(self.y) << 14)
            | (u16::from(self.select) << 13)
            | (u16::from(self.start) << 12)
            | (u16::from(self.up) << 11)
            | (u16::from(self.down) << 10)
            | (u16::from(self.left) << 9)
            | (u16::from(self.right) << 8)
            | (u16::from(self.a) << 7)
            | (u16::from(self.x) << 6)
            | (u16::from(self.l) << 5)
            | (u16::from(self.r) << 4)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct SuperScopeState {
    pub fire: bool,
    pub cursor: bool,
    pub pause: bool,
    pub turbo: bool,
    // X/Y position in SNES pixels starting from the top-left corner, or None if position is offscreen
    // X should be in the range 0..=255 and Y should be in the range 0..=223 (or 238 if in 239-line mode); other values
    // will be treated as offscreen
    pub position: Option<(u16, u16)>,
}

impl Default for SuperScopeState {
    fn default() -> Self {
        Self { fire: false, cursor: false, pause: false, turbo: true, position: None }
    }
}

impl SuperScopeState {
    #[inline]
    pub fn set_button(&mut self, button: SuperScopeButton, pressed: bool) {
        match button {
            SuperScopeButton::Fire => self.fire = pressed,
            SuperScopeButton::Cursor => self.cursor = pressed,
            SuperScopeButton::Pause => self.pause = pressed,
            SuperScopeButton::TurboToggle => {
                if pressed {
                    self.turbo = !self.turbo;
                }
            }
        }
    }

    #[inline]
    #[must_use]
    pub fn with_button(mut self, button: SuperScopeButton, pressed: bool) -> Self {
        self.set_button(button, pressed);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum SnesInputDevice {
    Controller(SnesJoypadState),
    SuperScope(SuperScopeState),
}

impl Default for SnesInputDevice {
    fn default() -> Self {
        Self::Controller(SnesJoypadState::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct SnesInputs {
    pub p1: SnesJoypadState,
    pub p2: SnesInputDevice,
}

impl SnesInputs {
    #[inline]
    pub fn set_button(&mut self, button: SnesButton, player: Player, pressed: bool) {
        match (button, player) {
            (SnesButton::SuperScope(button), _) => match &mut self.p2 {
                SnesInputDevice::SuperScope(super_scope_state) => {
                    super_scope_state.set_button(button, pressed);
                }
                SnesInputDevice::Controller(_) => {}
            },
            (SnesButton::Controller(button), Player::One) => self.p1.set_button(button, pressed),
            (SnesButton::Controller(button), Player::Two) => match &mut self.p2 {
                SnesInputDevice::Controller(joypad_state) => {
                    joypad_state.set_button(button, pressed);
                }
                SnesInputDevice::SuperScope(_) => {}
            },
        }
    }

    #[inline]
    #[must_use]
    pub fn with_button(mut self, button: SnesButton, player: Player, pressed: bool) -> Self {
        self.set_button(button, player, pressed);
        self
    }
}
