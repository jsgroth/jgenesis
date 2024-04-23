#![allow(clippy::doc_markdown)]

use clap::Parser;
use env_logger::Env;
use gb_core::api::{GbAspectRatio, GbPalette, GbcColorCorrection};
use genesis_core::{GenesisAspectRatio, GenesisControllerType, GenesisRegion};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::input::{
    GameBoyInputConfig, GenesisControllerConfig, GenesisInputConfig, HotkeyConfig, KeyboardInput,
    NesInputConfig, SmsGgControllerConfig, SmsGgInputConfig, SnesControllerType, SnesInputConfig,
    SuperScopeConfig,
};
use jgenesis_native_driver::config::{
    CommonConfig, GameBoyConfig, GenesisConfig, GgAspectRatio, NesConfig, SegaCdConfig,
    SmsAspectRatio, SmsGgConfig, SnesConfig, WindowSize,
};
use jgenesis_native_driver::NativeTickEffect;
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, PrescaleMode, RendererConfig, Scanlines,
    VSyncMode, WgpuBackend,
};
use nes_core::api::{NesAspectRatio, Overscan};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsRegion, VdpVersion};
use snes_core::api::SnesAspectRatio;
use std::ffi::OsStr;
use std::num::NonZeroU64;
use std::path::Path;
use std::process;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay, EnumFromStr)]
enum Hardware {
    MasterSystem,
    Genesis,
    SegaCd,
    Nes,
    Snes,
    GameBoy,
}

const SMSGG_OPTIONS_HEADING: &str = "Master System / Game Gear Options";
const GENESIS_OPTIONS_HEADING: &str = "Genesis / Sega CD Options";
const SCD_OPTIONS_HEADING: &str = "Sega CD Options";
const NES_OPTIONS_HEADING: &str = "NES Options";
const SNES_OPTIONS_HEADING: &str = "SNES Options";
const GB_OPTIONS_HEADING: &str = "Game Boy Options";
const VIDEO_OPTIONS_HEADING: &str = "Video Options";
const AUDIO_OPTIONS_HEADING: &str = "Audio Options";
const INPUT_OPTIONS_HEADING: &str = "Input Options";
const HOTKEY_OPTIONS_HEADING: &str = "Hotkey Options";

#[derive(Parser)]
struct Args {
    /// ROM file path
    #[arg(short = 'f', long)]
    file_path: String,

    /// Hardware (MasterSystem / Genesis / SegaCd / Nes / Snes), will default based on file extension if not set
    #[arg(long)]
    hardware: Option<Hardware>,

    /// Force timing mode (Ntsc / Pal)
    #[arg(long)]
    forced_timing_mode: Option<TimingMode>,

    /// Remove sprite-per-scanline and sprite-pixel-per-scanlines limits which reduces sprite flickering
    #[arg(long, default_value_t)]
    remove_sprite_limit: bool,

    /// Hide mouse cursor when over emulator window
    #[arg(long, default_value_t)]
    hide_cursor_over_window: bool,

    /// Force VDP version (NtscMasterSystem2 / NtscMasterSystem1 / PalMasterSystem2 / PalMasterSystem1 / GameGear)
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    vdp_version: Option<VdpVersion>,

    /// Force PSG version (MasterSystem2 / Standard)
    #[arg(long, help_heading = SMSGG_OPTIONS_HEADING)]
    psg_version: Option<PsgVersion>,

    /// Master System aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, default_value_t, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_aspect_ratio: SmsAspectRatio,

    /// Game Gear aspect ratio (GgLcd / SquarePixels / Stretched)
    #[arg(long, default_value_t, help_heading = SMSGG_OPTIONS_HEADING)]
    gg_aspect_ratio: GgAspectRatio,

    /// Master System region (International / Domestic)
    #[arg(long, default_value_t, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_region: SmsRegion,

    /// Crop SMS top and bottom border; almost all games display only the background color in this area
    #[arg(long, default_value_t, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_crop_vertical_border: bool,

    /// Crop SMS left border; many games display only the background color in this area
    #[arg(long, default_value_t, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_crop_left_border: bool,

    /// Disable SMS FM sound unit
    #[arg(long = "disable-sms-fm-unit", default_value_t = true, action = clap::ArgAction::SetFalse, help_heading = SMSGG_OPTIONS_HEADING)]
    sms_fm_unit_enabled: bool,

    /// Overclock the Z80 CPU to 2x speed
    #[arg(long, default_value_t, help_heading = SMSGG_OPTIONS_HEADING)]
    smsgg_overclock_z80: bool,

    /// Emulate the VDP's non-linear DAC, which tends to brighten darker colors and darken brighter colors
    #[arg(long, default_value_t, help_heading = GENESIS_OPTIONS_HEADING)]
    emulate_non_linear_vdp_dac: bool,

    /// Render the vertical border, which normally only displays the backdrop color
    #[arg(long, default_value_t, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_render_vertical_border: bool,

    /// Render the horizontal border, which normally only displays the backdrop color
    #[arg(long, default_value_t, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_render_horizontal_border: bool,

    /// Disable YM2612 output quantization, letting outputs cover the full 14-bit range instead of only using the highest 9 bits
    #[arg(long = "no-ym2612-quantization", default_value_t = true, action = clap::ArgAction::SetFalse, help_heading = GENESIS_OPTIONS_HEADING)]
    quantize_ym2612_output: bool,

    /// Aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, default_value_t, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_aspect_ratio: GenesisAspectRatio,

    /// Disable automatic pixel aspect ratio adjustment when Genesis interlacing double resolution mode
    /// is enabled
    #[arg(long = "no-genesis-adjust-aspect-ratio", default_value_t = true, action = clap::ArgAction::SetFalse, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_adjust_aspect_ratio: bool,

    /// Force region (Americas / Japan / Europe)
    #[arg(long, help_heading = GENESIS_OPTIONS_HEADING)]
    genesis_region: Option<GenesisRegion>,

    /// Sega CD BIOS path (required for Sega CD emulation)
    #[arg(short = 'b', long, help_heading = SCD_OPTIONS_HEADING)]
    bios_path: Option<String>,

    /// Disable Sega CD RAM cartridge mapping
    #[arg(long = "disable-ram-cartridge", default_value_t = true, action = clap::ArgAction::SetFalse, help_heading = SCD_OPTIONS_HEADING)]
    enable_ram_cartridge: bool,

    /// Run the Sega CD emulator with no disc
    #[arg(long, default_value_t, help_heading = SCD_OPTIONS_HEADING)]
    scd_no_disc: bool,

    /// Load the CD-ROM image into RAM at startup
    #[arg(long, default_value_t, help_heading = SCD_OPTIONS_HEADING)]
    scd_load_disc_into_ram: bool,

    /// Aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    nes_aspect_ratio: NesAspectRatio,

    /// Top overscan in pixels
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    overscan_top: u16,

    /// Bottom overscan in pixels
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    overscan_bottom: u16,

    /// Left overscan in pixels
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    overscan_left: u16,

    /// Right overscan in pixels
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    overscan_right: u16,

    /// Render the PAL black border (top scanline + two columns on each side)
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    nes_pal_black_border: bool,

    /// Allow opposing directional inputs (left+right or up+down)
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    nes_allow_opposing_inputs: bool,

    /// Silence ultrasonic triangle channel output (less accurate but reduces audio popping)
    #[arg(long, default_value_t, help_heading = NES_OPTIONS_HEADING)]
    nes_silence_ultrasonic_triangle: bool,

    /// Disable hack that times NES audio sync to 60Hz NTSC / 50Hz PAL instead of ~60.099Hz NTSC / ~50.007Hz PAL
    #[arg(long = "no-nes-audio-60hz-hack", default_value_t = true, action = clap::ArgAction::SetFalse, help_heading = NES_OPTIONS_HEADING)]
    nes_audio_60hz_hack: bool,

    /// SNES aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, default_value_t, help_heading = SNES_OPTIONS_HEADING)]
    snes_aspect_ratio: SnesAspectRatio,

    /// Disable hack that times SNES audio sync to 60Hz instead of ~60.098Hz
    #[arg(long = "no-snes-audio-60hz-hack", default_value_t = true, action = clap::ArgAction::SetFalse, help_heading = SNES_OPTIONS_HEADING)]
    snes_audio_60hz_hack: bool,

    /// Speed multiplier for the Super FX GSU
    #[arg(long, default_value_t = NonZeroU64::new(1).unwrap(), help_heading = SNES_OPTIONS_HEADING)]
    gsu_overclock_factor: NonZeroU64,

    /// Player 2 input device (Gamepad / SuperScope)
    #[arg(long, default_value_t, help_heading = SNES_OPTIONS_HEADING)]
    snes_p2_controller_type: SnesControllerType,

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
    #[arg(long, default_value_t, help_heading = GB_OPTIONS_HEADING)]
    force_dmg_mode: bool,

    /// Pretend to be a Game Boy Advance (for GBC games that vary behavior on GBA)
    #[arg(long, default_value_t, help_heading = GB_OPTIONS_HEADING)]
    pretend_to_be_gba: bool,

    /// Aspect ratio (SquarePixels / Stretched)
    #[arg(long, default_value_t, help_heading = GB_OPTIONS_HEADING)]
    gb_aspect_ratio: GbAspectRatio,

    /// Game Boy palette (BlackAndWhite / GreenTint / LimeGreen)
    #[arg(long, default_value_t, help_heading = GB_OPTIONS_HEADING)]
    gb_palette: GbPalette,

    /// Game Boy Color color correction (None / GbcLcd / GbaLcd)
    #[arg(long, default_value_t, help_heading = GB_OPTIONS_HEADING)]
    gbc_color_correction: GbcColorCorrection,

    /// Target 60 FPS instead of ~59.73 FPS
    #[arg(long, default_value_t, help_heading = GB_OPTIONS_HEADING)]
    gb_audio_60hz_hack: bool,

    /// Window width in pixels; height must also be set
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    window_width: Option<u32>,

    /// Window height in pixels; width must also be set
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    window_height: Option<u32>,

    /// Launch in fullscreen
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    fullscreen: bool,

    /// wgpu backend (Auto / Vulkan / DirectX12 / OpenGl)
    #[arg(long, default_value_t, help_heading = VIDEO_OPTIONS_HEADING)]
    wgpu_backend: WgpuBackend,

    /// VSync mode (Enabled / Disabled / Fast)
    #[arg(long, default_value_t = VSyncMode::Enabled, help_heading = VIDEO_OPTIONS_HEADING)]
    vsync_mode: VSyncMode,

    /// Manual prescale factor; must be a positive integer
    #[arg(long, help_heading = VIDEO_OPTIONS_HEADING)]
    prescale_factor: Option<u32>,

    /// Scanlines (None / Dim / Black)
    #[arg(long, default_value_t, help_heading = VIDEO_OPTIONS_HEADING)]
    scanlines: Scanlines,

    /// Force display area height to be an integer multiple of native console resolution
    #[arg(long, default_value_t, help_heading = VIDEO_OPTIONS_HEADING)]
    force_integer_height_scaling: bool,

    /// Filter mode (Nearest / Linear)
    #[arg(long, default_value_t = FilterMode::Linear, help_heading = VIDEO_OPTIONS_HEADING)]
    filter_mode: FilterMode,

    /// Preprocess shader (None / HorizontalBlurTwoPixels / HorizontalBlurThreePixels / HorizontalBlurSnesAdaptive / AntiDitherWeak / AntiDitherStrong)
    #[arg(long, default_value_t, help_heading = VIDEO_OPTIONS_HEADING)]
    preprocess_shader: PreprocessShader,

    /// Disable audio sync
    #[arg(long = "no-audio-sync", default_value_t = true, action = clap::ArgAction::SetFalse, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_sync: bool,

    /// Audio device queue size in samples
    #[arg(long, default_value_t = 512, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_device_queue_size: u16,

    /// Internal audio buffer size in samples
    #[arg(long, default_value_t = 64, help_heading = AUDIO_OPTIONS_HEADING)]
    internal_audio_buffer_size: u32,

    /// Audio sync threshold in bytes (1 sample = 2x4 bytes)
    #[arg(long, default_value_t = 8192, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_sync_threshold: u32,

    /// Audio gain in decibels; can be positive or negative
    #[arg(long, default_value_t = 0.0, help_heading = AUDIO_OPTIONS_HEADING)]
    audio_gain_db: f64,

    /// P1 Genesis controller type (ThreeButton / SixButton)
    #[arg(long, default_value_t, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_type: GenesisControllerType,

    /// P1 up key
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_up: Option<String>,

    /// P1 left key
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_left: Option<String>,

    /// P1 right key
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_right: Option<String>,

    /// P1 down key
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_down: Option<String>,

    /// P1 button 1 key (SMS/GG)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_button_1: Option<String>,

    /// P1 button 2 key (SMS/GG)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_button_2: Option<String>,

    /// P1 A button key (Genesis)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_a: Option<String>,

    /// P1 B button key (Genesis)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_b: Option<String>,

    /// P1 C button key (Genesis)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_c: Option<String>,

    /// P1 X button key (Genesis)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_x: Option<String>,

    /// P1 Y button key (Genesis)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_y: Option<String>,

    /// P1 Z button key (Genesis)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_z: Option<String>,

    /// P1 start/pause key
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_start: Option<String>,

    /// P1 mode key (Genesis)
    #[arg(long, help_heading = INPUT_OPTIONS_HEADING)]
    input_p1_mode: Option<String>,

    /// Joystick axis deadzone
    #[arg(long, default_value_t = 8000, help_heading = INPUT_OPTIONS_HEADING)]
    joy_axis_deadzone: i16,

    /// Fast forward multiplier
    #[arg(long, default_value_t = 2, help_heading = HOTKEY_OPTIONS_HEADING)]
    fast_forward_multiplier: u64,

    /// Rewind buffer length in seconds
    #[arg(long, default_value_t = 10, help_heading = HOTKEY_OPTIONS_HEADING)]
    rewind_buffer_length_seconds: u64,

    /// Quit hotkey
    #[arg(long, default_value_t = String::from("Escape"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_quit: String,

    /// Toggle fullscreen hotkey
    #[arg(long, default_value_t = String::from("F9"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_toggle_fullscreen: String,

    /// Save state hotkey
    #[arg(long, default_value_t = String::from("F5"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_save_state: String,

    /// Load state hotkey
    #[arg(long, default_value_t = String::from("F6"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_load_state: String,

    /// Soft reset hotkey
    #[arg(long, default_value_t = String::from("F1"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_soft_reset: String,

    /// Hard reset hotkey
    #[arg(long, default_value_t = String::from("F2"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_hard_reset: String,

    /// Pause hotkey
    #[arg(long, default_value_t = String::from("P"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_pause: String,

    /// Step frame hotkey
    #[arg(long, default_value_t = String::from("N"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_step_frame: String,

    /// Fast forward hotkey
    #[arg(long, default_value_t = String::from("Tab"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_fast_forward: String,

    /// Rewind hotkey
    #[arg(long, default_value_t = String::from("`"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_rewind: String,

    /// Open memory viewer window hotkey
    #[arg(long, default_value_t = String::from("'"), help_heading = HOTKEY_OPTIONS_HEADING)]
    hotkey_open_debugger: String,
}

impl Args {
    fn validate(&self) {
        assert!(
            self.joy_axis_deadzone >= 0,
            "joy_axis_deadzone must be non-negative; was {}",
            self.joy_axis_deadzone
        );
    }

    fn window_size(&self) -> Option<WindowSize> {
        match (self.window_width, self.window_height) {
            (Some(width), Some(height)) => Some(WindowSize { width, height }),
            (None, None) => None,
            (Some(_), None) | (None, Some(_)) => {
                panic!("Window width and height must either be both set or neither set")
            }
        }
    }

    fn renderer_config(&self) -> RendererConfig {
        let prescale_mode = match self.prescale_factor {
            Some(prescale_factor) => PrescaleMode::Manual(
                PrescaleFactor::try_from(prescale_factor)
                    .expect("prescale factor must be non-zero"),
            ),
            None => PrescaleMode::Auto,
        };

        RendererConfig {
            wgpu_backend: self.wgpu_backend,
            vsync_mode: self.vsync_mode,
            prescale_mode,
            scanlines: self.scanlines,
            force_integer_height_scaling: self.force_integer_height_scaling,
            filter_mode: self.filter_mode,
            preprocess_shader: self.preprocess_shader,
            use_webgl2_limits: false,
        }
    }

    fn smsgg_keyboard_config(&self) -> SmsGgInputConfig<KeyboardInput> {
        let default = SmsGgInputConfig::default();
        SmsGgInputConfig {
            p1: SmsGgControllerConfig {
                up: self.input_p1_up.as_ref().map(keyboard_input).or(default.p1.up),
                left: self.input_p1_left.as_ref().map(keyboard_input).or(default.p1.left),
                right: self.input_p1_right.as_ref().map(keyboard_input).or(default.p1.right),
                down: self.input_p1_down.as_ref().map(keyboard_input).or(default.p1.down),
                button1: self.input_p1_button_1.as_ref().map(keyboard_input).or(default.p1.button1),
                button2: self.input_p1_button_2.as_ref().map(keyboard_input).or(default.p1.button2),
            },
            p2: default.p2,
            pause: self.input_p1_start.as_ref().map(keyboard_input).or(default.pause),
        }
    }

    fn genesis_keyboard_config(&self) -> GenesisInputConfig<KeyboardInput> {
        let default = GenesisInputConfig::default();
        GenesisInputConfig {
            p1: GenesisControllerConfig {
                up: self.input_p1_up.as_ref().map(keyboard_input).or(default.p1.up),
                left: self.input_p1_left.as_ref().map(keyboard_input).or(default.p1.left),
                right: self.input_p1_right.as_ref().map(keyboard_input).or(default.p1.right),
                down: self.input_p1_down.as_ref().map(keyboard_input).or(default.p1.down),
                a: self.input_p1_a.as_ref().map(keyboard_input).or(default.p1.a),
                b: self.input_p1_b.as_ref().map(keyboard_input).or(default.p1.b),
                c: self.input_p1_c.as_ref().map(keyboard_input).or(default.p1.c),
                x: self.input_p1_x.as_ref().map(keyboard_input).or(default.p1.x),
                y: self.input_p1_x.as_ref().map(keyboard_input).or(default.p1.y),
                z: self.input_p1_x.as_ref().map(keyboard_input).or(default.p1.z),
                start: self.input_p1_start.as_ref().map(keyboard_input).or(default.p1.start),
                mode: self.input_p1_mode.as_ref().map(keyboard_input).or(default.p1.mode),
            },
            p2: default.p2,
        }
    }

    fn hotkey_config(&self) -> HotkeyConfig {
        HotkeyConfig {
            quit: Some(keyboard_input(&self.hotkey_quit)),
            toggle_fullscreen: Some(keyboard_input(&self.hotkey_toggle_fullscreen)),
            save_state: Some(keyboard_input(&self.hotkey_save_state)),
            load_state: Some(keyboard_input(&self.hotkey_load_state)),
            soft_reset: Some(keyboard_input(&self.hotkey_soft_reset)),
            hard_reset: Some(keyboard_input(&self.hotkey_hard_reset)),
            pause: Some(keyboard_input(&self.hotkey_pause)),
            step_frame: Some(keyboard_input(&self.hotkey_step_frame)),
            fast_forward: Some(keyboard_input(&self.hotkey_fast_forward)),
            rewind: Some(keyboard_input(&self.hotkey_rewind)),
            open_debugger: Some(keyboard_input(&self.hotkey_open_debugger)),
        }
    }

    fn common_config<KC, JC>(
        &self,
        keyboard_inputs: KC,
        joystick_inputs: JC,
    ) -> CommonConfig<KC, JC> {
        assert_ne!(self.fast_forward_multiplier, 0, "Fast forward multiplier must not be 0");

        CommonConfig {
            rom_file_path: self.file_path.clone(),
            audio_sync: self.audio_sync,
            audio_device_queue_size: self.audio_device_queue_size,
            internal_audio_buffer_size: self.internal_audio_buffer_size,
            audio_sync_threshold: self.audio_sync_threshold,
            audio_gain_db: self.audio_gain_db,
            window_size: self.window_size(),
            renderer_config: self.renderer_config(),
            fast_forward_multiplier: self.fast_forward_multiplier,
            rewind_buffer_length_seconds: self.rewind_buffer_length_seconds,
            launch_in_fullscreen: self.fullscreen,
            keyboard_inputs,
            axis_deadzone: self.joy_axis_deadzone,
            joystick_inputs,
            hotkeys: self.hotkey_config(),
            hide_cursor_over_window: self.hide_cursor_over_window,
        }
    }

    fn genesis_config(&self) -> GenesisConfig {
        let keyboard_inputs = self.genesis_keyboard_config();
        let common = self.common_config(keyboard_inputs, GenesisInputConfig::default());
        GenesisConfig {
            common,
            forced_timing_mode: self.forced_timing_mode,
            forced_region: self.genesis_region,
            p1_controller_type: self.input_p1_type,
            p2_controller_type: GenesisControllerType::default(),
            aspect_ratio: self.genesis_aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.genesis_adjust_aspect_ratio,
            remove_sprite_limits: self.remove_sprite_limit,
            emulate_non_linear_vdp_dac: self.emulate_non_linear_vdp_dac,
            render_vertical_border: self.genesis_render_vertical_border,
            render_horizontal_border: self.genesis_render_horizontal_border,
            quantize_ym2612_output: self.quantize_ym2612_output,
        }
    }
}

fn keyboard_input(s: &String) -> KeyboardInput {
    KeyboardInput { keycode: s.into() }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn"),
    )
    .init();

    let args = Args::parse();
    args.validate();

    let hardware = args.hardware.unwrap_or_else(|| {
        let file_ext = Path::new(&args.file_path).extension().and_then(OsStr::to_str).unwrap_or("");
        match file_ext {
            "sms" | "gg" => Hardware::MasterSystem,
            "md" | "bin" => Hardware::Genesis,
            "cue" | "chd" => Hardware::SegaCd,
            "nes" => Hardware::Nes,
            "sfc" | "smc" => Hardware::Snes,
            "gb" | "gbc" => Hardware::GameBoy,
            _ => {
                log::warn!("Unrecognized file extension: '{file_ext}' defaulting to Genesis");
                Hardware::Genesis
            }
        }
    });

    log::info!("Running with hardware {hardware}");

    match hardware {
        Hardware::MasterSystem => run_sms(args),
        Hardware::Genesis => run_genesis(args),
        Hardware::SegaCd => run_sega_cd(args),
        Hardware::Nes => run_nes(args),
        Hardware::Snes => run_snes(args),
        Hardware::GameBoy => run_gb(args),
    }
}

fn run_sms(args: Args) -> anyhow::Result<()> {
    let keyboard_inputs = args.smsgg_keyboard_config();
    let common = args.common_config(keyboard_inputs, SmsGgInputConfig::default());
    let config = SmsGgConfig {
        common,
        vdp_version: args.vdp_version,
        psg_version: args.psg_version,
        remove_sprite_limit: args.remove_sprite_limit,
        sms_aspect_ratio: args.sms_aspect_ratio,
        gg_aspect_ratio: args.gg_aspect_ratio,
        sms_region: args.sms_region,
        sms_crop_vertical_border: args.sms_crop_vertical_border,
        sms_crop_left_border: args.sms_crop_left_border,
        fm_sound_unit_enabled: args.sms_fm_unit_enabled,
        overclock_z80: args.smsgg_overclock_z80,
    };

    let mut emulator = jgenesis_native_driver::create_smsgg(config.into())?;
    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_genesis(args: Args) -> anyhow::Result<()> {
    let config = args.genesis_config();

    let mut emulator = jgenesis_native_driver::create_genesis(config.into())?;
    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_sega_cd(args: Args) -> anyhow::Result<()> {
    let bios_file_path = args.bios_path.clone().unwrap_or_else(|| {
        eprintln!(
            "ERROR: BIOS file path (-b / --bios-file-path) is required for Sega CD emulation"
        );
        process::exit(1);
    });

    let config = SegaCdConfig {
        genesis: args.genesis_config(),
        bios_file_path: Some(bios_file_path),
        enable_ram_cartridge: args.enable_ram_cartridge,
        run_without_disc: args.scd_no_disc,
        load_disc_into_ram: args.scd_load_disc_into_ram,
    };

    let mut emulator = jgenesis_native_driver::create_sega_cd(config.into())?;
    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_nes(args: Args) -> anyhow::Result<()> {
    let config = NesConfig {
        common: args.common_config(NesInputConfig::default(), NesInputConfig::default()),
        forced_timing_mode: args.forced_timing_mode,
        aspect_ratio: args.nes_aspect_ratio,
        overscan: Overscan {
            top: args.overscan_top,
            bottom: args.overscan_bottom,
            left: args.overscan_left,
            right: args.overscan_right,
        },
        remove_sprite_limit: args.remove_sprite_limit,
        pal_black_border: args.nes_pal_black_border,
        silence_ultrasonic_triangle_output: args.nes_silence_ultrasonic_triangle,
        audio_refresh_rate_adjustment: args.nes_audio_60hz_hack,
        allow_opposing_joypad_inputs: args.nes_allow_opposing_inputs,
    };

    let mut emulator = jgenesis_native_driver::create_nes(config.into())?;
    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_snes(args: Args) -> anyhow::Result<()> {
    let config = SnesConfig {
        common: args.common_config(SnesInputConfig::default(), SnesInputConfig::default()),
        p2_controller_type: args.snes_p2_controller_type,
        super_scope_config: SuperScopeConfig::default(),
        forced_timing_mode: args.forced_timing_mode,
        aspect_ratio: args.snes_aspect_ratio,
        audio_60hz_hack: args.snes_audio_60hz_hack,
        gsu_overclock_factor: args.gsu_overclock_factor,
        dsp1_rom_path: args.dsp1_rom_path,
        dsp2_rom_path: args.dsp2_rom_path,
        dsp3_rom_path: args.dsp3_rom_path,
        dsp4_rom_path: args.dsp4_rom_path,
        st010_rom_path: args.st010_rom_path,
        st011_rom_path: args.st011_rom_path,
    };

    let mut emulator = jgenesis_native_driver::create_snes(config.into())?;
    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}

fn run_gb(args: Args) -> anyhow::Result<()> {
    let config = GameBoyConfig {
        common: args.common_config(GameBoyInputConfig::default_p1(), GameBoyInputConfig::default()),
        force_dmg_mode: args.force_dmg_mode,
        pretend_to_be_gba: args.pretend_to_be_gba,
        aspect_ratio: args.gb_aspect_ratio,
        gb_palette: args.gb_palette,
        gbc_color_correction: args.gbc_color_correction,
        audio_60hz_hack: args.gb_audio_60hz_hack,
    };

    let mut emulator = jgenesis_native_driver::create_gb(config.into())?;
    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}
