use crate::NativeEmulatorResult;
use crate::mainloop::{NativeEmulatorError, bincode_config};
use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_native_config::common::SavePath;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
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

struct SavePaths {
    path: PathBuf,
    temp_path: PathBuf,
}

pub struct FsSaveWriter {
    base_path: PathBuf,
    extension_to_paths: HashMap<String, SavePaths>,
}

impl FsSaveWriter {
    pub fn new(path: PathBuf) -> Self {
        Self { base_path: path, extension_to_paths: HashMap::new() }
    }

    pub fn update_path(&mut self, path: PathBuf) {
        if path == self.base_path {
            return;
        }

        self.base_path = path;
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

    fn read_file<D, F>(&mut self, extension: &str, read_fn: F) -> Result<D, SaveWriteError>
    where
        F: FnOnce(BufReader<File>, &PathBuf) -> Result<D, SaveWriteError>,
    {
        let SavePaths { path, .. } = self.get_or_insert_paths(extension);

        let file = File::open(path).map_err(|source| SaveWriteError::OpenFile {
            path: path.display().to_string(),
            source,
        })?;
        let reader = BufReader::new(file);
        read_fn(reader, path)
    }

    fn write_file<F>(&mut self, extension: &str, write_fn: F) -> Result<(), SaveWriteError>
    where
        F: FnOnce(BufWriter<File>, &PathBuf) -> Result<(), SaveWriteError>,
    {
        let SavePaths { path, temp_path } = self.get_or_insert_paths(extension);

        {
            let temp_file =
                File::options().write(true).create(true).truncate(true).open(temp_path).map_err(
                    |source| SaveWriteError::OpenFile {
                        path: temp_path.display().to_string(),
                        source,
                    },
                )?;
            let temp_writer = BufWriter::new(temp_file);
            write_fn(temp_writer, temp_path)?;
        }

        fs::rename(temp_path, path).map_err(|source| SaveWriteError::RenameFile {
            path: path.display().to_string(),
            temp_path: temp_path.display().to_string(),
            source,
        })?;

        Ok(())
    }
}

impl SaveWriter for FsSaveWriter {
    type Err = SaveWriteError;

    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err> {
        self.read_file(extension, |mut reader, path| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map_err(|source| SaveWriteError::ReadFile {
                path: path.display().to_string(),
                source,
            })?;
            Ok(bytes)
        })
    }

    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err> {
        self.write_file(extension, |mut writer, path| {
            writer.write_all(bytes).map_err(|source| SaveWriteError::WriteFile {
                path: path.display().to_string(),
                source,
            })
        })
    }

    fn load_serialized<D: Decode<()>>(&mut self, extension: &str) -> Result<D, Self::Err> {
        self.read_file(extension, |mut reader, path| {
            bincode::decode_from_std_read(&mut reader, bincode_config!()).map_err(|source| {
                SaveWriteError::Decode { path: path.display().to_string(), source }
            })
        })
    }

    fn persist_serialized<E: Encode>(&mut self, extension: &str, data: E) -> Result<(), Self::Err> {
        self.write_file(extension, |mut writer, path| {
            bincode::encode_into_std_write(data, &mut writer, bincode_config!()).map_err(
                |source| SaveWriteError::Encode { path: path.display().to_string(), source },
            )?;

            Ok(())
        })
    }
}

fn determine_path(
    path: &SavePath,
    rom_path: &Path,
    rom_extension: &str,
    save_extension: &str,
    save_subdir: &str,
) -> NativeEmulatorResult<PathBuf> {
    let base_dir = match path {
        SavePath::RomFolder => {
            // Return early because this is a path directly to the save file, not the parent directory
            return Ok(rom_path.with_extension(save_extension));
        }
        SavePath::EmulatorFolder => {
            let base_dir = if jgenesis_common::is_appimage_build() {
                // When running in an AppImage, CWD does not return the location of the AppImage
                // file; read the OWD var instead (Original Working Directory)
                env::var("OWD").map_or_else(|err| {
                    log::error!("Unable to determine AppImage working directory, defaulting to current directory: {err}");
                    current_dir_with_fallback()
                }, PathBuf::from)
            } else {
                determine_current_exe_parent()
            };

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

    Ok(base_dir.join(Path::new(rom_file_name).with_extension(save_extension)))
}

fn determine_current_exe_parent() -> PathBuf {
    let current_exe = match env::current_exe() {
        Ok(current_exe) => current_exe,
        Err(err) => {
            log::error!(
                "Unable to detemine current executable, falling back to current directory: {err}"
            );
            return current_dir_with_fallback();
        }
    };

    let Some(parent) = current_exe.parent() else {
        log::error!(
            "Unable to determine parent directory of current executable '{}', falling back to current directory",
            current_exe.display()
        );
        return current_dir_with_fallback();
    };

    parent.into()
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
    let save_path =
        determine_path(base_save_path, rom_path, rom_extension, "sav", SavePath::SAVE_SUBDIR)?;
    let save_state_path =
        determine_path(base_state_path, rom_path, rom_extension, "jst", SavePath::STATE_SUBDIR)?;

    log::info!("Save file path: '{}'", save_path.display());
    log::info!("Base save state path: '{}'", save_state_path.display());

    Ok(DeterminedPaths { save_path, save_state_path })
}
