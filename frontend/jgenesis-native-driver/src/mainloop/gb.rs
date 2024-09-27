use crate::config::GameBoyConfig;
use crate::config::RomReadResult;
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{basic_input_mapper_fn, debug, file_name_no_ext};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, config};
use gb_core::api::{GameBoyEmulator, GameBoyEmulatorConfig};
use gb_core::inputs::{GameBoyButton, GameBoyInputs};
use jgenesis_common::frontend::EmulatorTrait;
use std::path::Path;

pub type NativeGameBoyEmulator =
    NativeEmulator<GameBoyInputs, GameBoyButton, GameBoyEmulatorConfig, GameBoyEmulator>;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["gb", "gbc"];

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
    let RomReadResult { rom, .. } = config.common.read_rom_file(SUPPORTED_EXTENSIONS)?;

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator = GameBoyEmulator::create(rom, emulator_config, &mut save_writer)?;

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window_title = format!("gb - {rom_title}");

    NativeGameBoyEmulator::new(
        emulator,
        emulator_config,
        config.common,
        config::DEFAULT_GB_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        basic_input_mapper_fn(&GameBoyButton::ALL),
        debug::gb::render_fn,
    )
}
