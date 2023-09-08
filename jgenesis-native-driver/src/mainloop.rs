mod debug;

use crate::config;
use crate::config::{CommonConfig, GenesisConfig, SmsGgConfig, WindowSize};
use crate::input::{
    Clearable, GenesisButton, GetButtonField, Hotkey, HotkeyMapper, InputMapper, Joysticks,
    SmsGgButton,
};
use crate::mainloop::debug::{CramDebug, VramDebug};
use anyhow::{anyhow, Context};
use bincode::{Decode, Encode};
use genesis_core::{GenesisEmulator, GenesisInputs};
use jgenesis_renderer::renderer::WgpuRenderer;
use jgenesis_traits::frontend::{AudioOutput, EmulatorTrait, SaveWriter, TickEffect};
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::event::{Event, WindowEvent};
use sdl2::video::{FullscreenType, Window};
use sdl2::{AudioSubsystem, EventPump, JoystickSubsystem, VideoSubsystem};
use smsgg_core::{SmsGgEmulator, SmsGgInputs};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

trait RendererExt {
    fn focus(&mut self);

    fn window_id(&self) -> u32;

    fn toggle_fullscreen(&mut self) -> anyhow::Result<()>;
}

impl RendererExt for WgpuRenderer<Window> {
    fn focus(&mut self) {
        self.window_mut().raise();
    }

    fn window_id(&self) -> u32 {
        self.window().id()
    }

    fn toggle_fullscreen(&mut self) -> anyhow::Result<()> {
        let window = self.window_mut();
        let new_fullscreen = match window.fullscreen_state() {
            FullscreenType::Off => FullscreenType::Desktop,
            FullscreenType::Desktop | FullscreenType::True => FullscreenType::Off,
        };
        window
            .set_fullscreen(new_fullscreen)
            .map_err(|err| anyhow!("Error toggling fullscreen: {err}"))?;

        Ok(())
    }
}

struct SdlAudioOutput {
    audio_queue: AudioQueue<f32>,
    audio_buffer: Vec<f32>,
    audio_sync: bool,
}

impl SdlAudioOutput {
    fn create_and_init(audio: &AudioSubsystem, audio_sync: bool) -> anyhow::Result<Self> {
        let audio_queue = audio
            .open_queue(
                None,
                &AudioSpecDesired { freq: Some(48000), channels: Some(2), samples: Some(64) },
            )
            .map_err(|err| anyhow!("Error opening SDL2 audio queue: {err}"))?;
        audio_queue.resume();

        Ok(Self { audio_queue, audio_buffer: Vec::with_capacity(64), audio_sync })
    }
}

// 1024 4-byte samples
const MAX_AUDIO_QUEUE_SIZE: u32 = 1024 * 4;

impl AudioOutput for SdlAudioOutput {
    type Err = anyhow::Error;

    #[inline]
    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err> {
        self.audio_buffer.push(sample_l as f32);
        self.audio_buffer.push(sample_r as f32);

        if self.audio_buffer.len() == 64 {
            if self.audio_sync {
                // Wait until audio queue is not full
                while self.audio_queue.size() >= MAX_AUDIO_QUEUE_SIZE {
                    sleep(Duration::from_micros(250));
                }
            } else if self.audio_queue.size() >= MAX_AUDIO_QUEUE_SIZE {
                // Audio queue is full; drop samples
                self.audio_buffer.clear();
                return Ok(());
            }

            self.audio_queue
                .queue_audio(&self.audio_buffer)
                .map_err(|err| anyhow!("Error pushing audio samples: {err}"))?;
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

struct NullSaveWriter;

impl SaveWriter for NullSaveWriter {
    type Err = anyhow::Error;

    fn persist_save(&mut self, _save_bytes: &[u8]) -> Result<(), Self::Err> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTickEffect {
    None,
    Exit,
}

pub struct NativeEmulator<Inputs, Button, Emulator> {
    emulator: Emulator,
    renderer: WgpuRenderer<Window>,
    audio_output: SdlAudioOutput,
    input_mapper: InputMapper<Inputs, Button>,
    hotkey_mapper: HotkeyMapper,
    save_writer: FsSaveWriter,
    event_pump: EventPump,
    save_state_path: PathBuf,
    video: VideoSubsystem,
    cram_debug: Option<CramDebug>,
    vram_debug: Option<VramDebug>,
}

impl<Inputs, Button, Emulator> NativeEmulator<Inputs, Button, Emulator> {
    fn reload_common_config<KC, JC>(&mut self, config: &CommonConfig<KC, JC>) {
        self.renderer.reload_config(config.renderer_config);
        self.audio_output.audio_sync = config.audio_sync;

        match HotkeyMapper::from_config(&config.hotkeys) {
            Ok(hotkey_mapper) => {
                self.hotkey_mapper = hotkey_mapper;
            }
            Err(err) => {
                log::error!("Error reloading hotkey config: {err}");
            }
        }
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

impl NativeEmulator<SmsGgInputs, SmsGgButton, SmsGgEmulator> {
    pub fn reload_smsgg_config(&mut self, config: Box<SmsGgConfig>) {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common);

        let emulator_config = config.to_emulator_config(self.emulator.vdp_version());
        self.emulator.reload_config(config.psg_version, emulator_config);

        if let Err(err) = self
            .input_mapper
            .reload_config(config.common.keyboard_inputs, config.common.joystick_inputs)
        {
            log::error!("Error reloading input config: {err}");
        }
    }
}

impl NativeEmulator<GenesisInputs, GenesisButton, GenesisEmulator> {
    pub fn reload_genesis_config(&mut self, config: Box<GenesisConfig>) {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common);
        self.emulator.reload_config(config.to_emulator_config());

        if let Err(err) = self.input_mapper.reload_config(
            config.p1_controller_type,
            config.p2_controller_type,
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
        ) {
            log::error!("Error reloading input config: {err}");
        }
    }
}

// TODO simplify or generalize these trait bounds
impl<Inputs, Button, Emulator> NativeEmulator<Inputs, Button, Emulator>
where
    Inputs: Clearable + GetButtonField<Button>,
    Button: Copy,
    Emulator: EmulatorTrait<Inputs>,
    anyhow::Error: From<Emulator::Err<anyhow::Error, anyhow::Error, anyhow::Error>>,
{
    /// Run the emulator until a frame is rendered.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered when rendering frames, pushing audio
    /// samples, or writing save files.
    pub fn render_frame(&mut self) -> anyhow::Result<NativeTickEffect> {
        loop {
            if self.emulator.tick(
                &mut self.renderer,
                &mut self.audio_output,
                self.input_mapper.inputs(),
                &mut self.save_writer,
            )? == TickEffect::FrameRendered
            {
                for event in self.event_pump.poll_iter() {
                    self.input_mapper.handle_event(&event)?;
                    if handle_hotkeys(
                        &self.hotkey_mapper,
                        &event,
                        &mut self.emulator,
                        &mut self.renderer,
                        &self.save_state_path,
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
pub fn create_smsgg(
    config: Box<SmsGgConfig>,
) -> anyhow::Result<NativeEmulator<SmsGgInputs, SmsGgButton, SmsGgEmulator>> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.common.rom_file_path);
    let rom_file_name = parse_file_name(rom_file_path)?;
    let file_ext = parse_file_ext(rom_file_path)?;

    let save_state_path = rom_file_path.with_extension("ss0");

    let rom = fs::read(rom_file_path)
        .with_context(|| format!("Failed to read ROM file at {}", rom_file_path.display()))?;

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

    let emulator_config = config.to_emulator_config(vdp_version);

    let renderer =
        pollster::block_on(WgpuRenderer::new(window, Window::size, config.common.renderer_config))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, config.common.audio_sync)?;
    let input_mapper = InputMapper::new_smsgg(
        joystick,
        config.common.keyboard_inputs,
        config.common.joystick_inputs,
        config.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;
    let save_writer = FsSaveWriter::new(save_path);

    let emulator = SmsGgEmulator::create(
        rom,
        initial_cartridge_ram,
        vdp_version,
        psg_version,
        emulator_config,
    );

    Ok(NativeEmulator {
        emulator,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        event_pump,
        save_state_path,
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
pub fn create_genesis(
    config: Box<GenesisConfig>,
) -> anyhow::Result<NativeEmulator<GenesisInputs, GenesisButton, GenesisEmulator>> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.common.rom_file_path);
    let rom = fs::read(rom_file_path)?;

    let save_path = rom_file_path.with_extension("sav");
    let save_state_path = rom_file_path.with_extension("ss0");

    let initial_ram = fs::read(&save_path).ok();
    if initial_ram.is_some() {
        log::info!("Loaded save file from {}", save_path.display());
    }

    let emulator = GenesisEmulator::create(rom, initial_ram, config.to_emulator_config());

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
    let audio_output = SdlAudioOutput::create_and_init(&audio, config.common.audio_sync)?;
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
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        event_pump,
        save_state_path,
        video,
        cram_debug: None,
        vram_debug: None,
    })
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

// Initialize SDL2 and hide the mouse cursor
fn init_sdl() -> anyhow::Result<(VideoSubsystem, AudioSubsystem, JoystickSubsystem, EventPump)> {
    let sdl = sdl2::init().map_err(|err| anyhow!("Error initializing SDL2: {err}"))?;
    let video =
        sdl.video().map_err(|err| anyhow!("Error initializing SDL2 video subsystem: {err}"))?;
    let audio =
        sdl.audio().map_err(|err| anyhow!("Error initializing SDL2 audio subsystem: {err}"))?;
    let joystick = sdl
        .joystick()
        .map_err(|err| anyhow!("Error initializing SDL2 joystick subsystem: {err}"))?;
    let event_pump =
        sdl.event_pump().map_err(|err| anyhow!("Error initializing SDL2 event pump: {err}"))?;

    sdl.mouse().show_cursor(false);

    Ok((video, audio, joystick, event_pump))
}

fn create_window(
    video: &VideoSubsystem,
    title: &str,
    width: u32,
    height: u32,
    fullscreen: bool,
) -> anyhow::Result<Window> {
    let mut window = video.window(title, width, height).resizable().build()?;

    if fullscreen {
        window
            .set_fullscreen(FullscreenType::Desktop)
            .map_err(|err| anyhow!("Error setting fullscreen: {err}"))?;
    }

    Ok(window)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyResult {
    None,
    Quit,
}

#[allow(clippy::too_many_arguments)]
fn handle_hotkeys<Inputs, Emulator, P>(
    hotkey_mapper: &HotkeyMapper,
    event: &Event,
    emulator: &mut Emulator,
    renderer: &mut WgpuRenderer<Window>,
    save_state_path: P,
    video: &VideoSubsystem,
    cram_debug: &mut Option<CramDebug>,
    vram_debug: &mut Option<VramDebug>,
) -> anyhow::Result<HotkeyResult>
where
    Emulator: EmulatorTrait<Inputs>,
    P: AsRef<Path>,
{
    let save_state_path = save_state_path.as_ref();

    for &hotkey in hotkey_mapper.check_for_hotkeys(event) {
        match hotkey {
            Hotkey::Quit => {
                return Ok(HotkeyResult::Quit);
            }
            Hotkey::ToggleFullscreen => {
                renderer
                    .toggle_fullscreen()
                    .map_err(|err| anyhow!("Error toggling fullscreen: {err}"))?;
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
                *emulator = loaded_emulator;
            }
            Hotkey::SoftReset => {
                emulator.soft_reset();
            }
            Hotkey::HardReset => {
                emulator.hard_reset();
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
                    *vram_debug = Some(VramDebug::new::<Emulator>(video)?);
                }
            },
        }
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
