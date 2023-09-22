use crate::cdrom::reader::CdRom;
use bincode::{Decode, Encode};

const BUFFER_RAM_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdController {
    disc: Option<CdRom>,
    buffer_ram: Box<[u8; BUFFER_RAM_LEN]>,
}

impl CdController {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self { disc, buffer_ram: vec![0; BUFFER_RAM_LEN].into_boxed_slice().try_into().unwrap() }
    }
}
