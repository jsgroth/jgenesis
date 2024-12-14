pub mod input;

use crate::config::input::{
    GameBoyInputConfig, GenesisInputConfig, HotkeyConfig, NesInputConfig, SmsGgInputConfig,
    SnesInputConfig,
};
use crate::mainloop::NativeEmulatorError;
use crate::{NativeEmulatorResult, archive};
use gb_core::api::GameBoyEmulatorConfig;
use genesis_core::GenesisEmulatorConfig;
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::{ConfigDisplay, EnumAll, EnumDisplay};
use jgenesis_renderer::config::RendererConfig;
use nes_core::api::NesEmulatorConfig;
use s32x_core::api::Sega32XEmulatorConfig;
use segacd_core::api::SegaCdEmulatorConfig;
use serde::{Deserialize, Serialize};
use smsgg_core::{SmsGgEmulatorConfig, SmsGgHardware};
use snes_core::api::{CoprocessorRomFn, CoprocessorRoms, SnesEmulatorConfig};
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_GENESIS_WINDOW_SIZE: WindowSize = WindowSize { width: 878, height: 672 };
pub(crate) const DEFAULT_GB_WINDOW_SIZE: WindowSize =
    WindowSize { width: 160 * 3, height: 144 * 3 };

#[derive(Debug, Clone, Copy)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    pub(crate) fn scale(self, scale_factor: f32) -> Self {
        Self {
            width: (self.width as f32 * scale_factor).round() as u32,
            height: (self.height as f32 * scale_factor).round() as u32,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SavePath {
    RomFolder,
    EmulatorFolder,
    Custom(PathBuf),
}

impl SavePath {
    pub const SAVE_SUBDIR: &'static str = "saves";
    pub const STATE_SUBDIR: &'static str = "states";
}

impl Display for SavePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RomFolder => write!(f, "ROM Folder"),
            Self::EmulatorFolder => write!(f, "Emulator Folder"),
            Self::Custom(path) => write!(f, "{}", path.display()),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum FullscreenMode {
    #[default]
    Borderless,
    Exclusive,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum HideMouseCursor {
    #[default]
    Fullscreen,
    Never,
    Always,
}

impl HideMouseCursor {
    pub(crate) fn should_hide(self, fullscreen: bool) -> bool {
        self == Self::Always || (fullscreen && self == Self::Fullscreen)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RomReadResult {
    pub rom: Vec<u8>,
    pub extension: String,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct CommonConfig {
    pub rom_file_path: String,
    pub audio_output_frequency: u64,
    pub audio_sync: bool,
    pub audio_dynamic_resampling_ratio: bool,
    pub audio_hardware_queue_size: u16,
    pub audio_buffer_size: u32,
    pub audio_gain_db: f64,
    pub save_path: SavePath,
    pub state_path: SavePath,
    #[debug_fmt]
    pub window_size: Option<WindowSize>,
    #[debug_fmt]
    pub window_scale_factor: Option<f32>,
    #[indent_nested]
    pub renderer_config: RendererConfig,
    pub fast_forward_multiplier: u64,
    pub rewind_buffer_length_seconds: u64,
    pub load_recent_state_at_launch: bool,
    pub launch_in_fullscreen: bool,
    pub fullscreen_mode: FullscreenMode,
    pub axis_deadzone: i16,
    #[indent_nested]
    pub hotkey_config: HotkeyConfig,
    pub hide_mouse_cursor: HideMouseCursor,
}

impl CommonConfig {
    pub(crate) fn read_rom_file(
        &self,
        supported_extensions: &[&str],
    ) -> NativeEmulatorResult<RomReadResult> {
        let path = Path::new(&self.rom_file_path);
        let extension = path.extension().and_then(OsStr::to_str).unwrap_or("");
        match extension {
            "zip" => archive::read_first_file_in_zip(path, supported_extensions)
                .map_err(NativeEmulatorError::Archive),
            "7z" => archive::read_first_file_in_7z(path, supported_extensions)
                .map_err(NativeEmulatorError::Archive),
            _ => {
                let contents = fs::read(path).map_err(|source| NativeEmulatorError::RomRead {
                    path: path.display().to_string(),
                    source,
                })?;

                Ok(RomReadResult { rom: contents, extension: extension.into() })
            }
        }
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SmsGgConfig {
    #[indent_nested]
    pub common: CommonConfig,
    #[indent_nested]
    pub inputs: SmsGgInputConfig,
    #[indent_nested]
    pub emulator_config: SmsGgEmulatorConfig,
}

pub(crate) fn default_smsgg_window_size(
    hardware: SmsGgHardware,
    sms_timing_mode: TimingMode,
) -> WindowSize {
    match (hardware, sms_timing_mode) {
        (SmsGgHardware::MasterSystem, TimingMode::Ntsc) => WindowSize { width: 878, height: 576 },
        (SmsGgHardware::MasterSystem, TimingMode::Pal) => WindowSize { width: 1056, height: 576 },
        (SmsGgHardware::GameGear, _) => WindowSize { width: 576, height: 432 },
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GenesisConfig {
    #[indent_nested]
    pub common: CommonConfig,
    #[indent_nested]
    pub inputs: GenesisInputConfig,
    #[indent_nested]
    pub emulator_config: GenesisEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SegaCdConfig {
    #[indent_nested]
    pub genesis: GenesisConfig,
    #[indent_nested]
    pub emulator_config: SegaCdEmulatorConfig,
    pub bios_file_path: Option<String>,
    pub run_without_disc: bool,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct Sega32XConfig {
    #[indent_nested]
    pub genesis: GenesisConfig,
    #[indent_nested]
    pub emulator_config: Sega32XEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct NesConfig {
    #[indent_nested]
    pub common: CommonConfig,
    #[indent_nested]
    pub inputs: NesInputConfig,
    #[indent_nested]
    pub emulator_config: NesEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SnesConfig {
    #[indent_nested]
    pub common: CommonConfig,
    #[indent_nested]
    pub inputs: SnesInputConfig,
    #[indent_nested]
    pub emulator_config: SnesEmulatorConfig,
    pub dsp1_rom_path: Option<String>,
    pub dsp2_rom_path: Option<String>,
    pub dsp3_rom_path: Option<String>,
    pub dsp4_rom_path: Option<String>,
    pub st010_rom_path: Option<String>,
    pub st011_rom_path: Option<String>,
}

impl SnesConfig {
    pub(crate) fn to_coprocessor_roms(&self) -> CoprocessorRoms {
        let dsp1 = self.dsp1_rom_path.clone().map(coprocessor_read_fn);
        let dsp2 = self.dsp2_rom_path.clone().map(coprocessor_read_fn);
        let dsp3 = self.dsp3_rom_path.clone().map(coprocessor_read_fn);
        let dsp4 = self.dsp4_rom_path.clone().map(coprocessor_read_fn);
        let st010 = self.st010_rom_path.clone().map(coprocessor_read_fn);
        let st011 = self.st011_rom_path.clone().map(coprocessor_read_fn);

        CoprocessorRoms { dsp1, dsp2, dsp3, dsp4, st010, st011 }
    }
}

fn coprocessor_read_fn(path: String) -> Box<CoprocessorRomFn> {
    Box::new(move || fs::read(&path).map_err(|err| (err, path.clone())))
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GameBoyConfig {
    #[indent_nested]
    pub common: CommonConfig,
    #[indent_nested]
    pub inputs: GameBoyInputConfig,
    #[indent_nested]
    pub emulator_config: GameBoyEmulatorConfig,
}
