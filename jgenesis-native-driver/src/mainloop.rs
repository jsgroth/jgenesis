use crate::config::SmsGgConfig;
use crate::renderer::config::{FilterMode, PrescaleFactor, RendererConfig, VSyncMode};
use crate::renderer::WgpuRenderer;
use crate::{config, smsgginput};
use anyhow::{anyhow, Context};
use jgenesis_traits::frontend::{AudioOutput, PixelAspectRatio, SaveWriter};
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::AudioSubsystem;
use smsgg_core::{SmsGgEmulator, SmsGgInputs, SmsGgTickEffect};
use std::ffi::OsStr;
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
    let rom_file_path = Path::new(&config.rom_file_path);
    let Some(rom_file_name) = rom_file_path.file_name().and_then(OsStr::to_str) else {
        return Err(anyhow!(
            "Unable to determine file name for path: {}",
            rom_file_path.display()
        ));
    };
    let Some(file_ext) = rom_file_path.extension().and_then(OsStr::to_str) else {
        return Err(anyhow!(
            "Unable to determine file extension for path: {}",
            rom_file_path.display()
        ));
    };

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
    let (window_width, window_height, pixel_aspect_ratio) = match file_ext {
        "gg" => (
            3 * 192,
            3 * 144,
            PixelAspectRatio::from_width_and_height(
                NonZeroU32::new(6).unwrap(),
                NonZeroU32::new(5).unwrap(),
            ),
        ),
        _ => (
            940,
            720,
            PixelAspectRatio::from_width_and_height(
                NonZeroU32::new(8).unwrap(),
                NonZeroU32::new(7).unwrap(),
            ),
        ),
    };
    let window = video
        .window(
            &format!("smsgg - {rom_file_name}"),
            window_width,
            window_height,
        )
        .build()?;

    // TODO configurable
    let mut renderer = pollster::block_on(WgpuRenderer::new(
        window,
        RendererConfig {
            vsync_mode: VSyncMode::Enabled,
            prescale_factor: PrescaleFactor::from(NonZeroU32::new(3).unwrap()),
            filter_mode: FilterMode::Linear,
        },
    ))?;
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
