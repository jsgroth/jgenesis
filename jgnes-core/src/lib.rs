#![forbid(unsafe_code)]

mod api;
mod apu;
pub mod audio;
mod bus;
mod cpu;
mod input;
mod num;
mod ppu;
mod serialize;

pub use api::{
    AudioPlayer, ColorEmphasis, EmulationError, EmulationResult, EmulationState, Emulator,
    EmulatorConfig, InputPoller, Renderer, SaveWriter, TickEffect,
};
pub use bus::TimingMode;
pub use input::JoypadState;
pub use ppu::{FrameBuffer, SCREEN_HEIGHT, SCREEN_WIDTH};
