use crate::config::{CommonConfig, PcEngineConfig};
use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use crate::mainloop::{CreatedEmulator, NativeDebugFn, create, file_name_no_ext};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, extensions};
use cdrom::reader::CdRomFileFormat;
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use pce_core::api::PcEngineEmulator;
use std::fs;
use std::path::PathBuf;

const CD_SAVE_EXTENSION: &str = "pcecd";

pub type NativePcEngineEmulator = NativeEmulator<PcEngineEmulator>;

#[derive(Clone)]
pub struct PceCreateInput {
    cartridge_rom: Vec<u8>,
    disc_path: Option<PathBuf>,
}

impl CreatableEmulator for PcEngineEmulator {
    type NativeConfig = PcEngineConfig;
    type CreateInput = PceCreateInput;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        let disc_format = CdRomFileFormat::from_file_path(&config.common.rom_file_path);

        match disc_format {
            Some(_) => {
                // Disc present, emulate CD-ROM²
                let Some(cd_bios_path) = &config.cd_bios_path else {
                    return Err(NativeEmulatorError::PceCdNoBios);
                };

                let bios_rom = fs::read(cd_bios_path).map_err(|source| {
                    NativeEmulatorError::PceCdBiosRead { path: cd_bios_path.clone(), source }
                })?;

                Ok(ReadInputResult {
                    input: PceCreateInput {
                        cartridge_rom: bios_rom,
                        disc_path: Some(config.common.rom_file_path.clone()),
                    },
                    rom_path: config.common.rom_file_path.clone(),
                    save_extension: CD_SAVE_EXTENSION.into(),
                })
            }
            None => {
                // No disc, assume HuCard game
                create::read_rom_file(&config.common.rom_file_path, extensions::PC_ENGINE).map(
                    |read_rom_result| ReadInputResult {
                        input: PceCreateInput {
                            cartridge_rom: read_rom_result.input,
                            disc_path: None,
                        },
                        rom_path: read_rom_result.rom_path,
                        save_extension: read_rom_result.save_extension,
                    },
                )
            }
        }
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let disc = match &input.input.disc_path {
            Some(disc_path) => Some(create::read_cdrom_image(
                disc_path,
                config.emulator_config.load_disc_into_ram,
            )?),
            None => None,
        };

        let emulator = PcEngineEmulator::create(
            input.input.cartridge_rom,
            disc,
            config.emulator_config,
            save_writer,
        );

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
