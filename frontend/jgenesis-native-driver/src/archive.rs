use crate::extensions;
use std::fs::File;
use std::io;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;
use zip::ZipArchive;
use zip::result::ZipError;

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
    #[error("File '{file_name}' not found in archive '{path}'")]
    FileNotFound { path: String, file_name: String },
    #[error("No files with supported extensions found in archive '{path}'")]
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

    fn file_not_found(path: &Path, file_name: &str) -> Self {
        Self::FileNotFound { path: path.display().to_string(), file_name: file_name.into() }
    }
}

#[derive(Debug, Clone)]
pub struct ZipEntryMetadata {
    pub file_name: String,
    pub extension: String,
    pub size: u64,
}

fn extension_matches(file_name: &str, target_extension: &str) -> bool {
    extensions::from_path(file_name).is_some_and(|file_ext| file_ext.as_str() == target_extension)
}

#[derive(Debug, Clone, Copy)]
pub struct ArchiveEntry<'a> {
    pub file_name: &'a str,
    pub size: u64,
}

/// List all files in a .zip archive.
///
/// The given callback will be called for every file entry unless there is an error.
///
/// # Errors
///
/// Propagates any I/O or decoding errors.
pub fn list_files_zip(
    zip_path: &Path,
    mut callback: impl FnMut(ArchiveEntry<'_>),
) -> Result<(), ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(zip_path, source);
    let zip_err_fn = |source| ArchiveError::zip(zip_path, source);

    let file = File::open(zip_path).map_err(io_err_fn)?;
    let reader = BufReader::new(file);
    let mut archive = ZipArchive::new(reader).map_err(zip_err_fn)?;

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(zip_err_fn)?;
        callback(ArchiveEntry { file_name: entry.name(), size: entry.size() });
    }

    Ok(())
}

/// List all files in a .7z archive.
///
/// The given callback will be called for every file entry unless there is an error.
///
/// # Errors
///
/// Propagates any I/O or decoding errors.
pub fn list_files_7z(
    sevenz_path: &Path,
    mut callback: impl FnMut(ArchiveEntry<'_>),
) -> Result<(), ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(sevenz_path, source);
    let sevenz_err_fn = |source| ArchiveError::sevenz(sevenz_path, source);

    let file = File::open(sevenz_path).map_err(io_err_fn)?;
    let file_len = file.metadata().map_err(io_err_fn)?.len();
    let mut reader = BufReader::new(file);
    let archive = sevenz_rust::Archive::read(&mut reader, file_len, &[]).map_err(sevenz_err_fn)?;

    for entry in &archive.files {
        if !entry.has_stream {
            // Not a readable file (e.g. is a directory)
            continue;
        }

        callback(ArchiveEntry { file_name: entry.name.as_str(), size: entry.size });
    }

    Ok(())
}

/// Read a file from within a .zip archive.
///
/// # Errors
///
/// Propagates any I/O or decoding errors.
///
/// Will return an error if the archive does not contain the specified file.
pub fn read_file_zip(zip_path: &Path, file_name: &str) -> Result<Vec<u8>, ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(zip_path, source);
    let zip_err_fn = |source| ArchiveError::zip(zip_path, source);

    let file = File::open(zip_path).map_err(io_err_fn)?;
    let reader = BufReader::new(file);
    let mut archive = ZipArchive::new(reader).map_err(zip_err_fn)?;

    let mut entry = archive.by_name(file_name).map_err(zip_err_fn)?;

    let mut buffer = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buffer).map_err(io_err_fn)?;

    Ok(buffer)
}

/// Read a file from within a .7z archive.
///
/// # Errors
///
/// Propagates any I/O or decoding errors.
///
/// Will return an error if the archive does not contain the specified file.
pub fn read_file_7z(sevenz_path: &Path, file_name: &str) -> Result<Vec<u8>, ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(sevenz_path, source);
    let sevenz_err_fn = |source| ArchiveError::sevenz(sevenz_path, source);

    let file = File::open(sevenz_path).map_err(io_err_fn)?;
    let file_len = file.metadata().map_err(io_err_fn)?.len();
    let mut reader = BufReader::new(file);
    let archive = sevenz_rust::Archive::read(&mut reader, file_len, &[]).map_err(sevenz_err_fn)?;

    for folder_idx in 0..archive.folders.len() {
        let decoder = sevenz_rust::BlockDecoder::new(folder_idx, &archive, &[], &mut reader);

        if !decoder.entries().iter().any(|entry| entry.name.as_str() == file_name) {
            continue;
        }

        let mut buffer = Vec::new();
        let mut found = false;
        decoder
            .for_each_entries(&mut |entry, reader| {
                if entry.name.as_str() == file_name {
                    reader.read_to_end(&mut buffer)?;
                    found = true;
                    Ok(false)
                } else {
                    io::copy(reader, &mut io::sink())?;
                    Ok(true)
                }
            })
            .map_err(sevenz_err_fn)?;

        if found {
            return Ok(buffer);
        }
    }

    Err(ArchiveError::file_not_found(sevenz_path, file_name))
}

/// Returns metadata of the first file in the .zip archive that has a supported extension, or
/// None if there are no files with a supported extension.
///
/// Will also return None if the archive contains any .cue files, under the assumption that the
/// archive contains a CD-ROM image.
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

    if archive.file_names().any(|file_name| extension_matches(file_name, "cue")) {
        // Archive contains a .cue file; assume it's a CD-ROM image
        return Ok(None);
    }

    let mut first_file_name_with_ext: Option<(String, String)> = None;
    for file_name in archive.file_names() {
        let Some(extension) = extensions::from_path(file_name) else {
            continue;
        };

        if supported_extensions.contains(&extension.as_str()) {
            first_file_name_with_ext = Some((file_name.into(), extension));
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
