use bincode::{Decode, Encode};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::define_controller_inputs;

define_controller_inputs! {
    enum SnesButton {
        Up,
        Left,
        Right,
        Down,
        A,
        B,
        X,
        Y,
        L,
        R,
        Start,
        Select,
        #[on_console]
        SuperScopeFire,
        #[on_console]
        SuperScopeCursor,
        #[on_console]
        SuperScopePause,
        #[on_console]
        SuperScopeTurboToggle,
    }

    struct SnesJoypadState {
        buttons!
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperScopeButton {
    Fire,
    Cursor,
    Pause,
    TurboToggle,
}

impl SnesButton {
    #[must_use]
    pub fn to_super_scope(self) -> Option<SuperScopeButton> {
        match self {
            Self::SuperScopeFire => Some(SuperScopeButton::Fire),
            Self::SuperScopeCursor => Some(SuperScopeButton::Cursor),
            Self::SuperScopePause => Some(SuperScopeButton::Pause),
            Self::SuperScopeTurboToggle => Some(SuperScopeButton::TurboToggle),
            _ => None,
        }
    }
}

impl SuperScopeButton {
    #[must_use]
    pub fn to_snes_button(self) -> SnesButton {
        match self {
            Self::Fire => SnesButton::SuperScopeFire,
            Self::Cursor => SnesButton::SuperScopeCursor,
            Self::Pause => SnesButton::SuperScopePause,
            Self::TurboToggle => SnesButton::SuperScopeTurboToggle,
        }
    }
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
        if let Some(super_scope_button) = button.to_super_scope() {
            if let SnesInputDevice::SuperScope(super_scope_state) = &mut self.p2 {
                super_scope_state.set_button(super_scope_button, pressed);
            }
            return;
        }

        match player {
            Player::One => {
                self.p1.set_button(button, pressed);
            }
            Player::Two => {
                if let SnesInputDevice::Controller(joypad_state) = &mut self.p2 {
                    joypad_state.set_button(button, pressed);
                }
            }
        }
    }

    #[inline]
    #[must_use]
    pub fn with_button(mut self, button: SnesButton, player: Player, pressed: bool) -> Self {
        self.set_button(button, player, pressed);
        self
    }
}
