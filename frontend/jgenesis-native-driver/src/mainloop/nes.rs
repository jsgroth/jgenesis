use crate::config::NesConfig;

use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{debug, file_name_no_ext, save};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, config};
use jgenesis_common::frontend::EmulatorTrait;

use nes_core::api::NesEmulator;
use nes_core::input::{NesInputDevice, NesInputs, NesJoypadState, ZapperState};

use crate::config::RomReadResult;
use crate::config::input::NesControllerType;
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

pub const SUPPORTED_EXTENSIONS: &[&str] = &["nes"];

impl NativeNesEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_nes_config(&mut self, config: Box<NesConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        self.emulator.reload_config(&config.emulator_config);
        self.config = config.emulator_config;

        // Config change could have changed target framerate (50/60 Hz hack)
        self.renderer.set_target_fps(self.emulator.target_fps());

        self.input_mapper.update_mappings(
            config.common.axis_deadzone,
            &config.inputs.to_mapping_vec(),
            &config.common.hotkey_config.to_mapping_vec(),
        );
        self.input_mapper.inputs_mut().p2 = config.inputs.p2_type.to_input_device();

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
    let RomReadResult { rom, extension } = config.common.read_rom_file(SUPPORTED_EXTENSIONS)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.emulator_config;
    let emulator = NesEmulator::create(rom, emulator_config, &mut save_writer)?;

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window_title = format!("nes - {rom_title}");

    let initial_inputs =
        NesInputs { p1: NesJoypadState::default(), p2: config.inputs.p2_type.to_input_device() };

    NativeNesEmulator::new(
        emulator,
        emulator_config,
        config.common,
        extension,
        config::DEFAULT_GENESIS_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        &config.inputs.to_mapping_vec(),
        initial_inputs,
        debug::nes::render_fn,
    )
}
