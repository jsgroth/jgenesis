use crate::config::{GameBoyConfig, WindowSize};
use crate::input::{HotkeyMapper, InputMapper};
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{
    create_window, debug, file_name_no_ext, init_sdl, HotkeyState, NativeEmulatorError,
};
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use gb_core::api::{GameBoyEmulator, GameBoyEmulatorConfig};
use gb_core::inputs::{GameBoyButton, GameBoyInputs};
use jgenesis_common::frontend::EmulatorTrait;
use jgenesis_renderer::renderer::WgpuRenderer;
use sdl2::video::Window;
use std::fs;
use std::path::Path;

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
            &GameBoyButton::ALL,
        ) {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
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

    let input_mapper = InputMapper::new(
        joystick,
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
        config.common.axis_deadzone,
        &GameBoyButton::ALL,
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
