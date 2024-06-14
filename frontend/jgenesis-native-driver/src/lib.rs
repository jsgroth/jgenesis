pub mod archive;
pub mod config;
pub mod input;
mod mainloop;

pub use mainloop::{
    all_supported_extensions, create_32x, create_gb, create_genesis, create_nes, create_sega_cd,
    create_smsgg, create_snes, AudioError, NativeEmulator, NativeEmulatorResult,
    NativeGameBoyEmulator, NativeGenesisEmulator, NativeNesEmulator, NativeSegaCdEmulator,
    NativeSmsGgEmulator, NativeSnesEmulator, NativeTickEffect, SaveStateMetadata, SaveWriteError,
    SAVE_STATE_SLOTS,
};
