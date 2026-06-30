use crate::config::{CommonConfig, GameBoyAdvanceConfig};
use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use crate::mainloop::{CreatedEmulator, NativeDebugFn, create, file_name_no_ext};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, extensions};
use gba_config::{GbaInputs, SolarSensorState};
use gba_core::api::GameBoyAdvanceEmulator;
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use std::fs;

pub type NativeGbaEmulator = NativeEmulator<GameBoyAdvanceEmulator>;

impl CreatableEmulator for GameBoyAdvanceEmulator {
    type NativeConfig = GameBoyAdvanceConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        create::read_rom_file(&config.common.rom_file_path, extensions::GAME_BOY_ADVANCE)
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let Some(bios_path) = &config.bios_path else {
            return Err(NativeEmulatorError::GbaNoBios);
        };

        let bios_rom = fs::read(bios_path).map_err(NativeEmulatorError::GbaBiosLoad)?;

        let emulator = GameBoyAdvanceEmulator::create(
            input.input,
            bios_rom,
            config.emulator_config,
            save_writer,
        )?;

        let rom_title = file_name_no_ext(&input.rom_path)?;
        let window_title = format!("gba - {rom_title}");

        let default_window_size = WindowSize::new_gba(config.common.initial_window_size);

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    }

    fn common_config(config: &Self::NativeConfig) -> &CommonConfig {
        &config.common
    }

    fn emulator_config(config: &Self::NativeConfig) -> &Self::Config {
        &config.emulator_config
    }

    fn reload_native_config(
        emulator: &mut NativeEmulator<Self>,
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<()> {
        emulator.inputs.solar = SolarSensorState {
            brightness: emulator.inputs.solar.brightness,
            ..new_solar_state(config)
        };

        Ok(())
    }

    fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
        GbaInputs { solar: new_solar_state(config), ..GbaInputs::default() }
    }

    fn input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.inputs.to_mapping_vec()
    }

    fn turbo_input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.inputs.to_turbo_mapping_vec()
    }

    fn debug_fn() -> Option<NativeDebugFn<Self>> {
        Some(|| {
            jgenesis_debugger_frontend::partial_clone_debug_fn(
                jgenesis_debugger_frontend::gba::render_fn(),
            )
        })
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
