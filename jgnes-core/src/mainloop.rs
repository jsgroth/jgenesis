use crate::apu::{ApuConfig, ApuState};
use crate::bus::cartridge::CartridgeFileError;
use crate::bus::{cartridge, Bus};
use crate::cpu::{CpuRegisters, CpuState};
use crate::input::JoypadState;
use crate::ppu::PpuState;
use crate::{apu, cpu, ppu};
use sdl2::audio::AudioSpecDesired;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::render::TextureValueError;
use sdl2::video::WindowBuildError;
use sdl2::IntegerOrSdlError;
use std::ffi::OsStr;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunError {
    #[error("SDL2 error: {msg}")]
    SdlString { msg: String },
    #[error("error loading cartridge: {source}")]
    Cartridge {
        #[from]
        source: CartridgeFileError,
    },
    #[error("SDL2 error: {source}")]
    SdlInteger {
        #[from]
        source: IntegerOrSdlError,
    },
    #[error("error creating SDL2 window: {source}")]
    WindowCreation {
        #[from]
        source: WindowBuildError,
    },
    #[error("error creating SDL2 texture: {source}")]
    TextureCreation {
        #[from]
        source: TextureValueError,
    },
}

impl From<String> for RunError {
    fn from(value: String) -> Self {
        Self::SdlString { msg: value }
    }
}

// TODO do colors properly
const COLOR_MAPPING: &[u8] = include_bytes!("../../nespalette.pal");

// TODO clean this up
/// # Errors
/// # Panics
pub fn run(path: &str) -> Result<(), RunError> {
    let sdl_ctx = sdl2::init()?;
    let video_subsystem = sdl_ctx.video()?;
    let audio_subsystem = sdl_ctx.audio()?;

    sdl_ctx.mouse().show_cursor(false);

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

    let audio_queue = audio_subsystem.open_queue::<f32, _>(
        None,
        &AudioSpecDesired {
            channels: Some(2),
            freq: Some(48000),
            samples: Some(1024),
        },
    )?;
    audio_queue.resume();

    let mut event_pump = sdl_ctx.event_pump()?;

    let mapper = cartridge::from_file(Path::new(path))?;

    let mut bus = Bus::from_cartridge(mapper);

    let cpu_registers = CpuRegisters::create(&mut bus.cpu());

    let mut cpu_state = CpuState::new(cpu_registers);
    let mut ppu_state = PpuState::new();
    let mut apu_state = ApuState::new();
    let mut apu_config = ApuConfig::new();
    let mut joypad_state = JoypadState::new();

    let mut count = 0;
    loop {
        let prev_in_vblank = ppu_state.in_vblank();

        cpu::tick(&mut cpu_state, &mut bus);
        apu::tick(&mut apu_state, &apu_config, &mut bus.cpu());
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

                        match keycode {
                            Keycode::Num1 => {
                                apu_config.ch1_enabled = !apu_config.ch1_enabled;
                            }
                            Keycode::Num2 => {
                                apu_config.ch2_enabled = !apu_config.ch2_enabled;
                            }
                            Keycode::Num3 => {
                                apu_config.ch3_enabled = !apu_config.ch3_enabled;
                            }
                            Keycode::Num4 => {
                                apu_config.ch4_enabled = !apu_config.ch4_enabled;
                            }
                            Keycode::Num5 => {
                                apu_config.ch5_enabled = !apu_config.ch5_enabled;
                            }
                            _ => {}
                        }
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

            let sample_queue = apu_state.get_sample_queue_mut();
            if !sample_queue.is_empty() {
                let samples: Vec<_> = sample_queue.drain(..).collect();
                audio_queue.queue_audio(&samples)?;
            }
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
