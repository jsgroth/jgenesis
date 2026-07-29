use crate::config::{CommonConfig, GenesisConfig};
use crate::extensions::Console;
use crate::mainloop::create::{CreatableEmulator, ReadInputResult, WindowTitle};
use crate::mainloop::{CreatedEmulator, NativeDebugFn, NativeEmulatorError, create};
use crate::{NativeEmulator, NativeEmulatorResult, extensions};
use cdrom::reader::CdRom;
use cdrom::reader::CdRomFileFormat;
use genesis_components::GenesisEmulatorConfigExt;
use genesis_config::{GenesisController, GenesisInputs, GenesisRegion};
use genesis_core::GenesisEmulator;
use genesis_core::api::GenesisHardware;
use jgenesis_common::frontend::{EmulatorTrait, SaveWriter};
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

        let mut rom_path = config.common.rom_file_path.clone();

        let sega_cd_bios_rom = if config.hardware.has_sega_cd() {
            let (region, bios_path) = determine_scd_bios_path(config);
            let Some(bios_path) = bios_path else {
                return Err(NativeEmulatorError::SegaCdNoBios(region));
            };

            if config.scd_run_without_disc {
                rom_path.clone_from(&bios_path);
            }

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
            rom_path,
            save_extension,
        })
    }

    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>> {
        let disc = match &input.input.disc_path {
            Some(path) => {
                Some(read_sega_cd_disc(path, config.emulator_config.sega_cd.load_disc_into_ram)?)
            }
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

        let default_window_size = if config.hardware.has_32x() {
            WindowSize::new_32x(
                config.common.initial_window_size,
                config.emulator_config.aspect_ratio,
                emulator.timing_mode(),
                config.emulator_config.to_gen_par_params(),
            )
        } else {
            WindowSize::new_genesis(
                config.common.initial_window_size,
                config.emulator_config.aspect_ratio,
                emulator.timing_mode(),
                config.emulator_config.to_gen_par_params(),
            )
        };

        let window_title = generate_window_title(&mut emulator, config.hardware);

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

    fn change_disc(
        &mut self,
        disc_path: &Path,
        config: &<Self as EmulatorTrait>::Config,
    ) -> NativeEmulatorResult<Option<WindowTitle>> {
        let disc = read_sega_cd_disc(disc_path, config.sega_cd.load_disc_into_ram)?;

        log::info!("Changing to disc read from path '{}'", disc_path.display());

        GenesisEmulator::change_disc(self, disc)?;

        let window_title = generate_window_title(self, self.hardware());
        Ok(Some(WindowTitle(window_title)))
    }

    fn remove_disc(&mut self) -> Option<WindowTitle> {
        GenesisEmulator::remove_disc(self);

        let window_title = generate_window_title(self, self.hardware());
        Some(WindowTitle(window_title))
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

fn generate_window_title(emulator: &mut GenesisEmulator, hardware: GenesisHardware) -> String {
    let system_name = hardware.to_string().to_ascii_lowercase();
    let mut game_title = emulator.game_title().unwrap_or("(no disc)".into());
    // Remove non-printable characters
    game_title.retain(|c| {
        c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation()
    });
    format!("{system_name} - {game_title}")
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
