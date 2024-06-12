mod audio;
mod debug;
mod gb;
mod genesis;
mod nes;
mod rewind;
mod save;
mod smsgg;
mod snes;
mod state;

pub use gb::{create_gb, NativeGameBoyEmulator};
pub use genesis::{create_genesis, create_sega_cd, NativeGenesisEmulator, NativeSegaCdEmulator};
pub use nes::{create_nes, NativeNesEmulator};
pub use smsgg::{create_smsgg, NativeSmsGgEmulator};
pub use snes::{create_snes, NativeSnesEmulator};
pub use state::{SaveStateMetadata, SAVE_STATE_SLOTS};

use crate::config::input::{InputConfig, JoystickInput, KeyboardInput};
use crate::config::{CommonConfig, WindowSize};
use crate::input::{Hotkey, HotkeyMapResult, HotkeyMapper, InputMapper, Joysticks, MappableInputs};
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::debug::{DebugRenderFn, DebuggerWindow};
use crate::mainloop::rewind::Rewinder;
use crate::mainloop::save::FsSaveWriter;
pub use audio::AudioError;
use bincode::error::{DecodeError, EncodeError};
use gb_core::api::GameBoyLoadError;
use jgenesis_common::frontend::{EmulatorTrait, PartialClone, TickEffect};
use jgenesis_renderer::renderer::{RendererError, WgpuRenderer};
use nes_core::api::NesInitializationError;
pub use save::SaveWriteError;
use sdl2::event::{Event, WindowEvent};
use sdl2::render::TextureValueError;
use sdl2::video::{FullscreenType, Window, WindowBuildError};
use sdl2::{AudioSubsystem, EventPump, IntegerOrSdlError, JoystickSubsystem, Sdl, VideoSubsystem};
use segacd_core::api::SegaCdLoadError;
use snes_core::api::SnesLoadError;
use std::cell::RefCell;
use std::error::Error;
use std::ffi::{NulError, OsStr};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use std::{io, thread};
use thiserror::Error;

const MODAL_DURATION: Duration = Duration::from_secs(3);

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
    save_state_paths: SaveStatePaths,
    save_state_slot: usize,
    save_state_metadata: SaveStateMetadata,
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
    ) -> NativeEmulatorResult<Self> {
        let save_state_paths = state::init_paths(&save_state_path)?;
        let save_state_metadata = SaveStateMetadata::load(&save_state_paths);

        Ok(Self {
            save_state_paths,
            save_state_slot: 0,
            save_state_metadata,
            paused: false,
            should_step_frame: false,
            fast_forward_multiplier: common_config.fast_forward_multiplier,
            rewinder: Rewinder::new(Duration::from_secs(
                common_config.rewind_buffer_length_seconds,
            )),
            debugger_window: None,
            debug_render_fn,
        })
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
    event_buffer: Rc<RefCell<Vec<Event>>>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyResult {
    None,
    Quit,
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
    SegaCdDisc(#[from] SegaCdLoadError),
    #[error("{0}")]
    NesLoad(#[from] NesInitializationError),
    #[error("{0}")]
    SnesLoad(#[from] SnesLoadError),
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
    #[allow(clippy::too_many_arguments)]
    fn new<KC, JC, InputMapperFn>(
        emulator: Emulator,
        emulator_config: Emulator::Config,
        common_config: CommonConfig<KC, JC>,
        default_window_size: WindowSize,
        window_title: &str,
        save_writer: FsSaveWriter,
        save_state_path: PathBuf,
        input_mapper_fn: InputMapperFn,
        debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
    ) -> NativeEmulatorResult<Self>
    where
        InputMapperFn: FnOnce(
            JoystickSubsystem,
            &CommonConfig<KC, JC>,
        ) -> NativeEmulatorResult<InputMapper<Inputs, Button>>,
    {
        let (sdl, video, audio, joystick, event_pump) =
            init_sdl(common_config.hide_cursor_over_window)?;

        let window_size = common_config.window_size.unwrap_or(default_window_size);
        let window = create_window(
            &video,
            window_title,
            window_size.width,
            window_size.height,
            common_config.launch_in_fullscreen,
        )?;

        let renderer = pollster::block_on(WgpuRenderer::new(
            window,
            Window::size,
            common_config.renderer_config,
        ))?;

        let audio_output = SdlAudioOutput::create_and_init(&audio, &common_config)?;

        let input_mapper = input_mapper_fn(joystick, &common_config)?;
        let hotkey_mapper = HotkeyMapper::from_config(&common_config.hotkeys)?;

        let hotkey_state = HotkeyState::new(&common_config, save_state_path, debug_render_fn)?;

        Ok(Self {
            emulator,
            config: emulator_config,
            renderer,
            audio_output,
            input_mapper,
            hotkey_mapper,
            save_writer,
            sdl,
            event_pump,
            event_buffer: Rc::new(RefCell::new(Vec::with_capacity(100))),
            video,
            hotkey_state,
        })
    }

    /// Run the emulator until a frame is rendered.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered when rendering frames, pushing audio
    /// samples, or writing save files.
    pub fn render_frame(&mut self) -> NativeEmulatorResult<NativeTickEffect> {
        loop {
            let rewinding = self.hotkey_state.rewinder.is_rewinding();
            let should_wait_for_audio = self.audio_output.should_wait_for_audio();
            let should_tick_emulator = !rewinding
                && !should_wait_for_audio
                && (!self.hotkey_state.paused || self.hotkey_state.should_step_frame);

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

                // Gymnastics to avoid borrow checker errors that would otherwise occur due to
                // calling `&mut self` methods while mutably borrowing the event pump
                let event_buffer_ref = Rc::clone(&self.event_buffer);
                let mut event_buffer = event_buffer_ref.borrow_mut();
                event_buffer.extend(self.event_pump.poll_iter());

                for event in event_buffer.drain(..) {
                    self.input_mapper.handle_event(
                        &event,
                        self.renderer.window_id(),
                        self.renderer.current_display_info(),
                    )?;

                    if let Some(debugger_window) = &mut self.hotkey_state.debugger_window {
                        debugger_window.handle_sdl_event(&event);
                    }

                    if self.handle_hotkeys(&event)? == HotkeyResult::Quit {
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

                if !should_tick_emulator {
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

    /// # Errors
    ///
    /// Returns an error if the state cannot be saved (e.g. due to I/O error).
    pub fn save_state(&mut self, slot: usize) -> NativeEmulatorResult<()> {
        if let Err(err) = state::save(
            &self.emulator,
            &self.hotkey_state.save_state_paths,
            slot,
            &mut self.hotkey_state.save_state_metadata,
        ) {
            self.renderer.add_modal(format!("Failed to save state to slot {slot}"), MODAL_DURATION);
            return Err(err);
        }

        self.renderer.add_modal(format!("Saved state to slot {slot}"), MODAL_DURATION);

        Ok(())
    }

    /// # Errors
    ///
    /// Return an error if the state cannot be loaded (e.g. due to I/O error or because the save
    /// state does not exist).
    pub fn load_state(&mut self, slot: usize) -> NativeEmulatorResult<()> {
        if let Err(err) =
            state::load(&mut self.emulator, &self.config, &self.hotkey_state.save_state_paths, slot)
        {
            self.renderer
                .add_modal(format!("Failed to load state from slot {slot}"), MODAL_DURATION);
            return Err(err);
        }

        self.renderer.add_modal(format!("Loaded state from slot {slot}"), MODAL_DURATION);

        Ok(())
    }

    pub fn save_state_metadata(&self) -> &SaveStateMetadata {
        &self.hotkey_state.save_state_metadata
    }

    fn handle_hotkeys(&mut self, event: &Event) -> NativeEmulatorResult<HotkeyResult> {
        match self.hotkey_mapper.check_for_hotkeys(event) {
            HotkeyMapResult::Pressed(hotkeys) => {
                for &hotkey in &*hotkeys {
                    if self.handle_hotkey_pressed(hotkey)? == HotkeyResult::Quit {
                        return Ok(HotkeyResult::Quit);
                    }
                }
            }
            HotkeyMapResult::Released(hotkeys) => {
                for &hotkey in &*hotkeys {
                    match hotkey {
                        Hotkey::FastForward => {
                            self.renderer.set_speed_multiplier(1);
                            self.audio_output.set_speed_multiplier(1);
                        }
                        Hotkey::Rewind => {
                            self.hotkey_state.rewinder.stop_rewinding();
                        }
                        _ => {}
                    }
                }
            }
            HotkeyMapResult::None => {}
        }

        Ok(HotkeyResult::None)
    }

    fn handle_hotkey_pressed(&mut self, hotkey: Hotkey) -> NativeEmulatorResult<HotkeyResult> {
        match hotkey {
            Hotkey::Quit => return Ok(HotkeyResult::Quit),
            Hotkey::ToggleFullscreen => {
                self.renderer.toggle_fullscreen().map_err(NativeEmulatorError::SdlSetFullscreen)?;
            }
            Hotkey::SaveState => {
                let slot = self.hotkey_state.save_state_slot;
                self.save_state(slot)?;
                log::info!(
                    "Saved state to slot {slot} in '{}'",
                    self.hotkey_state.save_state_paths[slot].display()
                );
            }
            Hotkey::LoadState => {
                let slot = self.hotkey_state.save_state_slot;
                match self.load_state(slot) {
                    Ok(()) => {
                        log::info!(
                            "Loaded state from slot {slot} in '{}'",
                            self.hotkey_state.save_state_paths[slot].display()
                        );
                    }
                    Err(err) => {
                        log::error!(
                            "Error loading save state from slot {slot} in '{}': {err}",
                            self.hotkey_state.save_state_paths[slot].display()
                        );
                    }
                }
            }
            Hotkey::SoftReset => {
                self.emulator.soft_reset();
            }
            Hotkey::HardReset => {
                self.emulator.hard_reset(&mut self.save_writer);
            }
            Hotkey::NextSaveStateSlot => {
                self.hotkey_state.save_state_slot =
                    (self.hotkey_state.save_state_slot + 1) % SAVE_STATE_SLOTS;
                self.renderer.add_modal(
                    format!("Selected save state slot {}", self.hotkey_state.save_state_slot),
                    MODAL_DURATION,
                );
            }
            Hotkey::PrevSaveStateSlot => {
                self.hotkey_state.save_state_slot = if self.hotkey_state.save_state_slot == 0 {
                    state::SAVE_STATE_SLOTS - 1
                } else {
                    self.hotkey_state.save_state_slot - 1
                };
                self.renderer.add_modal(
                    format!("Selected save state slot {}", self.hotkey_state.save_state_slot),
                    MODAL_DURATION,
                );
            }
            Hotkey::Pause => {
                self.hotkey_state.paused = !self.hotkey_state.paused;
            }
            Hotkey::StepFrame => {
                self.hotkey_state.should_step_frame = true;
            }
            Hotkey::FastForward => {
                let multiplier = self.hotkey_state.fast_forward_multiplier;
                self.renderer.set_speed_multiplier(multiplier);
                self.audio_output.set_speed_multiplier(multiplier);
            }
            Hotkey::Rewind => {
                self.hotkey_state.rewinder.start_rewinding();
            }
            Hotkey::OpenDebugger => {
                if self.hotkey_state.debugger_window.is_none() {
                    let debug_render_fn = (self.hotkey_state.debug_render_fn)();
                    match DebuggerWindow::new(&self.video, debug_render_fn) {
                        Ok(debugger_window) => {
                            self.hotkey_state.debugger_window = Some(debugger_window);
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

fn basic_input_mapper_fn<KC, JC, Inputs, Button>(
    all_buttons: &[Button],
) -> impl FnOnce(
    JoystickSubsystem,
    &CommonConfig<KC, JC>,
) -> NativeEmulatorResult<InputMapper<Inputs, Button>>
+ '_
where
    KC: InputConfig<Button = Button, Input = KeyboardInput>,
    JC: InputConfig<Button = Button, Input = JoystickInput>,
    Inputs: Default + MappableInputs<Button>,
    Button: Copy,
{
    |joystick, common_config| {
        InputMapper::new(
            joystick,
            &common_config.keyboard_inputs,
            &common_config.joystick_inputs,
            common_config.axis_deadzone,
            all_buttons,
        )
    }
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

use crate::mainloop::state::SaveStatePaths;
use bincode_config;
