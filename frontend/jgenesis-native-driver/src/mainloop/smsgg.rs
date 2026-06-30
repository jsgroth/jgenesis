use crate::config::{CommonConfig, SmsGgConfig};
use std::fs;

use crate::mainloop::{CreatedEmulator, NativeDebugFn, create, file_name_no_ext};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, extensions};

use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use smsgg_core::{SmsGgEmulator, SmsGgHardware};
use std::path::PathBuf;

pub type NativeSmsGgEmulator = NativeEmulator<SmsGgEmulator>;

trait SmsGgHardwareExt: Sized + Copy {
    fn bios_path(self, config: &SmsGgConfig) -> Option<&PathBuf>;

    fn no_bios_error(self) -> NativeEmulatorError;

    fn standard_extension(self) -> &'static str;

    fn boot_from_bios(self, config: &SmsGgConfig) -> bool;
}

impl SmsGgHardwareExt for SmsGgHardware {
    fn bios_path(self, config: &SmsGgConfig) -> Option<&PathBuf> {
        match self {
            Self::MasterSystem => config.sms_bios_path.as_ref(),
            Self::GameGear => config.gg_bios_path.as_ref(),
            Self::Sg1000 => None,
        }
    }

    fn no_bios_error(self) -> NativeEmulatorError {
        match self {
            Self::MasterSystem | Self::Sg1000 => NativeEmulatorError::SmsNoBios,
            Self::GameGear => NativeEmulatorError::GgNoBios,
        }
    }

    fn standard_extension(self) -> &'static str {
        match self {
            Self::MasterSystem => "sms",
            Self::GameGear => "gg",
            Self::Sg1000 => "sg",
        }
    }

    fn boot_from_bios(self, config: &SmsGgConfig) -> bool {
        match self {
            Self::MasterSystem => config.sms_boot_from_bios,
            Self::GameGear => config.gg_boot_from_bios,
            Self::Sg1000 => false,
        }
    }
}

impl CreatableEmulator for SmsGgEmulator {
    type NativeConfig = SmsGgConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        if config.run_without_cartridge {
            let hardware = config.hardware.unwrap_or_else(|| {
                log::error!(
                    "run_without_cartridge set without specifying hardware; this is probably a bug"
                );
                SmsGgHardware::MasterSystem
            });

            let Some(bios_path) = hardware.bios_path(config) else {
                return Err(hardware.no_bios_error());
            };

            let bios_rom = fs::read(bios_path).map_err(|source| {
                NativeEmulatorError::SmsGgBiosRead { path: bios_path.clone(), source }
            })?;

            Ok(ReadInputResult {
                input: bios_rom,
                rom_path: bios_path.clone(),
                save_extension: hardware.standard_extension().into(),
            })
        } else {
            create::read_rom_file(&config.common.rom_file_path, &extensions::SMSGG)
        }
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let rom: Option<Vec<u8>>;
        let bios_rom: Option<Vec<u8>>;
        let hardware: SmsGgHardware;
        let rom_title: String;

        if config.run_without_cartridge {
            rom = None;
            bios_rom = Some(input.input);
            hardware = config.hardware.unwrap_or(SmsGgHardware::MasterSystem);
            rom_title = "(BIOS)".into();
        } else {
            rom = Some(input.input);

            hardware = config.hardware.unwrap_or_else(|| hardware_for_ext(&input.save_extension));

            bios_rom = if hardware.boot_from_bios(config) {
                let Some(bios_path) = hardware.bios_path(config) else {
                    return Err(hardware.no_bios_error());
                };
                Some(fs::read(bios_path).map_err(|source| NativeEmulatorError::SmsGgBiosRead {
                    path: bios_path.clone(),
                    source,
                })?)
            } else {
                None
            };

            rom_title = file_name_no_ext(&input.rom_path)?;
        }

        let emulator = SmsGgEmulator::create(
            rom,
            bios_rom,
            hardware,
            config.emulator_config.clone(),
            save_writer,
        );

        let window_title = match hardware {
            SmsGgHardware::MasterSystem => format!("sms - {rom_title}"),
            SmsGgHardware::GameGear => format!("gg - {rom_title}"),
            SmsGgHardware::Sg1000 => format!("sg1000 - {rom_title}"),
        };

        let default_window_size = match hardware {
            SmsGgHardware::MasterSystem | SmsGgHardware::Sg1000 => WindowSize::new_sms(
                config.common.initial_window_size,
                config.emulator_config.sms_aspect_ratio,
            ),
            SmsGgHardware::GameGear => WindowSize::new_game_gear(
                config.common.initial_window_size,
                config.emulator_config.gg_aspect_ratio,
            ),
        };

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
                jgenesis_debugger_frontend::smsgg::render_fn(),
            )
        })
    }
}

fn hardware_for_ext(extension: &str) -> SmsGgHardware {
    let extension = extension.to_ascii_lowercase();

    if extensions::MASTER_SYSTEM.contains(&extension.as_str()) {
        SmsGgHardware::MasterSystem
    } else if extensions::GAME_GEAR.contains(&extension.as_str()) {
        SmsGgHardware::GameGear
    } else if extensions::SG_1000.contains(&extension.as_str()) {
        SmsGgHardware::Sg1000
    } else {
        log::error!("Unrecognized file extension '{extension}', defaulting to SMS mode");
        SmsGgHardware::MasterSystem
    }
}
