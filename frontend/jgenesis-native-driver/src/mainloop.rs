mod audio;
mod debug;
mod gb;
mod gba;
mod genesis;
mod nes;
mod rewind;
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
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::debug::{DebugRenderFn, DebuggerWindow};
use crate::mainloop::rewind::Rewinder;
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::state::SaveStatePaths;
pub use audio::AudioError;
use bincode::error::{DecodeError, EncodeError};
use gb_core::api::GameBoyLoadError;
use gba_core::api::GbaLoadError;
use jgenesis_common::frontend::{EmulatorConfigTrait, EmulatorTrait, MappableInputs, TickEffect};
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
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use std::{io, thread};
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
    base_save_state_path: PathBuf,
    save_state_paths: SaveStatePaths,
    save_state_slot: usize,
    save_state_metadata: SaveStateMetadata,
    paused: bool,
    should_step_frame: bool,
    fast_forward_multiplier: u64,
    rewinder: Rewinder<Emulator>,
    overclocking_enabled: bool,
    debugger_window: Option<DebuggerWindow<Emulator>>,
    window_scale_factor: Option<f32>,
    debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
}

impl<Emulator: EmulatorTrait> HotkeyState<Emulator> {
    fn new(
        common_config: &CommonConfig,
        save_state_path: PathBuf,
        debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
    ) -> NativeEmulatorResult<Self> {
        let save_state_paths = state::init_paths(&save_state_path)?;
        let save_state_metadata =
            SaveStateMetadata::load(&save_state_paths, Emulator::save_state_version());

        log::debug!("Save state paths: {save_state_paths:?}");

        Ok(Self {
            hide_mouse_cursor: common_config.hide_mouse_cursor,
            base_save_state_path: save_state_path,
            save_state_paths,
            save_state_slot: 0,
            save_state_metadata,
            paused: false,
            should_step_frame: false,
            fast_forward_multiplier: common_config.fast_forward_multiplier,
            rewinder: Rewinder::new(Duration::from_secs(
                common_config.rewind_buffer_length_seconds,
            )),
            overclocking_enabled: true,
            debugger_window: None,
            window_scale_factor: common_config.window_scale_factor,
            debug_render_fn,
        })
    }

    fn update_save_state_path(&mut self, save_state_path: PathBuf) -> NativeEmulatorResult<()> {
        if save_state_path == self.base_save_state_path {
            return Ok(());
        }

        self.save_state_paths = state::init_paths(&save_state_path)?;
        self.save_state_metadata =
            SaveStateMetadata::load(&self.save_state_paths, Emulator::save_state_version());
        self.base_save_state_path = save_state_path;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTickEffect {
    PowerOff,
    Exit,
}

pub struct NativeEmulator<Emulator: EmulatorTrait> {
    emulator: Emulator,
    // Config sent from the frontend
    raw_config: Emulator::Config,
    // Config with overclocking maybe forcibly disabled due to hotkey state
    config: Emulator::Config,
    common_config: CommonConfig,
    renderer: WgpuRenderer<Window>,
    audio_output: SdlAudioOutput,
    input_mapper: InputMapper<Emulator::Button>,
    inputs: Emulator::Inputs,
    save_writer: FsSaveWriter,
    sdl: Sdl,
    event_pump: EventPump,
    event_buffer: Rc<RefCell<Vec<Event>>>,
    video: VideoSubsystem,
    hotkey_state: HotkeyState<Emulator>,
    fps_tracker: FpsTracker,
    rom_path: PathBuf,
    rom_extension: String,
}

impl<Emulator: EmulatorTrait> NativeEmulator<Emulator> {
    fn reload_common_config(&mut self, config: &CommonConfig) -> Result<(), AudioError> {
        self.renderer.reload_config(config.renderer_config);

        self.audio_output.reload_config(config)?;
        self.emulator.update_audio_output_frequency(self.audio_output.output_frequency());

        self.hotkey_state.hide_mouse_cursor = config.hide_mouse_cursor;

        self.hotkey_state.fast_forward_multiplier = config.fast_forward_multiplier;
        // Reset speed multiplier in case the fast forward hotkey changed
        self.renderer.set_speed_multiplier(1);
        self.audio_output.set_speed_multiplier(1);

        if let Err(err) = self.update_save_paths(config) {
            log::error!("Error updating save paths: {err}");
        }

        self.hotkey_state
            .rewinder
            .set_buffer_duration(Duration::from_secs(config.rewind_buffer_length_seconds));

        let fullscreen = self.renderer.is_fullscreen();
        self.sdl.mouse().show_cursor(!config.hide_mouse_cursor.should_hide(fullscreen));

        Ok(())
    }

    fn update_save_paths(&mut self, config: &CommonConfig) -> NativeEmulatorResult<()> {
        let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
            &config.save_path,
            &config.state_path,
            &self.rom_path,
            &self.rom_extension,
        )?;

        self.save_writer.update_path(save_path);
        self.hotkey_state.update_save_state_path(save_state_path)?;

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

fn open_debugger_window<Emulator>(
    video: &VideoSubsystem,
    scale_factor: Option<f32>,
    debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
) -> Option<DebuggerWindow<Emulator>> {
    let render_fn = debug_render_fn();
    match DebuggerWindow::new(video, scale_factor, render_fn) {
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
    #[error("Error in emulation core: {0}")]
    Emulator(#[source] Box<dyn Error + Send + Sync + 'static>),
}

pub type NativeEmulatorResult<T> = Result<T, NativeEmulatorError>;

impl<Emulator> NativeEmulator<Emulator>
where
    Emulator: EmulatorTrait,
{
    #[allow(clippy::too_many_arguments)]
    fn new(
        mut emulator: Emulator,
        emulator_config: Emulator::Config,
        common_config: CommonConfig,
        rom_extension: String,
        default_window_size: WindowSize,
        window_title: &str,
        save_writer: FsSaveWriter,
        save_state_path: PathBuf,
        button_mappings: &ButtonMappingVec<'_, Emulator::Button>,
        initial_inputs: Emulator::Inputs,
        debug_render_fn: fn() -> Box<DebugRenderFn<Emulator>>,
    ) -> NativeEmulatorResult<Self> {
        let (sdl, video, audio, joystick, event_pump) = init_sdl3(&common_config)?;

        let mut initial_window_size = common_config.window_size.unwrap_or(default_window_size);
        if let Some(scale_factor) = common_config.window_scale_factor {
            initial_window_size = initial_window_size.scale(scale_factor);
        }

        let window = create_window(
            &video,
            window_title,
            initial_window_size.width,
            initial_window_size.height,
            common_config.launch_in_fullscreen,
        )?;

        let window_size = sdl_window_size(&window);
        let mut renderer = pollster::block_on(WgpuRenderer::new(
            window,
            window_size,
            common_config.renderer_config,
        ))?;
        renderer.set_target_fps(emulator.target_fps());

        let audio_output = SdlAudioOutput::create_and_init(audio, &common_config)?;
        emulator.update_audio_output_frequency(audio_output.output_frequency());

        let input_mapper = InputMapper::new(
            joystick,
            common_config.axis_deadzone,
            button_mappings,
            &common_config.hotkey_config.to_mapping_vec(),
        );

        let hotkey_state = HotkeyState::new(&common_config, save_state_path, debug_render_fn)?;

        let mut emulator = Self {
            emulator,
            raw_config: emulator_config.clone(),
            config: emulator_config,
            common_config: common_config.clone(),
            renderer,
            audio_output,
            input_mapper,
            inputs: initial_inputs,
            save_writer,
            sdl,
            event_pump,
            event_buffer: Rc::new(RefCell::new(Vec::with_capacity(100))),
            video,
            hotkey_state,
            fps_tracker: FpsTracker::new(),
            rom_path: common_config.rom_file_path,
            rom_extension,
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
    pub fn render_frame(&mut self) -> NativeEmulatorResult<Option<NativeTickEffect>> {
        let rewinding = self.hotkey_state.rewinder.is_rewinding();
        let should_run_emulator =
            !rewinding && (!self.hotkey_state.paused || self.hotkey_state.should_step_frame);

        if should_run_emulator {
            while self
                .emulator
                .tick(
                    &mut self.renderer,
                    &mut self.audio_output,
                    &self.inputs,
                    &mut self.save_writer,
                )
                .map_err(|err| NativeEmulatorError::Emulator(err.into()))?
                != TickEffect::FrameRendered
            {}

            self.fps_tracker.record_frame();
            self.hotkey_state.rewinder.record_frame(&self.emulator);

            self.audio_output.adjust_dynamic_resampling_ratio();
            self.emulator.update_audio_output_frequency(self.audio_output.output_frequency());

            self.renderer.set_target_fps(self.emulator.target_fps());
        }

        self.hotkey_state.should_step_frame = false;

        if let Some(debugger_window) = &mut self.hotkey_state.debugger_window
            && let Err(err) = debugger_window.update(&mut self.emulator)
        {
            log::error!("Debugger window error: {err}");
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
            );

            if let Some(debugger_window) = &mut self.hotkey_state.debugger_window {
                debugger_window.handle_sdl_event(&event);
            }

            match event {
                Event::Quit { .. } => {
                    self.hotkey_state.debugger_window = None;
                    return Ok(Some(NativeTickEffect::PowerOff));
                }
                Event::Window { win_event, window_id, .. } => {
                    if win_event == WindowEvent::CloseRequested {
                        if window_id == self.renderer.window_id() {
                            self.hotkey_state.debugger_window = None;
                            return Ok(Some(NativeTickEffect::PowerOff));
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

        if let Some(effect) = self.process_input_events()? {
            return Ok(Some(effect));
        }

        if rewinding {
            self.renderer.reset_interframe_state();
            self.hotkey_state.rewinder.tick(
                &mut self.emulator,
                &mut self.renderer,
                &self.config,
            )?;
        }

        if !should_run_emulator {
            // Don't spin loop when the emulator is paused or rewinding
            thread::sleep(Duration::from_millis(1));
        }

        Ok(None)
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
                            self.hotkey_state.debugger_window = None;
                            return Ok(Some(NativeTickEffect::PowerOff));
                        }
                        Some(HotkeyEffect::Exit) => {
                            self.hotkey_state.debugger_window = None;
                            return Ok(Some(NativeTickEffect::Exit));
                        }
                        None => {}
                    }
                }
            }
        }

        Ok(None)
    }

    pub fn soft_reset(&mut self) {
        self.emulator.soft_reset();
    }

    pub fn hard_reset(&mut self) {
        self.emulator.hard_reset(&mut self.save_writer);
    }

    pub fn open_memory_viewer(&mut self) {
        if self.hotkey_state.debugger_window.is_none() {
            self.hotkey_state.debugger_window = open_debugger_window(
                &self.video,
                self.hotkey_state.window_scale_factor,
                self.hotkey_state.debug_render_fn,
            );
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
        self.hotkey_state.save_state_slot = slot;

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

        // Force renderer to clear any interframe state
        self.renderer.reload();

        self.renderer.add_modal(format!("Loaded state from slot {slot}"), MODAL_DURATION);
        self.hotkey_state.save_state_slot = slot;

        Ok(())
    }

    /// Try to load the most recent save state.
    ///
    /// If there are no save states or the most recent save state is invalid, this method will log
    /// an error and not modify any emulator state.
    pub fn try_load_most_recent_state(&mut self) {
        let max_time = self
            .hotkey_state
            .save_state_metadata
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

    pub fn save_state_metadata(&self) -> &SaveStateMetadata {
        &self.hotkey_state.save_state_metadata
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
                self.audio_output.set_speed_multiplier(1);
            }
            Hotkey::Rewind => {
                self.hotkey_state.rewinder.stop_rewinding();
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
            CompactHotkey::SoftReset => self.emulator.soft_reset(),
            CompactHotkey::HardReset => self.emulator.hard_reset(&mut self.save_writer),
            CompactHotkey::NextSaveStateSlot => self.next_save_state_slot(),
            CompactHotkey::PrevSaveStateSlot => self.prev_save_state_slot(),
            CompactHotkey::Pause => {
                self.hotkey_state.paused = !self.hotkey_state.paused;
            }
            CompactHotkey::StepFrame => {
                self.hotkey_state.should_step_frame = true;
            }
            CompactHotkey::FastForward => self.enable_fast_forward(),
            CompactHotkey::Rewind => self.hotkey_state.rewinder.start_rewinding(),
            CompactHotkey::ToggleOverclocking => self.toggle_overclocking(),
            CompactHotkey::OpenDebugger => self.open_memory_viewer(),
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
            log::error!(
                "Error loading save state from slot {slot} in '{}': {err}",
                self.hotkey_state.save_state_paths[slot].display()
            );
        }
    }

    fn next_save_state_slot(&mut self) {
        self.hotkey_state.save_state_slot =
            (self.hotkey_state.save_state_slot + 1) % SAVE_STATE_SLOTS;
        self.renderer.add_modal(
            format!("Selected save state slot {}", self.hotkey_state.save_state_slot),
            MODAL_DURATION,
        );
    }

    fn prev_save_state_slot(&mut self) {
        self.hotkey_state.save_state_slot = if self.hotkey_state.save_state_slot == 0 {
            SAVE_STATE_SLOTS - 1
        } else {
            self.hotkey_state.save_state_slot - 1
        };
        self.renderer.add_modal(
            format!("Selected save state slot {}", self.hotkey_state.save_state_slot),
            MODAL_DURATION,
        );
    }

    fn enable_fast_forward(&mut self) {
        let multiplier = self.hotkey_state.fast_forward_multiplier;
        self.renderer.set_speed_multiplier(multiplier);
        self.audio_output.set_speed_multiplier(multiplier);
    }

    fn toggle_overclocking(&mut self) {
        self.hotkey_state.overclocking_enabled = !self.hotkey_state.overclocking_enabled;
        self.update_emulator_config(&self.raw_config.clone());

        let modal_text = if self.hotkey_state.overclocking_enabled {
            "Overclocking settings enabled"
        } else {
            "Overclocking settings disabled"
        };
        self.renderer.add_modal(modal_text.into(), MODAL_DURATION);
    }

    fn update_emulator_config(&mut self, config: &Emulator::Config) {
        self.raw_config = config.clone();
        self.config = if self.hotkey_state.overclocking_enabled {
            self.raw_config.clone()
        } else {
            self.raw_config.with_overclocking_disabled()
        };

        self.emulator.reload_config(&self.config);
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

fn handle_window_event(win_event: WindowEvent, renderer: &mut WgpuRenderer<Window>) {
    match win_event {
        WindowEvent::Resized(..) | WindowEvent::PixelSizeChanged(..) | WindowEvent::Maximized => {
            let window_size = sdl_window_size(renderer.window());
            renderer.handle_resize(window_size);
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
use genesis_config::GenesisRegion;
