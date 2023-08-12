use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::Memory;
use crate::psg::{Psg, PsgTickEffect, PsgVersion};
use crate::vdp::{FrameBuffer, Vdp, VdpTickEffect, VdpVersion};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use minifb::{Key, Window, WindowOptions};
use std::collections::VecDeque;
use std::error::Error;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, process, thread};
use z80_emu::{InterruptMode, Z80};

#[derive(Debug, Clone)]
pub struct SmsGgConfig {
    pub rom_file_path: String,
    pub vdp_version: Option<VdpVersion>,
    pub psg_version: Option<PsgVersion>,
    pub crop_sms_vertical_border: bool,
    pub crop_sms_left_border: bool,
}

fn default_vdp_version_for_ext(file_ext: &str) -> VdpVersion {
    match file_ext {
        "sms" => VdpVersion::NtscMasterSystem2,
        "gg" => VdpVersion::GameGear,
        _ => {
            log::warn!("Unknown file extension {file_ext}, defaulting to NTSC SMS VDP");
            VdpVersion::NtscMasterSystem2
        }
    }
}

fn default_psg_version_for_ext(file_ext: &str) -> PsgVersion {
    match file_ext {
        "sms" => PsgVersion::MasterSystem2,
        _ => PsgVersion::Standard,
    }
}

const NTSC_MASTER_CLOCK: f64 = 53693175.0;
const PAL_MASTER_CLOCK: f64 = 53203424.0;

// TODO generalize all this
/// # Errors
///
/// Returns an error if the file cannot be read or there is a video/audio error
///
/// # Panics
///
/// Panics if unable to determine filename or initialize audio (TODO: should be an error)
pub fn run(config: SmsGgConfig) -> Result<(), Box<dyn Error>> {
    log::info!("Running with config: {config:?}");

    let file_name = Path::new(&config.rom_file_path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let file_ext = Path::new(&config.rom_file_path)
        .extension()
        .unwrap()
        .to_str()
        .unwrap();

    let sav_file_path = Path::new(&config.rom_file_path).with_extension("sav");

    let vdp_version = config
        .vdp_version
        .unwrap_or_else(|| default_vdp_version_for_ext(file_ext));
    let psg_version = config
        .psg_version
        .unwrap_or_else(|| default_psg_version_for_ext(file_ext));

    log::info!("Using VDP {vdp_version} and PSG {psg_version}");

    let viewport = vdp_version.viewport_size();

    let window_height = if config.crop_sms_vertical_border {
        viewport.height_without_border()
    } else {
        viewport.height
    };
    let window_width = if config.crop_sms_left_border {
        viewport.width_without_border()
    } else {
        viewport.width
    };

    let mut window = Window::new(
        file_name,
        3 * window_width as usize,
        3 * window_height as usize,
        WindowOptions::default(),
    )?;
    window.limit_update_rate(Some(Duration::from_micros(16400)));

    let mut minifb_buffer = vec![0_u32; window_width as usize * window_height as usize];

    let mut audio_buffer = Vec::new();
    let audio_queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let callback_queue = Arc::clone(&audio_queue);

    let audio_host = cpal::default_host();
    let audio_device = audio_host.default_output_device().unwrap();
    let audio_stream = audio_device.build_output_stream(
        &StreamConfig {
            // TODO stereo sound for Game Gear
            channels: 1,
            sample_rate: SampleRate(48000),
            buffer_size: BufferSize::Fixed(1024),
        },
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut callback_queue = callback_queue.lock().unwrap();
            for output in data {
                let Some(sample) = callback_queue.pop_front() else { break };
                *output = sample;
            }
        },
        move |err| {
            log::error!("Audio error: {err}");
            process::exit(1);
        },
        None,
    )?;
    audio_stream.play()?;

    let rom = fs::read(Path::new(&config.rom_file_path))?;
    let initial_cartridge_ram = fs::read(&sav_file_path).ok();
    let mut memory = Memory::new(rom, initial_cartridge_ram);

    let mut z80 = Z80::new();
    z80.set_pc(0x0000);
    z80.set_sp(0xDFFF);
    z80.set_interrupt_mode(InterruptMode::Mode1);

    let mut vdp = Vdp::new(vdp_version);
    let mut psg = Psg::new(psg_version);
    let mut input = InputState::new();

    let mut sample_count = 0_u64;
    let master_clock = match vdp_version {
        VdpVersion::NtscMasterSystem2 | VdpVersion::GameGear => NTSC_MASTER_CLOCK,
        VdpVersion::PalMasterSystem2 => PAL_MASTER_CLOCK,
    };
    let downsampling_ratio = master_clock / 15.0 / 16.0 / 48000.0;

    let mut frame_count = 0_u64;
    let mut last_save_frame_count = 0_u64;

    let mut leftover_vdp_cycles = 0;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let t_cycles = z80.execute_instruction(&mut Bus::new(
            vdp_version,
            &mut memory,
            &mut vdp,
            &mut psg,
            &mut input,
        ));

        for _ in 0..t_cycles {
            if psg.tick() == PsgTickEffect::Clocked {
                let sample = psg.sample();

                let prev_count = sample_count;
                sample_count += 1;

                if (prev_count as f64 / downsampling_ratio).round() as u64
                    != (sample_count as f64 / downsampling_ratio).round() as u64
                {
                    audio_buffer.push(sample as f32);
                    if audio_buffer.len() == 64 {
                        loop {
                            {
                                let mut audio_queue = audio_queue.lock().unwrap();
                                if audio_queue.len() < 1024 {
                                    audio_queue.extend(audio_buffer.drain(..));
                                    break;
                                }
                            }

                            thread::sleep(Duration::from_micros(250));
                        }
                    }
                }
            }
        }

        let t_cycles_plus_leftover = t_cycles + leftover_vdp_cycles;
        leftover_vdp_cycles = t_cycles_plus_leftover % 2;

        let vdp_cycles = t_cycles_plus_leftover / 2 * 3;
        for _ in 0..vdp_cycles {
            if vdp.tick() == VdpTickEffect::FrameComplete {
                let vdp_buffer = vdp.frame_buffer();

                vdp_buffer_to_minifb_buffer(
                    vdp_buffer,
                    vdp_version,
                    window_width as usize,
                    config.crop_sms_vertical_border,
                    config.crop_sms_left_border,
                    &mut minifb_buffer,
                );

                window
                    .update_with_buffer(
                        &minifb_buffer,
                        window_width as usize,
                        window_height as usize,
                    )
                    .unwrap();

                let p1_input = input.p1();
                p1_input.up = window.is_key_down(Key::Up);
                p1_input.left = window.is_key_down(Key::Left);
                p1_input.right = window.is_key_down(Key::Right);
                p1_input.down = window.is_key_down(Key::Down);
                p1_input.button_1 = window.is_key_down(Key::S);
                p1_input.button_2 = window.is_key_down(Key::A);

                input.set_pause(window.is_key_down(Key::Enter));

                // TODO RESET button

                frame_count += 1;
                if memory.cartridge_has_battery()
                    && memory.cartridge_ram_dirty()
                    && (last_save_frame_count - frame_count) >= 60
                {
                    last_save_frame_count = frame_count;
                    memory.clear_cartridge_ram_dirty();

                    fs::write(Path::new(&sav_file_path), memory.cartridge_ram())?;
                }
            }
        }
    }

    Ok(())
}

fn vdp_buffer_to_minifb_buffer(
    vdp_buffer: &FrameBuffer,
    vdp_version: VdpVersion,
    window_width: usize,
    crop_vertical_border: bool,
    crop_left_border: bool,
    minifb_buffer: &mut [u32],
) {
    let viewport = vdp_version.viewport_size();

    let (row_skip, row_take) = if crop_vertical_border {
        (
            viewport.top_border_height as usize,
            viewport.height_without_border() as usize,
        )
    } else {
        (0, viewport.height as usize)
    };
    let col_skip = if crop_left_border {
        viewport.left_border_width as usize
    } else {
        0
    };

    for (i, row) in vdp_buffer.iter().skip(row_skip).take(row_take).enumerate() {
        for (j, color) in row.iter().copied().skip(col_skip).enumerate() {
            let (r, g, b) = match vdp_version {
                VdpVersion::NtscMasterSystem2 | VdpVersion::PalMasterSystem2 => (
                    convert_sms_color(color & 0x03),
                    convert_sms_color((color >> 2) & 0x03),
                    convert_sms_color((color >> 4) & 0x03),
                ),
                VdpVersion::GameGear => (
                    convert_gg_color(color & 0x0F),
                    convert_gg_color((color >> 4) & 0x0F),
                    convert_gg_color((color >> 8) & 0x0F),
                ),
            };

            minifb_buffer[i * window_width + j] = (r << 16) | (g << 8) | b;
        }
    }
}

fn convert_sms_color(color: u16) -> u32 {
    [0, 85, 170, 255][color as usize]
}

fn convert_gg_color(color: u16) -> u32 {
    [
        0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
    ][color as usize]
}
