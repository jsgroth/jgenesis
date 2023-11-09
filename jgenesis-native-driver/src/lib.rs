pub mod config;
pub mod input;
mod mainloop;

pub use mainloop::{
    create_genesis, create_sega_cd, create_smsgg, create_snes, AudioError, NativeEmulator,
    NativeGenesisEmulator, NativeSegaCdEmulator, NativeSmsGgEmulator, NativeSnesEmulator,
    NativeTickEffect, SaveWriteError,
};
