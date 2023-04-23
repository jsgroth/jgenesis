#![forbid(unsafe_code)]
// TODO remove when possible
#![allow(dead_code)]
#![allow(unused_variables)]

use crate::bus::{cartridge, Bus};
use crate::cpu::{CpuRegisters, CpuState};
use crate::input::JoypadState;
use crate::ppu::PpuState;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use std::error::Error;
use std::ffi::OsStr;
use std::path::Path;

mod bus;
mod cpu;
mod input;
mod ppu;

// TODO do colors properly
const COLOR_MAPPING: &[u8] = include_bytes!("../../nespalette.pal");

// TODO clean this up
/// # Errors
/// # Panics
pub fn run(path: &str) -> Result<(), Box<dyn Error>> {
    let sdl_ctx = sdl2::init()?;
    let video_subsystem = sdl_ctx.video()?;

    let file_name = Path::new(path).file_name().and_then(OsStr::to_str).unwrap();
    let window = video_subsystem
        .window(
            &format!("jgnes - {file_name}"),
            3 * u32::from(ppu::SCREEN_WIDTH),
            3 * u32::from(ppu::VISIBLE_SCREEN_HEIGHT),
        )
        .build()?;
    let mut canvas = window.into_canvas().present_vsync().build()?;

    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas.present();

    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(
        PixelFormatEnum::RGB24,
        ppu::SCREEN_WIDTH.into(),
        ppu::VISIBLE_SCREEN_HEIGHT.into(),
    )?;

    let mut event_pump = sdl_ctx.event_pump()?;

    let (cartridge, mapper) = cartridge::from_file(Path::new(path))?;

    let mut bus = Bus::from_cartridge(cartridge, mapper);

    let cpu_registers = CpuRegisters::create(&mut bus.cpu());

    let mut cpu_state = CpuState::new(cpu_registers);
    let mut ppu_state = PpuState::new();
    let mut joypad_state = JoypadState::new();

    let mut count = 0;
    loop {
        let prev_in_vblank = ppu_state.in_vblank();

        cpu::tick(&mut cpu_state, &mut bus);
        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        ppu::tick(&mut ppu_state, &mut bus.ppu());
        bus.tick();

        if !prev_in_vblank && ppu_state.in_vblank() {
            let frame_buffer = ppu_state.frame_buffer();
            texture.with_lock(None, |pixels, pitch| {
                for (i, scanline) in frame_buffer[8..232].iter().enumerate() {
                    for (j, nes_color) in scanline.iter().copied().enumerate() {
                        let color_map_index = (3 * nes_color) as usize;
                        let start = i * pitch + 3 * j;
                        pixels[start..start + 3]
                            .copy_from_slice(&COLOR_MAPPING[color_map_index..color_map_index + 3]);
                    }
                }
            })?;

            canvas.clear();
            canvas.copy(&texture, None, None)?;
            canvas.present();

            for event in event_pump.poll_iter() {
                match event {
                    Event::Quit { .. }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => {
                        return Ok(());
                    }
                    Event::KeyDown {
                        keycode: Some(keycode),
                        ..
                    } => {
                        joypad_state.key_down(keycode);
                    }
                    Event::KeyUp {
                        keycode: Some(keycode),
                        ..
                    } => {
                        joypad_state.key_up(keycode);
                    }
                    _ => {}
                }
            }

            bus.update_joypad_state(joypad_state);
        }

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
