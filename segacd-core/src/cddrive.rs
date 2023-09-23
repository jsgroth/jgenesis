use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::reader::CdRom;
use bincode::{Decode, Encode};
use genesis_core::GenesisRegion;
use regex::Regex;
use std::sync::OnceLock;
use std::{array, io};

const BUFFER_RAM_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdController {
    disc: Option<CdRom>,
    buffer_ram: Box<[u8; BUFFER_RAM_LEN]>,
    sector_buffer: [u8; cdrom::BYTES_PER_SECTOR as usize],
}

impl CdController {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self {
            disc,
            buffer_ram: vec![0; BUFFER_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            sector_buffer: array::from_fn(|_| 0),
        }
    }

    pub fn disc_title(&mut self, region: GenesisRegion) -> io::Result<Option<String>> {
        static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

        let Some(disc) = &mut self.disc else { return Ok(None) };

        // Title information is always stored in the first sector of track 1
        disc.read_sector(1, CdTime::ZERO, &mut self.sector_buffer)?;

        let title_bytes = match region {
            GenesisRegion::Japan => &self.sector_buffer[0x120..0x150],
            GenesisRegion::Americas | GenesisRegion::Europe => &self.sector_buffer[0x150..0x180],
        };
        let title: String = title_bytes
            .iter()
            .copied()
            .filter_map(|b| {
                let c = b as char;
                (c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation())
                    .then_some(c)
            })
            .collect();

        let whitespace_re = WHITESPACE_RE.get_or_init(|| Regex::new(r" +").unwrap());

        Ok(Some(whitespace_re.replace_all(title.trim(), " ").to_string()))
    }
}
