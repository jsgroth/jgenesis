use crate::mainloop::bincode_config;
use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SaveWriteError {
    #[error("Error writing save file to '{path}': {source}")]
    OpenFile {
        path: String,
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

pub struct FsSaveWriter {
    base_path: PathBuf,
    extension_to_path: HashMap<String, PathBuf>,
}

impl FsSaveWriter {
    pub fn new(path: PathBuf) -> Self {
        Self { base_path: path, extension_to_path: HashMap::new() }
    }

    fn open_file(
        &mut self,
        extension: &str,
        options: &mut OpenOptions,
    ) -> Result<(File, &PathBuf), SaveWriteError> {
        if !self.extension_to_path.contains_key(extension) {
            let path = self.base_path.with_extension(extension);
            self.extension_to_path.insert(extension.into(), path);
        }

        let path = &self.extension_to_path[extension];

        let file = options.open(path).map_err(|source| SaveWriteError::OpenFile {
            path: path.display().to_string(),
            source,
        })?;

        Ok((file, path))
    }
}

macro_rules! file_read_options {
    () => {
        File::options().read(true)
    };
}

macro_rules! file_write_options {
    () => {
        File::options().write(true).create(true).truncate(true)
    };
}

impl SaveWriter for FsSaveWriter {
    type Err = SaveWriteError;

    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err> {
        let (file, path) = self.open_file(extension, file_read_options!())?;
        let mut reader = BufReader::new(file);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(|source| SaveWriteError::OpenFile {
            path: path.display().to_string(),
            source,
        })?;

        Ok(bytes)
    }

    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err> {
        let (file, path) = self.open_file(extension, file_write_options!())?;
        let mut writer = BufWriter::new(file);
        writer.write_all(bytes).map_err(|source| SaveWriteError::OpenFile {
            path: path.display().to_string(),
            source,
        })?;

        Ok(())
    }

    fn load_serialized<D: Decode>(&mut self, extension: &str) -> Result<D, Self::Err> {
        let (file, path) = self.open_file(extension, file_read_options!())?;
        let mut reader = BufReader::new(file);
        bincode::decode_from_std_read(&mut reader, bincode_config!())
            .map_err(|source| SaveWriteError::Decode { path: path.display().to_string(), source })
    }

    fn persist_serialized<E: Encode>(&mut self, extension: &str, data: E) -> Result<(), Self::Err> {
        let (file, path) = self.open_file(extension, file_write_options!())?;
        let mut writer = BufWriter::new(file);
        bincode::encode_into_std_write(data, &mut writer, bincode_config!()).map_err(|source| {
            SaveWriteError::Encode { path: path.display().to_string(), source }
        })?;

        Ok(())
    }
}
