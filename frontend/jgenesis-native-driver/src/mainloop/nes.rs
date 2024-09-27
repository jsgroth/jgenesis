use crate::config::{CommonConfig, NesConfig};

use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{debug, file_name_no_ext};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, config};
use jgenesis_common::frontend::EmulatorTrait;

use nes_core::api::{NesEmulator, NesEmulatorConfig};
use nes_core::input::{NesButton, NesInputs};

use crate::config::RomReadResult;
use crate::input::InputMapper;
use std::path::Path;

pub type NativeNesEmulator = NativeEmulator<NesInputs, NesButton, NesEmulatorConfig, NesEmulator>;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["nes"];

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

        if let Err(err) = self.input_mapper.reload_config_nes(
            config.p2_controller_type,
            &config.common.keyboard_inputs,
            &config.common.joystick_inputs,
            &config.zapper_config,
            config.common.axis_deadzone,
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
    let RomReadResult { rom, .. } = config.common.read_rom_file(SUPPORTED_EXTENSIONS)?;

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator = NesEmulator::create(rom, emulator_config, &mut save_writer)?;

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window_title = format!("nes - {rom_title}");

    let input_mapper_fn = |joystick_subsystem, common_config: &CommonConfig<_, _>| {
        InputMapper::new_nes(
            joystick_subsystem,
            config.p2_controller_type,
            &common_config.keyboard_inputs,
            &common_config.joystick_inputs,
            &config.zapper_config,
            common_config.axis_deadzone,
        )
    };

    NativeNesEmulator::new(
        emulator,
        emulator_config,
        config.common,
        config::DEFAULT_GENESIS_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        input_mapper_fn,
        debug::nes::render_fn,
    )
}
