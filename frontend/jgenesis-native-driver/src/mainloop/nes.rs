use crate::config::{NesConfig, WindowSize};
use crate::input::{HotkeyMapper, InputMapper};
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{
    create_window, debug, file_name_no_ext, init_sdl, HotkeyState, NativeEmulatorError,
};
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use jgenesis_common::frontend::EmulatorTrait;
use jgenesis_renderer::renderer::WgpuRenderer;
use nes_core::api::{NesEmulator, NesEmulatorConfig};
use nes_core::input::{NesButton, NesInputs};
use sdl2::video::Window;
use std::fs;
use std::path::Path;

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
            &NesButton::ALL,
        ) {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
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

    let input_mapper = InputMapper::new(
        joystick,
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
        config.common.axis_deadzone,
        &NesButton::ALL,
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
