use crate::config::{CommonConfig, GenesisConfig, Sega32XConfig, SegaCdConfig};
use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use crate::mainloop::runner::{ChangeDiscFn, RemoveDiscFn, RunnerCommand};
use crate::mainloop::{CreatedEmulator, NativeDebugFn, NativeEmulatorError, create};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};
use cdrom::reader::CdRom;
use genesis_config::{GenesisController, GenesisInputs, GenesisRegion};
use genesis_core::GenesisEmulator;
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use s32x_core::api::Sega32XEmulator;
use segacd_core::CdRomFileFormat;
use segacd_core::api::SegaCdEmulator;
use std::fs;
use std::path::{Path, PathBuf};

pub type NativeGenesisEmulator = NativeEmulator<GenesisEmulator>;

impl CreatableEmulator for GenesisEmulator {
    type NativeConfig = GenesisConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        create::read_rom_file(&config.common.rom_file_path, extensions::GENESIS)
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let emulator =
            GenesisEmulator::create(input.input, config.emulator_config.clone(), save_writer);

        let mut cartridge_title = emulator.cartridge_title();
        // Remove non-printable characters
        cartridge_title.retain(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation()
        });
        let window_title = format!("genesis - {cartridge_title}");

        let default_window_size = WindowSize::new_genesis(
            config.common.initial_window_size,
            config.emulator_config.aspect_ratio,
            emulator.timing_mode(),
            config.emulator_config.to_gen_par_params(),
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    }

    fn common_config(config: &Self::NativeConfig) -> &CommonConfig {
        &config.common
    }

    fn emulator_config(config: &Self::NativeConfig) -> &Self::Config {
        &config.emulator_config
    }

    fn reload_native_config(
        emulator: &mut NativeEmulator<Self>,
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<()> {
        update_controller_types(config, &mut emulator.inputs);

        Ok(())
    }

    fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
        new_initial_inputs(config)
    }

    fn input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.inputs.to_mapping_vec()
    }

    fn turbo_input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.inputs.to_turbo_mapping_vec()
    }

    fn debug_fn() -> Option<NativeDebugFn<Self>> {
        Some(jgenesis_debugger_frontend::genesis::genesis_debug_fn)
    }
}

fn new_initial_inputs(config: &GenesisConfig) -> GenesisInputs {
    GenesisInputs {
        p1: GenesisController::new(config.inputs.p1_type),
        p2: GenesisController::new(config.inputs.p2_type),
    }
}

fn update_controller_types(config: &GenesisConfig, inputs: &mut GenesisInputs) {
    if config.inputs.p1_type != inputs.p1.controller_type() {
        inputs.p1 = GenesisController::new(config.inputs.p1_type);
    }

    if config.inputs.p2_type != inputs.p2.controller_type() {
        inputs.p2 = GenesisController::new(config.inputs.p2_type);
    }
}

pub type NativeSegaCdEmulator = NativeEmulator<SegaCdEmulator>;

impl CreatableEmulator for SegaCdEmulator {
    type NativeConfig = SegaCdConfig;
    type CreateInput = (PathBuf, CdRomFileFormat);

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        const SCD_SAVE_EXTENSION: &str = "scd";

        let (region, bios_file_path) = determine_scd_bios_path(config);
        let Some(bios_file_path) = bios_file_path else {
            return Err(NativeEmulatorError::SegaCdNoBios(region));
        };

        let rom_path: PathBuf;
        let disc_format: CdRomFileFormat;

        if config.run_without_disc {
            rom_path = bios_file_path.clone();
            disc_format = CdRomFileFormat::CueBin;
        } else {
            rom_path = config.genesis.common.rom_file_path.clone();
            disc_format = CdRomFileFormat::from_file_path(&rom_path).unwrap_or_else(|| {
                log::warn!(
                    "Unrecognized CD-ROM file extension, behaving as if this is a CUE file: {}",
                    rom_path.display()
                );
                CdRomFileFormat::CueBin
            });
        }

        Ok(ReadInputResult {
            input: (bios_file_path, disc_format),
            rom_path,
            save_extension: SCD_SAVE_EXTENSION.into(),
        })
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let (bios_path, disc_format) = input.input;

        let bios = fs::read(&bios_path)
            .map_err(|source| NativeEmulatorError::SegaCdBiosRead { path: bios_path, source })?;

        let rom_path = if config.run_without_disc { Path::new("") } else { &input.rom_path };

        let emulator = SegaCdEmulator::create(
            bios,
            rom_path,
            disc_format,
            config.run_without_disc,
            config.emulator_config.clone(),
            save_writer,
        )?;

        let window_title = format!("sega cd - {}", emulator.disc_title());

        let default_window_size = WindowSize::new_genesis(
            config.genesis.common.initial_window_size,
            config.emulator_config.genesis.aspect_ratio,
            emulator.timing_mode(),
            config.emulator_config.genesis.to_gen_par_params(),
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    }

    fn common_config(config: &Self::NativeConfig) -> &CommonConfig {
        &config.genesis.common
    }

    fn emulator_config(config: &Self::NativeConfig) -> &Self::Config {
        &config.emulator_config
    }

    fn reload_native_config(
        emulator: &mut NativeEmulator<Self>,
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<()> {
        update_controller_types(&config.genesis, &mut emulator.inputs);

        Ok(())
    }

    fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
        new_initial_inputs(&config.genesis)
    }

    fn input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.genesis.inputs.to_mapping_vec()
    }

    fn turbo_input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.genesis.inputs.to_turbo_mapping_vec()
    }

    fn disc_change_fns() -> Option<(ChangeDiscFn<Self>, RemoveDiscFn<Self>)> {
        let change_disc_fn = |emulator: &mut SegaCdEmulator, path: PathBuf| {
            let rom_format = CdRomFileFormat::from_file_path(&path).unwrap_or_else(|| {
                log::warn!("Unrecognized CD-ROM file format, treating as CUE: {}", path.display());
                CdRomFileFormat::CueBin
            });

            emulator.change_disc(path, rom_format)?;

            let title = format!("sega cd - {}", emulator.disc_title());
            Ok(title)
        };

        Some((change_disc_fn, SegaCdEmulator::remove_disc))
    }

    fn debug_fn() -> Option<NativeDebugFn<Self>> {
        Some(jgenesis_debugger_frontend::genesis::sega_cd_debug_fn)
    }
}

impl NativeSegaCdEmulator {
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

impl CreatableEmulator for Sega32XEmulator {
    type NativeConfig = Sega32XConfig;
    type CreateInput = Vec<u8>;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        create::read_rom_file(&config.genesis.common.rom_file_path, extensions::SEGA_32X)
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let emulator =
            Sega32XEmulator::create(input.input, config.emulator_config.clone(), save_writer);

        let cartridge_title = emulator.cartridge_title();
        let window_title = format!("32x - {cartridge_title}");

        let default_window_size = WindowSize::new_32x(
            config.genesis.common.initial_window_size,
            config.emulator_config.genesis.aspect_ratio,
            emulator.timing_mode(),
            config.emulator_config.genesis.to_gen_par_params(),
        );

        Ok(CreatedEmulator { emulator, window_title, default_window_size })
    }

    fn common_config(config: &Self::NativeConfig) -> &CommonConfig {
        &config.genesis.common
    }

    fn emulator_config(config: &Self::NativeConfig) -> &Self::Config {
        &config.emulator_config
    }

    fn reload_native_config(
        emulator: &mut NativeEmulator<Self>,
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<()> {
        update_controller_types(&config.genesis, &mut emulator.inputs);

        Ok(())
    }

    fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
        new_initial_inputs(&config.genesis)
    }

    fn input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.genesis.inputs.to_mapping_vec()
    }

    fn turbo_input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        config.genesis.inputs.to_turbo_mapping_vec()
    }

    fn debug_fn() -> Option<NativeDebugFn<Self>> {
        Some(jgenesis_debugger_frontend::genesis::sega_32x_debug_fn)
    }
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
