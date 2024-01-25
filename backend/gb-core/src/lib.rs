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
mod timer;

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum HardwareMode {
    // Game Boy
    Dmg,
    // Game Boy Color
    Cgb,
}
