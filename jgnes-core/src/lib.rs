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

    let cpu_registers = CpuRegisters::create(&mut bus.cpu());

    let mut cpu_state = CpuState::new(cpu_registers);
    let mut ppu_state = PpuState::new();

    let mut count = 0;
    loop {
        cpu::tick(&mut cpu_state, &mut bus);
        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        // TODO scaffolding for printing test ROM output, remove at some point
        count += 1;
        if count % 1000000 == 0
            && [0x6001, 0x6002, 0x6003].map(|address| bus.cpu().read_address(address))
                == [0xDE, 0xB0, 0x61]
        {
            let mut buf = String::new();
            let mut address = 0x6004;
            loop {
                let value = bus.cpu().read_address(address);
                if value == 0 {
                    break;
                }

                buf.push(char::from(value));
                address += 1;
            }
            log::info!("{}", buf);
        }
    }
}
