use crate::NativeEmulatorResult;
use crate::mainloop::{NativeEmulatorError, bincode_config};
use jgenesis_common::frontend::EmulatorTrait;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{array, fs};

pub const SAVE_STATE_SLOTS: usize = 10;
pub const EXTENSION: &str = "jst";

const FILE_PREFIX: &[u8] = b"jgenstate";

// Prefix + 2 bytes for version
const HEADER_LEN: usize = FILE_PREFIX.len() + 2;

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
    pub(crate) fn load(paths: &SaveStatePaths, version: u16) -> Self {
        let times_nanos = array::from_fn(|i| {
            if read_version_from_header(&paths[i]) != Some(version) {
                return None;
            }

            let metadata = fs::metadata(&paths[i]).ok()?;
            let modified = metadata.modified().ok()?;
            Some(modified.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos())
        });

        Self { times_nanos }
    }
}

fn read_version_from_header(path: &Path) -> Option<u16> {
    let mut file = File::open(path).ok()?;

    let mut buffer = [0_u8; HEADER_LEN];
    file.read_exact(&mut buffer).ok()?;

    if &buffer[..FILE_PREFIX.len()] != FILE_PREFIX {
        return None;
    }

    Some(read_version_from_buffer(&buffer))
}

fn read_version_from_buffer(buffer: &[u8]) -> u16 {
    u16::from_le_bytes([buffer[FILE_PREFIX.len()], buffer[FILE_PREFIX.len() + 1]])
}

pub fn save<Emulator: EmulatorTrait>(
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
    writer.write_all(FILE_PREFIX).map_err(NativeEmulatorError::SaveStateIo)?;
    writer
        .write_all(&Emulator::save_state_version().to_le_bytes())
        .map_err(NativeEmulatorError::SaveStateIo)?;

    let mut encoder =
        zstd::stream::Encoder::new(writer, 0).map_err(NativeEmulatorError::SaveStateIo)?;
    bincode::encode_into_std_write(emulator, &mut encoder, bincode_config!())?;
    encoder.finish().map_err(NativeEmulatorError::SaveStateIo)?;

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
    let mut header_buffer = [0_u8; HEADER_LEN];
    reader.read_exact(&mut header_buffer).map_err(NativeEmulatorError::LoadStateIo)?;

    if &header_buffer[..FILE_PREFIX.len()] != FILE_PREFIX {
        return Err(NativeEmulatorError::LoadStatePrefixMismatch);
    }

    let current_version = Emulator::save_state_version();
    let version_in_header = read_version_from_buffer(&header_buffer);
    if version_in_header != current_version {
        return Err(NativeEmulatorError::LoadStateVersionMismatch {
            expected: current_version,
            actual: version_in_header,
        });
    }

    let mut decoder =
        zstd::stream::Decoder::new(reader).map_err(NativeEmulatorError::LoadStateIo)?;
    let mut loaded_emulator: Emulator =
        bincode::decode_from_std_read(&mut decoder, bincode_config!())?;

    loaded_emulator.take_rom_from(emulator);
    *emulator = loaded_emulator;
    emulator.reload_config(config);

    Ok(())
}
