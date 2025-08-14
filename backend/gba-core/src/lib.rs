pub mod api;
mod apu;
mod bus;
mod cartridge;
mod dma;
mod input;
mod interrupts;
mod memory;
mod ppu;
mod prefetch;
mod sio;
mod timers;

// 16.777216 MHz
const GBA_CLOCK_SPEED: u64 = 1 << 24;
