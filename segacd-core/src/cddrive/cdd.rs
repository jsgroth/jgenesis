use crate::cdrom::reader::CdRom;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdDrive {
    disc: Option<CdRom>,
}

impl CdDrive {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self { disc }
    }
}
