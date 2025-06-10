pub mod input;

use crate::archive::{ArchiveEntry, ArchiveError};
use crate::config::input::{
    GameBoyInputConfig, GenesisInputConfig, HotkeyConfig, NesInputConfig, SmsGgInputConfig,
    SnesInputConfig,
};
use crate::mainloop::NativeEmulatorError;
use crate::{NativeEmulatorResult, archive, extensions};
use gb_core::api::GameBoyEmulatorConfig;
use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::{ConfigDisplay, EnumAll, EnumDisplay};
use jgenesis_renderer::config::RendererConfig;
use nes_core::api::{NesAspectRatio, NesEmulatorConfig};
use s32x_core::api::Sega32XEmulatorConfig;
use segacd_core::api::SegaCdEmulatorConfig;
use serde::{Deserialize, Serialize};
use smsgg_core::{GgAspectRatio, SmsAspectRatio, SmsGgEmulatorConfig};
use snes_core::api::{CoprocessorRomFn, CoprocessorRoms, SnesAspectRatio, SnesEmulatorConfig};
use std::fmt::{Display, Formatter};
use std::fs;
use std::num::NonZeroU8;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    const SMS_HEIGHT: f64 = 192.0;
    const SMS_WIDTH: f64 = 256.0;

    const GG_HEIGHT: f64 = 144.0;
    const GG_WIDTH: f64 = 160.0;

    const GENESIS_HEIGHT: f64 = 224.0;
    const GENESIS_WIDTH_H40: f64 = 320.0;

    const NES_NTSC_HEIGHT: f64 = 224.0;
    const NES_PAL_HEIGHT: f64 = 240.0;
    const NES_WIDTH: f64 = 256.0;

    const SNES_HEIGHT: f64 = 224.0;
    const SNES_WIDTH: f64 = 256.0;

    const GB_HEIGHT: f64 = 144.0;
    const GB_WIDTH: f64 = 160.0;

    pub(crate) fn new(native_width: f64, native_height: f64, size: NonZeroU8) -> Self {
        let size: f64 = size.get().into();

        let width = (native_width * size).ceil() as u32;
        let height = (native_height * size).ceil() as u32;
        Self { width, height }
    }

    pub(crate) fn new_sms(size: NonZeroU8, aspect_ratio: SmsAspectRatio) -> Self {
        let pixel_aspect_ratio = aspect_ratio.to_pixel_aspect_ratio_f64().unwrap_or_else(|| {
            SmsAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0)
        });
        let width = Self::SMS_WIDTH * pixel_aspect_ratio;

        Self::new(width, Self::SMS_HEIGHT, size)
    }

    pub(crate) fn new_game_gear(size: NonZeroU8, aspect_ratio: GgAspectRatio) -> Self {
        let pixel_aspect_ratio = aspect_ratio
            .to_pixel_aspect_ratio_f64()
            .unwrap_or_else(|| GgAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0));
        let width = Self::GG_WIDTH * pixel_aspect_ratio;

        Self::new(width, Self::GG_HEIGHT, size)
    }

    pub(crate) fn new_genesis(
        size: NonZeroU8,
        aspect_ratio: GenesisAspectRatio,
        timing_mode: TimingMode,
    ) -> Self {
        Self::new(Self::genesis_width(aspect_ratio, timing_mode), Self::GENESIS_HEIGHT, size)
    }

    fn genesis_width(aspect_ratio: GenesisAspectRatio, timing_mode: TimingMode) -> f64 {
        let h40_par = aspect_ratio.to_h40_pixel_aspect_ratio(timing_mode).unwrap_or_else(|| {
            GenesisAspectRatio::default().to_h40_pixel_aspect_ratio(timing_mode).unwrap_or(1.0)
        });
        Self::GENESIS_WIDTH_H40 * h40_par
    }

    pub(crate) fn new_32x(
        size: NonZeroU8,
        aspect_ratio: GenesisAspectRatio,
        timing_mode: TimingMode,
    ) -> Self {
        // Make 32X window a little wider than Genesis by default so that the frame won't shrink if a
        // game switches to H32 mode while the renderer has forced integer height scaling enabled
        let width = Self::genesis_width(aspect_ratio, timing_mode) * 323.25 / 320.0;

        Self::new(width, Self::GENESIS_HEIGHT, size)
    }

    pub(crate) fn new_nes(
        size: NonZeroU8,
        aspect_ratio: NesAspectRatio,
        timing_mode: TimingMode,
        ntsc_crop_v_overscan: bool,
    ) -> Self {
        let pixel_aspect_ratio = aspect_ratio.to_pixel_aspect_ratio_f64().unwrap_or_else(|| {
            NesAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0)
        });
        let width = Self::NES_WIDTH * pixel_aspect_ratio;

        let height = match timing_mode {
            TimingMode::Ntsc if ntsc_crop_v_overscan => Self::NES_NTSC_HEIGHT,
            _ => Self::NES_PAL_HEIGHT,
        };

        Self::new(width, height, size)
    }

    pub(crate) fn new_snes(size: NonZeroU8, aspect_ratio: SnesAspectRatio) -> Self {
        let pixel_aspect_ratio = aspect_ratio.to_pixel_aspect_ratio_f64().unwrap_or_else(|| {
            SnesAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0)
        });
        let width = Self::SNES_WIDTH * pixel_aspect_ratio;

        Self::new(width, Self::SNES_HEIGHT, size)
    }

    pub(crate) fn new_gb(size: NonZeroU8) -> Self {
        // Only GB aspect ratio options are square pixels and stretched
        Self::new(Self::GB_WIDTH, Self::GB_HEIGHT, size)
    }

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

struct NameWithExtension {
    file_name: String,
    extension: String,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct CommonConfig {
    #[cfg_display(path)]
    pub rom_file_path: PathBuf,
    pub mute_audio: bool,
    pub audio_output_frequency: u64,
    pub audio_sync: bool,
    pub audio_dynamic_resampling_ratio: bool,
    pub audio_hardware_queue_size: u16,
    pub audio_buffer_size: u32,
    pub audio_gain_db: f64,
    pub save_path: SavePath,
    pub state_path: SavePath,
    #[cfg_display(debug_fmt)]
    pub window_size: Option<WindowSize>,
    #[cfg_display(debug_fmt)]
    pub window_scale_factor: Option<f32>,
    #[cfg_display(indent_nested)]
    pub renderer_config: RendererConfig,
    pub fast_forward_multiplier: u64,
    pub rewind_buffer_length_seconds: u64,
    pub load_recent_state_at_launch: bool,
    pub launch_in_fullscreen: bool,
    pub fullscreen_mode: FullscreenMode,
    pub initial_window_size: NonZeroU8,
    pub axis_deadzone: i16,
    #[cfg_display(indent_nested)]
    pub hotkey_config: HotkeyConfig,
    pub hide_mouse_cursor: HideMouseCursor,
}

impl CommonConfig {
    pub(crate) fn read_rom_file(
        &self,
        supported_extensions: &[&str],
    ) -> NativeEmulatorResult<RomReadResult> {
        #[derive(Default)]
        struct ArchiveListCallback {
            first_supported_file: Option<NameWithExtension>,
        }

        impl ArchiveListCallback {
            fn as_fn_mut<'ext>(
                &mut self,
                supported_extensions: &'ext [&str],
            ) -> impl FnMut(ArchiveEntry<'_>) + use<'_, 'ext> {
                |entry| {
                    if self.first_supported_file.is_some() {
                        return;
                    }

                    let Some(extension) = extensions::from_path(entry.file_name) else { return };
                    if supported_extensions.contains(&extension.as_str()) {
                        self.first_supported_file = Some(NameWithExtension {
                            file_name: entry.file_name.into(),
                            extension,
                        });
                    }
                }
            }

            fn open_file(
                self,
                archive_path: &Path,
                read_fn: fn(&Path, &str) -> Result<Vec<u8>, ArchiveError>,
            ) -> NativeEmulatorResult<RomReadResult> {
                let first_supported_file = self.first_supported_file.ok_or_else(|| {
                    NativeEmulatorError::Archive(ArchiveError::NoSupportedFiles {
                        path: archive_path.display().to_string(),
                    })
                })?;

                let contents = read_fn(archive_path, &first_supported_file.file_name)
                    .map_err(NativeEmulatorError::Archive)?;
                Ok(RomReadResult { rom: contents, extension: first_supported_file.extension })
            }
        }

        let path = &self.rom_file_path;
        let extension = extensions::from_path(path).unwrap_or_default();
        match extension.as_str() {
            "zip" => {
                let mut callback = ArchiveListCallback::default();
                archive::list_files_zip(path, callback.as_fn_mut(supported_extensions))
                    .map_err(NativeEmulatorError::Archive)?;
                callback.open_file(path, archive::read_file_zip)
            }
            "7z" => {
                let mut callback = ArchiveListCallback::default();
                archive::list_files_7z(path, callback.as_fn_mut(supported_extensions))
                    .map_err(NativeEmulatorError::Archive)?;
                callback.open_file(path, archive::read_file_7z)
            }
            _ => {
                let contents = fs::read(path).map_err(|source| NativeEmulatorError::RomRead {
                    path: path.display().to_string(),
                    source,
                })?;

                Ok(RomReadResult { rom: contents, extension })
            }
        }
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SmsGgConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: SmsGgInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: SmsGgEmulatorConfig,
    pub boot_from_bios: bool,
    pub run_without_cartridge: bool,
    #[cfg_display(path)]
    pub bios_path: Option<PathBuf>,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GenesisConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: GenesisInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: GenesisEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SegaCdConfig {
    #[cfg_display(indent_nested)]
    pub genesis: GenesisConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: SegaCdEmulatorConfig,
    #[cfg_display(path)]
    pub bios_file_path: Option<PathBuf>,
    pub run_without_disc: bool,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct Sega32XConfig {
    #[cfg_display(indent_nested)]
    pub genesis: GenesisConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: Sega32XEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct NesConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: NesInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: NesEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SnesConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: SnesInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: SnesEmulatorConfig,
    #[cfg_display(path)]
    pub dsp1_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub dsp2_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub dsp3_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub dsp4_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub st010_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub st011_rom_path: Option<PathBuf>,
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

fn coprocessor_read_fn(path: PathBuf) -> Box<CoprocessorRomFn> {
    Box::new(move || fs::read(&path).map_err(|err| (err, path.display().to_string())))
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GameBoyConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: GameBoyInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: GameBoyEmulatorConfig,
}
