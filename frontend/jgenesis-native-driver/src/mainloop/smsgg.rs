use crate::config::SmsGgConfig;
use std::fs;

use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{CreatedEmulator, NativeEmulatorArgs, debug, file_name_no_ext, save};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, extensions};

use jgenesis_native_config::common::WindowSize;
use smsgg_core::{SmsGgEmulator, SmsGgHardware};
use std::path::{Path, PathBuf};

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
        }
    }

    fn no_bios_error(self) -> NativeEmulatorError {
        match self {
            Self::MasterSystem => NativeEmulatorError::SmsNoBios,
            Self::GameGear => NativeEmulatorError::GgNoBios,
        }
    }

    fn standard_extension(self) -> &'static str {
        match self {
            Self::MasterSystem => "sms",
            Self::GameGear => "gg",
        }
    }

    fn boot_from_bios(self, config: &SmsGgConfig) -> bool {
        match self {
            Self::MasterSystem => config.sms_boot_from_bios,
            Self::GameGear => config.gg_boot_from_bios,
        }
    }
}

impl NativeSmsGgEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_smsgg_config(&mut self, config: Box<SmsGgConfig>) -> NativeEmulatorResult<()> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        self.update_and_reload_config(&config.emulator_config)?;

        self.input_mapper.update_mappings(
            config.common.axis_deadzone,
            &config.inputs.to_mapping_vec(),
            &config.inputs.to_turbo_mapping_vec(),
            &config.common.hotkey_config.to_mapping_vec(),
        );

        Ok(())
    }
}

/// Create an emulator with the SMS/GG core with the given config.
///
/// # Errors
///
/// This function will propagate any video, audio, or disk errors encountered.
pub fn create_smsgg(config: Box<SmsGgConfig>) -> NativeEmulatorResult<NativeSmsGgEmulator> {
    log::info!("Running with config: {config}");

    let rom: Option<Vec<u8>>;
    let extension: String;
    let save_path: PathBuf;
    let save_state_path: PathBuf;
    let hardware: SmsGgHardware;
    let rom_title: String;

    let run_without_cartridge = config.run_without_cartridge;
    if !run_without_cartridge {
        let rom_path = Path::new(&config.common.rom_file_path);

        let rom_read_result = config.common.read_rom_file(&extensions::SMSGG)?;
        rom = Some(rom_read_result.rom);
        extension = rom_read_result.extension;

        let determined_paths = save::determine_save_paths(
            &config.common.save_path,
            &config.common.state_path,
            rom_path,
            &extension,
        )?;
        save_path = determined_paths.save_path;
        save_state_path = determined_paths.save_state_path;

        hardware = config.hardware.unwrap_or_else(|| hardware_for_ext(&extension));
        rom_title = file_name_no_ext(rom_path)?;
    } else {
        hardware = config.hardware.unwrap_or_else(|| {
            log::error!(
                "run_without_cartridge set without specifying hardware; this is probably a bug"
            );
            SmsGgHardware::MasterSystem
        });

        let bios_path = hardware.bios_path(&config);
        let Some(bios_path) = bios_path else { return Err(hardware.no_bios_error()) };

        rom = None;
        extension = hardware.standard_extension().into();

        let determined_paths = save::determine_save_paths(
            &config.common.save_path,
            &config.common.state_path,
            bios_path,
            &extension,
        )?;
        save_path = determined_paths.save_path;
        save_state_path = determined_paths.save_state_path;

        rom_title = "(BIOS)".into();
    }

    let bios_rom = if hardware.boot_from_bios(&config) {
        let Some(bios_path) = hardware.bios_path(&config) else {
            return Err(hardware.no_bios_error());
        };
        Some(fs::read(bios_path).map_err(|source| NativeEmulatorError::SmsGgBiosRead {
            path: bios_path.clone(),
            source,
        })?)
    } else {
        None
    };

    let emulator_config = config.emulator_config;
    let initial_window_size = config.common.initial_window_size;

    let create_emulator_fn = move |save_writer: &mut FsSaveWriter| {
        let emulator = SmsGgEmulator::create(rom, bios_rom, hardware, emulator_config, save_writer);

        let window_title = format!("smsgg - {rom_title}");

        let default_window_size = match hardware {
            SmsGgHardware::MasterSystem => {
                WindowSize::new_sms(initial_window_size, emulator_config.sms_aspect_ratio)
            }
            SmsGgHardware::GameGear => {
                WindowSize::new_game_gear(initial_window_size, emulator_config.gg_aspect_ratio)
            }
        };

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    NativeSmsGgEmulator::new(
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
        .with_debug_render_fn(debug::smsgg::render_fn),
    )
}

fn hardware_for_ext(extension: &str) -> SmsGgHardware {
    match extension.to_ascii_lowercase().as_str() {
        "sms" => SmsGgHardware::MasterSystem,
        "gg" => SmsGgHardware::GameGear,
        _ => {
            log::error!("Unrecognized file extension '{extension}', defaulting to SMS mode");
            SmsGgHardware::MasterSystem
        }
    }
}
