use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::{DisplayArea, FrameSize, MappableInputs};
use jgenesis_common::input::Player;

define_controller_inputs! {
    buttons: NesButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        Start -> start,
        Select -> select,
    },
    non_gamepad_buttons: [ZapperFire, ZapperForceOffscreen],
    joypad: NesJoypadState,
}

impl NesButton {
    #[inline]
    #[must_use]
    pub fn is_zapper(self) -> bool {
        matches!(self, Self::ZapperFire | Self::ZapperForceOffscreen)
    }
}

impl NesJoypadState {
    /// Prevent left+right or up+down from being pressed simultaneously from the NES's perspective.
    ///
    /// If left+right are pressed simultaneously, left will be preferred.
    /// If up+down are pressed simultaneously, up will be preferred.
    #[must_use]
    pub(crate) fn sanitize_opposing_directions(self) -> Self {
        let mut sanitized = self;

        if self.left && self.right {
            // Arbitrarily prefer left
            sanitized.right = false;
        }

        if self.up && self.down {
            // Arbitrarily prefer up
            sanitized.down = false;
        }

        sanitized
    }

    pub(crate) fn latch(self) -> LatchedJoypadState {
        let bitstream = (u8::from(self.right) << 7)
            | (u8::from(self.left) << 6)
            | (u8::from(self.down) << 5)
            | (u8::from(self.up) << 4)
            | (u8::from(self.start) << 3)
            | (u8::from(self.select) << 2)
            | (u8::from(self.b) << 1)
            | u8::from(self.a);
        LatchedJoypadState(bitstream)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct ZapperState {
    pub fire: bool,
    pub force_offscreen: bool,
    // Position in NES pixels, or None if offscreen
    // X value should be in the range 0..=255
    // Y value should be in the range 0..=223 (NTSC) or 0..=239 (PAL)
    // Out-of-bounds values will be considered offscreen
    pub position: Option<(u16, u16)>,
}

impl ZapperState {
    pub(crate) fn position(self) -> Option<(u16, u16)> {
        if self.force_offscreen { None } else { self.position }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum NesInputDevice {
    Controller(NesJoypadState),
    Zapper(ZapperState),
}

impl Default for NesInputDevice {
    fn default() -> Self {
        Self::Controller(NesJoypadState::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct NesInputs {
    pub p1: NesJoypadState,
    pub p2: NesInputDevice,
}

impl MappableInputs<NesButton> for NesInputs {
    #[inline]
    fn set_field(&mut self, button: NesButton, player: Player, pressed: bool) {
        match (button, player) {
            (NesButton::ZapperFire | NesButton::ZapperForceOffscreen, _) => {
                if let NesInputDevice::Zapper(zapper_state) = &mut self.p2 {
                    match button {
                        NesButton::ZapperFire => zapper_state.fire = pressed,
                        NesButton::ZapperForceOffscreen => zapper_state.force_offscreen = pressed,
                        _ => {}
                    }
                }
            }
            (button, Player::One) => self.p1.set_button(button, pressed),
            (button, Player::Two) => {
                if let NesInputDevice::Controller(joypad_state) = &mut self.p2 {
                    joypad_state.set_button(button, pressed);
                }
            }
        }
    }

    #[inline]
    fn handle_mouse_motion(
        &mut self,
        x: i32,
        y: i32,
        frame_size: FrameSize,
        display_area: DisplayArea,
    ) {
        if let NesInputDevice::Zapper(zapper_state) = &mut self.p2 {
            zapper_state.position = jgenesis_common::input::viewport_position_to_frame_position(
                x,
                y,
                frame_size,
                display_area,
            );
            log::debug!("Set Zapper position to {:?}", zapper_state.position);
        }
    }

    #[inline]
    fn handle_mouse_leave(&mut self) {
        if let NesInputDevice::Zapper(zapper_state) = &mut self.p2 {
            zapper_state.position = None;
        }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub(crate) struct LatchedJoypadState(u8);

impl LatchedJoypadState {
    pub fn next_bit(self) -> u8 {
        self.0 & 0x01
    }

    #[must_use]
    pub fn shift(self) -> Self {
        Self((self.0 >> 1) | 0x80)
    }
}
