use crate::config::SnesConfig;

use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{debug, save};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, extensions};

use crate::config::RomReadResult;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::SnesControllerType;
use snes_config::SnesJoypadState;
use snes_core::api::SnesEmulator;
use snes_core::input::{SnesInputDevice, SnesInputs, SuperScopeState};
use std::path::Path;

trait SnesControllerTypeExt {
    fn to_input_device(self) -> SnesInputDevice;
}

impl SnesControllerTypeExt for SnesControllerType {
    fn to_input_device(self) -> SnesInputDevice {
        match self {
            Self::Gamepad => SnesInputDevice::Controller(SnesJoypadState::default()),
            Self::SuperScope => SnesInputDevice::SuperScope(SuperScopeState::default()),
        }
    }
}

pub type NativeSnesEmulator = NativeEmulator<SnesEmulator>;

impl NativeSnesEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_snes_config(&mut self, config: Box<SnesConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        self.update_emulator_config(&config.emulator_config);

        self.input_mapper.update_mappings(
            config.common.axis_deadzone,
            &config.inputs.to_mapping_vec(),
            &config.common.hotkey_config.to_mapping_vec(),
        );
        self.input_mapper.inputs_mut().p2 = config.inputs.p2_type.to_input_device();

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
    let RomReadResult { rom, extension } = config.common.read_rom_file(extensions::SNES)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.emulator_config;
    let coprocessor_roms = config.to_coprocessor_roms();
    let mut emulator =
        SnesEmulator::create(rom, emulator_config, coprocessor_roms, &mut save_writer)?;

    let cartridge_title = emulator.cartridge_title();
    let window_title = format!("snes - {cartridge_title}");

    let initial_inputs =
        SnesInputs { p1: SnesJoypadState::default(), p2: config.inputs.p2_type.to_input_device() };

    let default_window_size =
        WindowSize::new_snes(config.common.initial_window_size, emulator_config.aspect_ratio);

    NativeSnesEmulator::new(
        emulator,
        emulator_config,
        config.common,
        extension,
        default_window_size,
        &window_title,
        save_writer,
        save_state_path,
        &config.inputs.to_mapping_vec(),
        initial_inputs,
        debug::snes::render_fn,
    )
}
