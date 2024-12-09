pub mod api;
mod apu;
mod audio;
mod bus;
mod cartridge;
mod control;
pub mod input;
mod memory;
mod ppu;
mod timers;

const GBA_CLOCK_RATE: u32 = 1 << 24;
