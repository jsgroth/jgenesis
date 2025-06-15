mod api;
pub mod audio;
mod bus;
mod input;
mod memory;
pub mod psg;
mod vdp;

pub use api::{SmsGgEmulator, SmsGgEmulatorConfig, SmsGgError, SmsGgHardware, SmsGgResult};
pub use vdp::{VdpVersion, gg_color_to_rgb, sms_color_to_rgb};

pub const NATIVE_Z80_DIVIDER: u32 = smsgg_config::NATIVE_Z80_DIVIDER;
