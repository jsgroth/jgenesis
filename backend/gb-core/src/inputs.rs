//! Game Boy input handling

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct GameBoyInputs {
    pub up: bool,
    pub left: bool,
    pub right: bool,
    pub down: bool,
    pub a: bool,
    pub b: bool,
    pub start: bool,
    pub select: bool,
}
