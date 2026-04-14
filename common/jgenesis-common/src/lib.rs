pub mod appimage;
pub mod audio;
pub mod boxedarray;
pub mod debug;
pub mod frontend;
pub mod input;
pub mod macros;
pub mod num;
pub mod rom;
pub mod sync;
pub mod timeutils;

pub use appimage::{fix_appimage_relative_path, is_appimage_build};

pub const DEFAULT_LOG_FILTER: &str =
    "info,wgpu_core=warn,wgpu_hal=warn,naga=warn,zbus=warn,tracing=warn";
