use crate::config::RomReadResult;
use crate::config::{GenesisConfig, Sega32XConfig, SegaCdConfig};
use crate::mainloop::runner::RunnerCommand;
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{CreatedEmulator, NativeEmulatorArgs, NativeEmulatorError, debug, save};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};
use cdrom::reader::CdRom;
use genesis_config::GenesisRegion;
use genesis_core::GenesisEmulator;
use jgenesis_native_config::common::WindowSize;
use s32x_core::api::Sega32XEmulator;
use segacd_core::CdRomFileFormat;
use segacd_core::api::SegaCdEmulator;
use std::fs;
use std::path::{Path, PathBuf};

pub type NativeGenesisEmulator = NativeEmulator<GenesisEmulator>;

impl NativeGenesisEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_genesis_config(
        &mut self,
        config: Box<GenesisConfig>,
    ) -> NativeEmulatorResult<()> {
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

pub type NativeSegaCdEmulator = NativeEmulator<SegaCdEmulator>;

impl NativeSegaCdEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_sega_cd_config(&mut self, config: Box<SegaCdConfig>) -> NativeEmulatorResult<()> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.genesis.common)?;

        self.update_and_reload_config(&config.emulator_config)?;

        self.input_mapper.update_mappings(
            config.genesis.common.axis_deadzone,
            &config.genesis.inputs.to_mapping_vec(),
            &config.genesis.inputs.to_turbo_mapping_vec(),
            &config.genesis.common.hotkey_config.to_mapping_vec(),
        );

        Ok(())
    }

    /// # Errors
    ///
    /// This method will return an error if unable to send the command to the emulator runner thread.
    #[allow(clippy::missing_panics_doc)]
    pub fn remove_disc(&mut self) -> NativeEmulatorResult<()> {
        self.runner.send_command(RunnerCommand::RemoveDisc)?;

        // SAFETY: This is not reassigning the window
        unsafe {
            self.renderer
                .window_mut()
                .set_title("sega cd - (no disc)")
                .expect("Given string literal will never contain a null character");
        }

        Ok(())
    }

    /// # Errors
    ///
    /// This method will return an error if unable to send the command to the emulator runner thread.
    #[allow(clippy::missing_panics_doc)]
    pub fn change_disc<P: AsRef<Path>>(&mut self, rom_path: P) -> NativeEmulatorResult<()> {
        self.rom_path = rom_path.as_ref().to_path_buf();

        self.runner.send_command(RunnerCommand::ChangeDisc(self.rom_path.clone()))
    }
}

pub type Native32XEmulator = NativeEmulator<Sega32XEmulator>;

impl Native32XEmulator {
    /// # Errors
    ///
    /// Propagates any errors encountered while reloading audio config.
    pub fn reload_32x_config(&mut self, config: Box<Sega32XConfig>) -> NativeEmulatorResult<()> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.genesis.common)?;

        self.update_and_reload_config(&config.emulator_config)?;

        self.input_mapper.update_mappings(
            config.genesis.common.axis_deadzone,
            &config.genesis.inputs.to_mapping_vec(),
            &config.genesis.inputs.to_turbo_mapping_vec(),
            &config.genesis.common.hotkey_config.to_mapping_vec(),
        );

        Ok(())
    }
}

/// Create an emulator with the Genesis core with the given config.
///
/// # Errors
///
/// This function will return an error upon encountering any video, audio, or I/O error.
pub fn create_genesis(config: Box<GenesisConfig>) -> NativeEmulatorResult<NativeGenesisEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let RomReadResult { rom, extension } = config.common.read_rom_file(extensions::GENESIS)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let emulator_config = config.emulator_config;
    let initial_window_size = config.common.initial_window_size;

    let create_emulator_fn = move |save_writer: &mut FsSaveWriter| {
        let emulator = GenesisEmulator::create(rom, emulator_config, save_writer);

        let mut cartridge_title = emulator.cartridge_title();
        // Remove non-printable characters
        cartridge_title.retain(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation()
        });
        let window_title = format!("genesis - {cartridge_title}");

        let default_window_size = WindowSize::new_genesis(
            initial_window_size,
            emulator_config.aspect_ratio,
            emulator.timing_mode(),
            emulator_config.to_gen_par_params(),
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    NativeGenesisEmulator::new(
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
        .with_debug_fn(|| debug::partial_clone_debug_fn(debug::genesis::render_fn())),
    )
}

/// Create an emulator with the Sega CD core with the given config.
///
/// # Errors
///
/// This function will return an error upon encountering any video, audio, or I/O error, including
/// any error encountered loading the Sega CD game disc.
pub fn create_sega_cd(config: Box<SegaCdConfig>) -> NativeEmulatorResult<NativeSegaCdEmulator> {
    const SCD_SAVE_EXTENSION: &str = "scd";

    log::info!("Running with config: {config}");

    let (region, bios_file_path) = determine_scd_bios_path(&config);
    let Some(bios_file_path) = bios_file_path else {
        return Err(NativeEmulatorError::SegaCdNoBios(region));
    };

    log::info!("Using BIOS for region {}", region.long_name());

    let rom_path: &Path;
    let rom_format: CdRomFileFormat;
    let save_path: PathBuf;
    let save_state_path: PathBuf;

    if !config.run_without_disc {
        rom_path = Path::new(&config.genesis.common.rom_file_path);
        rom_format = CdRomFileFormat::from_file_path(rom_path).unwrap_or_else(|| {
            log::warn!(
                "Unrecognized CD-ROM file extension, behaving as if this is a CUE file: {}",
                rom_path.display()
            );
            CdRomFileFormat::CueBin
        });

        let determined_paths = save::determine_save_paths(
            &config.genesis.common.save_path,
            &config.genesis.common.state_path,
            rom_path,
            SCD_SAVE_EXTENSION,
        )?;
        save_path = determined_paths.save_path;
        save_state_path = determined_paths.save_state_path;
    } else {
        rom_path = Path::new("");
        rom_format = CdRomFileFormat::CueBin;

        let determined_paths = save::determine_save_paths(
            &config.genesis.common.save_path,
            &config.genesis.common.state_path,
            &bios_file_path,
            SCD_SAVE_EXTENSION,
        )?;
        save_path = determined_paths.save_path;
        save_state_path = determined_paths.save_state_path;
    }

    let bios = fs::read(&bios_file_path).map_err(|source| NativeEmulatorError::SegaCdBiosRead {
        path: bios_file_path.clone(),
        source,
    })?;

    let emulator_config = config.emulator_config;
    let initial_window_size = config.genesis.common.initial_window_size;
    let run_without_disc = config.run_without_disc;
    let rom_path = rom_path.to_owned();

    let create_emulator_fn = move |save_writer: &mut FsSaveWriter| {
        let emulator = SegaCdEmulator::create(
            bios,
            rom_path,
            rom_format,
            run_without_disc,
            emulator_config,
            save_writer,
        )?;

        let window_title = format!("sega cd - {}", emulator.disc_title());

        let default_window_size = WindowSize::new_genesis(
            initial_window_size,
            emulator_config.genesis.aspect_ratio,
            emulator.timing_mode(),
            emulator_config.genesis.to_gen_par_params(),
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    let change_disc_fn = |emulator: &mut SegaCdEmulator, path: PathBuf| {
        let rom_format = CdRomFileFormat::from_file_path(&path).unwrap_or_else(|| {
            log::warn!("Unrecognized CD-ROM file format, treating as CUE: {}", path.display());
            CdRomFileFormat::CueBin
        });

        emulator.change_disc(path, rom_format)?;

        let title = format!("sega cd - {}", emulator.disc_title());
        Ok(title)
    };

    let remove_disc_fn = SegaCdEmulator::remove_disc;

    NativeSegaCdEmulator::new(
        NativeEmulatorArgs::new(
            Box::new(create_emulator_fn),
            emulator_config,
            config.genesis.common,
            SCD_SAVE_EXTENSION.into(),
            save_path,
            save_state_path,
            config.genesis.inputs.to_mapping_vec(),
        )
        .with_turbo_mappings(config.genesis.inputs.to_turbo_mapping_vec())
        .with_debug_fn(|| debug::partial_clone_debug_fn(debug::genesis::render_fn()))
        .with_disc_change_fns(change_disc_fn, remove_disc_fn),
    )
}

fn determine_scd_bios_path(config: &SegaCdConfig) -> (GenesisRegion, Option<PathBuf>) {
    if !config.per_region_bios {
        return (GenesisRegion::Americas, bios_path_for_region(config, GenesisRegion::Americas));
    }

    if let Some(region) = config.genesis.emulator_config.forced_region {
        return (region, bios_path_for_region(config, region));
    }

    let file_path = &config.genesis.common.rom_file_path;
    let region = CdRomFileFormat::from_file_path(file_path)
        .and_then(|cdrom_format| CdRom::open(file_path, cdrom_format).ok())
        .and_then(|mut disc| {
            segacd_core::parse_disc_region(&mut disc).ok()
        })
        .unwrap_or_else(|| {
            log::error!("Unable to determine region of disc at '{}' for purposes of selecting BIOS path; defaulting to US", file_path.display());
            GenesisRegion::Americas
        });

    (region, bios_path_for_region(config, region))
}

fn bios_path_for_region(config: &SegaCdConfig, region: GenesisRegion) -> Option<PathBuf> {
    match region {
        GenesisRegion::Americas => config.bios_file_path.clone(),
        GenesisRegion::Europe => config.eu_bios_file_path.clone(),
        GenesisRegion::Japan => config.jp_bios_file_path.clone(),
    }
}

/// Create an emulator with the 32X core with the given config.
///
/// # Errors
///
/// Propagates any errors encountered while initializing the emulator.
pub fn create_32x(config: Box<Sega32XConfig>) -> NativeEmulatorResult<Native32XEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.genesis.common.rom_file_path);
    let RomReadResult { rom, extension } =
        config.genesis.common.read_rom_file(extensions::SEGA_32X)?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.genesis.common.save_path,
        &config.genesis.common.state_path,
        rom_path,
        &extension,
    )?;

    let emulator_config = config.emulator_config;
    let initial_window_size = config.genesis.common.initial_window_size;

    let create_emulator_fn = move |save_writer: &mut FsSaveWriter| {
        let emulator = Sega32XEmulator::create(rom, emulator_config, save_writer);

        let cartridge_title = emulator.cartridge_title();
        let window_title = format!("32x - {cartridge_title}");

        let default_window_size = WindowSize::new_32x(
            initial_window_size,
            emulator_config.genesis.aspect_ratio,
            emulator.timing_mode(),
            emulator_config.genesis.to_gen_par_params(),
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    };

    Native32XEmulator::new(
        NativeEmulatorArgs::new(
            Box::new(create_emulator_fn),
            emulator_config,
            config.genesis.common,
            extension,
            save_path,
            save_state_path,
            config.genesis.inputs.to_mapping_vec(),
        )
        .with_turbo_mappings(config.genesis.inputs.to_turbo_mapping_vec())
        .with_debug_fn(|| debug::partial_clone_debug_fn(debug::genesis::render_fn())),
    )
}
