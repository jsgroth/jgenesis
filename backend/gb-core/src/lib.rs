pub mod api;
mod apu;
mod audio;
mod bus;
mod cartridge;
mod dma;
mod graphics;
pub mod inputs;
mod interrupts;
mod memory;
mod ppu;
mod sm83;
mod speed;
mod timer;

use bincode::{Decode, Encode};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum HardwareMode {
    // Game Boy
    Dmg,
    // Game Boy Color
    Cgb,
}

impl Display for HardwareMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dmg => write!(f, "DMG"),
            Self::Cgb => write!(f, "CGB"),
        }
    }
}
