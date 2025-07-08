pub mod archive;
pub mod config;
pub mod extensions;
mod fpstracker;
pub mod input;
mod mainloop;

pub use mainloop::{
    AudioError, Native32XEmulator, NativeEmulator, NativeEmulatorError, NativeEmulatorResult,
    NativeGameBoyEmulator, NativeGbaEmulator, NativeGenesisEmulator, NativeNesEmulator,
    NativeSegaCdEmulator, NativeSmsGgEmulator, NativeSnesEmulator, NativeTickEffect,
    SAVE_STATE_SLOTS, SaveStateMetadata, SaveWriteError, create_32x, create_gb, create_gba,
    create_genesis, create_nes, create_sega_cd, create_smsgg, create_snes,
};
use sdl3::VideoSubsystem;

#[must_use]
pub fn try_get_primary_display_scale(video: &VideoSubsystem) -> Option<f32> {
    video.get_primary_display().ok().and_then(|display| display.get_content_scale().ok())
}
