use crate::config::RomReadResult;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;
use zip::result::ZipError;
use zip::ZipArchive;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("I/O error reading archive file '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error reading .zip archive '{path}': {source}")]
    Zip {
        path: String,
        #[source]
        source: ZipError,
    },
    #[error("Error reading .7z archive '{path}': {source}")]
    SevenZ {
        path: String,
        #[source]
        source: sevenz_rust::Error,
    },
    #[error("No supported files found in .zip archive '{path}'")]
    NoSupportedFiles { path: String },
}

impl ArchiveError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io { path: path.display().to_string(), source }
    }

    fn zip(path: &Path, source: ZipError) -> Self {
        Self::Zip { path: path.display().to_string(), source }
    }

    fn sevenz(path: &Path, source: sevenz_rust::Error) -> Self {
        Self::SevenZ { path: path.display().to_string(), source }
    }

    fn no_supported_files(path: &Path) -> Self {
        Self::NoSupportedFiles { path: path.display().to_string() }
    }
}

#[derive(Debug, Clone)]
pub struct ZipEntryMetadata {
    pub file_name: String,
    pub extension: String,
    pub size: u64,
}

/// Returns metadata of the first file in the .zip archive that has a supported extension, or
/// None if there are no files with a supported extension.
///
/// # Errors
///
/// Propagates any I/O or ZIP errors.
pub fn first_supported_file_in_zip(
    zip_path: &Path,
    supported_extensions: &[&str],
) -> Result<Option<ZipEntryMetadata>, ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(zip_path, source);
    let zip_err_fn = |source| ArchiveError::zip(zip_path, source);

    let file = File::open(zip_path).map_err(io_err_fn)?;
    let reader = BufReader::new(file);
    let mut archive = ZipArchive::new(reader).map_err(zip_err_fn)?;

    let mut first_file_name_with_ext: Option<(String, String)> = None;
    for file_name in archive.file_names() {
        let Some(extension) = Path::new(&file_name).extension().and_then(OsStr::to_str) else {
            continue;
        };

        if supported_extensions.contains(&extension) {
            first_file_name_with_ext = Some((file_name.into(), extension.into()));
            break;
        }
    }

    let Some((file_name, extension)) = first_file_name_with_ext else {
        return Ok(None);
    };

    let file = archive.by_name(&file_name).map_err(zip_err_fn)?;
    let size = file.size();

    Ok(Some(ZipEntryMetadata { file_name, extension, size }))
}

/// Returns metadata of the first file in the .7z archive that has a supported extension, or
/// None if there are no files with a supported extension.
///
/// # Errors
///
/// Will propagate any I/O or 7ZIP errors.
pub fn first_supported_file_in_7z(
    sevenz_path: &Path,
    supported_extensions: &[&str],
) -> Result<Option<ZipEntryMetadata>, ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(sevenz_path, source);
    let sevenz_err_fn = |source| ArchiveError::sevenz(sevenz_path, source);

    let file = File::open(sevenz_path).map_err(io_err_fn)?;
    let file_len = file.metadata().map_err(io_err_fn)?.len();
    let mut reader = BufReader::new(file);
    let archive = sevenz_rust::Archive::read(&mut reader, file_len, &[]).map_err(sevenz_err_fn)?;

    for folder_idx in 0..archive.folders.len() {
        let folder_dec = sevenz_rust::BlockDecoder::new(folder_idx, &archive, &[], &mut reader);

        for entry in folder_dec.entries() {
            if !entry.has_stream {
                // Is a directory
                continue;
            }

            for &extension in supported_extensions {
                if entry.name.ends_with(extension) {
                    return Ok(Some(ZipEntryMetadata {
                        file_name: entry.name.clone(),
                        extension: extension.to_string(),
                        size: entry.size,
                    }));
                }
            }
        }
    }

    Ok(None)
}

/// Opens and reads the first file in the .zip archive that has a supported extension.
///
/// # Errors
///
/// Propagates any I/O or ZIP errors, and will also return an error if the .zip archive contains
/// no files with a supported extension.
pub(crate) fn read_first_file_in_zip(
    zip_path: &Path,
    supported_extensions: &[&str],
) -> Result<RomReadResult, ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(zip_path, source);
    let zip_err_fn = |source| ArchiveError::zip(zip_path, source);

    let file = File::open(zip_path).map_err(io_err_fn)?;
    let reader = BufReader::new(file);
    let mut archive = ZipArchive::new(reader).map_err(zip_err_fn)?;

    let file_names: Vec<_> = archive.file_names().map(String::from).collect();
    for file_name in file_names {
        let Some(extension) = Path::new(&file_name).extension().and_then(OsStr::to_str) else {
            continue;
        };

        if supported_extensions.contains(&extension) {
            let mut zip_file = archive.by_name(&file_name).map_err(zip_err_fn)?;

            let mut contents = Vec::with_capacity(zip_file.size() as usize);
            zip_file.read_to_end(&mut contents).map_err(io_err_fn)?;

            return Ok(RomReadResult { rom: contents, extension: extension.into() });
        }
    }

    Err(ArchiveError::no_supported_files(zip_path))
}

pub(crate) fn read_first_file_in_7z(
    sevenz_path: &Path,
    supported_extensions: &[&str],
) -> Result<RomReadResult, ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(sevenz_path, source);
    let sevenz_err_fn = |source| ArchiveError::sevenz(sevenz_path, source);

    let file = File::open(sevenz_path).map_err(io_err_fn)?;
    let file_len = file.metadata().map_err(io_err_fn)?.len();
    let mut reader = BufReader::new(file);
    let archive = sevenz_rust::Archive::read(&mut reader, file_len, &[]).map_err(sevenz_err_fn)?;

    for folder_idx in 0..archive.folders.len() {
        let folder_dec = sevenz_rust::BlockDecoder::new(folder_idx, &archive, &[], &mut reader);

        let Some((file_name, extension)) = folder_dec.entries().iter().find_map(|entry| {
            if !entry.has_stream {
                return None;
            }

            for &extension in supported_extensions {
                if entry.name.ends_with(&format!(".{extension}")) {
                    return Some((entry.name.clone(), extension.to_string()));
                }
            }

            None
        }) else {
            continue;
        };

        let mut decompressed = Vec::new();
        folder_dec
            .for_each_entries(&mut |entry, reader| {
                if entry.name == file_name {
                    reader.read_to_end(&mut decompressed)?;
                    return Ok(false);
                }

                io::copy(reader, &mut io::sink())?;
                Ok(true)
            })
            .map_err(sevenz_err_fn)?;

        return Ok(RomReadResult { rom: decompressed, extension });
    }

    Err(ArchiveError::no_supported_files(sevenz_path))
}
