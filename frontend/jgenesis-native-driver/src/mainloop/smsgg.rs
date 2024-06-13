use crate::config::SmsGgConfig;

use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{basic_input_mapper_fn, debug, file_name_no_ext};
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use jgenesis_common::frontend::EmulatorTrait;

use crate::config::RomReadResult;
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgButton, SmsGgEmulator, SmsGgEmulatorConfig, SmsGgInputs};
use std::path::Path;

pub type NativeSmsGgEmulator =
    NativeEmulator<SmsGgInputs, SmsGgButton, SmsGgEmulatorConfig, SmsGgEmulator>;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["sms", "gg"];

impl NativeSmsGgEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_smsgg_config(&mut self, config: Box<SmsGgConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let vdp_version = config.vdp_version.unwrap_or_else(|| self.emulator.vdp_version());
        let psg_version = config.psg_version.unwrap_or_else(|| {
            if vdp_version.is_master_system() {
                PsgVersion::MasterSystem2
            } else {
                PsgVersion::Standard
            }
        });

        let emulator_config = config.to_emulator_config(vdp_version, psg_version);
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self.input_mapper.reload_config(
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.common.axis_deadzone,
            &SmsGgButton::ALL,
        ) {
            log::error!("Error reloading input config: {err}");
        }

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

    let rom_file_path = Path::new(&config.common.rom_file_path);

    let save_state_path = rom_file_path.with_extension("ss0");

    let RomReadResult { rom, extension } = config.common.read_rom_file(SUPPORTED_EXTENSIONS)?;

    let save_path = rom_file_path.with_extension("sav");
    let mut save_writer = FsSaveWriter::new(save_path);

    let vdp_version =
        config.vdp_version.unwrap_or_else(|| config::default_vdp_version_for_ext(&extension));
    let psg_version =
        config.psg_version.unwrap_or_else(|| config::default_psg_version_for_ext(&extension));

    log::info!("VDP version: {vdp_version:?}");
    log::info!("PSG version: {psg_version:?}");

    let rom_title = file_name_no_ext(rom_file_path)?;
    let window_title = format!("smsgg - {rom_title}");

    let emulator_config = config.to_emulator_config(vdp_version, psg_version);
    let emulator = SmsGgEmulator::create(rom, emulator_config, &mut save_writer);

    NativeSmsGgEmulator::new(
        emulator,
        emulator_config,
        config.common,
        config::default_smsgg_window_size(vdp_version),
        &window_title,
        save_writer,
        save_state_path,
        basic_input_mapper_fn(&SmsGgButton::ALL),
        debug::smsgg::render_fn,
    )
}
