#![forbid(unsafe_code)]
// TODO remove when possible
#![allow(dead_code)]
#![allow(unused_variables)]

use crate::bus::{cartridge, Bus};
use crate::cpu::{CpuRegisters, CpuState};
use crate::ppu::PpuState;
use std::error::Error;
use std::path::Path;

mod bus;
mod cpu;
mod ppu;

// TODO clean this up
/// # Errors
pub fn run(path: &str) -> Result<(), Box<dyn Error>> {
    let (cartridge, mapper) = cartridge::from_file(Path::new(path))?;

    let mut bus = Bus::from_cartridge(cartridge, mapper);

    let cpu_registers = CpuRegisters::new(&mut bus.cpu());

    let mut cpu_state = CpuState::new(cpu_registers);
    let mut ppu_state = PpuState::new();

    loop {
        cpu::tick(&mut cpu_state, &mut bus);
        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();
    }
}
