use crate::config::{PcEngineConfig, RomReadResult};
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{CreatedEmulator, NativeEmulatorArgs, file_name_no_ext, save};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};
use jgenesis_native_config::common::WindowSize;
use pce_core::api::PcEngineEmulator;
use std::path::Path;

pub type NativePcEngineEmulator = NativeEmulator<PcEngineEmulator>;

impl NativePcEngineEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_pce_config(&mut self, config: Box<PcEngineConfig>) -> NativeEmulatorResult<()> {
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

pub fn create_pce(config: Box<PcEngineConfig>) -> NativeEmulatorResult<NativePcEngineEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let RomReadResult { rom, extension } = config.common.read_rom_file(&extensions::PC_ENGINE)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let emulator_config = config.emulator_config;
    let initial_window_size = config.common.initial_window_size;
    let rom_file_path = config.common.rom_file_path.clone();

    let create_emulator_fn = move |_save_writer: &mut FsSaveWriter| {
        let emulator = PcEngineEmulator::create(rom, emulator_config);

        let rom_title = file_name_no_ext(rom_file_path)?;
        let window_title = format!("pce - {rom_title}");

        let default_window_size = WindowSize::new_pce(initial_window_size);

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    NativePcEngineEmulator::new(
        NativeEmulatorArgs::new(
            Box::new(create_emulator_fn),
            emulator_config,
            config.common,
            extension,
            save_path,
            save_state_path,
            config.inputs.to_mapping_vec(),
        )
        .with_turbo_mappings(config.inputs.to_turbo_mapping_vec()),
    )
}
