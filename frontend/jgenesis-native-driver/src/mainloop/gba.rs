use crate::config::{GameBoyAdvanceConfig, RomReadResult};
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{CreatedEmulator, NativeEmulatorArgs, debug, file_name_no_ext, save};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, extensions};
use gba_config::{GbaInputs, SolarSensorState};
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
    ) -> NativeEmulatorResult<()> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        self.update_and_reload_config(&config.emulator_config)?;

        self.input_mapper.update_mappings(
            config.common.axis_deadzone,
            &config.inputs.to_mapping_vec(),
            &config.inputs.to_turbo_mapping_vec(),
            &config.common.hotkey_config.to_mapping_vec(),
        );

        self.inputs.solar = SolarSensorState {
            brightness: self.inputs.solar.brightness,
            ..new_solar_state(&config)
        };

        Ok(())
    }
}

fn new_solar_state(config: &GameBoyAdvanceConfig) -> SolarSensorState {
    SolarSensorState {
        brightness: config.solar_min_brightness,
        brightness_step: config.solar_brightness_step,
        min_brightness: config.solar_min_brightness,
        max_brightness: config.solar_max_brightness,
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

    let emulator_config = config.emulator_config;
    let initial_window_size = config.common.initial_window_size;
    let rom_path = rom_path.to_owned();

    let create_emulator_fn = move |save_writer: &mut FsSaveWriter| {
        let emulator = GameBoyAdvanceEmulator::create(rom, bios_rom, emulator_config, save_writer)?;

        let rom_title = file_name_no_ext(rom_path)?;
        let window_title = format!("gba - {rom_title}");

        let default_window_size = WindowSize::new_gba(initial_window_size);

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    let initial_inputs = GbaInputs { solar: new_solar_state(&config), ..GbaInputs::default() };

    NativeGbaEmulator::new(
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
        .with_debug_render_fn(debug::gba::render_fn),
    )
}
