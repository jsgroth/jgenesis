pub mod archive;
pub mod config;
mod fpstracker;
pub mod input;
mod mainloop;

pub use mainloop::{
    AudioError, Native32XEmulator, NativeEmulator, NativeEmulatorError, NativeEmulatorResult,
    NativeGameBoyEmulator, NativeGbaEmulator, NativeGenesisEmulator, NativeNesEmulator,
    NativeSegaCdEmulator, NativeSmsGgEmulator, NativeSnesEmulator, NativeTickEffect,
    SAVE_STATE_SLOTS, SaveStateMetadata, SaveWriteError, all_supported_extensions, create_32x,
    create_gb, create_gba, create_genesis, create_nes, create_sega_cd, create_smsgg, create_snes,
};
use sdl2::VideoSubsystem;

#[must_use]
pub fn determine_scale_factor(video: &VideoSubsystem, display_idx: Option<i32>) -> Option<f32> {
    let scale_factor =
        video.display_dpi(display_idx.unwrap_or(0)).ok().and_then(|(_, hdpi, vdpi)| {
            // Set scale factor to DPI/96 if HDPI and VDPI are equal and non-zero
            let delta = (hdpi - vdpi).abs();
            (delta < 1e-3 && hdpi > 0.0).then(|| {
                let doubled_scale_factor = (hdpi / 96.0 * 2.0).round() as u32;
                doubled_scale_factor as f32 / 2.0
            })
        })?;

    // Arbitrary min/max thresholds; egui will panic if pixels_per_point is too high
    Some(scale_factor.clamp(0.5, 10.0))
}
