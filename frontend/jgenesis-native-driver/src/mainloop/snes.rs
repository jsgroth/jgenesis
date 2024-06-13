use crate::config::{CommonConfig, SnesConfig};
use crate::input::InputMapper;

use crate::mainloop::debug;
use crate::mainloop::save::FsSaveWriter;
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use jgenesis_common::frontend::EmulatorTrait;

use snes_core::api::{SnesEmulator, SnesEmulatorConfig};
use snes_core::input::{SnesButton, SnesInputs};
use std::path::Path;

pub type NativeSnesEmulator =
    NativeEmulator<SnesInputs, SnesButton, SnesEmulatorConfig, SnesEmulator>;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["sfc", "smc"];

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
            &config.common.keyboard_inputs,
            &config.common.joystick_inputs,
            &config.super_scope_config,
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
    let rom = config.common.read_rom_file(SUPPORTED_EXTENSIONS)?;

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let coprocessor_roms = config.to_coprocessor_roms();
    let mut emulator =
        SnesEmulator::create(rom, emulator_config, coprocessor_roms, &mut save_writer)?;

    let cartridge_title = emulator.cartridge_title();
    let window_title = format!("snes - {cartridge_title}");

    let input_mapper_fn = |joystick, common_config: &CommonConfig<_, _>| {
        InputMapper::new_snes(
            joystick,
            config.p2_controller_type,
            &common_config.keyboard_inputs,
            &common_config.joystick_inputs,
            &config.super_scope_config,
            common_config.axis_deadzone,
        )
    };

    NativeSnesEmulator::new(
        emulator,
        emulator_config,
        config.common,
        config::DEFAULT_GENESIS_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        input_mapper_fn,
        debug::snes::render_fn,
    )
}
