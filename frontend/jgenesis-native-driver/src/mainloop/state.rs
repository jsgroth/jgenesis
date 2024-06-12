use crate::mainloop::{bincode_config, NativeEmulatorError};
use crate::NativeEmulatorResult;
use bincode::Encode;
use jgenesis_common::frontend::EmulatorTrait;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{array, fs};

pub const SAVE_STATE_SLOTS: usize = 10;
pub const EXTENSION: &str = "jst";

pub type SaveStatePaths = [PathBuf; SAVE_STATE_SLOTS];

pub fn init_paths(path: &Path) -> NativeEmulatorResult<[PathBuf; SAVE_STATE_SLOTS]> {
    let path_no_ext = path.with_extension("");
    let file_name = path_no_ext
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| NativeEmulatorError::ParseFileName(path.display().to_string()))?;

    let file_names: [_; SAVE_STATE_SLOTS] =
        array::from_fn(|i| format!("{file_name}_{i}.{EXTENSION}"));

    Ok(file_names.map(|name| path.with_file_name(name)))
}

#[derive(Debug, Clone, Default)]
pub struct SaveStateMetadata {
    pub times_nanos: [Option<u128>; SAVE_STATE_SLOTS],
}

impl SaveStateMetadata {
    pub(crate) fn load(paths: &SaveStatePaths) -> Self {
        let times_nanos = array::from_fn(|i| {
            let metadata = fs::metadata(&paths[i]).ok()?;
            let modified = metadata.modified().ok()?;
            Some(modified.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos())
        });

        Self { times_nanos }
    }
}

pub fn save<Emulator: Encode>(
    emulator: &Emulator,
    paths: &SaveStatePaths,
    slot: usize,
    metadata: &mut SaveStateMetadata,
) -> NativeEmulatorResult<()> {
    let path = &paths[slot];
    let file = File::create(path).map_err(|source| NativeEmulatorError::StateFileOpen {
        path: path.display().to_string(),
        source,
    })?;
    let mut writer = BufWriter::new(file);
    bincode::encode_into_std_write(emulator, &mut writer, bincode_config!())?;

    let now_nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
    metadata.times_nanos[slot] = Some(now_nanos);

    Ok(())
}

pub fn load<Emulator: EmulatorTrait>(
    emulator: &mut Emulator,
    config: &Emulator::Config,
    paths: &SaveStatePaths,
    slot: usize,
) -> NativeEmulatorResult<()> {
    let path = &paths[slot];
    let file = File::open(path).map_err(|source| NativeEmulatorError::StateFileOpen {
        path: path.display().to_string(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut loaded_emulator: Emulator =
        bincode::decode_from_std_read(&mut reader, bincode_config!())?;

    loaded_emulator.take_rom_from(emulator);
    *emulator = loaded_emulator;
    emulator.reload_config(config);

    Ok(())
}
