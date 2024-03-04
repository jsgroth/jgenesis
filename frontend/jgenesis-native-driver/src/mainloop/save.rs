use crate::mainloop::bincode_config;
use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::{fs, io};
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

    fn load_serialized<D: Decode>(&mut self, extension: &str) -> Result<D, Self::Err> {
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
