use crate::cdrom::reader::CdRom;

#[derive(Debug, Clone)]
pub struct CdDrive {
    disc: Option<CdRom>,
}

impl CdDrive {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self { disc }
    }
}
