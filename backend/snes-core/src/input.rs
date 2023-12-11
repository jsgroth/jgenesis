use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct SnesJoypadState {
    pub up: bool,
    pub left: bool,
    pub right: bool,
    pub down: bool,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub l: bool,
    pub r: bool,
    pub start: bool,
    pub select: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct SnesInputs {
    pub p1: SnesJoypadState,
    pub p2: SnesInputDevice,
}
