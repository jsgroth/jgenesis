use crate::config::NesConfig;

use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{CreatedEmulator, NativeEmulatorArgs, debug, file_name_no_ext, save};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, extensions};

use nes_core::api::NesEmulator;
use nes_core::input::{NesInputDevice, NesInputs, ZapperState};

use crate::config::RomReadResult;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::NesControllerType;
use nes_config::NesJoypadState;
use std::path::Path;

trait NesControllerTypeExt {
    fn to_input_device(self) -> NesInputDevice;
}

impl NesControllerTypeExt for NesControllerType {
    fn to_input_device(self) -> NesInputDevice {
        match self {
            Self::Gamepad => NesInputDevice::Controller(NesJoypadState::default()),
            Self::Zapper => NesInputDevice::Zapper(ZapperState::default()),
        }
    }
}

pub type NativeNesEmulator = NativeEmulator<NesEmulator>;

impl NativeNesEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_nes_config(&mut self, config: Box<NesConfig>) -> Result<(), AudioError> {
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

/// Create an emulator with the NES core with the given config.
///
/// # Errors
///
/// Propagates any errors encountered during initialization.
pub fn create_nes(config: Box<NesConfig>) -> NativeEmulatorResult<NativeNesEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let RomReadResult { rom, extension } = config.common.read_rom_file(extensions::NES)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let emulator_config = config.emulator_config;
    let initial_window_size = config.common.initial_window_size;
    let rom_file_path = config.common.rom_file_path.clone();

    let create_emulator_fn = move |save_writer: &mut FsSaveWriter| {
        let emulator = NesEmulator::create(rom, emulator_config, save_writer)?;

        let rom_title = file_name_no_ext(rom_file_path)?;
        let window_title = format!("nes - {rom_title}");

        let default_window_size = WindowSize::new_nes(
            initial_window_size,
            emulator_config.aspect_ratio,
            emulator.timing_mode(),
            emulator_config.ntsc_crop_vertical_overscan,
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    let initial_inputs =
        NesInputs { p1: NesJoypadState::default(), p2: config.inputs.p2_type.to_input_device() };

    NativeNesEmulator::new(
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
        .with_debug_render_fn(debug::nes::render_fn),
    )
}
