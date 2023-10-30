#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnesInputs {
    pub p1: SnesJoypadState,
    pub p2: SnesJoypadState,
}
