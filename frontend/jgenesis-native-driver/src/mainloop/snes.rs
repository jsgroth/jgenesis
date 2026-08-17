use crate::config::{CommonConfig, SnesConfig};

use crate::mainloop::{CreatedEmulator, NativeDebugFn, create};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};

use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::{ButtonMappingVec, SnesControllerType};
use snes_config::SnesJoypadState;
use snes_core::api::SnesEmulator;
use snes_core::input::{SnesController, SnesInputs, SnesMouseState, SuperScopeState};

trait SnesControllerTypeExt {
    fn to_controller(self) -> SnesController;
}

impl SnesControllerTypeExt for SnesControllerType {
    fn to_controller(self) -> SnesController {
        match self {
            Self::Gamepad => SnesController::Gamepad(SnesJoypadState::default()),
            Self::Mouse => SnesController::Mouse(SnesMouseState::default()),
            Self::SuperScope => SnesController::SuperScope(SuperScopeState::default()),
            Self::None => SnesController::None,
        }
    }
}

trait SnesControllerExt {
    fn to_type(self) -> SnesControllerType;
}

impl SnesControllerExt for SnesController {
    fn to_type(self) -> SnesControllerType {
        match self {
            Self::Gamepad(_) => SnesControllerType::Gamepad,
            Self::Mouse(_) => SnesControllerType::Mouse,
            Self::SuperScope(_) => SnesControllerType::SuperScope,
            Self::None => SnesControllerType::None,
        }
    }
}

pub type NativeSnesEmulator = NativeEmulator<SnesEmulator>;

impl CreatableEmulator for SnesEmulator {
    type NativeConfig = SnesConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        create::read_rom_file(&config.common.rom_file_path, extensions::SNES)
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let coprocessor_roms = config.to_coprocessor_roms();
        let mut emulator = SnesEmulator::create(
            input.input,
            config.emulator_config,
            coprocessor_roms,
            save_writer,
        )?;

        let cartridge_title = emulator.cartridge_title();
        let window_title = format!("snes - {cartridge_title}");

        let default_window_size = WindowSize::new_snes(
            config.common.initial_window_size,
            config.emulator_config.aspect_ratio,
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
        for (inputs, controller_type) in [
            (&mut emulator.inputs.p1, config.inputs.p1_type),
            (&mut emulator.inputs.p2, config.inputs.p2_type),
        ] {
            if inputs.to_type() != controller_type {
                *inputs = controller_type.to_controller();
            }
        }

        Ok(())
    }

    fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
        SnesInputs {
            p1: config.inputs.p1_type.to_controller(),
            p2: config.inputs.p2_type.to_controller(),
        }
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
                jgenesis_debugger_frontend::snes::render_fn(),
            )
        })
    }
}
