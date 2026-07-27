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
pub use wordram::WordRam;

pub const DEFAULT_PCM_LPF_CUTOFF: u32 = genesis_config::DEFAULT_PCM_LPF_CUTOFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ScdCpu {
    Main,
    Sub,
}
