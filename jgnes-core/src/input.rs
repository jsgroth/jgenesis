#[derive(Debug, Clone, Copy, Default)]
pub struct JoypadState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub a: bool,
    pub b: bool,
    pub start: bool,
    pub select: bool,
}

impl JoypadState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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

#[derive(Debug, Clone, Copy)]
pub struct LatchedJoypadState(u8);

impl LatchedJoypadState {
    pub fn next_bit(self) -> u8 {
        self.0 & 0x01
    }

    #[must_use]
    pub fn shift(self) -> Self {
        Self((self.0 >> 1) | 0x80)
    }
}
