pub mod audio;
pub mod boxedarray;
pub mod cheats;
pub mod debug;
pub mod frontend;
pub mod input;
pub mod macros;
pub mod num;
pub mod paths;
pub mod rom;
pub mod sync;
pub mod timeutils;

pub use paths::{determine_emulator_dir, fix_appimage_relative_path, is_appimage_build};

pub const DEFAULT_LOG_FILTER: &str =
    "info,wgpu_core=warn,wgpu_hal=warn,naga=warn,egui_wgpu=warn,zbus=warn,tracing=warn";
