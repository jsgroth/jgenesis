use crate::config::SnesConfig;

use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{CreatedEmulator, NativeEmulatorArgs, debug, save};
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
            &config.inputs.to_turbo_mapping_vec(),
            &config.common.hotkey_config.to_mapping_vec(),
        );
        self.inputs.p2 = config.inputs.p2_type.to_input_device();

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

    let emulator_config = config.emulator_config;
    let initial_window_size = config.common.initial_window_size;
    let coprocessor_roms = config.to_coprocessor_roms();

    let create_emulator_fn = move |save_writer: &mut FsSaveWriter| {
        let mut emulator =
            SnesEmulator::create(rom, emulator_config, coprocessor_roms, save_writer)?;

        let cartridge_title = emulator.cartridge_title();
        let window_title = format!("snes - {cartridge_title}");

        let default_window_size =
            WindowSize::new_snes(initial_window_size, emulator_config.aspect_ratio);

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    let initial_inputs =
        SnesInputs { p1: SnesJoypadState::default(), p2: config.inputs.p2_type.to_input_device() };

    NativeSnesEmulator::new(
        NativeEmulatorArgs::new(
            Box::new(create_emulator_fn),
            emulator_config,
            config.common,
            extension,
            save_path,
            save_state_path,
            config.inputs.to_mapping_vec(),
        )
        .with_turbo_mappings(config.inputs.to_turbo_mapping_vec())
        .with_initial_inputs(initial_inputs)
        .with_debug_render_fn(debug::snes::render_fn),
    )
}
