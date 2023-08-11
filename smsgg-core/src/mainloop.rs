use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::Memory;
use crate::vdp::{FrameBuffer, TickEffect, Vdp, VdpVersion};
use minifb::{Key, Window, WindowOptions};
use std::fs;
use std::path::Path;
use std::time::Duration;
use z80_emu::Z80;

// TODO generalize all this
/// # Panics
///
/// Panics if the file cannot be read
pub fn run(path: &str) {
    let file_name = Path::new(path).file_name().unwrap().to_str().unwrap();

    let mut window = Window::new(file_name, 3 * 256, 3 * 192, WindowOptions::default()).unwrap();
    window.limit_update_rate(Some(Duration::from_micros(16600)));

    let mut minifb_buffer = vec![0_u32; 256 * 192];

    let rom = fs::read(Path::new(path)).unwrap();
    let mut memory = Memory::new(rom);

    let mut z80 = Z80::new();
    z80.set_pc(0x0000);
    z80.set_sp(0xDFFF);

    let mut vdp = Vdp::new(VdpVersion::MasterSystem);

    let mut input = InputState::new();

    let mut leftover_vdp_cycles = 0;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let t_cycles = z80.execute_instruction(&mut Bus::new(&mut memory, &mut vdp, &mut input))
            + leftover_vdp_cycles;

        leftover_vdp_cycles = t_cycles % 2;

        let vdp_cycles = t_cycles / 2 * 3;
        for _ in 0..vdp_cycles {
            if vdp.tick() == TickEffect::FrameComplete {
                let vdb_buffer = vdp.frame_buffer();

                vdp_buffer_to_minifb_buffer(vdb_buffer, &mut minifb_buffer);

                window.update_with_buffer(&minifb_buffer, 256, 192).unwrap();

                let p1_input = input.p1();
                p1_input.up = window.is_key_down(Key::Up);
                p1_input.left = window.is_key_down(Key::Left);
                p1_input.right = window.is_key_down(Key::Right);
                p1_input.down = window.is_key_down(Key::Down);
                p1_input.button_1 = window.is_key_down(Key::S);
                p1_input.button_2 = window.is_key_down(Key::A);
            }
        }
    }
}

fn vdp_buffer_to_minifb_buffer(vdp_buffer: &FrameBuffer, minifb_buffer: &mut [u32]) {
    for (i, row) in vdp_buffer[..192].iter().enumerate() {
        for (j, sms_color) in row.iter().copied().enumerate() {
            let r = convert_sms_color(sms_color & 0x03);
            let g = convert_sms_color((sms_color >> 2) & 0x03);
            let b = convert_sms_color((sms_color >> 4) & 0x03);

            minifb_buffer[i * 256 + j] = (r << 16) | (g << 8) | b;
        }
    }
}

fn convert_sms_color(color: u8) -> u32 {
    [0, 85, 170, 255][color as usize]
}
