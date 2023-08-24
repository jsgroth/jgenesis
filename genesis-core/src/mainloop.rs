use crate::input::InputState;
use crate::memory::{Cartridge, MainBus, Memory};
use crate::vdp;
use crate::vdp::{Vdp, VdpTickEffect};
use crate::ym2612::{Ym2612, YmTickEffect};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use m68000_emu::M68000;
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use smsgg_core::psg::{Psg, PsgVersion};
use std::collections::VecDeque;
use std::error::Error;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{process, thread};
use z80_emu::Z80;

pub struct GenesisConfig {
    pub rom_file_path: String,
}

struct AudioOutput {
    audio_buffer: Vec<(f32, f32)>,
    audio_queue: Arc<Mutex<VecDeque<f32>>>,
    sample_count: u64,
}

impl AudioOutput {
    // 53_693_175 / 7 / 6 / 24 / 48000
    const DOWNSAMPLING_RATIO: f64 = 1.109729972718254;

    fn new() -> Self {
        Self {
            audio_buffer: Vec::new(),
            audio_queue: Arc::new(Mutex::new(VecDeque::new())),
            sample_count: 0,
        }
    }

    fn initialize(&self) -> Result<impl StreamTrait, Box<dyn Error>> {
        let callback_queue = Arc::clone(&self.audio_queue);

        let audio_host = cpal::default_host();
        let audio_device = audio_host.default_output_device().unwrap();
        let audio_stream = audio_device.build_output_stream(
            &StreamConfig {
                channels: 2,
                sample_rate: SampleRate(48000),
                buffer_size: BufferSize::Fixed(1024),
            },
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut callback_queue = callback_queue.lock().unwrap();
                for output in data {
                    let Some(sample) = callback_queue.pop_front() else {
                        break;
                    };
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

        Ok(audio_stream)
    }

    fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let prev_count = self.sample_count;
        self.sample_count += 1;

        if (prev_count as f64 / Self::DOWNSAMPLING_RATIO).round() as u64
            != (self.sample_count as f64 / Self::DOWNSAMPLING_RATIO).round() as u64
        {
            self.audio_buffer.push((sample_l as f32, sample_r as f32));
            if self.audio_buffer.len() == 64 {
                loop {
                    {
                        let mut audio_queue = self.audio_queue.lock().unwrap();
                        if audio_queue.len() < 1024 {
                            audio_queue.extend(
                                self.audio_buffer
                                    .drain(..)
                                    .flat_map(|(sample_l, sample_r)| [sample_l, sample_r]),
                            );
                            break;
                        }
                    }

                    thread::sleep(Duration::from_micros(250));
                }
            }
        }
    }
}

// -6dB (10 ^ -6/20)
// PSG is too loud if it's given the same volume level as the YM2612
const PSG_COEFFICIENT: f64 = 0.5011872336272722;

/// # Errors
///
/// # Panics
///
pub fn run(config: GenesisConfig) -> Result<(), Box<dyn Error>> {
    let cartridge = Cartridge::from_file(Path::new(&config.rom_file_path))?;
    let mut memory = Memory::new(cartridge);

    let mut m68k = M68000::new();
    let mut z80 = Z80::new();

    let mut vdp = Vdp::new();
    let mut psg = Psg::new(PsgVersion::Standard);
    let mut ym2612 = Ym2612::new();
    let mut input = InputState::new();

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

    // TODO generalize this
    let mut audio_output = AudioOutput::new();
    let _audio_stream = audio_output.initialize()?;

    let mut debug_window: Option<Window> = None;
    let mut debug_buffer = vec![0; 64 * 32 * 64];
    let mut debug_palette = 0;

    let mut color_window: Option<Window> = None;
    let mut color_buffer = vec![0; 64];

    let mut master_cycles = 0_u64;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let mut bus = MainBus::new(&mut memory, &mut vdp, &mut psg, &mut ym2612, &mut input);
        let m68k_cycles = m68k.execute_instruction(&mut bus);

        let m68k_master_cycles = 7 * u64::from(m68k_cycles);

        let z80_cycles = ((master_cycles + m68k_master_cycles) / 15) - master_cycles / 15;
        for _ in 0..z80_cycles {
            z80.tick(&mut bus);
        }
        master_cycles += m68k_master_cycles;

        for _ in 0..z80_cycles {
            psg.tick();
        }

        for _ in 0..m68k_cycles {
            if ym2612.tick() == YmTickEffect::OutputSample {
                let (ym_sample_l, ym_sample_r) = ym2612.sample();
                let (psg_sample_l, psg_sample_r) = psg.sample();

                // TODO more intelligent PSG mixing
                let sample_l = (ym_sample_l + PSG_COEFFICIENT * psg_sample_l).clamp(-1.0, 1.0);
                let sample_r = (ym_sample_r + PSG_COEFFICIENT * psg_sample_r).clamp(-1.0, 1.0);
                audio_output.collect_sample(sample_l, sample_r);
            }
        }

        if vdp.tick(m68k_master_cycles, &mut memory) == VdpTickEffect::FrameComplete {
            let screen_width = vdp.screen_width();
            populate_minifb_buffer(vdp.frame_buffer(), screen_width, &mut minifb_buffer);
            window.update_with_buffer(&minifb_buffer, screen_width as usize, 224)?;

            let p1 = input.p1_mut();
            p1.up = window.is_key_down(Key::Up);
            p1.left = window.is_key_down(Key::Left);
            p1.right = window.is_key_down(Key::Right);
            p1.down = window.is_key_down(Key::Down);
            p1.a = window.is_key_down(Key::A);
            p1.b = window.is_key_down(Key::S);
            p1.c = window.is_key_down(Key::D);
            p1.start = window.is_key_down(Key::Enter);

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
