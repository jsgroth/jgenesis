use crate::memory::{Cartridge, MainBus, Memory};
use crate::vdp;
use crate::vdp::{Vdp, VdpTickEffect};
use m68000_emu::M68000;
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use smsgg_core::psg::{Psg, PsgVersion};
use std::error::Error;
use std::ffi::OsStr;
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

    let mut window = Window::new(
        Path::new(&config.rom_file_path)
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap(),
        320 * 3,
        224 * 3,
        WindowOptions::default(),
    )?;
    window.limit_update_rate(Some(Duration::from_micros(16400)));

    let mut minifb_buffer = vec![0_u32; 320 * 224];

    let mut debug_window: Option<Window> = None;
    let mut debug_buffer = vec![0; 64 * 32 * 64];
    let mut debug_palette = 0;

    let mut color_window: Option<Window> = None;
    let mut color_buffer = vec![0; 64];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let m68k_cycles =
            m68k.execute_instruction(&mut MainBus::new(&mut memory, &mut vdp, &mut psg));

        let m68k_master_cycles = 7 * u64::from(m68k_cycles);

        if vdp.tick(m68k_master_cycles, &mut memory) == VdpTickEffect::FrameComplete {
            let screen_width = vdp.screen_width();
            populate_minifb_buffer(vdp.frame_buffer(), screen_width, &mut minifb_buffer);
            window.update_with_buffer(&minifb_buffer, screen_width as usize, 224)?;

            if let Some(debug_window) = &mut debug_window {
                vdp.render_pattern_debug(&mut debug_buffer, debug_palette);
                debug_window.update_with_buffer(&debug_buffer, 64 * 8, 32 * 8)?;
            }

            if let Some(color_window) = &mut color_window {
                vdp.render_color_debug(&mut color_buffer);
                color_window.update_with_buffer(&color_buffer, 64, 1)?;
            }
        }

        if window.is_key_pressed(Key::F2, KeyRepeat::No) && debug_window.is_none() {
            debug_window = Some(Window::new(
                "debug",
                64 * 8 * 3,
                32 * 8 * 3,
                WindowOptions::default(),
            )?);
        }

        if window.is_key_pressed(Key::F3, KeyRepeat::No) && color_window.is_none() {
            color_window = Some(Window::new("debug", 64 * 16, 16, WindowOptions::default())?);
        }

        if debug_window.as_ref().is_some_and(|debug_window| {
            !debug_window.is_open() || debug_window.is_key_down(Key::Escape)
        }) {
            debug_window = None;
        }

        if color_window.as_ref().is_some_and(|color_window| {
            !color_window.is_open() || color_window.is_key_down(Key::Escape)
        }) {
            color_window = None;
        }

        if window.is_key_pressed(Key::Key0, KeyRepeat::No) {
            debug_palette = 0;
        }
        if window.is_key_pressed(Key::Key1, KeyRepeat::No) {
            debug_palette = 1;
        }
        if window.is_key_pressed(Key::Key2, KeyRepeat::No) {
            debug_palette = 2;
        }
        if window.is_key_pressed(Key::Key3, KeyRepeat::No) {
            debug_palette = 3;
        }
    }

    Ok(())
}

fn populate_minifb_buffer(frame_buffer: &[u16], screen_width: u32, minifb_buffer: &mut [u32]) {
    for row in 0_u32..224 {
        for col in 0..screen_width {
            let idx = (row * screen_width + col) as usize;

            let gen_color = frame_buffer[idx];
            let r = vdp::gen_color_to_rgb((gen_color >> 1) & 0x07);
            let g = vdp::gen_color_to_rgb((gen_color >> 5) & 0x07);
            let b = vdp::gen_color_to_rgb((gen_color >> 9) & 0x07);

            minifb_buffer[idx] = (r << 16) | (g << 8) | b;
        }
    }
}
