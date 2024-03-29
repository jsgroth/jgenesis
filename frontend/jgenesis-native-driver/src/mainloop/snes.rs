use crate::config::{SnesConfig, WindowSize};
use crate::input::{HotkeyMapper, InputMapper};
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{create_window, debug, init_sdl, HotkeyState, NativeEmulatorError};
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use jgenesis_common::frontend::EmulatorTrait;
use jgenesis_renderer::renderer::WgpuRenderer;
use sdl2::video::Window;
use snes_core::api::{SnesEmulator, SnesEmulatorConfig};
use snes_core::input::{SnesButton, SnesInputs};
use std::fs;
use std::path::Path;

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

        if let Err(err) = self.input_mapper.reload_config_snes(
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
