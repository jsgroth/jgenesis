use crate::NativeEmulatorResult;
use crate::mainloop::{NativeEmulatorError, bincode_config};
use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::SavePath;
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use std::{env, fs, io};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SaveWriteError {
    #[error("Error writing save file to '{path}': {source}")]
    OpenFile {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error reading save bytes from '{path}': {source}")]
    ReadFile {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error writing save bytes to '{path}': {source}")]
    WriteFile {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Failed to rename temp file '{temp_path}' to '{path}': {source}")]
    RenameFile {
        path: String,
        temp_path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error serializing save data to '{path}': {source}")]
    Encode {
        path: String,
        #[source]
        source: EncodeError,
    },
    #[error("Error deserializing save data from '{path}': {source}")]
    Decode {
        path: String,
        #[source]
        source: DecodeError,
    },
}

enum ReadFileError {
    Io(io::Error),
    Decode(DecodeError),
}

impl ReadFileError {
    fn into_save_write_err(self, path: &Path) -> SaveWriteError {
        let path = path.display().to_string();

        match self {
            Self::Io(source) => SaveWriteError::ReadFile { path, source },
            Self::Decode(source) => SaveWriteError::Decode { path, source },
        }
    }
}

enum WriteFileError {
    Io(io::Error),
    Encode(EncodeError),
}

impl WriteFileError {
    fn into_save_write_err(self, path: &Path) -> SaveWriteError {
        let path = path.display().to_string();

        match self {
            Self::Io(source) => SaveWriteError::WriteFile { path, source },
            Self::Encode(source) => SaveWriteError::Encode { path, source },
        }
    }
}

struct SavePaths {
    path: PathBuf,
    temp_path: PathBuf,
}

struct SavePathsMap {
    base_path: PathBuf,
    extension_to_paths: HashMap<String, SavePaths>,
}

impl SavePathsMap {
    fn new(base_path: PathBuf) -> Self {
        Self { base_path, extension_to_paths: HashMap::new() }
    }

    fn update_base_path(&mut self, base_path: PathBuf) {
        self.base_path = base_path;
        self.extension_to_paths.clear();
    }

    fn get_or_insert_paths(&mut self, extension: &str) -> &SavePaths {
        // Double get necessary to avoid borrow checker issues related to returning a reference
        if !self.extension_to_paths.contains_key(extension) {
            let path = self.base_path.with_extension(extension);
            let temp_path = self.base_path.with_extension(format!("{extension}.tmp"));
            self.extension_to_paths.insert(extension.into(), SavePaths { path, temp_path });
        }

        &self.extension_to_paths[extension]
    }
}

pub struct FsSaveWriter {
    save_paths: SavePathsMap,
    buffered_writes: HashMap<String, Vec<u8>>,
    last_write_times: HashMap<String, Instant>,
    free_buffers: Vec<Vec<u8>>,
}

impl FsSaveWriter {
    const FLUSH_OLD_THRESHOLD: Duration = Duration::from_secs(60);

    pub fn new(path: PathBuf) -> Self {
        Self {
            save_paths: SavePathsMap::new(path),
            buffered_writes: HashMap::new(),
            last_write_times: HashMap::new(),
            free_buffers: Vec::new(),
        }
    }

    pub fn update_path(&mut self, path: PathBuf) -> Result<(), SaveWriteError> {
        if path == self.save_paths.base_path {
            return Ok(());
        }

        self.flush_all_writes()?;
        self.save_paths.update_base_path(path);

        Ok(())
    }

    /// Flush any buffered writes to files that haven't been written to disk in 60 seconds or more
    pub fn flush_old_writes(&mut self) -> Result<(), SaveWriteError> {
        self.flush_writes(Instant::now(), Some(Self::FLUSH_OLD_THRESHOLD))
    }

    /// Flush all buffered writes
    pub fn flush_all_writes(&mut self) -> Result<(), SaveWriteError> {
        self.flush_writes(Instant::now(), None)
    }

    fn flush_writes(
        &mut self,
        now: Instant,
        threshold: Option<Duration>,
    ) -> Result<(), SaveWriteError> {
        let time_threshold = threshold.map(|threshold| now - threshold);

        for (extension, last_write_time) in self.last_write_times.clone() {
            if time_threshold.is_none_or(|threshold| last_write_time <= threshold) {
                match self.buffered_writes.remove(&extension) {
                    Some(mut buffer) => {
                        self.flush_file(&extension, now, |writer| {
                            writer.write_all(&buffer).map_err(WriteFileError::Io)
                        })?;

                        buffer.clear();
                        self.free_buffers.push(buffer);
                    }
                    None => {
                        self.last_write_times.remove(&extension);
                    }
                }
            }
        }

        Ok(())
    }

    fn read_file<D, F>(&mut self, extension: &str, read_fn: F) -> Result<D, SaveWriteError>
    where
        F: FnOnce(&mut dyn Read) -> Result<D, ReadFileError>,
    {
        let SavePaths { path, .. } = self.save_paths.get_or_insert_paths(extension);

        if let Some(buffer) = self.buffered_writes.get(extension) {
            let mut cursor = Cursor::new(buffer);
            return read_fn(&mut cursor).map_err(|err| err.into_save_write_err(path));
        }

        let file = File::open(path).map_err(|source| SaveWriteError::OpenFile {
            path: path.display().to_string(),
            source,
        })?;
        let mut reader = BufReader::new(file);
        read_fn(&mut reader).map_err(|err| err.into_save_write_err(path))
    }

    fn write_file<F>(&mut self, extension: &str, write_fn: F) -> Result<(), SaveWriteError>
    where
        F: FnOnce(&mut dyn Write) -> Result<(), WriteFileError>,
    {
        let SavePaths { path, .. } = self.save_paths.get_or_insert_paths(extension);

        let now = Instant::now();
        if self
            .last_write_times
            .get(extension)
            .is_some_and(|&time| now < time + Self::FLUSH_OLD_THRESHOLD)
        {
            let mut buffer = self.free_buffers.pop().unwrap_or_default();
            write_fn(&mut buffer).map_err(|err| err.into_save_write_err(path))?;

            if let Some(mut old_buffer) = self.buffered_writes.insert(extension.into(), buffer) {
                old_buffer.clear();
                self.free_buffers.push(old_buffer);
            }

            return Ok(());
        }

        self.flush_file(extension, now, write_fn)
    }

    fn flush_file<F>(
        &mut self,
        extension: &str,
        now: Instant,
        write_fn: F,
    ) -> Result<(), SaveWriteError>
    where
        F: FnOnce(&mut dyn Write) -> Result<(), WriteFileError>,
    {
        let SavePaths { path, temp_path } = self.save_paths.get_or_insert_paths(extension);

        log::debug!("Flushing save data to path '{}'", path.display());

        {
            let temp_file = File::create(temp_path).map_err(|source| SaveWriteError::OpenFile {
                path: temp_path.display().to_string(),
                source,
            })?;
            let mut temp_writer = BufWriter::new(temp_file);

            write_fn(&mut temp_writer).map_err(|err| err.into_save_write_err(temp_path))?;
        }

        fs::rename(temp_path, path).map_err(|source| SaveWriteError::RenameFile {
            path: path.display().to_string(),
            temp_path: temp_path.display().to_string(),
            source,
        })?;

        self.last_write_times.insert(extension.into(), now);

        Ok(())
    }
}

impl SaveWriter for FsSaveWriter {
    type Err = SaveWriteError;

    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err> {
        self.read_file(extension, |reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map_err(ReadFileError::Io)?;
            Ok(bytes)
        })
    }

    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err> {
        self.write_file(extension, |writer| writer.write_all(bytes).map_err(WriteFileError::Io))
    }

    fn load_serialized<D: Decode<()>>(&mut self, extension: &str) -> Result<D, Self::Err> {
        self.read_file(extension, |mut reader| {
            bincode::decode_from_std_read(&mut reader, bincode_config!())
                .map_err(ReadFileError::Decode)
        })
    }

    fn persist_serialized<E: Encode>(&mut self, extension: &str, data: E) -> Result<(), Self::Err> {
        self.write_file(extension, |mut writer| {
            bincode::encode_into_std_write(data, &mut writer, bincode_config!())
                .map_err(WriteFileError::Encode)?;

            Ok(())
        })
    }
}

impl Drop for FsSaveWriter {
    fn drop(&mut self) {
        if let Err(err) = self.flush_all_writes() {
            log::error!("Error flushing save bytes: {err}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoveDiscFromPath {
    No,
    Yes,
}

fn determine_path(
    path: &SavePath,
    rom_path: &Path,
    rom_extension: &str,
    save_extension: &str,
    save_subdir: &str,
    remove_disc: RemoveDiscFromPath,
) -> NativeEmulatorResult<PathBuf> {
    let base_dir = match path {
        SavePath::RomFolder => {
            // Return early because this is a path directly to the save file, not the parent directory
            if remove_disc == RemoveDiscFromPath::Yes
                && let Some(file_name) = rom_path.file_name().and_then(OsStr::to_str)
            {
                let name_without_disc = strip_disc_from_file_name(file_name);
                return Ok(rom_path
                    .with_file_name(name_without_disc.as_ref())
                    .with_extension(save_extension));
            }
            return Ok(rom_path.with_extension(save_extension));
        }
        SavePath::EmulatorFolder => {
            let base_dir = jgenesis_common::determine_emulator_dir().unwrap_or_else(|| {
                log::error!(
                    "Unable to determine AppImage parent directory, defaulting to current directory"
                );
                current_dir_with_fallback()
            });

            base_dir.join(save_subdir).join(rom_extension)
        }
        SavePath::Custom(custom_path) => custom_path.join(rom_extension),
    };

    if !base_dir.exists() {
        fs::create_dir_all(&base_dir).map_err(|source| NativeEmulatorError::CreateSaveDir {
            path: base_dir.to_string_lossy().into(),
            source,
        })?;
    }

    let Some(rom_file_name) = rom_path.file_name().and_then(OsStr::to_str) else {
        return Err(NativeEmulatorError::ParseFileName(rom_path.to_string_lossy().into()));
    };

    let save_file_name = match remove_disc {
        RemoveDiscFromPath::No => rom_file_name.into(),
        RemoveDiscFromPath::Yes => strip_disc_from_file_name(rom_file_name),
    };

    Ok(base_dir.join(Path::new(save_file_name.as_ref()).with_extension(save_extension)))
}

fn strip_disc_from_file_name(file_name: &str) -> Cow<'_, str> {
    static DISC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" \(Disc \d\)").unwrap());

    DISC_RE.replace(file_name, "")
}

// Fall back to temp dir if getting the current directory fails
fn current_dir_with_fallback() -> PathBuf {
    env::current_dir().unwrap_or_else(|err| {
        log::error!(
            "Unable to determine current working directory, defaulting to temp directory: {err}"
        );
        env::temp_dir()
    })
}

pub struct DeterminedPaths {
    pub save_path: PathBuf,
    pub save_state_path: PathBuf,
}

pub fn determine_save_paths(
    base_save_path: &SavePath,
    base_state_path: &SavePath,
    rom_path: &Path,
    rom_extension: &str,
) -> NativeEmulatorResult<DeterminedPaths> {
    let save_path = determine_path(
        base_save_path,
        rom_path,
        rom_extension,
        "sav",
        SavePath::SAVE_SUBDIR,
        RemoveDiscFromPath::Yes,
    )?;
    let save_state_path = determine_path(
        base_state_path,
        rom_path,
        rom_extension,
        "jst",
        SavePath::STATE_SUBDIR,
        RemoveDiscFromPath::No,
    )?;

    log::info!("Save file path: '{}'", save_path.display());
    log::info!("Base save state path: '{}'", save_state_path.display());

    Ok(DeterminedPaths { save_path, save_state_path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
    struct DummyRtc {
        x: u32,
        y: u32,
        z: u32,
    }

    fn read_dummy_rtc(path: &Path) -> Result<DummyRtc, Box<dyn Error>> {
        let file = File::open(path)?;
        let rtc = bincode::decode_from_std_read(&mut BufReader::new(file), bincode_config!())?;
        Ok(rtc)
    }

    #[test]
    fn save_buffering() -> Result<(), Box<dyn Error>> {
        let temp_dir = env::temp_dir();
        let sav_file = temp_dir.join("test_save_buffering.sav");
        let rtc_file = temp_dir.join("test_save_buffering.rtc");

        let _ = fs::remove_file(&sav_file);
        let _ = fs::remove_file(&rtc_file);

        let mut writer = FsSaveWriter::new(sav_file.clone());

        // First writes should always go straight to disk
        writer.persist_bytes("sav", &[1, 2, 3, 4, 5, 6])?;
        writer.persist_serialized("rtc", DummyRtc { x: 7, y: 8, z: 9 })?;

        assert_eq!(fs::read(&sav_file)?, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(read_dummy_rtc(&rtc_file)?, DummyRtc { x: 7, y: 8, z: 9 });

        assert_eq!(writer.load_bytes("sav")?, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(writer.load_serialized::<DummyRtc>("rtc")?, DummyRtc { x: 7, y: 8, z: 9 });

        // Immediately following writes should not go straight to disk...
        writer.persist_bytes("sav", &[100, 101, 102, 103])?;
        writer.persist_serialized("rtc", DummyRtc { x: 9000, y: 10000, z: 11000 })?;

        assert_eq!(fs::read(&sav_file)?, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(read_dummy_rtc(&rtc_file)?, DummyRtc { x: 7, y: 8, z: 9 });

        // ...but the data should still be there if read through the FsSaveWriter
        assert_eq!(writer.load_bytes("sav")?, vec![100, 101, 102, 103]);
        assert_eq!(
            writer.load_serialized::<DummyRtc>("rtc")?,
            DummyRtc { x: 9000, y: 10000, z: 11000 }
        );

        // Changing buffered data should not go to disk (yet) but should affect FsSaveWriter reads
        writer.persist_bytes("sav", &[200, 201, 202, 203, 204])?;
        writer.persist_serialized("rtc", DummyRtc { x: 123, y: 456, z: 789 })?;

        assert_eq!(fs::read(&sav_file)?, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(read_dummy_rtc(&rtc_file)?, DummyRtc { x: 7, y: 8, z: 9 });

        assert_eq!(writer.load_bytes("sav")?, vec![200, 201, 202, 203, 204]);
        assert_eq!(writer.load_serialized::<DummyRtc>("rtc")?, DummyRtc { x: 123, y: 456, z: 789 });

        // Writer should flush buffered writes to disk when dropped
        drop(writer);
        assert_eq!(fs::read(&sav_file)?, vec![200, 201, 202, 203, 204]);
        assert_eq!(read_dummy_rtc(&rtc_file)?, DummyRtc { x: 123, y: 456, z: 789 });

        // Clean up
        let _ = fs::remove_file(sav_file);
        let _ = fs::remove_file(rtc_file);

        Ok(())
    }
}
