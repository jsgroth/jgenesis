use crate::config::GameBoyConfig;
use crate::config::RomReadResult;
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{debug, file_name_no_ext, save};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, extensions};
use gb_config::GameBoyInputs;
use gb_core::api::GameBoyEmulator;
use jgenesis_native_config::common::WindowSize;
use std::path::Path;

pub type NativeGameBoyEmulator = NativeEmulator<GameBoyEmulator>;

impl NativeGameBoyEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_gb_config(&mut self, config: Box<GameBoyConfig>) -> Result<(), AudioError> {
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

/// Create an emulator with the Game Boy core with the given config.
///
/// # Errors
///
/// This function will return an error if unable to initialize the emulator.
pub fn create_gb(config: Box<GameBoyConfig>) -> NativeEmulatorResult<NativeGameBoyEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let RomReadResult { rom, extension } = config.common.read_rom_file(&extensions::GB_GBC)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.emulator_config;
    let emulator = GameBoyEmulator::create(rom, emulator_config, &mut save_writer)?;

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window_title = format!("gb - {rom_title}");

    let default_window_size = WindowSize::new_gb(config.common.initial_window_size);

    NativeGameBoyEmulator::new(
        emulator,
        emulator_config,
        config.common,
        extension,
        default_window_size,
        &window_title,
        save_writer,
        save_state_path,
        &config.inputs.to_mapping_vec(),
        GameBoyInputs::default(),
        debug::gb::render_fn,
    )
}
