#![forbid(unsafe_code)]
// TODO remove when possible
#![allow(dead_code)]
#![allow(unused_variables)]

mod bus;
mod cpu;
mod input;
mod mainloop;
mod ppu;

pub use mainloop::RunError;

/// # Errors
pub fn run(path: &str) -> Result<(), RunError> {
    mainloop::run(path)
}
