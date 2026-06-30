use crate::config::{CommonConfig, NesConfig};

use crate::mainloop::{CreatedEmulator, NativeDebugFn, create, file_name_no_ext};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};

use nes_core::api::NesEmulator;
use nes_core::input::{NesInputDevice, NesInputs, ZapperState};

use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::{ButtonMappingVec, NesControllerType};
use nes_config::NesJoypadState;

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

impl CreatableEmulator for NesEmulator {
    type NativeConfig = NesConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        create::read_rom_file(&config.common.rom_file_path, extensions::NES)
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let emulator = NesEmulator::create(input.input, config.emulator_config, save_writer)?;

        let rom_title = file_name_no_ext(&input.rom_path)?;
        let window_title = format!("nes - {rom_title}");

        let default_window_size = WindowSize::new_nes(
            config.common.initial_window_size,
            config.emulator_config.aspect_ratio,
            emulator.timing_mode(),
            config.emulator_config.ntsc_crop_vertical_overscan,
        );

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
        emulator.inputs.p2 = config.inputs.p2_type.to_input_device();

        Ok(())
    }

    fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
        NesInputs { p1: NesJoypadState::default(), p2: config.inputs.p2_type.to_input_device() }
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
                jgenesis_debugger_frontend::nes::render_fn(),
            )
        })
    }
}
