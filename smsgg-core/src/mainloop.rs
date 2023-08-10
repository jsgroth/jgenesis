use crate::bus::Bus;
use crate::memory::Memory;
use crate::vdp::{FrameBuffer, TickEffect, Vdp, VdpVersion};
use minifb::{Key, Window, WindowOptions};
use std::fs;
use std::path::Path;
use z80_emu::Z80;

// TODO generalize all this
pub fn run(path: &str) {
    let file_name = Path::new(path).file_name().unwrap().to_str().unwrap();

    let mut window = Window::new(file_name, 512, 384, WindowOptions::default()).unwrap();

    let mut minifb_buffer = vec![0_u32; 256 * 192];

    let rom = fs::read(Path::new(path)).unwrap();
    let mut memory = Memory::new(rom);

    let mut z80 = Z80::new();
    z80.set_pc(0x0000);
    z80.set_sp(0xDFFF);

    let mut vdp = Vdp::new(VdpVersion::MasterSystem);

    let mut leftover_vdp_cycles = 0;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let t_cycles =
            z80.execute_instruction(&mut Bus::new(&mut memory, &mut vdp)) + leftover_vdp_cycles;

        leftover_vdp_cycles = t_cycles % 2;

        let vdp_cycles = t_cycles / 2 * 3;
        for _ in 0..vdp_cycles {
            if vdp.tick() == TickEffect::FrameComplete {
                let vdb_buffer = vdp.frame_buffer();

                vdp_buffer_to_minifb_buffer(vdb_buffer, &mut minifb_buffer);

                window.update_with_buffer(&minifb_buffer, 256, 192).unwrap();
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

            minifb_buffer[i * 256 + j] = (b << 16) | (g << 8) | r;
        }
    }
}

fn convert_sms_color(color: u8) -> u32 {
    [0, 85, 170, 255][color as usize]
}
