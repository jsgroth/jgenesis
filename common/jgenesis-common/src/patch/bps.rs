use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy)]
pub enum BpsError {
    FileTooSmall,
    InvalidHeader,
    IncompleteRecord,
    SourceSizeMismatch { expected: usize, actual: usize },
    TargetSizeMismatch { expected: usize, actual: usize },
    RomOutOfBounds,
}

impl Display for BpsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BpsError::FileTooSmall => write!(f, "file is too small to be a valid BPS patch"),
            BpsError::InvalidHeader => write!(f, "BPS patch has invalid header"),
            BpsError::IncompleteRecord => {
                write!(f, "BPS patch ends with an incomplete patch record")
            }
            BpsError::SourceSizeMismatch { expected, actual } => write!(
                f,
                "source ROM does not match source size in BPS patch; expected {expected} bytes, actual {actual} bytes"
            ),
            BpsError::TargetSizeMismatch { expected, actual } => write!(
                f,
                "patched ROM does not match target size in BPS patch; expected {expected} bytes, actual {actual} bytes"
            ),
            BpsError::RomOutOfBounds => {
                write!(f, "BPS patch contained an out-of-bounds ROM access")
            }
        }
    }
}

impl Error for BpsError {}

pub type BpsResult<T> = Result<T, BpsError>;

struct BpsReader<'a> {
    bps: &'a [u8],
    offset: usize,
}

impl BpsReader<'_> {
    fn read_u8(&mut self) -> BpsResult<u8> {
        if self.at_eof() {
            return Err(BpsError::IncompleteRecord);
        }

        let value = self.bps[self.offset];
        self.offset += 1;
        Ok(value)
    }

    fn read_varint(&mut self) -> BpsResult<usize> {
        let mut value = 0;
        let mut shift = 0;

        loop {
            let byte = self.read_u8()?;
            value += usize::from(byte & 0x7F) << shift;

            if byte & 0x80 != 0 {
                return Ok(value);
            }

            shift += 7;
            value += 1 << shift;
        }
    }

    fn read_relative_offset(&mut self) -> BpsResult<isize> {
        let data = self.read_varint()?;
        let sign = data & 1 != 0;
        let magnitude = (data >> 1) as isize;
        Ok(if sign { -magnitude } else { magnitude })
    }

    fn at_eof(&self) -> bool {
        // BPS footer is 12 bytes (3x 4-byte checksums)
        self.offset >= self.bps.len() - 12
    }
}

/// Apply a BPS patch to binary data.
///
/// # Errors
///
/// Returns an error if the BPS patch does not match the expected binary format, or if it contains
/// commands that are invalid given the input data (e.g. an out-of-bounds read).
pub fn apply_bps(rom: &[u8], bps: &[u8]) -> BpsResult<Vec<u8>> {
    // Every BPS file must have at least 19 bytes
    // 4-byte ID string, source+target+metadata sizes (at least 1 byte each), source+target+patch
    // checksums (4 bytes each)
    if bps.len() < 19 {
        return Err(BpsError::FileTooSmall);
    }

    if &bps[..4] != b"BPS1" {
        return Err(BpsError::InvalidHeader);
    }

    let mut reader = BpsReader { bps, offset: 4 };

    let source_size = reader.read_varint()?;
    let target_size = reader.read_varint()?;
    let metadata_size = reader.read_varint()?;

    if source_size != rom.len() {
        return Err(BpsError::SourceSizeMismatch { expected: source_size, actual: rom.len() });
    }

    // metadata isn't needed for patching, so skip over it if it exists
    reader.offset += metadata_size;

    let mut patched = Vec::with_capacity(target_size);
    let mut source_relative_offset = 0;
    let mut target_relative_offset = 0;

    while !reader.at_eof() {
        let data = reader.read_varint()?;
        let length = (data >> 2) + 1;

        match data & 3 {
            0 => {
                // SourceRead
                for _ in 0..length {
                    let byte = get_byte(rom, patched.len() as isize)?;
                    patched.push(byte);
                }
            }
            1 => {
                // TargetRead
                for _ in 0..length {
                    let byte = reader.read_u8()?;
                    patched.push(byte);
                }
            }
            2 => {
                // SourceCopy
                source_relative_offset += reader.read_relative_offset()?;

                for _ in 0..length {
                    let byte = get_byte(rom, source_relative_offset)?;
                    patched.push(byte);
                    source_relative_offset += 1;
                }
            }
            3 => {
                // TargetCopy
                target_relative_offset += reader.read_relative_offset()?;

                for _ in 0..length {
                    let byte = get_byte(&patched, target_relative_offset)?;
                    patched.push(byte);
                    target_relative_offset += 1;
                }
            }
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    if target_size != patched.len() {
        return Err(BpsError::TargetSizeMismatch { expected: target_size, actual: patched.len() });
    }

    Ok(patched)
}

fn get_byte(rom: &[u8], i: isize) -> BpsResult<u8> {
    if i < 0 {
        return Err(BpsError::RomOutOfBounds);
    }

    rom.get(i as usize).copied().ok_or(BpsError::RomOutOfBounds)
}
