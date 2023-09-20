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

    pub fn send_command(&mut self, command_buffer: [u16; 5]) {
        todo!("CDD send command")
    }

    pub fn get_status(&self) -> [u16; 5] {
        todo!("CDD get status")
    }
}
