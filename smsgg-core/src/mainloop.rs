use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::Memory;
use crate::psg::{Psg, PsgTickEffect, PsgVersion};
use crate::vdp::{FrameBuffer, Vdp, VdpTickEffect, VdpVersion};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use minifb::{Key, Window, WindowOptions};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, process, thread};
use z80_emu::Z80;

#[derive(Debug, Clone)]
pub struct SmsGgConfig {
    pub rom_file_path: String,
    pub vdp_version: Option<VdpVersion>,
    pub psg_version: Option<PsgVersion>,
}

fn default_vdp_version_for_ext(file_ext: &str) -> VdpVersion {
    match file_ext {
        "sms" => VdpVersion::MasterSystem2,
        "gg" => VdpVersion::GameGear,
        _ => {
            log::warn!("Unknown file extension {file_ext}, defaulting to SMS VDP");
            VdpVersion::MasterSystem2
        }
    }
}

fn default_psg_version_for_ext(file_ext: &str) -> PsgVersion {
    match file_ext {
        "sms" => PsgVersion::MasterSystem2,
        _ => PsgVersion::Other,
    }
}

// TODO generalize all this
/// # Panics
///
/// Panics if the file cannot be read
pub fn run(config: SmsGgConfig) {
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

    let vdp_version = config
        .vdp_version
        .unwrap_or_else(|| default_vdp_version_for_ext(file_ext));
    let psg_version = config
        .psg_version
        .unwrap_or_else(|| default_psg_version_for_ext(file_ext));

    log::info!("Using VDP {vdp_version} and PSG {psg_version}");

    let mut window = Window::new(file_name, 3 * 256, 3 * 192, WindowOptions::default()).unwrap();
    window.limit_update_rate(Some(Duration::from_micros(16400)));

    // TODO variable resolutions for Game Gear, 224-line mode, borders, etc.
    let mut minifb_buffer = vec![0_u32; 256 * 192];

    let mut audio_buffer = Vec::new();
    let audio_queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let callback_queue = Arc::clone(&audio_queue);

    let audio_host = cpal::default_host();
    let audio_device = audio_host.default_output_device().unwrap();
    let audio_stream = audio_device
        .build_output_stream(
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
        )
        .unwrap();
    audio_stream.play().unwrap();

    let rom = fs::read(Path::new(&config.rom_file_path)).unwrap();
    let mut memory = Memory::new(rom);

    let mut z80 = Z80::new();
    z80.set_pc(0x0000);
    z80.set_sp(0xDFFF);

    let mut vdp = Vdp::new(vdp_version);
    let mut psg = Psg::new(psg_version);
    let mut input = InputState::new();

    let mut sample_count = 0_u64;
    let downsampling_ratio = 53693175.0 / 15.0 / 16.0 / 48000.0;

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

                vdp_buffer_to_minifb_buffer(vdp_buffer, vdp_version, &mut minifb_buffer);

                window.update_with_buffer(&minifb_buffer, 256, 192).unwrap();

                let p1_input = input.p1();
                p1_input.up = window.is_key_down(Key::Up);
                p1_input.left = window.is_key_down(Key::Left);
                p1_input.right = window.is_key_down(Key::Right);
                p1_input.down = window.is_key_down(Key::Down);
                p1_input.button_1 = window.is_key_down(Key::S);
                p1_input.button_2 = window.is_key_down(Key::A);

                input.set_pause(window.is_key_down(Key::Enter));

                // TODO RESET button
            }
        }
    }
}

fn vdp_buffer_to_minifb_buffer(
    vdp_buffer: &FrameBuffer,
    vdp_version: VdpVersion,
    minifb_buffer: &mut [u32],
) {
    for (i, row) in vdp_buffer[..192].iter().enumerate() {
        for (j, color) in row.iter().copied().enumerate() {
            let (r, g, b) = match vdp_version {
                VdpVersion::MasterSystem2 => (
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

            minifb_buffer[i * 256 + j] = (r << 16) | (g << 8) | b;
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
