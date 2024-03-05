mod audio;
mod debug;
mod rewind;
mod save;

use crate::config;
use crate::config::{
    CommonConfig, GameBoyConfig, GenesisConfig, NesConfig, SegaCdConfig, SmsGgConfig, SnesConfig,
    WindowSize,
};
use crate::input::{
    GameBoyButton, GenesisButton, Hotkey, HotkeyMapResult, HotkeyMapper, InputMapper, Joysticks,
    MappableInputs, NesButton, SmsGgButton, SnesButton,
};
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::debug::{DebugRenderFn, DebuggerWindow};
use crate::mainloop::rewind::Rewinder;
use crate::mainloop::save::FsSaveWriter;
pub use audio::AudioError;
use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use gb_core::api::{GameBoyEmulator, GameBoyEmulatorConfig, GameBoyLoadError};
use gb_core::inputs::GameBoyInputs;
use genesis_core::{GenesisEmulator, GenesisEmulatorConfig, GenesisInputs};
use jgenesis_common::frontend::{EmulatorTrait, PartialClone, TickEffect};
use jgenesis_renderer::renderer::{RendererError, WgpuRenderer};
use nes_core::api::{NesEmulator, NesEmulatorConfig, NesInitializationError};
use nes_core::input::NesInputs;
pub use save::SaveWriteError;
use sdl2::event::{Event, WindowEvent};
use sdl2::render::TextureValueError;
use sdl2::video::{FullscreenType, Window, WindowBuildError};
use sdl2::{AudioSubsystem, EventPump, IntegerOrSdlError, JoystickSubsystem, Sdl, VideoSubsystem};
use segacd_core::api::{DiscError, DiscResult, SegaCdEmulator, SegaCdEmulatorConfig};
use segacd_core::CdRomFileFormat;
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgEmulator, SmsGgEmulatorConfig, SmsGgInputs};
use snes_core::api::{LoadError, SnesEmulator, SnesEmulatorConfig};
use snes_core::input::SnesInputs;
use std::error::Error;
use std::ffi::{NulError, OsStr};
use std::fs::File;
use std::io::{BufReader, BufWriter};
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

struct HotkeyState<Emulator> {
    save_state_path: PathBuf,
    paused: bool,
    should_step_frame: bool,
    fast_forward_multiplier: u64,
    rewinder: Rewinder<Emulator>,
    debugger_window: Option<DebuggerWindow<Emulator>>,
    debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
}

impl<Emulator: PartialClone> HotkeyState<Emulator> {
    fn new<KC, JC>(
        common_config: &CommonConfig<KC, JC>,
        save_state_path: PathBuf,
        debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
    ) -> Self {
        Self {
            save_state_path,
            paused: false,
            should_step_frame: false,
            fast_forward_multiplier: common_config.fast_forward_multiplier,
            rewinder: Rewinder::new(Duration::from_secs(
                common_config.rewind_buffer_length_seconds,
            )),
            debugger_window: None,
            debug_render_fn,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTickEffect {
    None,
    Exit,
}

pub struct NativeEmulator<Inputs, Button, Config, Emulator> {
    emulator: Emulator,
    config: Config,
    renderer: WgpuRenderer<Window>,
    audio_output: SdlAudioOutput,
    input_mapper: InputMapper<Inputs, Button>,
    hotkey_mapper: HotkeyMapper,
    save_writer: FsSaveWriter,
    sdl: Sdl,
    event_pump: EventPump,
    video: VideoSubsystem,
    hotkey_state: HotkeyState<Emulator>,
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

        self.hotkey_state.fast_forward_multiplier = config.fast_forward_multiplier;
        // Reset speed multiplier in case the fast forward hotkey changed
        self.renderer.set_speed_multiplier(1);
        self.audio_output.set_speed_multiplier(1);

        self.hotkey_state
            .rewinder
            .set_buffer_duration(Duration::from_secs(config.rewind_buffer_length_seconds));

        match HotkeyMapper::from_config(&config.hotkeys) {
            Ok(hotkey_mapper) => {
                self.hotkey_mapper = hotkey_mapper;
            }
            Err(err) => {
                log::error!("Error reloading hotkey config: {err}");
            }
        }

        self.sdl.mouse().show_cursor(!config.hide_cursor_over_window);

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

        if let Err(err) = self.input_mapper.reload_config(
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.common.axis_deadzone,
        ) {
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
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.common.axis_deadzone,
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
            config.genesis.common.keyboard_inputs,
            config.genesis.common.joystick_inputs,
            config.genesis.common.axis_deadzone,
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
    pub fn change_disc<P: AsRef<Path>>(&mut self, rom_path: P) -> DiscResult<()> {
        let rom_format = CdRomFileFormat::from_file_path(rom_path.as_ref()).unwrap_or_else(|| {
            log::warn!(
                "Unrecognized CD-ROM file format, treating as CUE: {}",
                rom_path.as_ref().display()
            );
            CdRomFileFormat::CueBin
        });

        self.emulator.change_disc(rom_path, rom_format)?;

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

pub type NativeNesEmulator = NativeEmulator<NesInputs, NesButton, NesEmulatorConfig, NesEmulator>;

impl NativeNesEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_nes_config(&mut self, config: Box<NesConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let emulator_config = config.to_emulator_config();
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self.input_mapper.reload_config(
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.common.axis_deadzone,
        ) {
            log::error!("Error reloading input config: {err}");
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

        if let Err(err) = self.input_mapper.reload_config(
            config.p2_controller_type,
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.super_scope_config,
            config.common.axis_deadzone,
        ) {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
}

pub type NativeGameBoyEmulator =
    NativeEmulator<GameBoyInputs, GameBoyButton, GameBoyEmulatorConfig, GameBoyEmulator>;

impl NativeGameBoyEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_gb_config(&mut self, config: Box<GameBoyConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let emulator_config = config.to_emulator_config();
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self.input_mapper.reload_config(
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.common.axis_deadzone,
        ) {
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
    NesLoad(#[from] NesInitializationError),
    #[error("{0}")]
    SnesLoad(#[from] LoadError),
    #[error("{0}")]
    GameBoyLoad(#[from] GameBoyLoadError),
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
    Inputs: Default + MappableInputs<Button>,
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
            let rewinding = self.hotkey_state.rewinder.is_rewinding();
            let should_tick_emulator =
                !rewinding && (!self.hotkey_state.paused || self.hotkey_state.should_step_frame);
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
                self.hotkey_state.should_step_frame = false;

                if let Some(debugger_window) = &mut self.hotkey_state.debugger_window {
                    if let Err(err) = debugger_window.update(&mut self.emulator) {
                        log::error!("Debugger window error: {err}");
                    }
                }

                for event in self.event_pump.poll_iter() {
                    self.input_mapper.handle_event(
                        &event,
                        self.renderer.window_id(),
                        self.renderer.current_display_info(),
                    )?;

                    if let Some(debugger_window) = &mut self.hotkey_state.debugger_window {
                        debugger_window.handle_sdl_event(&event);
                    }

                    if handle_hotkeys(HandleHotkeysArgs {
                        hotkey_mapper: &self.hotkey_mapper,
                        event: &event,
                        emulator: &mut self.emulator,
                        config: &self.config,
                        renderer: &mut self.renderer,
                        audio_output: &mut self.audio_output,
                        save_writer: &mut self.save_writer,
                        video: &self.video,
                        hotkey_state: &mut self.hotkey_state,
                    })? == HotkeyResult::Quit
                    {
                        return Ok(NativeTickEffect::Exit);
                    }

                    match event {
                        Event::Quit { .. } => {
                            return Ok(NativeTickEffect::Exit);
                        }
                        Event::Window { win_event, window_id, .. } => {
                            if win_event == WindowEvent::Close {
                                if window_id == self.renderer.window_id() {
                                    return Ok(NativeTickEffect::Exit);
                                }

                                if self
                                    .hotkey_state
                                    .debugger_window
                                    .as_ref()
                                    .is_some_and(|debugger| window_id == debugger.window_id())
                                {
                                    self.hotkey_state.debugger_window = None;
                                }
                            }

                            if window_id == self.renderer.window_id() {
                                handle_window_event(win_event, &mut self.renderer);
                            }
                        }
                        _ => {}
                    }
                }

                if frame_rendered {
                    self.hotkey_state.rewinder.record_frame(&self.emulator);
                }

                if rewinding {
                    self.hotkey_state.rewinder.tick(
                        &mut self.emulator,
                        &mut self.renderer,
                        &self.config,
                    )?;
                }

                if rewinding || self.hotkey_state.paused {
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
        self.emulator.hard_reset(&mut self.save_writer);
    }

    pub fn open_memory_viewer(&mut self) {
        if self.hotkey_state.debugger_window.is_none() {
            self.hotkey_state.debugger_window =
                open_debugger_window(&self.video, self.hotkey_state.debug_render_fn);
        }
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
    let file_ext = parse_file_ext(rom_file_path)?;

    let save_state_path = rom_file_path.with_extension("ss0");

    let rom = fs::read(rom_file_path).map_err(|source| NativeEmulatorError::RomRead {
        path: rom_file_path.display().to_string(),
        source,
    })?;

    let save_path = rom_file_path.with_extension("sav");
    let mut save_writer = FsSaveWriter::new(save_path);

    let vdp_version =
        config.vdp_version.unwrap_or_else(|| config::default_vdp_version_for_ext(file_ext));
    let psg_version =
        config.psg_version.unwrap_or_else(|| config::default_psg_version_for_ext(file_ext));

    log::info!("VDP version: {vdp_version:?}");
    log::info!("PSG version: {psg_version:?}");

    let (sdl, video, audio, joystick, event_pump) =
        init_sdl(config.common.hide_cursor_over_window)?;

    let WindowSize { width: window_width, height: window_height } =
        config.common.window_size.unwrap_or_else(|| config::default_smsgg_window_size(vdp_version));

    let rom_title = file_name_no_ext(rom_file_path)?;
    let window = create_window(
        &video,
        &format!("smsgg - {rom_title}"),
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
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
        config.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;

    let emulator = SmsGgEmulator::create(rom, emulator_config, &mut save_writer);

    Ok(NativeEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        sdl,
        event_pump,
        video,
        hotkey_state: HotkeyState::new(&config.common, save_state_path, debug::smsgg::render_fn),
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
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator = GenesisEmulator::create(rom, emulator_config, &mut save_writer);

    let (sdl, video, audio, joystick, event_pump) =
        init_sdl(config.common.hide_cursor_over_window)?;

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
        joystick,
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
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
        sdl,
        event_pump,
        video,
        hotkey_state: HotkeyState::new(&config.common, save_state_path, debug::genesis::render_fn),
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

    let rom_path = Path::new(&config.genesis.common.rom_file_path);
    let rom_format = CdRomFileFormat::from_file_path(rom_path).unwrap_or_else(|| {
        log::warn!(
            "Unrecognized CD-ROM file extension, behaving as if this is a CUE file: {}",
            rom_path.display()
        );
        CdRomFileFormat::CueBin
    });

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let bios_file_path = config.bios_file_path.as_ref().ok_or(NativeEmulatorError::SegaCdNoBios)?;
    let bios = fs::read(bios_file_path).map_err(|source| NativeEmulatorError::SegaCdBiosRead {
        path: bios_file_path.clone(),
        source,
    })?;

    let emulator_config = config.to_emulator_config();
    let emulator = SegaCdEmulator::create(
        bios,
        rom_path,
        rom_format,
        config.run_without_disc,
        emulator_config,
        &mut save_writer,
    )?;

    let (sdl, video, audio, joystick, event_pump) =
        init_sdl(config.genesis.common.hide_cursor_over_window)?;

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
        joystick,
        config.genesis.common.keyboard_inputs.clone(),
        config.genesis.common.joystick_inputs.clone(),
        config.genesis.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.genesis.common.hotkeys)?;

    Ok(NativeEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        sdl,
        event_pump,
        video,
        hotkey_state: HotkeyState::new(
            &config.genesis.common,
            save_state_path,
            debug::genesis::render_fn,
        ),
    })
}

/// Create an emulator with the NES core with the given config.
///
/// # Errors
///
/// Propagates any errors encountered during initialization.
pub fn create_nes(config: Box<NesConfig>) -> NativeEmulatorResult<NativeNesEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let rom = fs::read(rom_path).map_err(|source| NativeEmulatorError::RomRead {
        path: config.common.rom_file_path.clone(),
        source,
    })?;

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator = NesEmulator::create(rom, emulator_config, &mut save_writer)?;

    let (sdl, video, audio, joystick, event_pump) =
        init_sdl(config.common.hide_cursor_over_window)?;

    let WindowSize { width: window_width, height: window_height } =
        config.common.window_size.unwrap_or(config::DEFAULT_GENESIS_WINDOW_SIZE);

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window = create_window(
        &video,
        &format!("nes - {rom_title}"),
        window_width,
        window_height,
        config.common.launch_in_fullscreen,
    )?;

    let renderer =
        pollster::block_on(WgpuRenderer::new(window, Window::size, config.common.renderer_config))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, &config.common)?;

    let input_mapper = InputMapper::new_nes(
        joystick,
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
        config.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;

    Ok(NativeNesEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        sdl,
        event_pump,
        video,
        hotkey_state: HotkeyState::new(&config.common, save_state_path, debug::nes::render_fn),
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
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let coprocessor_roms = config.to_coprocessor_roms();
    let mut emulator =
        SnesEmulator::create(rom, emulator_config, coprocessor_roms, &mut save_writer)?;

    let (sdl, video, audio, joystick, event_pump) =
        init_sdl(config.common.hide_cursor_over_window)?;

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

    let input_mapper = InputMapper::new_snes(
        joystick,
        config.p2_controller_type,
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
        config.super_scope_config.clone(),
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
        sdl,
        event_pump,
        video,
        hotkey_state: HotkeyState::new(&config.common, save_state_path, debug::snes::render_fn),
    })
}

/// Create an emulator with the Game Boy core with the given config.
///
/// # Errors
///
/// This function will return an error if unable to initialize the emulator.
pub fn create_gb(config: Box<GameBoyConfig>) -> NativeEmulatorResult<NativeGameBoyEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let rom = fs::read(rom_path).map_err(|source| NativeEmulatorError::RomRead {
        path: config.common.rom_file_path.clone(),
        source,
    })?;

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator = GameBoyEmulator::create(rom, emulator_config, &mut save_writer)?;

    let (sdl, video, audio, joystick, event_pump) =
        init_sdl(config.common.hide_cursor_over_window)?;

    let WindowSize { width: window_width, height: window_height } = config::DEFAULT_GB_WINDOW_SIZE;

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window = create_window(
        &video,
        &format!("gb - {rom_title}"),
        window_width,
        window_height,
        config.common.launch_in_fullscreen,
    )?;

    let renderer =
        pollster::block_on(WgpuRenderer::new(window, Window::size, config.common.renderer_config))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, &config.common)?;

    let input_mapper = InputMapper::new_gb(
        joystick,
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
        config.common.axis_deadzone,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;

    Ok(NativeGameBoyEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        sdl,
        event_pump,
        video,
        hotkey_state: HotkeyState::new(&config.common, save_state_path, debug::gb::render_fn),
    })
}

fn file_name_no_ext<P: AsRef<Path>>(path: P) -> NativeEmulatorResult<String> {
    path.as_ref()
        .with_extension("")
        .file_name()
        .map(|file_name| file_name.to_string_lossy().into_owned())
        .ok_or_else(|| NativeEmulatorError::ParseFileName(path.as_ref().display().to_string()))
}

fn parse_file_ext(path: &Path) -> NativeEmulatorResult<&str> {
    path.extension()
        .and_then(OsStr::to_str)
        .ok_or_else(|| NativeEmulatorError::ParseFileExtension(path.display().to_string()))
}

// Initialize SDL2
fn init_sdl(
    hide_cursor_over_window: bool,
) -> NativeEmulatorResult<(Sdl, VideoSubsystem, AudioSubsystem, JoystickSubsystem, EventPump)> {
    let sdl = sdl2::init().map_err(NativeEmulatorError::SdlInit)?;
    let video = sdl.video().map_err(NativeEmulatorError::SdlVideoInit)?;
    let audio = sdl.audio().map_err(NativeEmulatorError::SdlAudioInit)?;
    let joystick = sdl.joystick().map_err(NativeEmulatorError::SdlJoystickInit)?;
    let event_pump = sdl.event_pump().map_err(NativeEmulatorError::SdlEventPumpInit)?;

    sdl.mouse().show_cursor(!hide_cursor_over_window);

    Ok((sdl, video, audio, joystick, event_pump))
}

fn create_window(
    video: &VideoSubsystem,
    title: &str,
    width: u32,
    height: u32,
    fullscreen: bool,
) -> NativeEmulatorResult<Window> {
    let mut window = video.window(title, width, height).metal_view().resizable().build()?;

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

struct HandleHotkeysArgs<'a, Emulator: EmulatorTrait> {
    hotkey_mapper: &'a HotkeyMapper,
    event: &'a Event,
    emulator: &'a mut Emulator,
    config: &'a Emulator::Config,
    renderer: &'a mut WgpuRenderer<Window>,
    audio_output: &'a mut SdlAudioOutput,
    save_writer: &'a mut FsSaveWriter,
    video: &'a VideoSubsystem,
    hotkey_state: &'a mut HotkeyState<Emulator>,
}

fn handle_hotkeys<Emulator>(
    mut args: HandleHotkeysArgs<'_, Emulator>,
) -> NativeEmulatorResult<HotkeyResult>
where
    Emulator: EmulatorTrait,
{
    match args.hotkey_mapper.check_for_hotkeys(args.event) {
        HotkeyMapResult::Pressed(hotkeys) => {
            for &hotkey in hotkeys {
                if handle_hotkey_pressed(hotkey, &mut args)? == HotkeyResult::Quit {
                    return Ok(HotkeyResult::Quit);
                }
            }
        }
        HotkeyMapResult::Released(hotkeys) => {
            for &hotkey in hotkeys {
                match hotkey {
                    Hotkey::FastForward => {
                        args.renderer.set_speed_multiplier(1);
                        args.audio_output.set_speed_multiplier(1);
                    }
                    Hotkey::Rewind => {
                        args.hotkey_state.rewinder.stop_rewinding();
                    }
                    _ => {}
                }
            }
        }
        HotkeyMapResult::None => {}
    }

    Ok(HotkeyResult::None)
}

fn handle_hotkey_pressed<Emulator>(
    hotkey: Hotkey,
    args: &mut HandleHotkeysArgs<'_, Emulator>,
) -> NativeEmulatorResult<HotkeyResult>
where
    Emulator: EmulatorTrait,
{
    let save_state_path = &args.hotkey_state.save_state_path;

    match hotkey {
        Hotkey::Quit => {
            return Ok(HotkeyResult::Quit);
        }
        Hotkey::ToggleFullscreen => {
            args.renderer.toggle_fullscreen().map_err(NativeEmulatorError::SdlSetFullscreen)?;
        }
        Hotkey::SaveState => {
            save_state(args.emulator, save_state_path)?;
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
            loaded_emulator.take_rom_from(args.emulator);

            // Force a config reload because the emulator will contain some config fields
            loaded_emulator.reload_config(args.config);

            *args.emulator = loaded_emulator;
        }
        Hotkey::SoftReset => {
            args.emulator.soft_reset();
        }
        Hotkey::HardReset => {
            args.emulator.hard_reset(args.save_writer);
        }
        Hotkey::Pause => {
            args.hotkey_state.paused = !args.hotkey_state.paused;
        }
        Hotkey::StepFrame => {
            args.hotkey_state.should_step_frame = true;
        }
        Hotkey::FastForward => {
            args.renderer.set_speed_multiplier(args.hotkey_state.fast_forward_multiplier);
            args.audio_output.set_speed_multiplier(args.hotkey_state.fast_forward_multiplier);
        }
        Hotkey::Rewind => {
            args.hotkey_state.rewinder.start_rewinding();
        }
        Hotkey::OpenDebugger => {
            if args.hotkey_state.debugger_window.is_none() {
                let debug_render_fn = (args.hotkey_state.debug_render_fn)();
                match DebuggerWindow::new(args.video, debug_render_fn) {
                    Ok(debugger_window) => {
                        args.hotkey_state.debugger_window = Some(debugger_window);
                    }
                    Err(err) => {
                        log::error!("Error opening debugger window: {err}");
                    }
                }
            }
        }
    }

    Ok(HotkeyResult::None)
}

fn open_debugger_window<Emulator>(
    video: &VideoSubsystem,
    debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
) -> Option<DebuggerWindow<Emulator>> {
    let render_fn = debug_render_fn();
    match DebuggerWindow::new(video, render_fn) {
        Ok(debugger_window) => Some(debugger_window),
        Err(err) => {
            log::error!("Error opening debugger window: {err}");
            None
        }
    }
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
        bincode::config::standard()
            .with_little_endian()
            .with_fixed_int_encoding()
            .with_limit::<{ 100 * 1024 * 1024 }>()
    };
}

use bincode_config;

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
