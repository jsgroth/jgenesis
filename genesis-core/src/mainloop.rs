use crate::audio::AudioOutput;
use crate::input::InputState;
use crate::memory::{Cartridge, MainBus, Memory};
use crate::vdp::{Vdp, VdpTickEffect};
use crate::ym2612::{Ym2612, YmTickEffect};
use crate::{audio, vdp};
use m68000_emu::M68000;
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use smsgg_core::psg::{Psg, PsgVersion};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::time::Duration;
use z80_emu::Z80;

pub struct GenesisConfig {
    pub rom_file_path: String,
}

/// # Errors
///
/// # Panics
///
pub fn run(config: GenesisConfig) -> anyhow::Result<()> {
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
        &format!("genesis - {}", &memory.cartridge_title()),
        320 * 3,
        224 * 3,
        WindowOptions::default(),
    )?;
    window.limit_update_rate(Some(Duration::from_micros(16000)));

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

    let save_state_path = Path::new(&config.rom_file_path).with_extension("ss0");

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
                let sample_l =
                    (ym_sample_l + audio::PSG_COEFFICIENT * psg_sample_l).clamp(-1.0, 1.0);
                let sample_r =
                    (ym_sample_r + audio::PSG_COEFFICIENT * psg_sample_r).clamp(-1.0, 1.0);
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

            if window.is_key_pressed(Key::F5, KeyRepeat::No) {
                save_state(
                    &save_state_path,
                    &m68k,
                    &z80,
                    &memory,
                    &vdp,
                    &ym2612,
                    &psg,
                    &input,
                    master_cycles,
                )?;
                log::info!("Saved state to {}", save_state_path.display());
            }

            if window.is_key_pressed(Key::F6, KeyRepeat::No) {
                match load_state(&save_state_path) {
                    Ok((
                        loaded_m68k,
                        loaded_z80,
                        mut loaded_memory,
                        loaded_vdp,
                        loaded_ym2612,
                        loaded_psg,
                        loaded_input,
                        loaded_master_cycles,
                    )) => {
                        m68k = loaded_m68k;
                        z80 = loaded_z80;
                        vdp = loaded_vdp;
                        ym2612 = loaded_ym2612;
                        psg = loaded_psg;
                        input = loaded_input;
                        master_cycles = loaded_master_cycles;

                        loaded_memory.take_rom_from(&mut memory);
                        memory = loaded_memory;

                        log::info!("Loaded state from {}", save_state_path.display());
                    }
                    Err(err) => {
                        log::error!(
                            "Unable to load save state from {}: {err}",
                            save_state_path.display()
                        );
                    }
                }
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

macro_rules! bincode_config {
    () => {
        bincode::config::standard()
            .with_little_endian()
            .with_fixed_int_encoding()
    };
}

#[allow(clippy::too_many_arguments)]
fn save_state<P: AsRef<Path>>(
    path: P,
    m68k: &M68000,
    z80: &Z80,
    memory: &Memory,
    vdp: &Vdp,
    ym2612: &Ym2612,
    psg: &Psg,
    input: &InputState,
    master_cycles: u64,
) -> anyhow::Result<()> {
    let path = path.as_ref();
    let mut file = BufWriter::new(File::create(path)?);

    let conf = bincode_config!();

    bincode::encode_into_std_write(m68k, &mut file, conf)?;
    bincode::encode_into_std_write(z80, &mut file, conf)?;
    bincode::encode_into_std_write(memory, &mut file, conf)?;
    bincode::encode_into_std_write(vdp, &mut file, conf)?;
    bincode::encode_into_std_write(ym2612, &mut file, conf)?;
    bincode::encode_into_std_write(psg, &mut file, conf)?;
    bincode::encode_into_std_write(input, &mut file, conf)?;
    bincode::encode_into_std_write(master_cycles, &mut file, conf)?;

    Ok(())
}

#[allow(clippy::type_complexity)]
fn load_state<P: AsRef<Path>>(
    path: P,
) -> anyhow::Result<(M68000, Z80, Memory, Vdp, Ym2612, Psg, InputState, u64)> {
    let path = path.as_ref();
    let mut file = BufReader::new(File::open(path)?);

    let conf = bincode_config!();

    let m68k = bincode::decode_from_std_read(&mut file, conf)?;
    let z80 = bincode::decode_from_std_read(&mut file, conf)?;
    let memory = bincode::decode_from_std_read(&mut file, conf)?;
    let vdp = bincode::decode_from_std_read(&mut file, conf)?;
    let ym2612 = bincode::decode_from_std_read(&mut file, conf)?;
    let psg = bincode::decode_from_std_read(&mut file, conf)?;
    let input = bincode::decode_from_std_read(&mut file, conf)?;
    let master_cycles = bincode::decode_from_std_read(&mut file, conf)?;

    Ok((m68k, z80, memory, vdp, ym2612, psg, input, master_cycles))
}
