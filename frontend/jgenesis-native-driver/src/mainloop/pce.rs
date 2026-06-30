use crate::config::{CommonConfig, PcEngineConfig};
use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use crate::mainloop::{CreatedEmulator, NativeDebugFn, create, file_name_no_ext};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use pce_core::api::PcEngineEmulator;

pub type NativePcEngineEmulator = NativeEmulator<PcEngineEmulator>;

impl CreatableEmulator for PcEngineEmulator {
    type NativeConfig = PcEngineConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        create::read_rom_file(&config.common.rom_file_path, extensions::PC_ENGINE)
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let emulator = PcEngineEmulator::create(input.input, config.emulator_config, save_writer);

        let rom_title = file_name_no_ext(&input.rom_path)?;
        let window_title = format!("pce - {rom_title}");

        let default_window_size = WindowSize::new_pce(
            config.common.initial_window_size,
            config.emulator_config.aspect_ratio,
            config.emulator_config.crop_overscan,
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    }

    fn common_config(config: &Self::NativeConfig) -> &CommonConfig {
        &config.common
    }

    fn emulator_config(config: &Self::NativeConfig) -> &Self::Config {
        &config.emulator_config
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
                jgenesis_debugger_frontend::pce::render_fn(),
            )
        })
    }
}
