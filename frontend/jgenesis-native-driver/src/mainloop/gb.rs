use crate::config::{CommonConfig, GameBoyConfig};
use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use crate::mainloop::{CreatedEmulator, NativeDebugFn, create, file_name_no_ext};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, extensions};
use gb_core::api::{BootRoms, GameBoyEmulator};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use std::fs;
use std::path::PathBuf;

pub type NativeGameBoyEmulator = NativeEmulator<GameBoyEmulator>;

impl CreatableEmulator for GameBoyEmulator {
    type NativeConfig = GameBoyConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        create::read_rom_file(&config.common.rom_file_path, &extensions::GB_GBC)
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let dmg_boot_rom = load_boot_rom(
            config.dmg_boot_rom,
            config.dmg_boot_rom_path.as_ref(),
            NativeEmulatorError::GbNoDmgBootRom,
        )?;
        let cgb_boot_rom = load_boot_rom(
            config.cgb_boot_rom,
            config.cgb_boot_rom_path.as_ref(),
            NativeEmulatorError::GbNoCgbBootRom,
        )?;
        let boot_roms = BootRoms { dmg: dmg_boot_rom, cgb: cgb_boot_rom };

        let emulator =
            GameBoyEmulator::create(input.input, boot_roms, config.emulator_config, save_writer)?;

        let rom_title = file_name_no_ext(&input.rom_path)?;
        let window_title = format!("gb - {rom_title}");

        let default_window_size = WindowSize::new_gb(config.common.initial_window_size);

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
                jgenesis_debugger_frontend::gb::render_fn(),
            )
        })
    }
}

fn load_boot_rom(
    load: bool,
    path: Option<&PathBuf>,
    no_boot_rom_err: NativeEmulatorError,
) -> NativeEmulatorResult<Option<Vec<u8>>> {
    if !load {
        return Ok(None);
    }

    let Some(path) = path else { return Err(no_boot_rom_err) };

    let boot_rom = fs::read(path).map_err(NativeEmulatorError::GbBootRomLoad)?;
    Ok(Some(boot_rom))
}
