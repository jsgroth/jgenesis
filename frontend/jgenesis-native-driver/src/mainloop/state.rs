use crate::mainloop::{bincode_config, NativeEmulatorError};
use crate::NativeEmulatorResult;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::PartialClone;
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
        array::from_fn(|i| String::from_iter([file_name, format!("_{i}").as_str()]));

    Ok(file_names.map(|name| path.with_file_name(name).with_extension(EXTENSION)))
}

#[derive(Debug, Clone)]
pub struct SaveStateMetadata {
    pub times_nanos: [Option<u128>; SAVE_STATE_SLOTS],
}

impl SaveStateMetadata {
    pub fn load(paths: &SaveStatePaths) -> Self {
        let times_nanos = array::from_fn(|i| {
            let metadata = fs::metadata(&paths[i]).ok()?;
            let modified = metadata.modified().ok()?;
            Some(modified.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos())
        });

        Self { times_nanos }
    }
}

pub fn save<Emulator: PartialClone + Encode>(
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

pub fn load<Emulator: Decode>(
    paths: &SaveStatePaths,
    slot: usize,
) -> NativeEmulatorResult<Emulator> {
    let path = &paths[slot];
    let file = File::open(path).map_err(|source| NativeEmulatorError::StateFileOpen {
        path: path.display().to_string(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let emulator = bincode::decode_from_std_read(&mut reader, bincode_config!())?;

    Ok(emulator)
}
