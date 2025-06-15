use crate::config::RomReadResult;
use crate::config::{GenesisConfig, Sega32XConfig, SegaCdConfig};
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{NativeEmulatorError, debug, save};
use crate::{AudioError, NativeEmulator, NativeEmulatorResult, extensions};
use genesis_core::{GenesisEmulator, GenesisInputs};
use jgenesis_native_config::common::WindowSize;
use s32x_core::api::Sega32XEmulator;
use segacd_core::CdRomFileFormat;
use segacd_core::api::{SegaCdEmulator, SegaCdLoadResult};
use std::fs;
use std::path::{Path, PathBuf};

pub type NativeGenesisEmulator = NativeEmulator<GenesisEmulator>;

impl NativeGenesisEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_genesis_config(&mut self, config: Box<GenesisConfig>) -> Result<(), AudioError> {
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

pub type NativeSegaCdEmulator = NativeEmulator<SegaCdEmulator>;

impl NativeSegaCdEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_sega_cd_config(&mut self, config: Box<SegaCdConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.genesis.common)?;

        self.update_emulator_config(&config.emulator_config);

        self.input_mapper.update_mappings(
            config.genesis.common.axis_deadzone,
            &config.genesis.inputs.to_mapping_vec(),
            &config.genesis.common.hotkey_config.to_mapping_vec(),
        );

        Ok(())
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn remove_disc(&mut self) {
        self.emulator.remove_disc();

        // SAFETY: This is not reassigning the window
        unsafe {
            self.renderer
                .window_mut()
                .set_title("sega cd - (no disc)")
                .expect("Given string literal will never contain a null character");
        }
    }

    /// # Errors
    ///
    /// This method will return an error if the disc drive is unable to load the disc.
    #[allow(clippy::missing_panics_doc)]
    pub fn change_disc<P: AsRef<Path>>(&mut self, rom_path: P) -> SegaCdLoadResult<()> {
        let rom_format = CdRomFileFormat::from_file_path(rom_path.as_ref()).unwrap_or_else(|| {
            log::warn!(
                "Unrecognized CD-ROM file format, treating as CUE: {}",
                rom_path.as_ref().display()
            );
            CdRomFileFormat::CueBin
        });

        self.rom_path = rom_path.as_ref().to_path_buf();
        self.emulator.change_disc(rom_path, rom_format)?;

        let title = format!("sega cd - {}", self.emulator.disc_title());

        // SAFETY: This is not reassigning the window
        unsafe {
            self.renderer
                .window_mut()
                .set_title(&title)
                .expect("Disc title should have non-printable characters already removed");
        }

        if let Err(err) = self.update_save_paths(&self.common_config.clone()) {
            log::error!("Error updating save paths on disc change: {err}");
        }

        Ok(())
    }
}

pub type Native32XEmulator = NativeEmulator<Sega32XEmulator>;

impl Native32XEmulator {
    /// # Errors
    ///
    /// Propagates any errors encountered while reloading audio config.
    pub fn reload_32x_config(&mut self, config: Box<Sega32XConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.genesis.common)?;

        self.update_emulator_config(&config.emulator_config);

        self.input_mapper.update_mappings(
            config.genesis.common.axis_deadzone,
            &config.genesis.inputs.to_mapping_vec(),
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

    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.emulator_config;
    let emulator = GenesisEmulator::create(rom, emulator_config, &mut save_writer);

    let mut cartridge_title = emulator.cartridge_title();
    // Remove non-printable characters
    cartridge_title.retain(|c| {
        c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation()
    });
    let window_title = format!("genesis - {cartridge_title}");

    let default_window_size = WindowSize::new_genesis(
        config.common.initial_window_size,
        emulator_config.aspect_ratio,
        emulator.timing_mode(),
    );

    NativeGenesisEmulator::new(
        emulator,
        emulator_config,
        config.common,
        extension,
        default_window_size,
        &window_title,
        save_writer,
        save_state_path,
        &config.inputs.to_mapping_vec(),
        GenesisInputs::default(),
        debug::genesis::render_fn,
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

    let bios_file_path = config.bios_file_path.as_ref().ok_or(NativeEmulatorError::SegaCdNoBios)?;

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
            bios_file_path,
            SCD_SAVE_EXTENSION,
        )?;
        save_path = determined_paths.save_path;
        save_state_path = determined_paths.save_state_path;
    }

    let mut save_writer = FsSaveWriter::new(save_path);

    let bios = fs::read(bios_file_path).map_err(|source| NativeEmulatorError::SegaCdBiosRead {
        path: bios_file_path.clone(),
        source,
    })?;

    let emulator_config = config.emulator_config;
    let emulator = SegaCdEmulator::create(
        bios,
        rom_path,
        rom_format,
        config.run_without_disc,
        emulator_config,
        &mut save_writer,
    )?;

    let window_title = format!("sega cd - {}", emulator.disc_title());

    let default_window_size = WindowSize::new_genesis(
        config.genesis.common.initial_window_size,
        emulator_config.genesis.aspect_ratio,
        emulator.timing_mode(),
    );

    NativeSegaCdEmulator::new(
        emulator,
        emulator_config,
        config.genesis.common,
        SCD_SAVE_EXTENSION.into(),
        default_window_size,
        &window_title,
        save_writer,
        save_state_path,
        &config.genesis.inputs.to_mapping_vec(),
        GenesisInputs::default(),
        debug::genesis::render_fn,
    )
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

    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.emulator_config;
    let emulator =
        Sega32XEmulator::create(rom.into_boxed_slice(), emulator_config, &mut save_writer);

    let cartridge_title = emulator.cartridge_title();
    let window_title = format!("32x - {cartridge_title}");

    let default_window_size = WindowSize::new_32x(
        config.genesis.common.initial_window_size,
        emulator_config.genesis.aspect_ratio,
        emulator.timing_mode(),
    );

    Native32XEmulator::new(
        emulator,
        emulator_config,
        config.genesis.common,
        extension,
        default_window_size,
        &window_title,
        save_writer,
        save_state_path,
        &config.genesis.inputs.to_mapping_vec(),
        GenesisInputs::default(),
        debug::genesis::render_fn,
    )
}
