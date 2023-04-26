#![forbid(unsafe_code)]

mod apu;
mod bus;
mod cpu;
mod input;
mod mainloop;
mod ppu;
mod ppu2;

pub use mainloop::RunError;

/// # Errors
pub fn run(path: &str) -> Result<(), RunError> {
    mainloop::run(path)
}
