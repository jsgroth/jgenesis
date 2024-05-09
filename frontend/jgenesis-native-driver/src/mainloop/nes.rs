use crate::config::NesConfig;

use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{basic_input_mapper_fn, debug, file_name_no_ext, NativeEmulatorError};
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use jgenesis_common::frontend::EmulatorTrait;

use nes_core::api::{NesEmulator, NesEmulatorConfig};
use nes_core::input::{NesButton, NesInputs};

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

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window_title = format!("nes - {rom_title}");

    NativeNesEmulator::new(
        emulator,
        emulator_config,
        config.common,
        config::DEFAULT_GENESIS_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        basic_input_mapper_fn(&NesButton::ALL),
        debug::nes::render_fn,
    )
}
