use crate::config::{GameBoyAdvanceConfig, RomReadResult};
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::{NativeEmulatorError, file_name_no_ext, save};
use crate::{NativeEmulator, NativeEmulatorResult, config};
use gba_core::api::{GameBoyAdvanceEmulator, GbaEmulatorConfig};
use gba_core::input::{GbaButton, GbaInputs};
use std::fs;
use std::path::Path;

pub type NativeGbaEmulator =
    NativeEmulator<GbaInputs, GbaButton, GbaEmulatorConfig, GameBoyAdvanceEmulator>;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["gba"];

pub fn create_gba(config: Box<GameBoyAdvanceConfig>) -> NativeEmulatorResult<NativeGbaEmulator> {
    log::info!("Running with config: {config}");

    let rom_path = Path::new(&config.common.rom_file_path);
    let RomReadResult { rom, extension } = config.common.read_rom_file(SUPPORTED_EXTENSIONS)?;

    let Some(bios_path) = &config.bios_path else {
        return Err(NativeEmulatorError::GbaNoBios);
    };

    let bios_rom = fs::read(bios_path)
        .map_err(|source| NativeEmulatorError::GbaBiosRead { path: bios_path.clone(), source })?;

    let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
        &config.common.save_path,
        &config.common.state_path,
        rom_path,
        &extension,
    )?;

    let mut save_writer = FsSaveWriter::new(save_path);

    let emulator_config = config.to_emulator_config();
    let emulator =
        GameBoyAdvanceEmulator::create(rom, bios_rom, emulator_config, &mut save_writer)?;

    let rom_title = file_name_no_ext(&config.common.rom_file_path)?;
    let window_title = format!("gba - {rom_title}");

    NativeGbaEmulator::new(
        emulator,
        emulator_config,
        config.common,
        extension,
        config::DEFAULT_GBA_WINDOW_SIZE,
        &window_title,
        save_writer,
        save_state_path,
        &config.inputs.to_mapping_vec(),
        GbaInputs::default(),
        || Box::new(|_ctx| {}),
    )
}
