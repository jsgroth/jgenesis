use crate::config::{GenesisConfig, SmsGgConfig};
use crate::renderer::WgpuRenderer;
use crate::{config, genesisinput, smsgginput};
use anyhow::{anyhow, Context};
use bincode::{Decode, Encode};
use genesis_core::{GenesisEmulator, GenesisInputs, GenesisTickEffect};
use jgenesis_traits::frontend::{AudioOutput, PixelAspectRatio, SaveWriter};
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::AudioSubsystem;
use smsgg_core::{SmsGgEmulator, SmsGgInputs, SmsGgTickEffect};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

struct SdlAudioOutput {
    audio_queue: AudioQueue<f32>,
    audio_buffer: Vec<f32>,
}

impl SdlAudioOutput {
    fn create_and_init(audio: &AudioSubsystem) -> anyhow::Result<Self> {
        let audio_queue = audio
            .open_queue(
                None,
                &AudioSpecDesired {
                    freq: Some(48000),
                    channels: Some(2),
                    samples: Some(64),
                },
            )
            .map_err(|err| anyhow!("Error opening SDL2 audio queue: {err}"))?;
        audio_queue.resume();

        Ok(Self {
            audio_queue,
            audio_buffer: Vec::with_capacity(64),
        })
    }
}

impl AudioOutput for SdlAudioOutput {
    type Err = anyhow::Error;

    #[inline]
    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err> {
        self.audio_buffer.push(sample_l as f32);
        self.audio_buffer.push(sample_r as f32);

        if self.audio_buffer.len() == 64 {
            while self.audio_queue.size() >= 1024 * 4 {
                thread::sleep(Duration::from_micros(250));
            }

            self.audio_queue
                .queue_audio(&self.audio_buffer)
                .map_err(|err| anyhow!("Error pushing audio samples: {err}"))?;
            self.audio_buffer.clear();
        }

        Ok(())
    }
}

struct FsSaveWriter {
    path: PathBuf,
}

impl FsSaveWriter {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SaveWriter for FsSaveWriter {
    type Err = anyhow::Error;

    #[inline]
    fn persist_save(&mut self, save_bytes: &[u8]) -> Result<(), Self::Err> {
        fs::write(&self.path, save_bytes)?;
        Ok(())
    }
}

/// Run the SMS/GG core with the given config.
///
/// # Errors
///
/// This function will propagate any video, audio, or disk errors encountered.
#[allow(clippy::missing_panics_doc)]
pub fn run_smsgg(config: SmsGgConfig) -> anyhow::Result<()> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.rom_file_path);
    let rom_file_name = parse_file_name(rom_file_path)?;
    let file_ext = parse_file_ext(rom_file_path)?;

    let save_state_path = rom_file_path.with_extension("ss0");

    let rom = fs::read(rom_file_path)
        .with_context(|| format!("Failed to read ROM file at {}", rom_file_path.display()))?;

    let save_path = rom_file_path.with_extension("sav");
    let initial_cartridge_ram = fs::read(&save_path).ok();

    let vdp_version = config
        .vdp_version
        .unwrap_or_else(|| config::default_vdp_version_for_ext(file_ext));
    let psg_version = config
        .psg_version
        .unwrap_or_else(|| config::default_psg_version_for_ext(file_ext));

    log::info!("VDP version: {vdp_version:?}");
    log::info!("PSG version: {psg_version:?}");

    let sdl = sdl2::init().map_err(|err| anyhow!("Error initializing SDL2: {err}"))?;
    let video = sdl
        .video()
        .map_err(|err| anyhow!("Error initializing SDL2 video subsystem: {err}"))?;
    let audio = sdl
        .audio()
        .map_err(|err| anyhow!("Error initializing SDL2 audio subsystem: {err}"))?;
    let mut event_pump = sdl
        .event_pump()
        .map_err(|err| anyhow!("Error initializing SDL2 event pump: {err}"))?;

    // TODO configurable
    let (window_width, window_height, pixel_aspect_ratio) = if vdp_version.is_master_system() {
        (
            940,
            720,
            PixelAspectRatio::from_width_and_height(
                NonZeroU32::new(8).unwrap(),
                NonZeroU32::new(7).unwrap(),
            ),
        )
    } else {
        (
            3 * 192,
            3 * 144,
            PixelAspectRatio::from_width_and_height(
                NonZeroU32::new(6).unwrap(),
                NonZeroU32::new(5).unwrap(),
            ),
        )
    };
    let window = video
        .window(
            &format!("smsgg - {rom_file_name}"),
            window_width,
            window_height,
        )
        .build()?;

    let mut renderer = pollster::block_on(WgpuRenderer::new(window, config.renderer_config))?;
    let mut audio_output = SdlAudioOutput::create_and_init(&audio)?;
    let mut inputs = SmsGgInputs::default();
    let mut save_writer = FsSaveWriter::new(save_path);

    let mut emulator = SmsGgEmulator::create(
        rom,
        initial_cartridge_ram,
        vdp_version,
        Some(pixel_aspect_ratio),
        psg_version,
        config.remove_sprite_limit,
    );

    loop {
        if emulator.tick(&mut renderer, &mut audio_output, &inputs, &mut save_writer)?
            == SmsGgTickEffect::FrameRendered
        {
            for event in event_pump.poll_iter() {
                smsgginput::update_inputs(&event, &mut inputs);
                handle_hotkeys(&event, &mut emulator, &save_state_path)?;

                match event {
                    Event::Quit { .. }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Run the Genesis core with the given config.
///
/// # Errors
///
/// This function will return an error upon encountering any video, audio, or I/O error.
#[allow(clippy::missing_panics_doc)]
pub fn run_genesis(config: GenesisConfig) -> anyhow::Result<()> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.rom_file_path);
    let rom = fs::read(rom_file_path)?;

    let save_state_path = rom_file_path.with_extension("ss0");

    let mut emulator = GenesisEmulator::from_rom(rom)?;

    let sdl = sdl2::init().map_err(|err| anyhow!("Error initializing SDL2: {err}"))?;
    let video = sdl
        .video()
        .map_err(|err| anyhow!("Error initializing SDL2 video subsystem: {err}"))?;
    let audio = sdl
        .audio()
        .map_err(|err| anyhow!("Error initializing SDL2 audio subsystem: {err}"))?;
    let mut event_pump = sdl
        .event_pump()
        .map_err(|err| anyhow!("Error initializing SDL2 event pump: {err}"))?;

    // TODO configurable
    let window = video
        .window(
            &format!("genesis - {}", emulator.cartridge_title()),
            878,
            672,
        )
        .build()?;

    let mut renderer = pollster::block_on(WgpuRenderer::new(window, config.renderer_config))?;

    let mut audio_output = SdlAudioOutput::create_and_init(&audio)?;

    let mut inputs = GenesisInputs::default();

    loop {
        if emulator.tick(&mut renderer, &mut audio_output, &inputs)?
            == GenesisTickEffect::FrameRendered
        {
            for event in event_pump.poll_iter() {
                genesisinput::update_inputs(&event, &mut inputs);
                handle_hotkeys(&event, &mut emulator, &save_state_path)?;

                match event {
                    Event::Quit { .. }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn parse_file_name(path: &Path) -> anyhow::Result<&str> {
    path.file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow!("Unable to determine file name for path: {}", path.display()))
}

fn parse_file_ext(path: &Path) -> anyhow::Result<&str> {
    path.extension()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow!("Unable to determine extension for path: {}", path.display()))
}

trait TakeRomFrom {
    fn take_rom_from(&mut self, other: &mut Self);
}

impl TakeRomFrom for SmsGgEmulator {
    fn take_rom_from(&mut self, other: &mut Self) {
        self.take_rom_from(other);
    }
}

impl TakeRomFrom for GenesisEmulator {
    fn take_rom_from(&mut self, other: &mut Self) {
        self.take_rom_from(other);
    }
}

fn handle_hotkeys<Emulator, P>(
    event: &Event,
    emulator: &mut Emulator,
    save_state_path: P,
) -> anyhow::Result<()>
where
    Emulator: Encode + Decode + TakeRomFrom,
    P: AsRef<Path>,
{
    let save_state_path = save_state_path.as_ref();

    match event {
        Event::KeyDown {
            keycode: Some(Keycode::F5),
            ..
        } => {
            save_state(emulator, save_state_path)?;
        }
        Event::KeyDown {
            keycode: Some(Keycode::F6),
            ..
        } => {
            let mut loaded_emulator: Emulator = match load_state(save_state_path) {
                Ok(emulator) => emulator,
                Err(err) => {
                    log::error!(
                        "Error loading save state from {}: {err}",
                        save_state_path.display()
                    );
                    return Ok(());
                }
            };
            loaded_emulator.take_rom_from(emulator);
            *emulator = loaded_emulator;
        }
        _ => {}
    }

    Ok(())
}

macro_rules! bincode_config {
    () => {
        bincode::config::standard()
            .with_little_endian()
            .with_fixed_int_encoding()
    };
}

fn save_state<E, P>(emulator: &E, path: P) -> anyhow::Result<()>
where
    E: Encode,
    P: AsRef<Path>,
{
    let path = path.as_ref();

    let mut file = BufWriter::new(File::create(path)?);

    let conf = bincode_config!();
    bincode::encode_into_std_write(emulator, &mut file, conf)?;

    log::info!("Saved state to {}", path.display());

    Ok(())
}

fn load_state<D, P>(path: P) -> anyhow::Result<D>
where
    D: Decode,
    P: AsRef<Path>,
{
    let path = path.as_ref();

    let mut file = BufReader::new(File::open(path)?);

    let conf = bincode_config!();
    let emulator = bincode::decode_from_std_read(&mut file, conf)?;

    log::info!("Loaded state from {}", path.display());

    Ok(emulator)
}
