use std::error::Error;
use std::fmt::{Display, Formatter};
use std::iter;

#[derive(Debug, Clone, Copy)]
pub enum IpsError {
    FileTooSmall,
    InvalidHeader,
    InvalidFooter,
    IncompleteRecord,
}

impl Display for IpsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileTooSmall => write!(f, "file is too small to be a valid IPS patch"),
            Self::InvalidHeader => write!(f, "IPS patch has invalid header"),
            Self::InvalidFooter => write!(f, "IPS patch has invalid footer"),
            Self::IncompleteRecord => write!(f, "IPS patch ends with an incomplete patch record"),
        }
    }
}

impl Error for IpsError {}

pub type IpsResult<T> = Result<T, IpsError>;

struct IpsReader<'a> {
    ips: &'a [u8],
    offset: usize,
}

impl IpsReader<'_> {
    fn read_u8(&mut self) -> IpsResult<u8> {
        if self.at_eof() {
            return Err(IpsError::IncompleteRecord);
        }

        let value = self.ips[self.offset];
        self.offset += 1;
        Ok(value)
    }

    fn read_u16(&mut self) -> IpsResult<usize> {
        let high = self.read_u8()?;
        let low = self.read_u8()?;
        Ok(u16::from_le_bytes([low, high]).into())
    }

    fn read_u24(&mut self) -> IpsResult<usize> {
        let high = self.read_u8()?;
        let mid = self.read_u8()?;
        let low = self.read_u8()?;
        Ok(u32::from_le_bytes([low, mid, high, 0]) as usize)
    }

    fn at_eof(&self) -> bool {
        // IPS footer is 3 bytes ("EOF")
        self.offset >= self.ips.len() - 3
    }
}

/// Apply an IPS patch to binary data.
///
/// # Errors
///
/// Returns an error if the IPS patch does not match the expected binary format.
pub fn apply_ips(rom: &[u8], ips: &[u8]) -> IpsResult<Vec<u8>> {
    // Every IPS file must have at least 8 bytes (5-byte header + 3-byte footer)
    if ips.len() < 8 {
        return Err(IpsError::FileTooSmall);
    }

    if &ips[..5] != b"PATCH" {
        return Err(IpsError::InvalidHeader);
    }

    if &ips[ips.len() - 3..] != b"EOF" {
        return Err(IpsError::InvalidFooter);
    }

    let mut patched = rom.to_vec();
    let mut reader = IpsReader { ips, offset: 5 };

    while !reader.at_eof() {
        let offset = reader.read_u24()?;
        let length = reader.read_u16()?;

        if length != 0 {
            // Normal record
            ensure_capacity(&mut patched, offset + length);

            for value in &mut patched[offset..offset + length] {
                *value = reader.read_u8()?;
            }
        } else {
            // RLE record
            let rle_length = reader.read_u16()?;
            let rle_byte = reader.read_u8()?;

            ensure_capacity(&mut patched, offset + rle_length);

            patched[offset..offset + rle_length].fill(rle_byte);
        }
    }

    Ok(patched)
}

fn ensure_capacity(patched: &mut Vec<u8>, len: usize) {
    if patched.len() < len {
        patched.extend(iter::repeat_n(0, len - patched.len()));
    }
}
