mod api;
pub mod audio;
mod bus;
mod input;
mod memory;
pub mod psg;
mod vdp;
mod ym2413;

pub use api::{SmsGgEmulator, SmsGgEmulatorConfig, SmsGgError, SmsGgResult, SmsRegion};
pub use input::{SmsGgButton, SmsGgInputs, SmsGgJoypadState};
pub use vdp::{gg_color_to_rgb, sms_color_to_rgb, VdpVersion};

// 8:7
pub const SMS_NTSC_ASPECT_RATIO: f64 = 1.1428571428571428;

// 11:8
pub const SMS_PAL_ASPECT_RATIO: f64 = 1.375;

// 6:5
pub const GAME_GEAR_LCD_ASPECT_RATIO: f64 = 1.2;
