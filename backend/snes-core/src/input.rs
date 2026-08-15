use bincode::{Decode, Encode};
use jgenesis_common::frontend::{DisplayInfo, MappableInputs, Modal};
use jgenesis_common::input::Player;
use snes_config::{SnesButton, SnesJoypadState, SuperScopeButton};

pub(crate) trait SnesJoypadStateExt: Sized + Copy {
    #[must_use]
    fn to_register_word(self) -> u16;
}

impl SnesJoypadStateExt for SnesJoypadState {
    fn to_register_word(self) -> u16 {
        u16::from(self.b)
            | (u16::from(self.y) << 1)
            | (u16::from(self.select) << 2)
            | (u16::from(self.start) << 3)
            | (u16::from(self.up) << 4)
            | (u16::from(self.down) << 5)
            | (u16::from(self.left) << 6)
            | (u16::from(self.right) << 7)
            | (u16::from(self.a) << 8)
            | (u16::from(self.x) << 9)
            | (u16::from(self.l) << 10)
            | (u16::from(self.r) << 11)
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
pub enum SnesController {
    Gamepad(SnesJoypadState),
    SuperScope(SuperScopeState),
    None,
}

impl SnesController {
    pub fn set_field(&mut self, button: SnesButton, pressed: bool) {
        match self {
            Self::Gamepad(state) => state.set_button(button, pressed),
            Self::SuperScope(state) => {
                if let Some(super_scope_button) = button.to_super_scope() {
                    state.set_button(super_scope_button, pressed);
                }
            }
            Self::None => {}
        }
    }

    #[must_use]
    pub fn with_allow_opposing_directions(self, allow_opposing_directions: bool) -> Self {
        match self {
            Self::Gamepad(joypad) => {
                Self::Gamepad(joypad.with_allow_opposing_directions(allow_opposing_directions))
            }
            Self::SuperScope(_) | Self::None => self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct SnesInputs {
    pub p1: SnesController,
    pub p2: SnesController,
}

impl Default for SnesInputs {
    fn default() -> Self {
        Self { p1: SnesController::Gamepad(SnesJoypadState::default()), p2: SnesController::None }
    }
}

impl MappableInputs<SnesButton> for SnesInputs {
    #[inline]
    fn set_field(&mut self, button: SnesButton, player: Player, pressed: bool) {
        match player {
            Player::One => self.p1.set_field(button, pressed),
            Player::Two => self.p2.set_field(button, pressed),
            _ => {}
        }
    }

    #[inline]
    fn handle_mouse_motion(
        &mut self,
        (x, y): (f32, f32),
        _delta: (f32, f32),
        display_info: DisplayInfo,
    ) {
        for controller in [&mut self.p1, &mut self.p2] {
            if let SnesController::SuperScope(super_scope_state) = controller {
                super_scope_state.position =
                    jgenesis_common::input::viewport_position_to_frame_position(x, y, display_info);
                log::debug!("Set Super Scope position to {:?}", super_scope_state.position);
            }
        }
    }

    #[inline]
    fn handle_mouse_leave(&mut self) {
        for controller in [&mut self.p1, &mut self.p2] {
            if let SnesController::SuperScope(super_scope_state) = controller {
                super_scope_state.position = None;
            }
        }
    }

    fn modal_for_input(&self, button: SnesButton, player: Player, pressed: bool) -> Option<Modal> {
        if button != SnesButton::SuperScopeTurboToggle || !pressed {
            return None;
        }

        let controller = match player {
            Player::One => &self.p1,
            Player::Two => &self.p2,
            _ => return None,
        };
        let SnesController::SuperScope(super_scope_state) = controller else { return None };

        let text =
            format!("Super Scope Turbo: {}", if super_scope_state.turbo { "On" } else { "Off" });
        Some(Modal { id: Some("super_scope_turbo".into()), text })
    }
}
