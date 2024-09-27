#![allow(clippy::doc_markdown)]

use clap::Parser;
use env_logger::Env;
use gb_core::api::{GbAspectRatio, GbPalette, GbcColorCorrection};
use genesis_core::{GenesisAspectRatio, GenesisControllerType, GenesisRegion};
use jgenesis_common::frontend::{EmulatorTrait, TimingMode};
use jgenesis_native_config::AppConfig;
use jgenesis_native_config::smsgg::SmsModel;
use jgenesis_native_driver::config::input::{NesControllerType, SnesControllerType};
use jgenesis_native_driver::config::{GgAspectRatio, SmsAspectRatio};
use jgenesis_native_driver::input::MappableInputs;
use jgenesis_native_driver::{AudioError, NativeEmulator, NativeTickEffect, SaveWriteError};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, Scanlines, VSyncMode, WgpuBackend,
};
use jgenesis_renderer::renderer::RendererError;
use nes_core::api::NesAspectRatio;
use s32x_core::api::S32XVideoOut;
use smsgg_core::SmsRegion;
use smsgg_core::psg::PsgVersion;
use snes_core::api::{AudioInterpolationMode, SnesAspectRatio};
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay, EnumFromStr)]
enum Hardware {
    MasterSystem,
    Genesis,
    SegaCd,
    Sega32X,
    Nes,
    Snes,
    GameBoy,
}

const SMSGG_OPTIONS_HEADING: &str = "Master System / Game Gear Options";
const GENESIS_OPTIONS_HEADING: &str = "Genesis / Sega CD Options";
const SCD_OPTIONS_HEADING: &str = "Sega CD Options";
const S32X_OPTIONS_HEADING: &str = "32X Options";
const NES_OPTIONS_HEADING: &str = "NES Options";
const SNES_OPTIONS_HEADING: &str = "SNES Options";
const GB_OPTIONS_HEADING: &str = "Game Boy Options";
const VIDEO_OPTIONS_HEADING: &str = "Video Options";
const AUDIO_OPTIONS_HEADING: &str = "Audio Options";
const HOTKEY_OPTIONS_HEADING: &str = "Hotkey Options";

#[derive(Debug, Parser)]
struct Args {
    /// Hardware (MasterSystem / Genesis / SegaCd / Sega32X / Nes / Snes / GameBoy); defaults based on file extension if not set
    #[arg(long)]
    hardware: Option<Hardware>,

    /// ROM file path
    #[arg(short = 'f', long)]
    file_path: String,

    /// Override default config file path (jgenesis-config.toml)
    #[arg(long = "config")]
    config_path_override: Option<String>,

    /// Attempt to load the specified save state slot during startup
    #[arg(long, value_name = "SLOT")]
    load_save_state: Option<usize>,

    /// Force timing mode (Ntsc / Pal)
    #[arg(long)]
    forced_timing_mode: Option<TimingMode>,

    /// Remove sprite-per-scanline and sprite-pixel-per-scanline limits which reduces sprite flickering (for applicable consoles)
    #[arg(long)]
    remove_sprite_limit: Option<bool>,

    /// Hide mouse cursor when over emulator window
    #[arg(long)]
    hide_cursor_over_window: Option<bool>,

    /// MasterSystem model (Sms2 / Sms1)
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_model: Option<SmsModel>,

    /// Force PSG version (MasterSystem2 / Standard)
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    psg_version: Option<PsgVersion>,

    /// Master System aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_aspect_ratio: Option<SmsAspectRatio>,

    /// Game Gear aspect ratio (GgLcd / SquarePixels / Stretched)
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    gg_aspect_ratio: Option<GgAspectRatio>,

    /// Master System region (International / Domestic)
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_region: Option<SmsRegion>,

    /// Crop SMS top and bottom border; almost all games display only the background color in this area
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_crop_vertical_border: Option<bool>,

    /// Crop SMS left border; many games display only the background color in this area
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_crop_left_border: Option<bool>,

    /// Enable SMS FM sound unit
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_fm_unit_enabled: Option<bool>,

    /// Overclock the Z80 CPU to 2x speed
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    smsgg_overclock_z80: Option<bool>,

    /// Emulate the VDP's non-linear DAC, which tends to brighten darker colors and darken brighter colors
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    emulate_non_linear_vdp_dac: Option<bool>,

    /// Render the vertical border, which normally only displays the backdrop color
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_render_vertical_border: Option<bool>,

    /// Render the horizontal border, which normally only displays the backdrop color
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_render_horizontal_border: Option<bool>,

    /// Enable YM2612 channel output quantization
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    quantize_ym2612_output: Option<bool>,

    /// Emulate the YM2612 "ladder effect"
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    emulate_ym2612_ladder_effect: Option<bool>,

    /// Enable audio from the YM2612 FM chip
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    ym2612_enabled: Option<bool>,

    /// Enable audio from the SN76489 PSG chip
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_psg_enabled: Option<bool>,

    /// Aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_aspect_ratio: Option<GenesisAspectRatio>,

    /// Automatically adjust pixel aspect ratio in double-screen interlaced mode
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_adjust_aspect_ratio: Option<bool>,

    /// Force region (Americas / Japan / Europe)
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_region: Option<GenesisRegion>,

    /// P1 Genesis controller type (ThreeButton / SixButton / None)
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_p1_controller_type: Option<GenesisControllerType>,

    /// P2 Genesis controller type (ThreeButton / SixButton / None)
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_p2_controller_type: Option<GenesisControllerType>,

    /// Sega CD BIOS path
    #[arg(short = 'b', long, help_heading = SCD_OPTIONS_HEADING)]
    bios_path: Option<String>,

    /// Enable Sega CD RAM cartridge mapping
    #[arg(long, help_heading = SCD_OPTIONS_HEADING)]
    enable_ram_cartridge: Option<bool>,

    /// Run the Sega CD emulator with no disc
    #[arg(long, help_heading = SCD_OPTIONS_HEADING)]
    scd_no_disc: bool,

    /// Load the CD-ROM image into RAM at startup
    #[arg(long, help_heading = SCD_OPTIONS_HEADING)]
    scd_load_disc_into_ram: Option<bool>,

    /// Enable audio from the RF5C164 PCM chip
    #[arg(long, help_heading = SCD_OPTIONS_HEADING)]
    scd_pcm_enabled: Option<bool>,

    /// Enable CD audio playback
    #[arg(long, help_heading = SCD_OPTIONS_HEADING)]
    scd_cd_da_enabled: Option<bool>,

    /// Set 32X video output (Combined / GenesisOnly / S32XOnly)
    #[arg(long, help_heading = S32X_OPTIONS_HEADING)]
    s32x_video_out: Option<S32XVideoOut>,

    /// Enable audio from the 32X PWM chip
    #[arg(long, help_heading = S32X_OPTIONS_HEADING)]
    s32x_pwm_enabled: Option<bool>,

    /// Aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    nes_aspect_ratio: Option<NesAspectRatio>,

    /// NES P2 controller type (Gamepad / Zapper)
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    nes_p2_controller_type: Option<NesControllerType>,

    /// Top overscan in pixels
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    overscan_top: Option<u16>,

    /// Bottom overscan in pixels
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    overscan_bottom: Option<u16>,

    /// Left overscan in pixels
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    overscan_left: Option<u16>,

    /// Right overscan in pixels
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    overscan_right: Option<u16>,

    /// Render the PAL black border (top scanline + two columns on each side)
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    nes_pal_black_border: Option<bool>,

    /// Allow opposing directional inputs (left+right or up+down)
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    nes_allow_opposing_inputs: Option<bool>,

    /// Silence ultrasonic triangle channel output (less accurate but reduces audio popping)
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    nes_silence_ultrasonic_triangle: Option<bool>,

    /// Enable hack that times NES audio sync to 60Hz NTSC / 50Hz PAL instead of ~60.099Hz NTSC / ~50.007Hz PAL
    #[arg(long, help_heading = NES_OPTIONS_HEADING)]
    nes_audio_60hz_hack: Option<bool>,

    /// SNES aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    snes_aspect_ratio: Option<SnesAspectRatio>,

    /// Audio interpolation mode (Gaussian / Hermite / Lagrange)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    snes_audio_interpolation: Option<AudioInterpolationMode>,

    /// Enable hack that times SNES audio sync to 60Hz instead of ~60.098Hz
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    snes_audio_60hz_hack: Option<bool>,

    /// Speed multiplier for the Super FX GSU
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    gsu_overclock_factor: Option<NonZeroU64>,

    /// Player 2 input device (Gamepad / SuperScope)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    snes_p2_controller_type: Option<SnesControllerType>,

    /// Specify SNES DSP-1 ROM path (required for DSP-1 games)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    dsp1_rom_path: Option<String>,

    /// Specify SNES DSP-2 ROM path (required for DSP-2 games)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    dsp2_rom_path: Option<String>,

    /// Specify SNES DSP-3 ROM path (required for DSP-3 games)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    dsp3_rom_path: Option<String>,

    /// Specify SNES DSP-4 ROM path (required for DSP-4 games)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    dsp4_rom_path: Option<String>,

    /// Specify SNES ST010 ROM path (required for ST010 games)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    st010_rom_path: Option<String>,

    /// Specify SNES ST011 ROM path (required for ST011 games)
    #[arg(long, help_heading = SNES_OPTIONS_HEADING)]
    st011_rom_path: Option<String>,

    /// Force DMG / original Game Boy mode in software with Game Boy Color support
    #[arg(long, help_heading = GB_OPTIONS_HEADING)]
    force_dmg_mode: Option<bool>,

    /// Pretend to be a Game Boy Advance (for GBC games that vary behavior on GBA)
    #[arg(long, help_heading = GB_OPTIONS_HEADING)]
    pretend_to_be_gba: Option<bool>,

    /// Aspect ratio (SquarePixels / Stretched)
    #[arg(long, help_heading = GB_OPTIONS_HEADING)]
    gb_aspect_ratio: Option<GbAspectRatio>,

    /// Game Boy palette (BlackAndWhite / GreenTint / LimeGreen)
    #[arg(long, help_heading = GB_OPTIONS_HEADING)]
    gb_palette: Option<GbPalette>,

    /// Game Boy Color color correction (None / GbcLcd / GbaLcd)
    #[arg(long, help_heading = GB_OPTIONS_HEADING)]
    gbc_color_correction: Option<GbcColorCorrection>,

    /// Target 60 FPS instead of ~59.73 FPS
    #[arg(long, help_heading = GB_OPTIONS_HEADING)]
    gb_audio_60hz_hack: Option<bool>,

    /// Initial window width in pixels
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    window_width: Option<u32>,

    /// Initial window height in pixels
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    window_height: Option<u32>,

    /// Launch in fullscreen
    #[arg(long, default_value_t, help_heading = VIDEO_OPTIONS_HEADING)]
    fullscreen: bool,

    /// wgpu backend (Auto / Vulkan / DirectX12 / OpenGl)
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    wgpu_backend: Option<WgpuBackend>,

    /// VSync mode (Enabled / Disabled / Fast)
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    vsync_mode: Option<VSyncMode>,

    /// Enable auto-prescaling
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    auto_prescale: Option<bool>,

    /// Manual prescale factor; must be a positive integer
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    prescale_factor: Option<u32>,

    /// Scanlines (None / Dim / Black)
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    scanlines: Option<Scanlines>,

    /// Force display area height to be an integer multiple of native console resolution
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    force_integer_height_scaling: Option<bool>,

    /// Filter mode (Nearest / Linear)
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    filter_mode: Option<FilterMode>,

    /// Preprocess shader (None / HorizontalBlurTwoPixels / HorizontalBlurThreePixels / HorizontalBlurSnesAdaptive / AntiDitherWeak / AntiDitherStrong)
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    preprocess_shader: Option<PreprocessShader>,

    /// Enable audio sync
    #[arg(long, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_sync: Option<bool>,

    /// Audio device queue size in samples
    #[arg(long, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_device_queue_size: Option<u16>,

    /// Internal audio buffer size in samples
    #[arg(long, help_heading = AUDIO_OPTIONS_HEADING)]
    internal_audio_buffer_size: Option<u32>,

    /// Audio sync threshold in bytes (1 sample = 2x4 bytes)
    #[arg(long, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_sync_threshold: Option<u32>,

    /// Audio gain in decibels; can be positive or negative
    #[arg(long, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_gain_db: Option<f64>,

    /// Fast forward multiplier
    #[arg(long, help_heading = HOTKEY_OPTIONS_HEADING)]
    fast_forward_multiplier: Option<u64>,

    /// Rewind buffer length in seconds
    #[arg(long, help_heading = HOTKEY_OPTIONS_HEADING)]
    rewind_buffer_length_seconds: Option<u64>,
}

macro_rules! apply_overrides {
    (@set_field $config:expr, $value:expr, $arg_field:ident) => {
        $config.$arg_field = $value;
    };
    (@set_field $config:expr, $value:expr, $arg_field:ident $config_field:ident) => {
        $config.$config_field = $value;
    };
    ($self:expr, $config:expr, [$($arg_field:ident $(-> $config_field:ident)?),* $(,)?]) => {
        $(
            if let Some(field) = $self.$arg_field {
                apply_overrides!(@set_field $config, field, $arg_field $($config_field)?);
            }
        )*
    };
}

macro_rules! apply_path_overrides {
    ($self:expr, $config:expr, [$($field:ident),* $(,)?]) => {
        $(
            if let Some(field) = &$self.$field {
                $config.$field = Some(field.clone());
            }
        )*
    }
}

impl Args {
    fn apply_overrides(&self, config: &mut AppConfig) {
        self.apply_common_overrides(config);
        self.apply_smsgg_overrides(config);
        self.apply_genesis_overrides(config);
        self.apply_sega_cd_overrides(config);
        self.apply_32x_overrides(config);
        self.apply_nes_overrides(config);
        self.apply_snes_overrides(config);
        self.apply_gb_overrides(config);
        self.apply_video_overrides(config);
        self.apply_audio_overrides(config);
        self.apply_hotkey_overrides(config);
    }

    fn apply_common_overrides(&self, config: &mut AppConfig) {
        if let Some(timing_mode) = self.forced_timing_mode {
            config.smsgg.sms_timing_mode = timing_mode;
            config.genesis.forced_timing_mode = Some(timing_mode);
            config.nes.forced_timing_mode = Some(timing_mode);
            config.snes.forced_timing_mode = Some(timing_mode);
        }

        if let Some(remove_sprite_limit) = self.remove_sprite_limit {
            config.smsgg.remove_sprite_limit = remove_sprite_limit;
            config.genesis.remove_sprite_limits = remove_sprite_limit;
            config.nes.remove_sprite_limit = remove_sprite_limit;
        }

        apply_overrides!(self, config.common, [hide_cursor_over_window]);
    }

    fn apply_smsgg_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.smsgg, [
            sms_model,
            sms_aspect_ratio,
            gg_aspect_ratio,
            sms_region,
            sms_crop_vertical_border,
            sms_crop_left_border,
            sms_fm_unit_enabled -> fm_sound_unit_enabled,
            smsgg_overclock_z80 -> overclock_z80,
        ]);

        if let Some(psg_version) = self.psg_version {
            config.smsgg.psg_version = Some(psg_version);
        }
    }

    fn apply_genesis_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.genesis, [
            emulate_non_linear_vdp_dac,
            genesis_render_vertical_border -> render_vertical_border,
            genesis_render_horizontal_border -> render_horizontal_border,
            quantize_ym2612_output,
            emulate_ym2612_ladder_effect,
            ym2612_enabled,
            genesis_psg_enabled -> psg_enabled,
            genesis_aspect_ratio -> aspect_ratio,
            genesis_adjust_aspect_ratio -> adjust_aspect_ratio_in_2x_resolution,
        ]);

        if let Some(region) = self.genesis_region {
            config.genesis.forced_region = Some(region);
        }

        apply_overrides!(self, config.inputs, [
            genesis_p1_controller_type -> genesis_p1_type,
            genesis_p2_controller_type -> genesis_p2_type,
        ]);
    }

    fn apply_sega_cd_overrides(&self, config: &mut AppConfig) {
        apply_path_overrides!(self, config.sega_cd, [bios_path]);

        apply_overrides!(self, config.sega_cd, [
            enable_ram_cartridge,
            scd_load_disc_into_ram -> load_disc_into_ram,
            scd_pcm_enabled -> pcm_enabled,
            scd_cd_da_enabled -> cd_audio_enabled,
        ]);
    }

    fn apply_32x_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.sega_32x, [
            s32x_video_out -> video_out,
            s32x_pwm_enabled -> pwm_enabled,
        ]);
    }

    fn apply_nes_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.nes, [
            nes_aspect_ratio -> aspect_ratio,
            nes_pal_black_border -> pal_black_border,
            nes_allow_opposing_inputs -> allow_opposing_joypad_inputs,
            nes_silence_ultrasonic_triangle -> silence_ultrasonic_triangle_output,
            nes_audio_60hz_hack -> audio_60hz_hack,
        ]);

        apply_overrides!(self, config.nes.overscan, [
            overscan_top -> top,
            overscan_bottom -> bottom,
            overscan_left -> left,
            overscan_right -> right,
        ]);

        apply_overrides!(self, config.inputs, [nes_p2_controller_type -> nes_p2_type]);
    }

    fn apply_snes_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.snes, [
            snes_aspect_ratio -> aspect_ratio,
            snes_audio_interpolation -> audio_interpolation,
            snes_audio_60hz_hack -> audio_60hz_hack,
            gsu_overclock_factor,
        ]);

        if let Some(p2_controller_type) = self.snes_p2_controller_type {
            config.inputs.snes_p2_type = p2_controller_type;
        }

        apply_path_overrides!(self, config.snes, [
            dsp1_rom_path,
            dsp2_rom_path,
            dsp3_rom_path,
            dsp4_rom_path,
            st010_rom_path,
            st011_rom_path,
        ]);
    }

    fn apply_gb_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.game_boy, [
            force_dmg_mode,
            pretend_to_be_gba,
            gb_aspect_ratio -> aspect_ratio,
            gb_palette,
            gbc_color_correction,
            gb_audio_60hz_hack -> audio_60hz_hack,
        ]);
    }

    fn apply_video_overrides(&self, config: &mut AppConfig) {
        config.common.window_width = self.window_width;
        config.common.window_height = self.window_height;

        if self.fullscreen {
            config.common.launch_in_fullscreen = true;
        }

        apply_overrides!(self, config.common, [
            wgpu_backend,
            vsync_mode,
            auto_prescale,
            scanlines,
            force_integer_height_scaling,
            filter_mode,
            preprocess_shader,
        ]);

        if let Some(prescale_factor) = self.prescale_factor {
            config.common.prescale_factor =
                PrescaleFactor::try_from(prescale_factor).expect("prescale factor is invalid");
        }
    }

    fn apply_audio_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.common, [
            audio_sync,
            audio_device_queue_size,
            internal_audio_buffer_size,
            audio_sync_threshold,
            audio_gain_db,
        ]);
    }

    fn apply_hotkey_overrides(&self, config: &mut AppConfig) {
        apply_overrides!(self, config.common, [
            fast_forward_multiplier,
            rewind_buffer_length_seconds
        ]);
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn"),
    )
    .init();

    let args = Args::parse();

    let hardware = match args.hardware {
        Some(hardware) => hardware,
        None => {
            let file_path = Path::new(&args.file_path);
            let mut file_ext: String =
                file_path.extension().and_then(OsStr::to_str).unwrap_or("").into();
            match file_ext.as_str() {
                "zip" => {
                    let zip_entry = jgenesis_native_driver::archive::first_supported_file_in_zip(
                        file_path,
                        jgenesis_native_driver::all_supported_extensions(),
                    )?
                    .unwrap_or_else(|| {
                        panic!(
                            "No files with supported extensions found in .zip archive: {}",
                            args.file_path
                        )
                    });
                    file_ext = zip_entry.extension;
                }
                "7z" => {
                    let zip_entry = jgenesis_native_driver::archive::first_supported_file_in_7z(
                        file_path,
                        jgenesis_native_driver::all_supported_extensions(),
                    )?
                    .unwrap_or_else(|| {
                        panic!(
                            "No files with supported extensions found in .7z archive: {}",
                            args.file_path
                        )
                    });
                    file_ext = zip_entry.extension;
                }
                _ => {}
            }

            match file_ext.as_str() {
                "sms" | "gg" => Hardware::MasterSystem,
                "md" | "bin" => Hardware::Genesis,
                "cue" | "chd" => Hardware::SegaCd,
                "32x" => Hardware::Sega32X,
                "nes" => Hardware::Nes,
                "sfc" | "smc" => Hardware::Snes,
                "gb" | "gbc" => Hardware::GameBoy,
                _ => {
                    log::warn!("Unrecognized file extension: '{file_ext}' defaulting to Genesis");
                    Hardware::Genesis
                }
            }
        }
    };

    log::info!("Running with hardware {hardware}");

    let config_path = args
        .config_path_override
        .clone()
        .map_or_else(jgenesis_native_config::default_config_path, PathBuf::from);
    log::info!("Loading config from '{}'", config_path.display());

    let config_str = fs::read_to_string(&config_path).unwrap_or_else(|err| {
        log::warn!("Unable to read config file from '{}': {err}", config_path.display());
        "".into()
    });

    let mut config = toml::from_str::<AppConfig>(&config_str).unwrap_or_else(|err| {
        log::error!("Unable to deserialize config file at '{}': {err}", config_path.display());
        AppConfig::default()
    });

    args.apply_overrides(&mut config);

    match hardware {
        Hardware::MasterSystem => run_sms(args, config),
        Hardware::Genesis => run_genesis(args, config),
        Hardware::SegaCd => run_sega_cd(args, config),
        Hardware::Sega32X => run_32x(args, config),
        Hardware::Nes => run_nes(args, config),
        Hardware::Snes => run_snes(args, config),
        Hardware::GameBoy => run_gb(args, config),
    }
}

fn run_sms(args: Args, config: AppConfig) -> anyhow::Result<()> {
    let mut emulator =
        jgenesis_native_driver::create_smsgg(config.smsgg_config(args.file_path.clone()))?;
    init_emulator(&mut emulator, &args);

    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_genesis(args: Args, config: AppConfig) -> anyhow::Result<()> {
    let mut emulator =
        jgenesis_native_driver::create_genesis(config.genesis_config(args.file_path.clone()))?;
    init_emulator(&mut emulator, &args);

    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_sega_cd(args: Args, config: AppConfig) -> anyhow::Result<()> {
    let mut scd_config = config.sega_cd_config(args.file_path.clone());
    scd_config.run_without_disc = args.scd_no_disc;

    let mut emulator = jgenesis_native_driver::create_sega_cd(scd_config)?;
    init_emulator(&mut emulator, &args);

    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_32x(args: Args, config: AppConfig) -> anyhow::Result<()> {
    let mut emulator =
        jgenesis_native_driver::create_32x(config.sega_32x_config(args.file_path.clone()))?;
    init_emulator(&mut emulator, &args);

    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_nes(args: Args, config: AppConfig) -> anyhow::Result<()> {
    let mut emulator =
        jgenesis_native_driver::create_nes(config.nes_config(args.file_path.clone()))?;
    init_emulator(&mut emulator, &args);

    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_snes(args: Args, config: AppConfig) -> anyhow::Result<()> {
    let mut emulator =
        jgenesis_native_driver::create_snes(config.snes_config(args.file_path.clone()))?;
    init_emulator(&mut emulator, &args);

    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_gb(args: Args, config: AppConfig) -> anyhow::Result<()> {
    let mut emulator = jgenesis_native_driver::create_gb(config.gb_config(args.file_path.clone()))?;
    init_emulator(&mut emulator, &args);

    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

// TODO simplify these trait bounds (required by load_state() definition)
fn init_emulator<Inputs, Button, Config, Emulator>(
    emulator: &mut NativeEmulator<Inputs, Button, Config, Emulator>,
    args: &Args,
) where
    Inputs: Default + MappableInputs<Button>,
    Button: Copy,
    Emulator: EmulatorTrait<Inputs = Inputs, Config = Config>,
    Emulator::Err<RendererError, AudioError, SaveWriteError>: Error + Send + Sync + 'static,
{
    let Some(save_state_slot) = args.load_save_state else { return };

    log::info!("Loading save state slot {save_state_slot} at launch");

    if let Err(err) = emulator.load_state(save_state_slot) {
        log::error!("Error loading save state slot {save_state_slot} at launch: {err}");
    }
}
