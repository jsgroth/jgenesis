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

    fn no_supported_files(path: &Path) -> Self {
        Self::NoSupportedFiles { path: path.display().to_string() }
    }
}

/// Returns the file name of the first file in the .zip archive that has a supported extension, or
/// None if there are no files with a supported extension.
///
/// # Errors
///
/// Propagates any I/O or ZIP errors.
pub fn first_supported_file_in_zip(
    zip_path: &Path,
    supported_extensions: &[&str],
) -> Result<Option<String>, ArchiveError> {
    let io_err_fn = |source| ArchiveError::io(zip_path, source);
    let zip_err_fn = |source| ArchiveError::zip(zip_path, source);

    let file = File::open(zip_path).map_err(io_err_fn)?;
    let reader = BufReader::new(file);
    let archive = ZipArchive::new(reader).map_err(zip_err_fn)?;

    for file_name in archive.file_names() {
        let Some(extension) = Path::new(&file_name).extension().and_then(OsStr::to_str) else {
            continue;
        };

        if supported_extensions.contains(&extension) {
            return Ok(Some(file_name.into()));
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
pub fn read_first_file_in_zip(
    zip_path: &Path,
    supported_extensions: &[&str],
) -> Result<Vec<u8>, ArchiveError> {
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
            return Ok(contents);
        }
    }

    Err(ArchiveError::no_supported_files(zip_path))
}
