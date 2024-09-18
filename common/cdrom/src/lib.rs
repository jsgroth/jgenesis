pub mod cdtime;
pub mod cue;
pub mod reader;

use std::io;
use thiserror::Error;

// Data: 16 header bytes + 2048 data bytes + 288 error detection/correction bytes
// Audio: 1176 signed 16-bit PCM samples, half for the left channel and half for the right channel
pub const BYTES_PER_SECTOR: u64 = 2352;

#[derive(Debug, Error)]
pub enum CdRomError {
    #[error("Unable to determine parent directory of CUE file '{0}'")]
    CueParentDir(String),
    #[error("Error parsing CUE file: {0}")]
    CueParse(String),
    #[error("Invalid/unsupported FILE line in CUE file: {0}")]
    CueInvalidFileLine(String),
    #[error("Invalid/unsupported TRACK line in CUE file: {0}")]
    CueInvalidTrackLine(String),
    #[error("Invalid/unsupported INDEX line in CUE file: {0}")]
    CueInvalidIndexLine(String),
    #[error("Invalid/unsupported PREGAP line in CUE file: {0}")]
    CueInvalidPregapLine(String),
    #[error("Unable to get file metadata for file '{path}': {source}")]
    FsMetadata {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error opening CUE file '{path}': {source}")]
    CueOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error opening BIN file '{path}': {source}")]
    BinOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("CHD-related error: {0}")]
    ChdError(#[from] chd::Error),
    #[error("Error opening CHD file '{path}': {source}")]
    ChdOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Unable to parse CD-ROM metadata in CHD header: '{metadata_value}'")]
    ChdHeaderParseError { metadata_value: String },
    #[error("CHD header contains an invalid CD-ROM track list: {track_numbers:?}")]
    ChdInvalidTrackList { track_numbers: Vec<u8> },
    #[error("I/O error reading from disc: {0}")]
    DiscReadIo(#[source] io::Error),
    #[error(
        "CD-ROM error detection check failed for track {track_number} sector {sector_number}; expected={expected:08X}, actual={actual:08X}"
    )]
    DiscReadInvalidChecksum { track_number: u8, sector_number: u32, expected: u32, actual: u32 },
}

pub type CdRomResult<T> = Result<T, CdRomError>;
