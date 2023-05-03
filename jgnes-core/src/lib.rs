#![forbid(unsafe_code)]

mod api;
mod apu;
mod bus;
mod cpu;
mod input;
mod ppu;

pub use api::{
    AudioPlayer, ColorEmphasis, EmulationError, EmulationResult, Emulator, InputPoller, Renderer,
    SaveWriter,
};
pub use input::JoypadState;
pub use ppu::{FrameBuffer, SCREEN_HEIGHT, SCREEN_WIDTH, VISIBLE_SCREEN_HEIGHT};
