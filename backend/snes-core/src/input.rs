use bincode::{Decode, Encode};
use jgenesis_common::frontend::{DisplayArea, FrameSize, InputModal, MappableInputs};
use jgenesis_common::input::Player;
use snes_config::{SnesButton, SnesJoypadState, SuperScopeButton};

pub(crate) trait SnesJoypadStateExt: Sized + Copy {
    #[must_use]
    fn to_register_word(self) -> u16;
}

impl SnesJoypadStateExt for SnesJoypadState {
    fn to_register_word(self) -> u16 {
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

impl MappableInputs<SnesButton> for SnesInputs {
    #[inline]
    fn set_field(&mut self, button: SnesButton, player: Player, pressed: bool) {
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
    fn handle_mouse_motion(
        &mut self,
        x: f32,
        y: f32,
        frame_size: FrameSize,
        display_area: DisplayArea,
    ) {
        if let SnesInputDevice::SuperScope(super_scope_state) = &mut self.p2 {
            super_scope_state.position =
                jgenesis_common::input::viewport_position_to_frame_position(
                    x,
                    y,
                    frame_size,
                    display_area,
                );
            log::debug!("Set Super Scope position to {:?}", super_scope_state.position);
        }
    }

    #[inline]
    fn handle_mouse_leave(&mut self) {
        if let SnesInputDevice::SuperScope(super_scope_state) = &mut self.p2 {
            super_scope_state.position = None;
        }
    }

    fn modal_for_input(
        &self,
        button: SnesButton,
        _player: Player,
        pressed: bool,
    ) -> Option<InputModal> {
        if button != SnesButton::SuperScopeTurboToggle || !pressed {
            return None;
        }

        let SnesInputDevice::SuperScope(super_scope_state) = self.p2 else { return None };

        let text =
            format!("Super Scope Turbo: {}", if super_scope_state.turbo { "On" } else { "Off" });
        Some(InputModal { id: Some("super_scope_turbo".into()), text })
    }
}
