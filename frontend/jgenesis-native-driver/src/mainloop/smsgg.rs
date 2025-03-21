use crate::config::SmsGgConfig;

use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{debug, file_name_no_ext, save};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, config, extensions};

use crate::config::RomReadResult;
use smsgg_core::{SmsGgEmulator, SmsGgHardware, SmsGgInputs};
use std::path::Path;

pub type NativeSmsGgEmulator = NativeEmulator<SmsGgEmulator>;

impl NativeSmsGgEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_smsgg_config(&mut self, config: Box<SmsGgConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        self.update_emulator_config(&config.emulator_config);

        self.input_mapper.update_mappings(
            config.common.axis_deadzone,
            &config.inputs.to_mapping_vec(),
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

    let rom_path = Path::new(&config.common.rom_file_path);

    let RomReadResult { rom, extension } = config.common.read_rom_file(&extensions::SMSGG)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let mut save_writer = FsSaveWriter::new(save_path);

    let hardware = hardware_for_ext(&extension);

    let rom_title = file_name_no_ext(rom_path)?;
    let window_title = format!("smsgg - {rom_title}");

    let emulator_config = config.emulator_config;
    let emulator = SmsGgEmulator::create(rom, hardware, emulator_config, &mut save_writer);

    NativeSmsGgEmulator::new(
        emulator,
        emulator_config,
        config.common,
        extension,
        config::default_smsgg_window_size(hardware, emulator_config.sms_aspect_ratio),
        &window_title,
        save_writer,
        save_state_path,
        &config.inputs.to_mapping_vec(),
        SmsGgInputs::default(),
        debug::smsgg::render_fn,
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
