mod audio;
mod debug;
mod gb;
mod gba;
mod genesis;
mod input;
mod nes;
mod render;
mod rewind;
mod runner;
mod save;
mod smsgg;
mod snes;
mod state;

pub use gb::{NativeGameBoyEmulator, create_gb};
pub use gba::{NativeGbaEmulator, create_gba};
pub use genesis::{
    Native32XEmulator, NativeGenesisEmulator, NativeSegaCdEmulator, create_32x, create_genesis,
    create_sega_cd,
};
pub use nes::{NativeNesEmulator, create_nes};
pub use smsgg::{NativeSmsGgEmulator, create_smsgg};
pub use snes::{NativeSnesEmulator, create_snes};
pub use state::{SAVE_STATE_SLOTS, SaveStateMetadata};

use crate::archive::ArchiveError;
use crate::config::CommonConfig;
use crate::fpstracker::FpsTracker;
use crate::input::{InputEvent, InputMapper, Joysticks};
use crate::mainloop::audio::{SdlAudioOutput, SdlAudioOutputHandle};
use crate::mainloop::debug::{DebugFn, DebuggerWindow};
use crate::mainloop::render::RecvFrameError;
use crate::mainloop::runner::{
    ChangeDiscFn, RemoveDiscFn, RunnerCommand, RunnerCommandResponse, RunnerSpawnArgs, RunnerThread,
};
use crate::mainloop::save::FsSaveWriter;
pub use audio::AudioError;
use bincode::error::{DecodeError, EncodeError};
use gb_core::api::GameBoyLoadError;
use gba_core::api::GbaLoadError;
use genesis_config::GenesisRegion;
use jgenesis_common::frontend::{EmulatorConfigTrait, EmulatorTrait, MappableInputs};
use jgenesis_native_config::EguiTheme;
use jgenesis_native_config::common::{HideMouseCursor, WindowSize};
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use jgenesis_native_config::input::{CompactHotkey, Hotkey};
use jgenesis_renderer::renderer;
use jgenesis_renderer::renderer::{RendererError, WgpuRenderer};
use nes_core::api::NesInitializationError;
pub use save::SaveWriteError;
use sdl3::event::{Event, WindowEvent};
use sdl3::video::{FullscreenType, Window, WindowBuildError};
use sdl3::{AudioSubsystem, EventPump, IntegerOrSdlError, JoystickSubsystem, Sdl, VideoSubsystem};
use segacd_core::api::SegaCdLoadError;
use snes_core::api::SnesLoadError;
use std::cell::RefCell;
use std::error::Error;
use std::ffi::NulError;
use std::fmt::Debug;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use thiserror::Error;

const MODAL_DURATION: Duration = Duration::from_secs(3);

trait RendererExt {
    fn focus(&mut self);

    fn window_id(&self) -> u32;

    fn is_fullscreen(&self) -> bool;

    // Returns new fullscreen state
    fn toggle_fullscreen(&mut self) -> Result<bool, sdl3::Error>;
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

    fn is_fullscreen(&self) -> bool {
        matches!(self.window().fullscreen_state(), FullscreenType::Desktop | FullscreenType::True)
    }

    fn toggle_fullscreen(&mut self) -> Result<bool, sdl3::Error> {
        // SAFETY: This is not reassigning the window
        unsafe {
            let window = self.window_mut();
            let currently_fullscreen = window.fullscreen_state() != FullscreenType::Off;
            let new_fullscreen = !currently_fullscreen;
            window.set_fullscreen(new_fullscreen)?;

            Ok(new_fullscreen)
        }
    }
}

struct HotkeyState<Emulator> {
    hide_mouse_cursor: HideMouseCursor,
    save_state_slot: usize,
    paused: bool,
    fast_forward_multiplier: u64,
    rewinding: bool,
    overclocking_enabled: bool,
    debugger_window: Option<DebuggerWindow>,
    window_scale_factor: Option<f32>,
    egui_theme: EguiTheme,
    debug_fn: DebugFn<Emulator>,
}

impl<Emulator: EmulatorTrait> HotkeyState<Emulator> {
    fn new(common_config: &CommonConfig, debug_fn: DebugFn<Emulator>) -> Self {
        Self {
            hide_mouse_cursor: common_config.hide_mouse_cursor,
            save_state_slot: 0,
            paused: false,
            fast_forward_multiplier: common_config.fast_forward_multiplier,
            rewinding: false,
            overclocking_enabled: true,
            debugger_window: None,
            window_scale_factor: common_config.window_scale_factor,
            egui_theme: common_config.egui_theme,
            debug_fn,
        }
    }

    fn is_debugger_window_id(&self, window_id: u32) -> bool {
        self.debugger_window.as_ref().is_some_and(|debugger| window_id == debugger.window_id())
    }
}

#[derive(Debug, Clone, Copy)]
struct WindowState {
    gui_focused: bool,
    emulator_focused: bool,
    debugger_focused: bool,
}

impl WindowState {
    fn new() -> Self {
        Self {
            gui_focused: false,
            emulator_focused: true, // Assume emulator window has focus at launch
            debugger_focused: false,
        }
    }

    fn any_focused(self) -> bool {
        self.gui_focused || self.emulator_focused || self.debugger_focused
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTickEffect {
    PowerOff,
    Exit,
}

pub struct NativeEmulator<Emulator: EmulatorTrait> {
    runner: RunnerThread<Emulator>,
    // Config sent from the frontend
    raw_config: Emulator::Config,
    // Config with overclocking maybe forcibly disabled due to hotkey state
    config: Emulator::Config,
    common_config: CommonConfig,
    renderer: WgpuRenderer<Window>,
    audio_output_handle: SdlAudioOutputHandle,
    input_mapper: InputMapper<Emulator::Button>,
    inputs: Emulator::Inputs,
    event_pump: EventPump,
    event_buffer: Rc<RefCell<Vec<Event>>>,
    video: VideoSubsystem,
    hotkey_state: HotkeyState<Emulator>,
    window_state: WindowState,
    fps_tracker: FpsTracker,
    rom_path: PathBuf,
    // Put SDL handle last so that it is dropped last when the emulator is dropped
    sdl: Sdl,
}

impl<Emulator: EmulatorTrait> NativeEmulator<Emulator> {
    fn reload_common_config(&mut self, config: &CommonConfig) -> Result<(), AudioError> {
        self.common_config = config.clone();

        self.renderer.reload_config(config.renderer_config);

        self.audio_output_handle.reload_config(config)?;

        self.hotkey_state.hide_mouse_cursor = config.hide_mouse_cursor;

        self.hotkey_state.fast_forward_multiplier = config.fast_forward_multiplier;
        // Reset speed multiplier in case the fast forward hotkey changed
        self.renderer.set_speed_multiplier(1);

        let fullscreen = self.renderer.is_fullscreen();
        self.sdl.mouse().show_cursor(!config.hide_mouse_cursor.should_hide(fullscreen));

        self.hotkey_state.egui_theme = config.egui_theme;
        if let Some(debugger) = &mut self.hotkey_state.debugger_window {
            debugger.update_egui_theme(config.egui_theme);
        }

        Ok(())
    }

    pub fn focus(&mut self) {
        self.renderer.focus();
    }

    pub fn event_pump_and_joysticks_mut(&mut self) -> (&mut EventPump, &mut Joysticks) {
        (&mut self.event_pump, self.input_mapper.joysticks_mut())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyEffect {
    PowerOff,
    Exit,
}

#[derive(Debug, Error)]
pub enum NativeEmulatorError {
    #[error("{0}")]
    Render(#[from] RendererError),
    #[error("{0}")]
    Audio(#[from] AudioError),
    #[error("{0}")]
    SaveWrite(#[from] SaveWriteError),
    #[error("Error initializing SDL3: {0}")]
    SdlInit(sdl3::Error),
    #[error("Error initializing SDL3 video subsystem: {0}")]
    SdlVideoInit(sdl3::Error),
    #[error("Error initializing SDL3 audio subsystem: {0}")]
    SdlAudioInit(sdl3::Error),
    #[error("Error initializing SDL3 joystick subsystem: {0}")]
    SdlJoystickInit(sdl3::Error),
    #[error("Error initializing SDL3 event pump: {0}")]
    SdlEventPumpInit(sdl3::Error),
    #[error("Error creating SDL3 window: {0}")]
    SdlCreateWindow(#[from] WindowBuildError),
    #[error("Error changing window title to '{title}': {source}")]
    SdlSetWindowTitle {
        title: String,
        #[source]
        source: NulError,
    },
    #[error("Error toggling window fullscreen: {0}")]
    SdlSetFullscreen(sdl3::Error),
    #[error("Error opening joystick {device_id}: {source}")]
    SdlJoystickOpen {
        device_id: u32,
        #[source]
        source: IntegerOrSdlError,
    },
    #[error("Unable to determine file name for path: '{0}'")]
    ParseFileName(String),
    #[error("Unable to determine file extension for path: '{0}'")]
    ParseFileExtension(String),
    #[error("Failed to create save directory at '{path}': {source}")]
    CreateSaveDir {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Failed to read ROM file at '{path}': {source}")]
    RomRead {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    Archive(#[from] ArchiveError),
    #[error("No SMS BIOS provided")]
    SmsNoBios,
    #[error("No Game Gear BIOS provided")]
    GgNoBios,
    #[error("Error opening BIOS file at '{path}': {source}")]
    SmsGgBiosRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0} BIOS is required for Sega CD emulation")]
    SegaCdNoBios(GenesisRegion),
    #[error("Error opening BIOS file at '{path}': {source}")]
    SegaCdBiosRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    SegaCdDisc(#[from] SegaCdLoadError),
    #[error("{0}")]
    NesLoad(#[from] NesInitializationError),
    #[error("{0}")]
    SnesLoad(#[from] SnesLoadError),
    #[error("No Game Boy boot ROM provided")]
    GbNoDmgBootRom,
    #[error("No Game Boy Color boot ROM provided")]
    GbNoCgbBootRom,
    #[error("Failed to load boot ROM: {0}")]
    GbBootRomLoad(io::Error),
    #[error("{0}")]
    GameBoyLoad(#[from] GameBoyLoadError),
    #[error("No Game Boy Advance BIOS provided")]
    GbaNoBios,
    #[error("Failed to load GBA BIOS: {0}")]
    GbaBiosLoad(io::Error),
    #[error("Failed to initialize GBA emulator: {0}")]
    GbaLoad(#[from] GbaLoadError),
    #[error("I/O error opening save state file '{path}': {source}")]
    StateFileOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error saving state: {0}")]
    SaveState(#[from] EncodeError),
    #[error("Error saving state: {0}")]
    SaveStateIo(io::Error),
    #[error("Error loading state: {0}")]
    LoadState(#[from] DecodeError),
    #[error("Error loading state: {0}")]
    LoadStateIo(io::Error),
    #[error("Save state begins with invalid prefix")]
    LoadStatePrefixMismatch,
    #[error("Save state version mismatch; expected '{expected}', got '{actual}'")]
    LoadStateVersionMismatch { expected: String, actual: String },
    #[error("Lost connection to runner thread")]
    LostRunnerConnection,
    #[error("Error changing/removing disc: {0}")]
    ChangeDisc(#[source] Box<dyn Error + Send + Sync + 'static>),
    #[error("Error in emulation core: {0}")]
    Emulator(#[source] Box<dyn Error + Send + Sync + 'static>),
}

pub type NativeEmulatorResult<T> = Result<T, NativeEmulatorError>;

pub(crate) struct CreatedEmulator<Emulator: EmulatorTrait> {
    pub emulator: Emulator,
    pub window_title: String,
    pub default_window_size: WindowSize,
}

pub(crate) type CreateEmulatorFn<Emulator> = dyn FnOnce(&mut FsSaveWriter) -> Result<CreatedEmulator<Emulator>, NativeEmulatorError>
    + Send
    + Sync
    + 'static;

pub(crate) struct NativeEmulatorArgs<'input, 'turbo, Emulator: EmulatorTrait> {
    pub create_emulator_fn: Box<CreateEmulatorFn<Emulator>>,
    pub change_disc_fn: ChangeDiscFn<Emulator>,
    pub remove_disc_fn: RemoveDiscFn<Emulator>,
    pub emulator_config: Emulator::Config,
    pub common_config: CommonConfig,
    pub rom_extension: String,
    pub save_path: PathBuf,
    pub save_state_path: PathBuf,
    pub button_mappings: ButtonMappingVec<'input, Emulator::Button>,
    pub turbo_mappings: ButtonMappingVec<'turbo, Emulator::Button>,
    pub initial_inputs: Emulator::Inputs,
    pub debug_fn: DebugFn<Emulator>,
}

impl<'input, 'turbo, Emulator> NativeEmulatorArgs<'input, 'turbo, Emulator>
where
    Emulator: EmulatorTrait,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        create_emulator_fn: Box<CreateEmulatorFn<Emulator>>,
        emulator_config: Emulator::Config,
        common_config: CommonConfig,
        rom_extension: String,
        save_path: PathBuf,
        save_state_path: PathBuf,
        button_mappings: ButtonMappingVec<'input, Emulator::Button>,
    ) -> Self {
        NativeEmulatorArgs {
            create_emulator_fn,
            change_disc_fn: |_emulator, _path| Ok(String::new()),
            remove_disc_fn: |_emulator| {},
            emulator_config,
            common_config,
            rom_extension,
            save_path,
            save_state_path,
            button_mappings,
            turbo_mappings: vec![],
            initial_inputs: Emulator::Inputs::default(),
            debug_fn: debug::null_debug_fn,
        }
    }

    pub fn with_turbo_mappings(
        mut self,
        turbo_mappings: ButtonMappingVec<'turbo, Emulator::Button>,
    ) -> Self {
        self.turbo_mappings = turbo_mappings;
        self
    }

    pub fn with_initial_inputs(mut self, inputs: Emulator::Inputs) -> Self {
        self.initial_inputs = inputs;
        self
    }

    pub fn with_debug_fn(mut self, debug_fn: DebugFn<Emulator>) -> Self {
        self.debug_fn = debug_fn;
        self
    }

    pub fn with_disc_change_fns(
        mut self,
        change_disc_fn: ChangeDiscFn<Emulator>,
        remove_disc_fn: RemoveDiscFn<Emulator>,
    ) -> Self {
        self.change_disc_fn = change_disc_fn;
        self.remove_disc_fn = remove_disc_fn;
        self
    }
}

impl<Emulator> NativeEmulator<Emulator>
where
    Emulator: EmulatorTrait,
{
    fn new(
        NativeEmulatorArgs {
            create_emulator_fn,
            change_disc_fn,
            remove_disc_fn,
            emulator_config,
            common_config,
            rom_extension,
            save_path,
            save_state_path,
            button_mappings,
            turbo_mappings,
            initial_inputs,
            debug_fn,
        }: NativeEmulatorArgs<'_, '_, Emulator>,
    ) -> NativeEmulatorResult<Self> {
        let (sdl, video, audio, joystick, event_pump) = init_sdl3(&common_config)?;

        let (audio_output, mut audio_output_handle) =
            SdlAudioOutput::create_and_init(audio, &common_config)?;

        let input_mapper = InputMapper::new(
            joystick,
            common_config.axis_deadzone,
            &button_mappings,
            &turbo_mappings,
            &common_config.hotkey_config.to_mapping_vec(),
        );

        let hotkey_state = HotkeyState::new(&common_config, debug_fn);

        let save_writer = FsSaveWriter::new(save_path);

        let runner = RunnerThread::spawn(RunnerSpawnArgs {
            create_emulator_fn,
            change_disc_fn,
            remove_disc_fn,
            common_config: common_config.clone(),
            emulator_config: emulator_config.clone(),
            rom_extension: rom_extension.clone(),
            save_state_path,
            initial_inputs: initial_inputs.clone(),
            audio_output_handle: &mut audio_output_handle,
            audio_output,
            save_writer,
        })?;

        let mut initial_window_size =
            common_config.window_size.unwrap_or(runner.default_window_size());
        if let Some(scale_factor) = common_config.window_scale_factor {
            initial_window_size = initial_window_size.scale(scale_factor);
        }

        let window = create_window(
            &video,
            runner.initial_window_title(),
            initial_window_size.width,
            initial_window_size.height,
            common_config.launch_in_fullscreen,
        )?;

        let window_size = sdl_window_size(&window);
        let renderer = pollster::block_on(WgpuRenderer::new(
            window,
            window_size,
            common_config.renderer_config,
        ))?;

        let mut emulator = Self {
            runner,
            raw_config: emulator_config.clone(),
            config: emulator_config,
            common_config: common_config.clone(),
            renderer,
            audio_output_handle,
            input_mapper,
            inputs: initial_inputs,
            sdl,
            event_pump,
            event_buffer: Rc::new(RefCell::new(Vec::with_capacity(100))),
            video,
            hotkey_state,
            window_state: WindowState::new(),
            fps_tracker: FpsTracker::new(),
            rom_path: common_config.rom_file_path,
        };

        if common_config.load_recent_state_at_launch {
            emulator.try_load_most_recent_state();
        }

        // Make a best effort to focus the newly-created emulator window
        emulator.renderer.focus();

        Ok(emulator)
    }

    /// Run the emulator until a frame is rendered.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered when rendering frames, pushing audio
    /// samples, or writing save files.
    pub fn run(&mut self) -> NativeEmulatorResult<Option<NativeTickEffect>> {
        self.runner.try_recv_error()?;

        let paused = self.hotkey_state.paused
            || self
                .common_config
                .pause_emulator
                .should_pause(self.window_state.emulator_focused, self.window_state.any_focused());
        self.runner.set_paused(paused);

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
            );

            if let Some(debugger_window) = &mut self.hotkey_state.debugger_window {
                debugger_window.handle_sdl_event(&event);
            }

            match event {
                Event::Quit { .. } => {
                    return Ok(Some(NativeTickEffect::PowerOff));
                }
                Event::Window { win_event, window_id, .. } => {
                    if let Some(effect) = self.handle_window_event(win_event, window_id) {
                        return Ok(Some(effect));
                    }
                }
                _ => {}
            }
        }

        if let Some(effect) = self.process_input_events()? {
            return Ok(Some(effect));
        }

        self.runner.update_inputs(&self.inputs);

        if self.hotkey_state.rewinding {
            self.renderer.reset_interframe_state();
        }

        while let Some(response) = self.runner.try_recv_command_response() {
            self.handle_runner_cmd_response(response);
        }

        match self.runner.try_recv_frame(&mut self.renderer, Duration::from_millis(1)) {
            Ok(()) => {
                self.fps_tracker.record_frame();
            }
            Err(RecvFrameError::Render(err)) => return Err(NativeEmulatorError::Render(err)),
            Err(
                RecvFrameError::Recv(RecvTimeoutError::Disconnected)
                | RecvFrameError::LostConnection,
            ) => {
                return Err(NativeEmulatorError::LostRunnerConnection);
            }
            Err(RecvFrameError::Recv(RecvTimeoutError::Timeout)) => {}
        }

        if let Some(debugger_window) = &mut self.hotkey_state.debugger_window
            && let Err(err) = debugger_window.update()
        {
            log::error!("Error updating debugger window: {err}");
        }

        Ok(None)
    }

    fn handle_window_event(
        &mut self,
        win_event: WindowEvent,
        window_id: u32,
    ) -> Option<NativeTickEffect> {
        match win_event {
            WindowEvent::CloseRequested => {
                if window_id == self.renderer.window_id() {
                    return Some(NativeTickEffect::PowerOff);
                }

                if self.hotkey_state.is_debugger_window_id(window_id) {
                    self.window_state.debugger_focused = false;
                    self.hotkey_state.debugger_window = None;
                    let _ = self.runner.send_command(RunnerCommand::StopDebugger);
                }
            }
            WindowEvent::FocusGained => {
                if window_id == self.renderer.window_id() {
                    self.window_state.emulator_focused = true;
                } else if self.hotkey_state.is_debugger_window_id(window_id) {
                    self.window_state.debugger_focused = true;
                }
            }
            WindowEvent::FocusLost => {
                if window_id == self.renderer.window_id() {
                    self.window_state.emulator_focused = false;
                } else if self.hotkey_state.is_debugger_window_id(window_id) {
                    self.window_state.debugger_focused = false;
                }
            }
            WindowEvent::Resized(..)
            | WindowEvent::PixelSizeChanged(..)
            | WindowEvent::Maximized
                if window_id == self.renderer.window_id() =>
            {
                let window_size = sdl_window_size(self.renderer.window());
                self.renderer.handle_resize(window_size);
            }
            _ => {}
        }

        None
    }

    fn process_input_events(&mut self) -> NativeEmulatorResult<Option<NativeTickEffect>> {
        let input_events = self.input_mapper.input_events();
        for event in input_events.borrow_mut().drain(..) {
            match event {
                InputEvent::Button { button, player, pressed } => {
                    self.inputs.set_field(button, player, pressed);
                    if let Some(modal) = self.inputs.modal_for_input(button, player, pressed) {
                        self.renderer.add_or_update_modal(modal.id, modal.text, MODAL_DURATION);
                    }
                }
                InputEvent::MouseMotion { x, y, frame_size, display_area } => {
                    self.inputs.handle_mouse_motion(x, y, frame_size, display_area);
                }
                InputEvent::MouseLeave => {
                    self.inputs.handle_mouse_leave();
                }
                InputEvent::Hotkey { hotkey, pressed } => {
                    let effect = self.handle_hotkey_event(hotkey, pressed)?;
                    match effect {
                        Some(HotkeyEffect::PowerOff) => {
                            return Ok(Some(NativeTickEffect::PowerOff));
                        }
                        Some(HotkeyEffect::Exit) => {
                            return Ok(Some(NativeTickEffect::Exit));
                        }
                        None => {}
                    }
                }
            }
        }

        Ok(None)
    }

    fn handle_runner_cmd_response(&mut self, response: RunnerCommandResponse) {
        match response {
            RunnerCommandResponse::SaveStateSucceeded { slot } => {
                self.renderer.add_modal(format!("Saved state to slot {slot}"), MODAL_DURATION);
                self.hotkey_state.save_state_slot = slot;
            }
            RunnerCommandResponse::LoadStateSucceeded { slot } => {
                // Force renderer to clear any interframe state
                self.renderer.reload();

                self.renderer.add_modal(format!("Loaded state from slot {slot}"), MODAL_DURATION);
                self.hotkey_state.save_state_slot = slot;
            }
            RunnerCommandResponse::SaveStateFailed { slot, err } => {
                self.renderer
                    .add_modal(format!("Failed to save state to slot {slot}"), MODAL_DURATION);
                log::error!("Failed to save state to slot {slot}: {err}");
            }
            RunnerCommandResponse::LoadStateFailed { slot, err } => {
                self.renderer
                    .add_modal(format!("Failed to load state from slot {slot}"), MODAL_DURATION);
                log::error!("Failed to load state from slot {slot}: {err}");
            }
            RunnerCommandResponse::ChangeDiscSucceeded { mut window_title } => {
                window_title.retain(|c| (c as u8) != 0);

                // SAFETY: This is not reassigning the window
                unsafe {
                    self.renderer
                        .window_mut()
                        .set_title(&window_title)
                        .expect("Window title does not have any null characters");
                }
            }
            RunnerCommandResponse::ChangeDiscFailed(err) => {
                log::error!("Failed to change disc: {err}");
            }
        }
    }

    /// # Errors
    ///
    /// This method will return an error if unable to send the command to the emulator runner thread.
    pub fn soft_reset(&mut self) -> NativeEmulatorResult<()> {
        self.runner.send_command(RunnerCommand::SoftReset)
    }

    /// # Errors
    ///
    /// This method will return an error if unable to send the command to the emulator runner thread.
    pub fn hard_reset(&mut self) -> NativeEmulatorResult<()> {
        self.runner.send_command(RunnerCommand::HardReset)
    }

    /// # Errors
    ///
    /// This method will return an error if unable to send the command to the emulator runner thread.
    pub fn open_memory_viewer(&mut self) -> NativeEmulatorResult<()> {
        if self.hotkey_state.debugger_window.is_some() {
            return Ok(());
        }

        let (runner_process, main_process) = (self.hotkey_state.debug_fn)();

        let debugger_window = match DebuggerWindow::new(
            &self.video,
            self.hotkey_state.window_scale_factor,
            self.hotkey_state.egui_theme,
            self.renderer.config(),
            main_process,
        ) {
            Ok(window) => window,
            Err(err) => {
                log::error!("Error creating debugger window: {err}");
                return Ok(());
            }
        };

        self.hotkey_state.debugger_window = Some(debugger_window);
        self.runner.send_command(RunnerCommand::StartDebugger(runner_process))
    }

    /// # Errors
    ///
    /// Returns an error if the state cannot be saved (e.g. due to I/O error).
    pub fn save_state(&mut self, slot: usize) -> NativeEmulatorResult<()> {
        self.runner.send_command(RunnerCommand::SaveState { slot })
    }

    /// # Errors
    ///
    /// Return an error if the state cannot be loaded (e.g. due to I/O error or because the save
    /// state does not exist).
    pub fn load_state(&mut self, slot: usize) -> NativeEmulatorResult<()> {
        self.runner.send_command(RunnerCommand::LoadState { slot })
    }

    /// Try to load the most recent save state.
    ///
    /// If there are no save states or the most recent save state is invalid, this method will log
    /// an error and not modify any emulator state.
    #[allow(clippy::missing_panics_doc)]
    pub fn try_load_most_recent_state(&mut self) {
        let max_time = self
            .runner
            .save_state_metadata()
            .lock()
            .unwrap()
            .times_nanos
            .into_iter()
            .enumerate()
            .filter_map(|(i, option)| option.map(|time| (i, time)))
            .max_by_key(|&(_, time)| time);

        let Some((slot, _)) = max_time else {
            log::error!("No save states found; not loading a save state at launch");
            return;
        };

        if let Err(err) = self.load_state(slot) {
            log::error!("Error loading save state slot {slot} at launch: {err}");
        }
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn save_state_metadata(&self) -> SaveStateMetadata {
        self.runner.save_state_metadata().lock().unwrap().clone()
    }

    pub fn update_gui_focused(&mut self, gui_focused: bool) {
        if gui_focused != self.window_state.gui_focused {
            log::debug!("GUI window focus changed to: {gui_focused}");
        }

        self.window_state.gui_focused = gui_focused;
    }

    fn handle_hotkey_event(
        &mut self,
        hotkey: Hotkey,
        pressed: bool,
    ) -> NativeEmulatorResult<Option<HotkeyEffect>> {
        if pressed {
            let effect = self.handle_hotkey_pressed(hotkey)?;
            return Ok(effect);
        }
        // else, hotkey released

        match hotkey {
            Hotkey::FastForward => {
                self.renderer.set_speed_multiplier(1);
                self.runner.send_command(RunnerCommand::FastForward(false))?;
            }
            Hotkey::Rewind => {
                self.runner.send_command(RunnerCommand::Rewind(false))?;
                self.hotkey_state.rewinding = false;
            }
            _ => {}
        }

        Ok(None)
    }

    fn handle_hotkey_pressed(
        &mut self,
        hotkey: Hotkey,
    ) -> NativeEmulatorResult<Option<HotkeyEffect>> {
        let hotkey = hotkey.to_compact();
        match hotkey {
            CompactHotkey::PowerOff => return Ok(Some(HotkeyEffect::PowerOff)),
            CompactHotkey::Exit => return Ok(Some(HotkeyEffect::Exit)),
            CompactHotkey::ToggleFullscreen => self.toggle_fullscreen()?,
            CompactHotkey::SaveState => self.save_state(self.hotkey_state.save_state_slot)?,
            CompactHotkey::SaveStateSlot(slot) => self.save_state(slot)?,
            CompactHotkey::LoadState => self.hotkey_load_state(None),
            CompactHotkey::LoadStateSlot(slot) => self.hotkey_load_state(Some(slot)),
            CompactHotkey::SoftReset => self.soft_reset()?,
            CompactHotkey::HardReset => self.hard_reset()?,
            CompactHotkey::NextSaveStateSlot => self.next_save_state_slot(),
            CompactHotkey::PrevSaveStateSlot => self.prev_save_state_slot(),
            CompactHotkey::Pause => {
                self.hotkey_state.paused = !self.hotkey_state.paused;
            }
            CompactHotkey::StepFrame => {
                self.runner.send_command(RunnerCommand::StepFrame)?;
            }
            CompactHotkey::FastForward => self.enable_fast_forward()?,
            CompactHotkey::Rewind => {
                self.runner.send_command(RunnerCommand::Rewind(true))?;
                self.hotkey_state.rewinding = true;
            }
            CompactHotkey::ToggleOverclocking => self.toggle_overclocking()?,
            CompactHotkey::OpenDebugger => self.open_memory_viewer()?,
        }

        Ok(None)
    }

    fn toggle_fullscreen(&mut self) -> NativeEmulatorResult<()> {
        let fullscreen =
            self.renderer.toggle_fullscreen().map_err(NativeEmulatorError::SdlSetFullscreen)?;
        self.sdl.mouse().show_cursor(!self.hotkey_state.hide_mouse_cursor.should_hide(fullscreen));

        Ok(())
    }

    fn hotkey_load_state(&mut self, slot: Option<usize>) {
        let slot = slot.unwrap_or(self.hotkey_state.save_state_slot);

        if let Err(err) = self.load_state(slot) {
            log::error!("Error loading save state from slot {slot}: {err}",);
        }
    }

    fn next_save_state_slot(&mut self) {
        self.hotkey_state.save_state_slot =
            (self.hotkey_state.save_state_slot + 1) % SAVE_STATE_SLOTS;
        self.render_selected_slot_modal();
    }

    fn prev_save_state_slot(&mut self) {
        self.hotkey_state.save_state_slot = if self.hotkey_state.save_state_slot == 0 {
            SAVE_STATE_SLOTS - 1
        } else {
            self.hotkey_state.save_state_slot - 1
        };
        self.render_selected_slot_modal();
    }

    fn render_selected_slot_modal(&mut self) {
        self.renderer.add_or_update_modal(
            Some("selected_state_slot".into()),
            format!("Selected save state slot {}", self.hotkey_state.save_state_slot),
            MODAL_DURATION,
        );
    }

    fn enable_fast_forward(&mut self) -> NativeEmulatorResult<()> {
        self.renderer.set_speed_multiplier(self.hotkey_state.fast_forward_multiplier);
        self.runner.send_command(RunnerCommand::FastForward(true))
    }

    fn toggle_overclocking(&mut self) -> NativeEmulatorResult<()> {
        self.hotkey_state.overclocking_enabled = !self.hotkey_state.overclocking_enabled;
        self.update_and_reload_config(&self.raw_config.clone())?;

        let modal_text = if self.hotkey_state.overclocking_enabled {
            "Overclocking settings enabled"
        } else {
            "Overclocking settings disabled"
        };
        self.renderer.add_or_update_modal(
            Some("overclocking_settings".into()),
            modal_text.into(),
            MODAL_DURATION,
        );

        Ok(())
    }

    fn update_and_reload_config(
        &mut self,
        emulator_config: &Emulator::Config,
    ) -> NativeEmulatorResult<()> {
        self.raw_config = emulator_config.clone();
        self.config = if self.hotkey_state.overclocking_enabled {
            self.raw_config.clone()
        } else {
            self.raw_config.with_overclocking_disabled()
        };

        self.runner.send_command(RunnerCommand::ReloadConfig(Box::new((
            self.common_config.clone(),
            self.config.clone(),
        ))))
    }
}

impl<Emulator: EmulatorTrait> Drop for NativeEmulator<Emulator> {
    fn drop(&mut self) {
        let _ = self.runner.send_command(RunnerCommand::Terminate);
    }
}

fn file_name_no_ext<P: AsRef<Path>>(path: P) -> NativeEmulatorResult<String> {
    path.as_ref()
        .with_extension("")
        .file_name()
        .map(|file_name| file_name.to_string_lossy().into_owned())
        .ok_or_else(|| NativeEmulatorError::ParseFileName(path.as_ref().display().to_string()))
}

fn init_sdl3(
    config: &CommonConfig,
) -> NativeEmulatorResult<(Sdl, VideoSubsystem, AudioSubsystem, JoystickSubsystem, EventPump)> {
    let sdl = sdl3::init().map_err(NativeEmulatorError::SdlInit)?;
    let video = sdl.video().map_err(NativeEmulatorError::SdlVideoInit)?;
    let audio = sdl.audio().map_err(NativeEmulatorError::SdlAudioInit)?;
    let joystick = sdl.joystick().map_err(NativeEmulatorError::SdlJoystickInit)?;
    let event_pump = sdl.event_pump().map_err(NativeEmulatorError::SdlEventPumpInit)?;

    // Allow gamepad inputs while window does not have focus
    // https://wiki.libsdl.org/SDL3/SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS
    sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");

    sdl.mouse().show_cursor(!config.hide_mouse_cursor.should_hide(config.launch_in_fullscreen));

    Ok((sdl, video, audio, joystick, event_pump))
}

fn create_window(
    video: &VideoSubsystem,
    title: &str,
    width: u32,
    height: u32,
    fullscreen: bool,
) -> NativeEmulatorResult<Window> {
    let mut window_builder = video.window(title, width, height);
    window_builder.metal_view();
    window_builder.resizable();
    window_builder.position_centered();

    if fullscreen {
        window_builder.fullscreen();
    }

    let window = window_builder.build()?;
    Ok(window)
}

fn sdl_window_size(window: &Window) -> renderer::WindowSize {
    let (width, height) = window.size();
    renderer::WindowSize { width, height }
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
