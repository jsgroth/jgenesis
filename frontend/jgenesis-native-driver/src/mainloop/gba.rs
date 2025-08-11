use crate::config::{GameBoyAdvanceConfig, RomReadResult};
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{file_name_no_ext, save};
use crate::{AudioError, NativeEmulator, NativeEmulatorError, NativeEmulatorResult, extensions};
use gba_config::GbaInputs;
use gba_core::api::GameBoyAdvanceEmulator;
use jgenesis_native_config::common::WindowSize;
use std::fs;
use std::path::Path;

pub type NativeGbaEmulator = NativeEmulator<GameBoyAdvanceEmulator>;

impl NativeGbaEmulator {
    /// # Errors
    ///
    /// Propagates any errors encountered while reloading audio config.
    pub fn reload_gba_config(
        &mut self,
        config: Box<GameBoyAdvanceConfig>,
    ) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        self.update_emulator_config(&config.emulator_config);

        self.input_mapper.update_mappings(
            config.common.axis_deadzone,
            &config.inputs.to_mapping_vec(),
            &config.common.hotkey_config.to_mapping_vec(),
        );

        Ok(())
    }
}

/// # Errors
///
/// Propagates any errors encountered while initializing the emulator.
pub fn create_gba(config: Box<GameBoyAdvanceConfig>) -> NativeEmulatorResult<NativeGbaEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let RomReadResult { rom, extension } =
        config.common.read_rom_file(extensions::GAME_BOY_ADVANCE)?;

    let Some(bios_path) = &config.bios_path else {
        return Err(NativeEmulatorError::GbaNoBios);
    };

    let bios_rom = fs::read(bios_path).map_err(NativeEmulatorError::GbaBiosLoad)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.emulator_config;
    let emulator =
        GameBoyAdvanceEmulator::create(rom, bios_rom, emulator_config, &mut save_writer)?;

    let rom_title = file_name_no_ext(rom_path)?;
    let window_title = format!("gba - {rom_title}");

    let default_window_size = WindowSize::new_gba(config.common.initial_window_size);

    NativeGbaEmulator::new(
        emulator,
        emulator_config,
        config.common,
        extension,
        default_window_size,
        &window_title,
        save_writer,
        save_state_path,
        &config.inputs.to_mapping_vec(),
        GbaInputs::default(),
        || Box::new(|_ctx| {}),
    )
}
