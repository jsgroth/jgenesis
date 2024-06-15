use crate::config::RomReadResult;
use crate::config::{GenesisConfig, Sega32XConfig, SegaCdConfig};
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{basic_input_mapper_fn, debug, NativeEmulatorError};
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use genesis_core::input::GenesisButton;
use genesis_core::{GenesisEmulator, GenesisEmulatorConfig, GenesisInputs};
use jgenesis_common::frontend::EmulatorTrait;
use s32x_core::api::{Sega32XEmulator, Sega32XEmulatorConfig};
use segacd_core::api::{SegaCdEmulator, SegaCdEmulatorConfig, SegaCdLoadResult};
use segacd_core::CdRomFileFormat;
use std::fs;
use std::path::Path;

pub type NativeGenesisEmulator =
    NativeEmulator<GenesisInputs, GenesisButton, GenesisEmulatorConfig, GenesisEmulator>;

pub const GENESIS_SUPPORTED_EXTENSIONS: &[&str] = &["md", "bin"];

impl NativeGenesisEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_genesis_config(&mut self, config: Box<GenesisConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let emulator_config = config.to_emulator_config();
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self.input_mapper.reload_config(
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.common.axis_deadzone,
            &GenesisButton::ALL,
        ) {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
}

pub type NativeSegaCdEmulator =
    NativeEmulator<GenesisInputs, GenesisButton, SegaCdEmulatorConfig, SegaCdEmulator>;

impl NativeSegaCdEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_sega_cd_config(&mut self, config: Box<SegaCdConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.genesis.common)?;
        self.emulator.reload_config(&config.to_emulator_config());

        if let Err(err) = self.input_mapper.reload_config(
            config.genesis.common.keyboard_inputs,
            config.genesis.common.joystick_inputs,
            config.genesis.common.axis_deadzone,
            &GenesisButton::ALL,
        ) {
            log::error!("Error reloading input config: {err}");
        }

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

        self.emulator.change_disc(rom_path, rom_format)?;

        let title = format!("sega cd - {}", self.emulator.disc_title());

        // SAFETY: This is not reassigning the window
        unsafe {
            self.renderer
                .window_mut()
                .set_title(&title)
                .expect("Disc title should have non-printable characters already removed");
        }

        Ok(())
    }
}

pub type Native32XEmulator =
    NativeEmulator<GenesisInputs, GenesisButton, Sega32XEmulatorConfig, Sega32XEmulator>;

/// Create an emulator with the Genesis core with the given config.
///
/// # Errors
///
/// This function will return an error upon encountering any video, audio, or I/O error.
pub fn create_genesis(config: Box<GenesisConfig>) -> NativeEmulatorResult<NativeGenesisEmulator> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.common.rom_file_path);
    let RomReadResult { rom, .. } = config.common.read_rom_file(GENESIS_SUPPORTED_EXTENSIONS)?;

    let save_path = rom_file_path.with_extension("sav");
    let save_state_path = rom_file_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator = GenesisEmulator::create(rom, emulator_config, &mut save_writer);

    let mut cartridge_title = emulator.cartridge_title();
    // Remove non-printable characters
    cartridge_title.retain(|c| {
        c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation()
    });
    let window_title = format!("genesis - {cartridge_title}");

    NativeGenesisEmulator::new(
        emulator,
        emulator_config,
        config.common,
        config::DEFAULT_GENESIS_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        basic_input_mapper_fn(&GenesisButton::ALL),
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
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.genesis.common.rom_file_path);
    let rom_format = CdRomFileFormat::from_file_path(rom_path).unwrap_or_else(|| {
        log::warn!(
            "Unrecognized CD-ROM file extension, behaving as if this is a CUE file: {}",
            rom_path.display()
        );
        CdRomFileFormat::CueBin
    });

    let save_path = rom_path.with_extension("sav");
    let save_state_path = rom_path.with_extension("ss0");
    let mut save_writer = FsSaveWriter::new(save_path);

    let bios_file_path = config.bios_file_path.as_ref().ok_or(NativeEmulatorError::SegaCdNoBios)?;
    let bios = fs::read(bios_file_path).map_err(|source| NativeEmulatorError::SegaCdBiosRead {
        path: bios_file_path.clone(),
        source,
    })?;

    let emulator_config = config.to_emulator_config();
    let emulator = SegaCdEmulator::create(
        bios,
        rom_path,
        rom_format,
        config.run_without_disc,
        emulator_config,
        &mut save_writer,
    )?;

    let window_title = format!("sega cd - {}", emulator.disc_title());

    NativeSegaCdEmulator::new(
        emulator,
        emulator_config,
        config.genesis.common,
        config::DEFAULT_GENESIS_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        basic_input_mapper_fn(&GenesisButton::ALL),
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
    let rom = fs::read(rom_path).map_err(|source| NativeEmulatorError::RomRead {
        path: rom_path.display().to_string(),
        source,
    })?;

    let save_state_path = rom_path.with_extension("ss0");

    let save_path = rom_path.with_extension("sav");
    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator =
        Sega32XEmulator::create(rom.into_boxed_slice(), emulator_config, &mut save_writer);

    let cartridge_title = emulator.cartridge_title();
    let window_title = format!("32x - {cartridge_title}");

    Native32XEmulator::new(
        emulator,
        emulator_config,
        config.genesis.common,
        config::DEFAULT_GENESIS_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        basic_input_mapper_fn(&GenesisButton::ALL),
        debug::genesis::render_fn,
    )
}
