use crate::config::{CommonConfig, GenesisConfig, Sega32XConfig, SegaCdConfig};
use crate::extensions::Console;
use crate::mainloop::create::{CreatableEmulator, ReadInputResult};
use crate::mainloop::runner::{ChangeDiscFn, RemoveDiscFn};
use crate::mainloop::{CreatedEmulator, NativeDebugFn, NativeEmulatorError, create};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};
use cdrom::reader::CdRom;
use cdrom::reader::CdRomFileFormat;
use genesis_components::GenesisEmulatorConfigExt;
use genesis_config::{GenesisController, GenesisInputs, GenesisRegion};
use genesis_core::GenesisEmulator;
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::WindowSize;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use segacd_core::api::SegaCdLoadError;
use std::fs;
use std::path::{Path, PathBuf};

pub type NativeGenesisEmulator = NativeEmulator<GenesisEmulator>;

#[derive(Clone)]
pub struct GenesisCreateInput {
    cartridge_rom: Option<Vec<u8>>,
    sega_cd_bios_rom: Option<Vec<u8>>,
    disc_path: Option<PathBuf>,
}

impl CreatableEmulator for GenesisEmulator {
    type NativeConfig = GenesisConfig;
    type CreateInput = GenesisCreateInput;

    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
        let read_cartridge_input =
            || create::read_rom_file(&config.common.rom_file_path, &extensions::GENESIS_32X);

        let sega_cd_bios_rom = if config.hardware.has_sega_cd() {
            let (region, bios_path) = determine_scd_bios_path(config);
            let Some(bios_path) = bios_path else {
                return Err(NativeEmulatorError::SegaCdNoBios(region));
            };

            let bios_rom = fs::read(&bios_path).map_err(|source| {
                NativeEmulatorError::SegaCdBiosRead { path: bios_path, source }
            })?;
            Some(bios_rom)
        } else {
            None
        };

        let (cartridge_input, disc_path) = if config.hardware.has_sega_cd() {
            if config.scd_run_without_disc {
                (None, None)
            } else {
                let primary_path_is_disc =
                    CdRomFileFormat::from_file_path(&config.common.rom_file_path).is_some();
                if primary_path_is_disc {
                    (None, Some(config.common.rom_file_path.clone()))
                } else {
                    // Assume primary path is a cartridge ROM image
                    let cartridge_input = read_cartridge_input()?;
                    (Some(cartridge_input), config.secondary_path.clone())
                }
            }
        } else {
            let cartridge_input = read_cartridge_input()?;
            (Some(cartridge_input), None)
        };

        let save_extension = match &cartridge_input {
            Some(input) => input.save_extension.clone(),
            None => Console::SegaCd.standard_extension().to_owned(),
        };

        Ok(ReadInputResult {
            input: GenesisCreateInput {
                cartridge_rom: cartridge_input.map(|input| input.input),
                sega_cd_bios_rom,
                disc_path,
            },
            rom_path: config.common.rom_file_path.clone(),
            save_extension,
        })
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let disc = match &input.input.disc_path {
            Some(path) => Some(read_sega_cd_disc(path, config.scd_load_disc_into_ram)?),
            None => None,
        };

        let mut emulator = GenesisEmulator::create(
            config.hardware,
            input.input.cartridge_rom,
            input.input.sega_cd_bios_rom,
            disc,
            config.emulator_config.clone(),
            save_writer,
        )?;

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

    fn disc_change_fns() -> Option<(ChangeDiscFn<Self>, RemoveDiscFn<Self>)> {
        // TODO CD32X
        None
    }

    fn debug_fn() -> Option<NativeDebugFn<Self>> {
        // TODO CD32X
        None
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

fn read_sega_cd_disc(path: &Path, open_in_memory: bool) -> NativeEmulatorResult<CdRom> {
    let disc_format = CdRomFileFormat::from_file_path(path).unwrap_or_else(|| {
        log::warn!("Unable to determine CD-ROM image format; assuming CUE/BIN");
        CdRomFileFormat::CueBin
    });

    let disc = if open_in_memory {
        CdRom::open_in_memory(path, disc_format)
    } else {
        CdRom::open(path, disc_format)
    };

    disc.map_err(|err| NativeEmulatorError::SegaCdDisc(SegaCdLoadError::CdRom(err)))
}

//
// pub type NativeSegaCdEmulator = NativeEmulator<SegaCdEmulator>;
//
// impl CreatableEmulator for SegaCdEmulator {
//     type NativeConfig = SegaCdConfig;
//     type CreateInput = (PathBuf, CdRomFileFormat);
//
//     fn read_create_input(
//         config: &Self::NativeConfig,
//     ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
//         const SCD_SAVE_EXTENSION: &str = "scd";
//
//         let (region, bios_file_path) = determine_scd_bios_path(config);
//         let Some(bios_file_path) = bios_file_path else {
//             return Err(NativeEmulatorError::SegaCdNoBios(region));
//         };
//
//         let rom_path: PathBuf;
//         let disc_format: CdRomFileFormat;
//
//         if config.run_without_disc {
//             rom_path = bios_file_path.clone();
//             disc_format = CdRomFileFormat::CueBin;
//         } else {
//             rom_path = config.genesis.common.rom_file_path.clone();
//             disc_format = CdRomFileFormat::from_file_path(&rom_path).unwrap_or_else(|| {
//                 log::warn!(
//                     "Unrecognized CD-ROM file extension, behaving as if this is a CUE file: {}",
//                     rom_path.display()
//                 );
//                 CdRomFileFormat::CueBin
//             });
//         }
//
//         Ok(ReadInputResult {
//             input: (bios_file_path, disc_format),
//             rom_path,
//             save_extension: SCD_SAVE_EXTENSION.into(),
//         })
//     }
//
//     fn create(
//         input: ReadInputResult<Self::CreateInput>,
//         config: &Self::NativeConfig,
//         save_writer: &mut impl SaveWriter,
//     ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
//         let (bios_path, disc_format) = input.input;
//
//         let bios = fs::read(&bios_path)
//             .map_err(|source| NativeEmulatorError::SegaCdBiosRead { path: bios_path, source })?;
//
//         let rom_path = if config.run_without_disc { Path::new("") } else { &input.rom_path };
//
//         let emulator = SegaCdEmulator::create(
//             bios,
//             rom_path,
//             disc_format,
//             config.run_without_disc,
//             config.emulator_config.clone(),
//             save_writer,
//         )?;
//
//         let window_title = format!("sega cd - {}", emulator.disc_title());
//
//         let default_window_size = WindowSize::new_genesis(
//             config.genesis.common.initial_window_size,
//             config.emulator_config.genesis.aspect_ratio,
//             emulator.timing_mode(),
//             config.emulator_config.genesis.to_gen_par_params(),
//         );
//
//         Ok(CreatedEmulator { emulator, window_title, default_window_size })
//     }
//
//     fn common_config(config: &Self::NativeConfig) -> &CommonConfig {
//         &config.genesis.common
//     }
//
//     fn emulator_config(config: &Self::NativeConfig) -> &Self::Config {
//         &config.emulator_config
//     }
//
//     fn reload_native_config(
//         emulator: &mut NativeEmulator<Self>,
//         config: &Self::NativeConfig,
//     ) -> NativeEmulatorResult<()> {
//         update_controller_types(&config.genesis, &mut emulator.inputs);
//
//         Ok(())
//     }
//
//     fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
//         new_initial_inputs(&config.genesis)
//     }
//
//     fn input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
//         config.genesis.inputs.to_mapping_vec()
//     }
//
//     fn turbo_input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
//         config.genesis.inputs.to_turbo_mapping_vec()
//     }
//
//     fn disc_change_fns() -> Option<(ChangeDiscFn<Self>, RemoveDiscFn<Self>)> {
//         let change_disc_fn = |emulator: &mut SegaCdEmulator, path: PathBuf| {
//             let rom_format = CdRomFileFormat::from_file_path(&path).unwrap_or_else(|| {
//                 log::warn!("Unrecognized CD-ROM file format, treating as CUE: {}", path.display());
//                 CdRomFileFormat::CueBin
//             });
//
//             emulator.change_disc(path, rom_format)?;
//
//             let title = format!("sega cd - {}", emulator.disc_title());
//             Ok(title)
//         };
//
//         Some((change_disc_fn, SegaCdEmulator::remove_disc))
//     }
//
//     fn debug_fn() -> Option<NativeDebugFn<Self>> {
//         Some(jgenesis_debugger_frontend::genesis::sega_cd_debug_fn)
//     }
// }
//
// impl NativeSegaCdEmulator {
//     /// # Errors
//     ///
//     /// This method will return an error if unable to send the command to the emulator runner thread.
//     #[allow(clippy::missing_panics_doc)]
//     pub fn remove_disc(&mut self) -> NativeEmulatorResult<()> {
//         self.runner.send_command(RunnerCommand::RemoveDisc)?;
//
//         // SAFETY: This is not reassigning the window
//         unsafe {
//             self.renderer
//                 .window_mut()
//                 .set_title("sega cd - (no disc)")
//                 .expect("Given string literal will never contain a null character");
//         }
//
//         Ok(())
//     }
//
//     /// # Errors
//     ///
//     /// This method will return an error if unable to send the command to the emulator runner thread.
//     #[allow(clippy::missing_panics_doc)]
//     pub fn change_disc<P: AsRef<Path>>(&mut self, rom_path: P) -> NativeEmulatorResult<()> {
//         self.rom_path = rom_path.as_ref().to_path_buf();
//
//         self.runner.send_command(RunnerCommand::ChangeDisc(self.rom_path.clone()))
//     }
// }
//
// pub type Native32XEmulator = NativeEmulator<Sega32XEmulator>;
//
// impl CreatableEmulator for Sega32XEmulator {
//     type NativeConfig = Sega32XConfig;
//     type CreateInput = Vec<u8>;
//
//     fn read_create_input(
//         config: &Self::NativeConfig,
//     ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>> {
//         create::read_rom_file(&config.genesis.common.rom_file_path, extensions::SEGA_32X)
//     }
//
//     fn create(
//         input: ReadInputResult<Self::CreateInput>,
//         config: &Self::NativeConfig,
//         save_writer: &mut impl SaveWriter,
//     ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
//         let emulator =
//             Sega32XEmulator::create(input.input, config.emulator_config.clone(), save_writer);
//
//         let cartridge_title = emulator.cartridge_title();
//         let window_title = format!("32x - {cartridge_title}");
//
//         let default_window_size = WindowSize::new_32x(
//             config.genesis.common.initial_window_size,
//             config.emulator_config.genesis.aspect_ratio,
//             emulator.timing_mode(),
//             config.emulator_config.genesis.to_gen_par_params(),
//         );
//
//         Ok(CreatedEmulator { emulator, window_title, default_window_size })
//     }
//
//     fn common_config(config: &Self::NativeConfig) -> &CommonConfig {
//         &config.genesis.common
//     }
//
//     fn emulator_config(config: &Self::NativeConfig) -> &Self::Config {
//         &config.emulator_config
//     }
//
//     fn reload_native_config(
//         emulator: &mut NativeEmulator<Self>,
//         config: &Self::NativeConfig,
//     ) -> NativeEmulatorResult<()> {
//         update_controller_types(&config.genesis, &mut emulator.inputs);
//
//         Ok(())
//     }
//
//     fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
//         new_initial_inputs(&config.genesis)
//     }
//
//     fn input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
//         config.genesis.inputs.to_mapping_vec()
//     }
//
//     fn turbo_input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
//         config.genesis.inputs.to_turbo_mapping_vec()
//     }
//
//     fn debug_fn() -> Option<NativeDebugFn<Self>> {
//         Some(jgenesis_debugger_frontend::genesis::sega_32x_debug_fn)
//     }
// }

fn determine_scd_bios_path(config: &GenesisConfig) -> (GenesisRegion, Option<PathBuf>) {
    if !config.scd_per_region_bios {
        return (GenesisRegion::Americas, bios_path_for_region(config, GenesisRegion::Americas));
    }

    if let Some(region) = config.emulator_config.forced_region {
        return (region, bios_path_for_region(config, region));
    }

    let file_path = &config.common.rom_file_path;
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

fn bios_path_for_region(config: &GenesisConfig, region: GenesisRegion) -> Option<PathBuf> {
    match region {
        GenesisRegion::Americas => config.scd_us_bios_path.clone(),
        GenesisRegion::Europe => config.scd_eu_bios_path.clone(),
        GenesisRegion::Japan => config.scd_jp_bios_path.clone(),
    }
}
