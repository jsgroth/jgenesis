pub mod cdd;

use crate::cddrive::cdd::CdDrive;
use bincode::{Decode, Encode};

const BUFFER_RAM_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdController {
    drive: CdDrive,
    buffer_ram: Vec<u8>,
}

impl CdController {
    pub fn new(drive: CdDrive) -> Self {
        Self { drive, buffer_ram: vec![0; BUFFER_RAM_LEN] }
    }

    pub fn cdd(&mut self) -> &mut CdDrive {
        &mut self.drive
    }
}
