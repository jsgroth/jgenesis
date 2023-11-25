mod debug;
mod rewind;

use crate::config;
use crate::config::{
    CommonConfig, GenesisConfig, SegaCdConfig, SmsGgConfig, SnesConfig, WindowSize,
};
use crate::input::{
    Clearable, GenesisButton, GetButtonField, Hotkey, HotkeyMapResult, HotkeyMapper, InputMapper,
    Joysticks, SmsGgButton, SnesButton,
};
use crate::mainloop::debug::{CramDebug, VramDebug};
use crate::mainloop::rewind::Rewinder;
use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use genesis_core::{GenesisEmulator, GenesisEmulatorConfig, GenesisInputs};
use jgenesis_common::frontend::{AudioOutput, EmulatorTrait, PartialClone, SaveWriter, TickEffect};
use jgenesis_renderer::renderer::{RendererError, WgpuRenderer};
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::event::{Event, WindowEvent};
use sdl2::render::TextureValueError;
use sdl2::video::{FullscreenType, Window, WindowBuildError};
use sdl2::{AudioSubsystem, EventPump, IntegerOrSdlError, JoystickSubsystem, VideoSubsystem};
use segacd_core::api::{DiscError, DiscResult, SegaCdEmulator, SegaCdEmulatorConfig};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgEmulator, SmsGgEmulatorConfig, SmsGgInputs};
use snes_core::api::{LoadError, SnesEmulator, SnesEmulatorConfig};
use snes_core::input::SnesInputs;
use std::error::Error;
use std::ffi::{NulError, OsStr};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io, thread};
use thiserror::Error;

trait RendererExt {
    fn focus(&mut self);

    fn window_id(&self) -> u32;

    fn toggle_fullscreen(&mut self) -> Result<(), String>;
}

impl RendererExt for WgpuRenderer<Window> {
    fn focus(&mut self) {
        // SAFETY: This is not reassigning the window
        unsafe {
            self.window_mut().raise();
        }
    }

    fn window_id(&self) -> u32 {
        self.window().id()
    }

    fn toggle_fullscreen(&mut self) -> Result<(), String> {
        // SAFETY: This is not reassigning the window
        unsafe {
            let window = self.window_mut();
            let new_fullscreen = match window.fullscreen_state() {
                FullscreenType::Off => FullscreenType::Desktop,
                FullscreenType::Desktop | FullscreenType::True => FullscreenType::Off,
            };
            window.set_fullscreen(new_fullscreen)
        }
    }
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Error opening SDL2 audio queue: {0}")]
    OpenQueue(String),
    #[error("Error pushing audio samples to SDL2 audio queue: {0}")]
    QueueAudio(String),
}

struct SdlAudioOutput {
    audio_queue: AudioQueue<f32>,
    audio_buffer: Vec<f32>,
    audio_sync: bool,
    internal_audio_buffer_len: u32,
    audio_sync_threshold: u32,
    audio_gain_multiplier: f64,
    sample_count: u64,
    speed_multiplier: u64,
}

impl SdlAudioOutput {
    fn create_and_init<KC, JC>(
        audio: &AudioSubsystem,
        config: &CommonConfig<KC, JC>,
    ) -> Result<Self, AudioError> {
        let audio_queue = audio
            .open_queue(
                None,
                &AudioSpecDesired {
                    freq: Some(48000),
                    channels: Some(2),
                    samples: Some(config.audio_device_queue_size),
                },
            )
            .map_err(AudioError::OpenQueue)?;
        audio_queue.resume();

        Ok(Self {
            audio_queue,
            audio_buffer: Vec::with_capacity(config.internal_audio_buffer_size as usize),
            audio_sync: config.audio_sync,
            internal_audio_buffer_len: config.internal_audio_buffer_size,
            audio_sync_threshold: config.audio_sync_threshold,
            audio_gain_multiplier: decibels_to_multiplier(config.audio_gain_db),
            sample_count: 0,
            speed_multiplier: 1,
        })
    }

    fn reload_config<KC, JC>(&mut self, config: &CommonConfig<KC, JC>) -> Result<(), AudioError> {
        self.audio_sync = config.audio_sync;
        self.internal_audio_buffer_len = config.internal_audio_buffer_size;
        self.audio_sync_threshold = config.audio_sync_threshold;
        self.audio_gain_multiplier = decibels_to_multiplier(config.audio_gain_db);

        if config.audio_device_queue_size != self.audio_queue.spec().samples {
            log::info!("Recreating SDL audio queue with size {}", config.audio_device_queue_size);
            self.audio_queue.pause();

            let new_audio_queue = self
                .audio_queue
                .subsystem()
                .open_queue(
                    None,
                    &AudioSpecDesired {
                        freq: Some(48000),
                        channels: Some(2),
                        samples: Some(config.audio_device_queue_size),
                    },
                )
                .map_err(AudioError::OpenQueue)?;
            self.audio_queue = new_audio_queue;
            self.audio_queue.resume();
        }

        Ok(())
    }
}

fn decibels_to_multiplier(decibels: f64) -> f64 {
    10.0_f64.powf(decibels / 20.0)
}

impl AudioOutput for SdlAudioOutput {
    type Err = AudioError;

    #[inline]
    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err> {
        self.sample_count += 1;
        if self.sample_count % self.speed_multiplier != 0 {
            return Ok(());
        }

        self.audio_buffer.push((sample_l * self.audio_gain_multiplier) as f32);
        self.audio_buffer.push((sample_r * self.audio_gain_multiplier) as f32);

        if self.audio_buffer.len() >= self.internal_audio_buffer_len as usize {
            if self.audio_sync {
                // Wait until audio queue is not full
                while self.audio_queue.size() >= self.audio_sync_threshold {
                    sleep(Duration::from_micros(250));
                }
            } else if self.audio_queue.size() >= self.audio_sync_threshold {
                // Audio queue is full; drop samples
                self.audio_buffer.clear();
                return Ok(());
            }

            self.audio_queue.queue_audio(&self.audio_buffer).map_err(AudioError::QueueAudio)?;
            self.audio_buffer.clear();
        }

        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn sleep(duration: Duration) {
    // SAFETY: thread::sleep cannot panic, so timeEndPeriod will always be called after timeBeginPeriod.
    unsafe {
        windows::Win32::Media::timeBeginPeriod(1);
        thread::sleep(duration);
        windows::Win32::Media::timeEndPeriod(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn sleep(duration: Duration) {
    thread::sleep(duration);
}

#[derive(Debug, Error)]
#[error("Error writing save file to '{path}': {source}")]
pub struct SaveWriteError {
    path: String,
    #[source]
    source: io::Error,
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
    type Err = SaveWriteError;

    #[inline]
    fn persist_save<'a>(
        &mut self,
        save_bytes: impl Iterator<Item = &'a [u8]>,
    ) -> Result<(), Self::Err> {
        let err_fn = |source| SaveWriteError { path: self.path.display().to_string(), source };

        let file = File::create(&self.path).map_err(err_fn)?;
        let mut writer = BufWriter::new(file);

        for save_slice in save_bytes {
            writer.write_all(save_slice).map_err(err_fn)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTickEffect {
    None,
    Exit,
}

pub struct NativeEmulator<Inputs, Button, Config, Emulator: PartialClone> {
    emulator: Emulator,
    config: Config,
    renderer: WgpuRenderer<Window>,
    audio_output: SdlAudioOutput,
    input_mapper: InputMapper<Inputs, Button>,
    hotkey_mapper: HotkeyMapper,
    save_writer: FsSaveWriter,
    event_pump: EventPump,
    save_state_path: PathBuf,
    paused: bool,
    should_step_frame: bool,
    fast_forward_multiplier: u64,
    rewinder: Rewinder<Emulator>,
    video: VideoSubsystem,
    cram_debug: Option<CramDebug>,
    vram_debug: Option<VramDebug>,
}

impl<Inputs, Button, Config, Emulator: PartialClone>
    NativeEmulator<Inputs, Button, Config, Emulator>
{
    fn reload_common_config<KC, JC>(
        &mut self,
        config: &CommonConfig<KC, JC>,
    ) -> Result<(), AudioError> {
        self.renderer.reload_config(config.renderer_config);
        self.audio_output.reload_config(config)?;

        self.fast_forward_multiplier = config.fast_forward_multiplier;
        // Reset speed multiplier in case the fast forward hotkey changed
        self.renderer.set_speed_multiplier(1);
        self.audio_output.speed_multiplier = 1;

        self.rewinder.set_buffer_duration(Duration::from_secs(config.rewind_buffer_length_seconds));

        match HotkeyMapper::from_config(&config.hotkeys) {
            Ok(hotkey_mapper) => {
                self.hotkey_mapper = hotkey_mapper;
            }
            Err(err) => {
                log::error!("Error reloading hotkey config: {err}");
            }
        }

        Ok(())
    }

    pub fn focus(&mut self) {
        self.renderer.focus();
    }

    pub fn event_pump_and_joysticks_mut(
        &mut self,
    ) -> (&mut EventPump, &mut Joysticks, &JoystickSubsystem) {
        let (joysticks, joystick_subsystem) = self.input_mapper.joysticks_mut();
        (&mut self.event_pump, joysticks, joystick_subsystem)
    }
}

pub type NativeSmsGgEmulator =
    NativeEmulator<SmsGgInputs, SmsGgButton, SmsGgEmulatorConfig, SmsGgEmulator>;

impl NativeSmsGgEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_smsgg_config(&mut self, config: Box<SmsGgConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let vdp_version = config.vdp_version.unwrap_or_else(|| self.emulator.vdp_version());
        let psg_version = config.psg_version.unwrap_or_else(|| {
            if vdp_version.is_master_system() {
                PsgVersion::MasterSystem2
            } else {
                PsgVersion::Standard
            }
        });

        let emulator_config = config.to_emulator_config(vdp_version, psg_version);
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self
            .input_mapper
            .reload_config(config.common.keyboard_inputs, config.common.joystick_inputs)
        {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
}

pub type NativeGenesisEmulator =
    NativeEmulator<GenesisInputs, GenesisButton, GenesisEmulatorConfig, GenesisEmulator>;

impl NativeGenesisEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_genesis_config(&mut self, config: Box<GenesisConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let emulator_config = config.to_emulator_config();
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self.input_mapper.reload_config(
            config.p1_controller_type,
            config.p2_controller_type,
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
        ) {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
}

pub type NativeSegaCdEmulator =
    NativeEmulator<GenesisInputs, GenesisButton, SegaCdEmulatorConfig, SegaCdEmulator>;

impl NativeSegaCdEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_sega_cd_config(&mut self, config: Box<SegaCdConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.genesis.common)?;
        self.emulator.reload_config(&config.to_emulator_config());

        if let Err(err) = self.input_mapper.reload_config(
            config.genesis.p1_controller_type,
            config.genesis.p2_controller_type,
            config.genesis.common.keyboard_inputs,
            config.genesis.common.joystick_inputs,
        ) {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn remove_disc(&mut self) {
        self.emulator.remove_disc();

        // SAFETY: This is not reassigning the window
        unsafe {
            self.renderer
                .window_mut()
                .set_title("sega cd - (no disc)")
                .expect("Given string literal will never contain a null character");
        }
    }

    /// # Errors
    ///
    /// This method will return an error if the disc drive is unable to load the disc.
    #[allow(clippy::missing_panics_doc)]
    pub fn change_disc<P: AsRef<Path>>(&mut self, cue_path: P) -> DiscResult<()> {
        self.emulator.change_disc(cue_path)?;

        let title = format!("sega cd - {}", self.emulator.disc_title());

        // SAFETY: This is not reassigning the window
        unsafe {
            self.renderer
                .window_mut()
                .set_title(&title)
                .expect("Disc title should have non-printable characters already removed");
        }

        Ok(())
    }
}

pub type NativeSnesEmulator =
    NativeEmulator<SnesInputs, SnesButton, SnesEmulatorConfig, SnesEmulator>;

impl NativeSnesEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_snes_config(&mut self, config: Box<SnesConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let emulator_config = config.to_emulator_config();
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self
            .input_mapper
            .reload_config(config.common.keyboard_inputs, config.common.joystick_inputs)
        {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum NativeEmulatorError {
    #[error("{0}")]
    Render(#[from] RendererError),
    #[error("{0}")]
    Audio(#[from] AudioError),
    #[error("{0}")]
    SaveWrite(#[from] SaveWriteError),
    #[error("Error initializing SDL2: {0}")]
    SdlInit(String),
    #[error("Error initializing SDL2 video subsystem: {0}")]
    SdlVideoInit(String),
    #[error("Error initializing SDL2 audio subsystem: {0}")]
    SdlAudioInit(String),
    #[error("Error initializing SDL2 joystick subsystem: {0}")]
    SdlJoystickInit(String),
    #[error("Error initializing SDL2 event pump: {0}")]
    SdlEventPumpInit(String),
    #[error("Error creating SDL2 window: {0}")]
    SdlCreateWindow(#[from] WindowBuildError),
    #[error("Error changing window title to '{title}': {source}")]
    SdlSetWindowTitle {
        title: String,
        #[source]
        source: NulError,
    },
    #[error("Error creating SDL2 canvas/renderer: {0}")]
    SdlCreateCanvas(#[source] IntegerOrSdlError),
    #[error("Error creating SDL2 texture: {0}")]
    SdlCreateTexture(#[from] TextureValueError),
    #[error("Error toggling window fullscreen: {0}")]
    SdlSetFullscreen(String),
    #[error("Error opening joystick {device_id}: {source}")]
    SdlJoystickOpen {
        device_id: u32,
        #[source]
        source: IntegerOrSdlError,
    },
    #[error("SDL2 error rendering CRAM debug window: {0}")]
    SdlCramDebug(String),
    #[error("SDL2 error rendering VRAM debug window: {0}")]
    SdlVramDebug(String),
    #[error("Invalid SDL2 keycode: '{0}'")]
    InvalidKeycode(String),
    #[error("Unable to determine file name for path: '{0}'")]
    ParseFileName(String),
    #[error("Unable to determine file extension for path: '{0}'")]
    ParseFileExtension(String),
    #[error("Failed to read ROM file at '{path}': {source}")]
    RomRead {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("BIOS is required for Sega CD emulation")]
    SegaCdNoBios,
    #[error("Error opening BIOS file at '{path}': {source}")]
    SegaCdBiosRead {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("{0}")]
    SegaCdDisc(#[from] DiscError),
    #[error("{0}")]
    SnesLoad(#[from] LoadError),
    #[error("I/O error opening save state file '{path}': {source}")]
    StateFileOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error saving state: {0}")]
    SaveState(#[from] EncodeError),
    #[error("Error loading state: {0}")]
    LoadState(#[from] DecodeError),
    #[error("Error in emulation core: {0}")]
    Emulator(#[source] Box<dyn Error + Send + Sync + 'static>),
}

pub type NativeEmulatorResult<T> = Result<T, NativeEmulatorError>;

// TODO simplify or generalize these trait bounds
impl<Inputs, Button, Config, Emulator> NativeEmulator<Inputs, Button, Config, Emulator>
where
    Inputs: Clearable + GetButtonField<Button>,
    Button: Copy,
    Emulator: EmulatorTrait<Inputs = Inputs, Config = Config>,
    Emulator::Err<RendererError, AudioError, SaveWriteError>: Error + Send + Sync + 'static,
{
    /// Run the emulator until a frame is rendered.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered when rendering frames, pushing audio
    /// samples, or writing save files.
    pub fn render_frame(&mut self) -> NativeEmulatorResult<NativeTickEffect> {
        loop {
            let rewinding = self.rewinder.is_rewinding();
            let should_tick_emulator = !rewinding && (!self.paused || self.should_step_frame);
            let frame_rendered = should_tick_emulator
                && self
                    .emulator
                    .tick(
                        &mut self.renderer,
                        &mut self.audio_output,
                        self.input_mapper.inputs(),
                        &mut self.save_writer,
                    )
                    .map_err(|err| NativeEmulatorError::Emulator(err.into()))?
                    == TickEffect::FrameRendered;

            if !should_tick_emulator || frame_rendered {
                self.should_step_frame = false;

                for event in self.event_pump.poll_iter() {
                    self.input_mapper.handle_event(&event)?;
                    if handle_hotkeys(
                        &self.hotkey_mapper,
                        &event,
                        &mut self.emulator,
                        &self.config,
                        &mut self.renderer,
                        &mut self.audio_output,
                        &self.save_state_path,
                        &mut self.paused,
                        &mut self.should_step_frame,
                        self.fast_forward_multiplier,
                        &mut self.rewinder,
                        &self.video,
                        &mut self.cram_debug,
                        &mut self.vram_debug,
                    )? == HotkeyResult::Quit
                    {
                        return Ok(NativeTickEffect::Exit);
                    }

                    match event {
                        Event::Quit { .. } => {
                            return Ok(NativeTickEffect::Exit);
                        }
                        Event::Window { win_event, window_id, .. } => {
                            if window_id == self.renderer.window_id() {
                                handle_window_event(win_event, &mut self.renderer);
                            } else if self
                                .cram_debug
                                .as_ref()
                                .is_some_and(|cram_debug| cram_debug.window_id() == window_id)
                            {
                                if let WindowEvent::Close = win_event {
                                    self.cram_debug = None;
                                }
                            } else if self
                                .vram_debug
                                .as_ref()
                                .is_some_and(|vram_debug| vram_debug.window_id() == window_id)
                            {
                                if let WindowEvent::Close = win_event {
                                    self.vram_debug = None;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(cram_debug) = &mut self.cram_debug {
                    cram_debug.render(&self.emulator)?;
                }

                if let Some(vram_debug) = &mut self.vram_debug {
                    vram_debug.render(&self.emulator)?;
                }

                if frame_rendered {
                    self.rewinder.record_frame(&self.emulator);
                }

                if rewinding {
                    self.rewinder.tick(&mut self.emulator, &mut self.renderer)?;
                }

                if rewinding || self.paused {
                    // Don't spin loop when the emulator is not actively running
                    sleep(Duration::from_millis(1));
                }

                return Ok(NativeTickEffect::None);
            }
        }
    }

    pub fn soft_reset(&mut self) {
        self.emulator.soft_reset();
    }

    pub fn hard_reset(&mut self) {
        self.emulator.hard_reset();
    }
}

/// Create an emulator with the SMS/GG core with the given config.
///
/// # Errors
///
/// This function will propagate any video, audio, or disk errors encountered.
pub fn create_smsgg(config: Box<SmsGgConfig>) -> NativeEmulatorResult<NativeSmsGgEmulator> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.common.rom_file_path);
    let rom_file_name = parse_file_name(rom_file_path)?;
    let file_ext = parse_file_ext(rom_file_path)?;

    let save_state_path = rom_file_path.with_extension("ss0");

    let rom = fs::read(rom_file_path).map_err(|source| NativeEmulatorError::RomRead {
        path: rom_file_path.display().to_string(),
        source,
    })?;

    let save_path = rom_file_path.with_extension("sav");
    let initial_cartridge_ram = fs::read(&save_path).ok();

    let vdp_version =
        config.vdp_version.unwrap_or_else(|| config::default_vdp_version_for_ext(file_ext));
    let psg_version =
        config.psg_version.unwrap_or_else(|| config::default_psg_version_for_ext(file_ext));

    log::info!("VDP version: {vdp_version:?}");
    log::info!("PSG version: {psg_version:?}");

    let (video, audio, joystick, event_pump) = init_sdl()?;

    let WindowSize { width: window_width, height: window_height } =
        config.common.window_size.unwrap_or_else(|| config::default_smsgg_window_size(vdp_version));
    let window = create_window(
        &video,
        &format!("smsgg - {rom_file_name}"),
        window_width,
        window_height,
        config.common.launch_in_fullscreen,
    )?;

    let emulator_config = config.to_emulator_config(vdp_version, psg_version);

    let renderer =
        pollster::block_on(WgpuRenderer::new(window, Window::size, config.common.renderer_config))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, &config.common)?;
    let input_mapper = InputMapper::new_smsgg(
        joystick,
        config.common.keyboard_inputs,
        config.common.joystick_inputs,
        config.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;
    let save_writer = FsSaveWriter::new(save_path);

    let emulator = SmsGgEmulator::create(rom, initial_cartridge_ram, emulator_config);

    Ok(NativeEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        event_pump,
        save_state_path,
        paused: false,
        should_step_frame: false,
        fast_forward_multiplier: config.common.fast_forward_multiplier,
        rewinder: Rewinder::new(Duration::from_secs(config.common.rewind_buffer_length_seconds)),
        video,
        cram_debug: None,
        vram_debug: None,
    })
}

/// Create an emulator with the Genesis core with the given config.
///
/// # Errors
///
/// This function will return an error upon encountering any video, audio, or I/O error.
pub fn create_genesis(config: Box<GenesisConfig>) -> NativeEmulatorResult<NativeGenesisEmulator> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.common.rom_file_path);
    let rom = fs::read(rom_file_path).map_err(|source| NativeEmulatorError::RomRead {
        path: rom_file_path.display().to_string(),
        source,
    })?;

    let save_path = rom_file_path.with_extension("sav");
    let save_state_path = rom_file_path.with_extension("ss0");

    let initial_ram = fs::read(&save_path).ok();
    if initial_ram.is_some() {
        log::info!("Loaded save file from {}", save_path.display());
    }

    let emulator_config = config.to_emulator_config();
    let emulator = GenesisEmulator::create(rom, initial_ram, emulator_config);

    let (video, audio, joystick, event_pump) = init_sdl()?;

    let WindowSize { width: window_width, height: window_height } =
        config.common.window_size.unwrap_or(config::DEFAULT_GENESIS_WINDOW_SIZE);
    let mut cartridge_title = emulator.cartridge_title();
    // Remove non-printable characters
    cartridge_title.retain(|c| {
        c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation()
    });
    let window = create_window(
        &video,
        &format!("genesis - {cartridge_title}"),
        window_width,
        window_height,
        config.common.launch_in_fullscreen,
    )?;

    let renderer =
        pollster::block_on(WgpuRenderer::new(window, Window::size, config.common.renderer_config))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, &config.common)?;
    let input_mapper = InputMapper::new_genesis(
        config.p1_controller_type,
        config.p2_controller_type,
        joystick,
        config.common.keyboard_inputs,
        config.common.joystick_inputs,
        config.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;
    let save_writer = FsSaveWriter::new(save_path);

    Ok(NativeEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        event_pump,
        save_state_path,
        paused: false,
        should_step_frame: false,
        fast_forward_multiplier: config.common.fast_forward_multiplier,
        rewinder: Rewinder::new(Duration::from_secs(config.common.rewind_buffer_length_seconds)),
        video,
        cram_debug: None,
        vram_debug: None,
    })
}

/// Create an emulator with the Sega CD core with the given config.
///
/// # Errors
///
/// This function will return an error upon encountering any video, audio, or I/O error, including
/// any error encountered loading the Sega CD game disc.
pub fn create_sega_cd(config: Box<SegaCdConfig>) -> NativeEmulatorResult<NativeSegaCdEmulator> {
    log::info!("Running with config: {config}");

    let cue_path = Path::new(&config.genesis.common.rom_file_path);
    let save_path = cue_path.with_extension("sav");
    let save_state_path = cue_path.with_extension("ss0");

    let initial_backup_ram = fs::read(&save_path).ok();

    let bios_file_path = config.bios_file_path.as_ref().ok_or(NativeEmulatorError::SegaCdNoBios)?;
    let bios = fs::read(bios_file_path).map_err(|source| NativeEmulatorError::SegaCdBiosRead {
        path: bios_file_path.clone(),
        source,
    })?;

    let emulator_config = config.to_emulator_config();
    let emulator = SegaCdEmulator::create(
        bios,
        cue_path,
        initial_backup_ram,
        config.run_without_disc,
        emulator_config,
    )?;

    let (video, audio, joystick, event_pump) = init_sdl()?;

    let WindowSize { width: window_width, height: window_height } =
        config.genesis.common.window_size.unwrap_or(config::DEFAULT_GENESIS_WINDOW_SIZE);

    let window = create_window(
        &video,
        &format!("sega cd - {}", emulator.disc_title()),
        window_width,
        window_height,
        config.genesis.common.launch_in_fullscreen,
    )?;

    let renderer = pollster::block_on(WgpuRenderer::new(
        window,
        Window::size,
        config.genesis.common.renderer_config,
    ))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, &config.genesis.common)?;
    let input_mapper = InputMapper::new_genesis(
        config.genesis.p1_controller_type,
        config.genesis.p2_controller_type,
        joystick,
        config.genesis.common.keyboard_inputs,
        config.genesis.common.joystick_inputs,
        config.genesis.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.genesis.common.hotkeys)?;
    let save_writer = FsSaveWriter::new(save_path);

    Ok(NativeEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        event_pump,
        save_state_path,
        paused: false,
        should_step_frame: false,
        fast_forward_multiplier: config.genesis.common.fast_forward_multiplier,
        rewinder: Rewinder::new(Duration::from_secs(
            config.genesis.common.rewind_buffer_length_seconds,
        )),
        video,
        cram_debug: None,
        vram_debug: None,
    })
}

/// Create an emulator with the SNES core with the given config.
///
/// # Errors
///
/// This function will return an error if unable to initialize the emulator.
pub fn create_snes(config: Box<SnesConfig>) -> NativeEmulatorResult<NativeSnesEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let rom = fs::read(rom_path).map_err(|source| NativeEmulatorError::RomRead {
        path: config.common.rom_file_path.clone(),
        source,
    })?;

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");

    let initial_sram = fs::read(&save_path).ok();
    if initial_sram.as_ref().is_some_and(|sram| !sram.is_empty()) {
        log::info!("Loaded save file from '{}'", save_path.display());
    }

    let emulator_config = config.to_emulator_config();
    let coprocessor_roms = config.to_coprocessor_roms();
    let mut emulator = SnesEmulator::create(rom, initial_sram, emulator_config, coprocessor_roms)?;

    let (video, audio, joystick, event_pump) = init_sdl()?;

    // Use same default window size as Genesis / Sega CD
    let WindowSize { width: window_width, height: window_height } =
        config.common.window_size.unwrap_or(config::DEFAULT_GENESIS_WINDOW_SIZE);

    let cartridge_title = emulator.cartridge_title();
    let window = create_window(
        &video,
        &format!("snes - {cartridge_title}"),
        window_width,
        window_height,
        config.common.launch_in_fullscreen,
    )?;

    let renderer =
        pollster::block_on(WgpuRenderer::new(window, Window::size, config.common.renderer_config))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, &config.common)?;
    let save_writer = FsSaveWriter::new(save_path);

    let input_mapper = InputMapper::new_snes(
        joystick,
        config.common.keyboard_inputs,
        config.common.joystick_inputs,
        config.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;

    Ok(NativeEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        event_pump,
        save_state_path,
        paused: false,
        should_step_frame: false,
        fast_forward_multiplier: config.common.fast_forward_multiplier,
        rewinder: Rewinder::new(Duration::from_secs(config.common.rewind_buffer_length_seconds)),
        video,
        cram_debug: None,
        vram_debug: None,
    })
}

fn parse_file_name(path: &Path) -> NativeEmulatorResult<&str> {
    path.file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| NativeEmulatorError::ParseFileName(path.display().to_string()))
}

fn parse_file_ext(path: &Path) -> NativeEmulatorResult<&str> {
    path.extension()
        .and_then(OsStr::to_str)
        .ok_or_else(|| NativeEmulatorError::ParseFileExtension(path.display().to_string()))
}

// Initialize SDL2 and hide the mouse cursor
fn init_sdl() -> NativeEmulatorResult<(VideoSubsystem, AudioSubsystem, JoystickSubsystem, EventPump)>
{
    let sdl = sdl2::init().map_err(NativeEmulatorError::SdlInit)?;
    let video = sdl.video().map_err(NativeEmulatorError::SdlVideoInit)?;
    let audio = sdl.audio().map_err(NativeEmulatorError::SdlAudioInit)?;
    let joystick = sdl.joystick().map_err(NativeEmulatorError::SdlJoystickInit)?;
    let event_pump = sdl.event_pump().map_err(NativeEmulatorError::SdlEventPumpInit)?;

    sdl.mouse().show_cursor(false);

    Ok((video, audio, joystick, event_pump))
}

fn create_window(
    video: &VideoSubsystem,
    title: &str,
    width: u32,
    height: u32,
    fullscreen: bool,
) -> NativeEmulatorResult<Window> {
    let mut window = video.window(title, width, height).resizable().build()?;

    if fullscreen {
        window
            .set_fullscreen(FullscreenType::Desktop)
            .map_err(NativeEmulatorError::SdlSetFullscreen)?;
    }

    Ok(window)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyResult {
    None,
    Quit,
}

#[allow(clippy::too_many_arguments)]
fn handle_hotkeys<Emulator, P>(
    hotkey_mapper: &HotkeyMapper,
    event: &Event,
    emulator: &mut Emulator,
    config: &Emulator::Config,
    renderer: &mut WgpuRenderer<Window>,
    audio_output: &mut SdlAudioOutput,
    save_state_path: P,
    paused: &mut bool,
    should_step_frame: &mut bool,
    fast_forward_multiplier: u64,
    rewinder: &mut Rewinder<Emulator>,
    video: &VideoSubsystem,
    cram_debug: &mut Option<CramDebug>,
    vram_debug: &mut Option<VramDebug>,
) -> NativeEmulatorResult<HotkeyResult>
where
    Emulator: EmulatorTrait,
    P: AsRef<Path>,
{
    let save_state_path = save_state_path.as_ref();

    match hotkey_mapper.check_for_hotkeys(event) {
        HotkeyMapResult::Pressed(hotkeys) => {
            for &hotkey in hotkeys {
                if handle_hotkey_pressed(
                    hotkey,
                    emulator,
                    config,
                    renderer,
                    audio_output,
                    paused,
                    should_step_frame,
                    fast_forward_multiplier,
                    rewinder,
                    video,
                    cram_debug,
                    vram_debug,
                    save_state_path,
                )? == HotkeyResult::Quit
                {
                    return Ok(HotkeyResult::Quit);
                }
            }
        }
        HotkeyMapResult::Released(hotkeys) => {
            for &hotkey in hotkeys {
                match hotkey {
                    Hotkey::FastForward => {
                        renderer.set_speed_multiplier(1);
                        audio_output.speed_multiplier = 1;
                    }
                    Hotkey::Rewind => {
                        rewinder.stop_rewinding();
                    }
                    _ => {}
                }
            }
        }
        HotkeyMapResult::None => {}
    }

    Ok(HotkeyResult::None)
}

#[allow(clippy::too_many_arguments)]
fn handle_hotkey_pressed<Emulator>(
    hotkey: Hotkey,
    emulator: &mut Emulator,
    config: &Emulator::Config,
    renderer: &mut WgpuRenderer<Window>,
    audio_output: &mut SdlAudioOutput,
    paused: &mut bool,
    should_step_frame: &mut bool,
    fast_forward_multiplier: u64,
    rewinder: &mut Rewinder<Emulator>,
    video: &VideoSubsystem,
    cram_debug: &mut Option<CramDebug>,
    vram_debug: &mut Option<VramDebug>,
    save_state_path: &Path,
) -> NativeEmulatorResult<HotkeyResult>
where
    Emulator: EmulatorTrait,
{
    match hotkey {
        Hotkey::Quit => {
            return Ok(HotkeyResult::Quit);
        }
        Hotkey::ToggleFullscreen => {
            renderer.toggle_fullscreen().map_err(NativeEmulatorError::SdlSetFullscreen)?;
        }
        Hotkey::SaveState => {
            save_state(emulator, save_state_path)?;
        }
        Hotkey::LoadState => {
            let mut loaded_emulator: Emulator = match load_state(save_state_path) {
                Ok(emulator) => emulator,
                Err(err) => {
                    log::error!(
                        "Error loading save state from {}: {err}",
                        save_state_path.display()
                    );
                    return Ok(HotkeyResult::None);
                }
            };
            loaded_emulator.take_rom_from(emulator);

            // Force a config reload because the emulator will contain some config fields
            loaded_emulator.reload_config(config);

            *emulator = loaded_emulator;
        }
        Hotkey::SoftReset => {
            emulator.soft_reset();
        }
        Hotkey::HardReset => {
            emulator.hard_reset();
        }
        Hotkey::Pause => {
            *paused = !(*paused);
        }
        Hotkey::StepFrame => {
            *should_step_frame = true;
        }
        Hotkey::FastForward => {
            renderer.set_speed_multiplier(fast_forward_multiplier);
            audio_output.speed_multiplier = fast_forward_multiplier;
        }
        Hotkey::Rewind => {
            rewinder.start_rewinding();
        }
        Hotkey::OpenCramDebug => {
            if cram_debug.is_none() {
                *cram_debug = Some(CramDebug::new::<Emulator>(video)?);
            }
        }
        Hotkey::OpenVramDebug => match vram_debug {
            Some(vram_debug) => {
                vram_debug.toggle_palette()?;
            }
            None => {
                if Emulator::SUPPORTS_VRAM_DEBUG {
                    *vram_debug = Some(VramDebug::new::<Emulator>(video)?);
                } else {
                    log::error!("Currently running console does not support VRAM debug window");
                }
            }
        },
    }

    Ok(HotkeyResult::None)
}

fn handle_window_event(win_event: WindowEvent, renderer: &mut WgpuRenderer<Window>) {
    match win_event {
        WindowEvent::Resized(..) | WindowEvent::SizeChanged(..) | WindowEvent::Maximized => {
            renderer.handle_resize();
        }
        _ => {}
    }
}

macro_rules! bincode_config {
    () => {
        bincode::config::standard().with_little_endian().with_fixed_int_encoding()
    };
}

fn save_state<E, P>(emulator: &E, path: P) -> NativeEmulatorResult<()>
where
    E: Encode,
    P: AsRef<Path>,
{
    let path = path.as_ref();

    let mut file = BufWriter::new(File::create(path).map_err(|source| {
        NativeEmulatorError::StateFileOpen { path: path.display().to_string(), source }
    })?);

    let conf = bincode_config!();
    bincode::encode_into_std_write(emulator, &mut file, conf)?;

    log::info!("Saved state to {}", path.display());

    Ok(())
}

fn load_state<D, P>(path: P) -> NativeEmulatorResult<D>
where
    D: Decode,
    P: AsRef<Path>,
{
    let path = path.as_ref();

    let mut file = BufReader::new(File::open(path).map_err(|source| {
        NativeEmulatorError::StateFileOpen { path: path.display().to_string(), source }
    })?);

    let conf = bincode_config!();
    let emulator = bincode::decode_from_std_read(&mut file, conf)?;

    log::info!("Loaded state from {}", path.display());

    Ok(emulator)
}
