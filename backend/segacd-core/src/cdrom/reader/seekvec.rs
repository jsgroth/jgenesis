use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Deref;

#[derive(Debug, Clone)]
pub struct SeekableVec {
    bytes: Vec<u8>,
    address: u64,
}

impl Deref for SeekableVec {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

impl SeekableVec {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, address: 0 }
    }
}

impl Read for SeekableVec {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.address >= self.bytes.len() as u64 {
            return Ok(0);
        }

        let bytes_read = self.bytes[self.address as usize..].as_ref().read(buf)?;
        self.address += bytes_read as u64;
        Ok(bytes_read)
    }
}

impl Seek for SeekableVec {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::Start(address) => {
                self.address = address;
            }
            SeekFrom::End(offset) => {
                let address = self.bytes.len() as i64 + offset;
                if address < 0 {
                    return Err(new_seek_error(address));
                }
                self.address = address as u64;
            }
            SeekFrom::Current(offset) => {
                let address = self.address as i64 + offset;
                if address < 0 {
                    return Err(new_seek_error(address));
                }
                self.address = address as u64;
            }
        }

        Ok(self.address)
    }
}

fn new_seek_error(address: i64) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid seek address: {address}"))
}
