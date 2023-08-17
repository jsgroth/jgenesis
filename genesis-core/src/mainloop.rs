use crate::memory::{Cartridge, MainBus, Memory};
use crate::vdp::Vdp;
use m68000_emu::M68000;
use minifb::{Key, Window, WindowOptions};
use smsgg_core::psg::{Psg, PsgVersion};
use std::error::Error;
use std::path::Path;
use std::time::Duration;

pub struct GenesisConfig {
    pub rom_file_path: String,
}

/// # Errors
///
/// # Panics
///
pub fn run(config: GenesisConfig) -> Result<(), Box<dyn Error>> {
    let cartridge = Cartridge::from_file(Path::new(&config.rom_file_path))?;
    let mut memory = Memory::new(cartridge);

    let mut m68k = M68000::new();
    let mut vdp = Vdp::new();
    let mut psg = Psg::new(PsgVersion::Standard);

    // Genesis cartridges store the initial stack pointer in the first 4 bytes and the entry point
    // in the next 4 bytes
    m68k.set_supervisor_stack_pointer(memory.read_rom_u32(0));
    m68k.set_pc(memory.read_rom_u32(4));

    let mut window = Window::new("genesis", 3 * 320, 3 * 224, WindowOptions::default())?;
    window.limit_update_rate(Some(Duration::from_micros(16400)));

    let mut master_cycles = 0_u64;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let m68k_cycles =
            m68k.execute_instruction(&mut MainBus::new(&mut memory, &mut vdp, &mut psg));

        let m68k_master_cycles = 7 * m68k_cycles;

        // TODO render real things
        let prev_master_cycles = master_cycles;
        master_cycles += u64::from(m68k_master_cycles);
        if prev_master_cycles / 896040 != master_cycles / 896040 {
            window.update_with_buffer(&[0x00FF0000, 0x0000FF00, 0x000000FF, 0x00000000], 2, 2)?;
        }
    }

    Ok(())
}
