pub mod api;
pub mod audio;
mod backupram;
mod bus;
mod cddrive;
mod font;
mod graphics;
mod memory;
mod rf5c164;
mod wordram;

pub use api::parse_disc_region;
use bincode::{Decode, Encode};
use cdrom::cdtime::CdTime;
use cdrom::reader::CdRom;
pub use wordram::WordRam;

pub const DEFAULT_PCM_LPF_CUTOFF: u32 = genesis_config::DEFAULT_PCM_LPF_CUTOFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ScdCpu {
    Main,
    Sub,
}

#[must_use]
pub fn is_cd_32x_disc(disc: &mut CdRom) -> bool {
    let mut sector_buffer = [0; cdrom::BYTES_PER_SECTOR as usize];

    if let Err(err) = disc.read_sector(1, CdTime::SECTOR_0_START, &mut sector_buffer) {
        log::error!("Error reading game header: {err}");
        return false;
    }

    &sector_buffer[0x110..0x118] == b"SEGA 32X"
}
