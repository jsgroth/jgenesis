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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnesInputs {
    pub p1: SnesJoypadState,
    pub p2: SnesJoypadState,
}
