use clap::Parser;
use env_logger::Env;
use genesis_core::{GenesisAspectRatio, GenesisControllerType, GenesisRegion, GenesisTimingMode};
use jgenesis_native_driver::config::input::{
    GenesisControllerConfig, GenesisInputConfig, HotkeyConfig, KeyboardInput,
    SmsGgControllerConfig, SmsGgInputConfig,
};
use jgenesis_native_driver::config::{
    CommonConfig, GenesisConfig, GgAspectRatio, SmsAspectRatio, SmsGgConfig, WindowSize,
};
use jgenesis_native_driver::NativeTickEffect;
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use jgenesis_renderer::config::{
    FilterMode, PrescaleFactor, RendererConfig, VSyncMode, WgpuBackend,
};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsRegion, VdpVersion};
use std::ffi::OsStr;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay, EnumFromStr)]
enum Hardware {
    MasterSystem,
    Genesis,
}

#[derive(Parser)]
struct Args {
    /// ROM file path
    #[arg(short = 'f', long)]
    file_path: String,

    /// Hardware (MasterSystem / Genesis)
    ///
    /// Will default based on file extension if not set. MasterSystem is appropriate for both SMS
    /// and Game Gear games.
    #[arg(long)]
    hardware: Option<Hardware>,

    /// Force SMS/GG VDP version (NtscMasterSystem2 / NtscMasterSystem1 / PalMasterSystem2 / PalMasterSystem1 / GameGear)
    #[arg(long)]
    vdp_version: Option<VdpVersion>,

    /// Force SMS/GG PSG version (MasterSystem2 / Standard)
    #[arg(long)]
    psg_version: Option<PsgVersion>,

    /// Force Genesis timing mode (Ntsc / Pal)
    #[arg(long)]
    genesis_timing_mode: Option<GenesisTimingMode>,

    /// Remove SMS/GG 8-sprite-per-scanline limit which disables sprite flickering
    #[arg(long)]
    remove_sprite_limit: bool,

    /// SMS aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, default_value_t)]
    sms_aspect_ratio: SmsAspectRatio,

    /// GG aspect ratio (GgLcd / SquarePixels / Stretched)
    #[arg(long, default_value_t)]
    gg_aspect_ratio: GgAspectRatio,

    /// Genesis aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, default_value_t)]
    genesis_aspect_ratio: GenesisAspectRatio,

    /// Disable automatic pixel aspect ratio adjustment when Genesis interlacing double resolution mode
    /// is enabled
    #[arg(long = "no-genesis-adjust-aspect-ratio", default_value_t = true, action = clap::ArgAction::SetFalse)]
    genesis_adjust_aspect_ratio: bool,

    /// SMS region (International / Domestic)
    #[arg(long, default_value_t)]
    sms_region: SmsRegion,

    /// Force Genesis region (Americas / Japan / Europe)
    #[arg(long)]
    genesis_region: Option<GenesisRegion>,

    /// Crop SMS top and bottom border; almost all games display only the background color in this area
    #[arg(long, default_value_t)]
    sms_crop_vertical_border: bool,

    /// Crop SMS left border; many games display only the background color in this area
    #[arg(long, default_value_t)]
    sms_crop_left_border: bool,

    /// Disable SMS FM sound unit
    #[arg(long = "disable-sms-fm-unit", default_value_t = true, action = clap::ArgAction::SetFalse)]
    sms_fm_unit_enabled: bool,

    /// Overclock the SMS/GG Z80 CPU to 2x speed
    #[arg(long, default_value_t)]
    smsgg_overclock_z80: bool,

    /// Disable audio sync
    #[arg(long = "no-audio-sync", default_value_t = true, action = clap::ArgAction::SetFalse)]
    audio_sync: bool,

    /// Window width in pixels; height must also be set
    #[arg(long)]
    window_width: Option<u32>,

    /// Window height in pixels; width must also be set
    #[arg(long)]
    window_height: Option<u32>,

    /// Launch in fullscreen
    #[arg(long)]
    fullscreen: bool,

    /// wgpu backend (Auto / Vulkan / DirectX12 / Metal / OpenGl)
    #[arg(long, default_value_t)]
    wgpu_backend: WgpuBackend,

    /// VSync mode (Enabled / Disabled / Fast)
    #[arg(long, default_value_t = VSyncMode::Enabled)]
    vsync_mode: VSyncMode,

    /// Prescale factor; must be a positive integer
    #[arg(long, default_value_t = 3)]
    prescale_factor: u32,

    /// Force display area height to be an integer multiple of native console resolution
    #[arg(long, default_value_t)]
    force_integer_height_scaling: bool,

    /// Filter mode (Nearest / Linear)
    #[arg(long, default_value_t = FilterMode::Linear)]
    filter_mode: FilterMode,

    /// Fast forward multiplier
    #[arg(long, default_value_t = 2)]
    fast_forward_multiplier: u64,

    /// Rewind buffer length in seconds
    #[arg(long, default_value_t = 10)]
    rewind_buffer_length_seconds: u64,

    /// P1 Genesis controller type (ThreeButton / SixButton)
    #[arg(long, default_value_t)]
    input_p1_type: GenesisControllerType,

    /// P1 up key
    #[arg(long)]
    input_p1_up: Option<String>,

    /// P1 left key
    #[arg(long)]
    input_p1_left: Option<String>,

    /// P1 right key
    #[arg(long)]
    input_p1_right: Option<String>,

    /// P1 down key
    #[arg(long)]
    input_p1_down: Option<String>,

    /// P1 button 1 key (SMS/GG)
    #[arg(long)]
    input_p1_button_1: Option<String>,

    /// P1 button 2 key (SMS/GG)
    #[arg(long)]
    input_p1_button_2: Option<String>,

    /// P1 A button key (Genesis)
    #[arg(long)]
    input_p1_a: Option<String>,

    /// P1 B button key (Genesis)
    #[arg(long)]
    input_p1_b: Option<String>,

    /// P1 C button key (Genesis)
    #[arg(long)]
    input_p1_c: Option<String>,

    /// P1 X button key (Genesis)
    #[arg(long)]
    input_p1_x: Option<String>,

    /// P1 Y button key (Genesis)
    #[arg(long)]
    input_p1_y: Option<String>,

    /// P1 Z button key (Genesis)
    #[arg(long)]
    input_p1_z: Option<String>,

    /// P1 start/pause key
    #[arg(long)]
    input_p1_start: Option<String>,

    /// P1 mode key (Genesis)
    #[arg(long)]
    input_p1_mode: Option<String>,

    /// Joystick axis deadzone
    #[arg(long, default_value_t = 8000)]
    joy_axis_deadzone: i16,

    /// Quit hotkey
    #[arg(long, default_value_t = String::from("Escape"))]
    hotkey_quit: String,

    /// Toggle fullscreen hotkey
    #[arg(long, default_value_t = String::from("F9"))]
    hotkey_toggle_fullscreen: String,

    /// Save state hotkey
    #[arg(long, default_value_t = String::from("F5"))]
    hotkey_save_state: String,

    /// Load state hotkey
    #[arg(long, default_value_t = String::from("F6"))]
    hotkey_load_state: String,

    /// Soft reset hotkey
    #[arg(long, default_value_t = String::from("F1"))]
    hotkey_soft_reset: String,

    /// Hard reset hotkey
    #[arg(long, default_value_t = String::from("F2"))]
    hotkey_hard_reset: String,

    /// Fast forward hotkey
    #[arg(long, default_value_t = String::from("Tab"))]
    hotkey_fast_forward: String,

    /// Rewind hotkey
    #[arg(long, default_value_t = String::from("`"))]
    hotkey_rewind: String,

    /// CRAM debug window hotkey
    #[arg(long, default_value_t = String::from(";"))]
    hotkey_cram_debug: String,

    /// VRAM debug window hotkey
    #[arg(long, default_value_t = String::from("'"))]
    hotkey_vram_debug: String,
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
        let prescale_factor = PrescaleFactor::try_from(self.prescale_factor)
            .expect("prescale factor must be non-zero");
        RendererConfig {
            wgpu_backend: self.wgpu_backend,
            vsync_mode: self.vsync_mode,
            prescale_factor,
            force_integer_height_scaling: self.force_integer_height_scaling,
            filter_mode: self.filter_mode,
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
                button_1: self
                    .input_p1_button_1
                    .as_ref()
                    .map(keyboard_input)
                    .or(default.p1.button_1),
                button_2: self
                    .input_p1_button_2
                    .as_ref()
                    .map(keyboard_input)
                    .or(default.p1.button_2),
                pause: self.input_p1_start.as_ref().map(keyboard_input).or(default.p1.pause),
            },
            p2: default.p2,
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
            fast_forward: Some(keyboard_input(&self.hotkey_fast_forward)),
            rewind: Some(keyboard_input(&self.hotkey_rewind)),
            open_cram_debug: Some(keyboard_input(&self.hotkey_cram_debug)),
            open_vram_debug: Some(keyboard_input(&self.hotkey_vram_debug)),
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
            window_size: self.window_size(),
            renderer_config: self.renderer_config(),
            fast_forward_multiplier: self.fast_forward_multiplier,
            rewind_buffer_length_seconds: self.rewind_buffer_length_seconds,
            launch_in_fullscreen: self.fullscreen,
            keyboard_inputs,
            axis_deadzone: self.joy_axis_deadzone,
            joystick_inputs,
            hotkeys: self.hotkey_config(),
        }
    }
}

fn keyboard_input(s: &String) -> KeyboardInput {
    KeyboardInput { keycode: s.into() }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("info,wgpu_core::device::global=warn"),
    )
    .init();

    let args = Args::parse();
    args.validate();

    let hardware = args.hardware.unwrap_or_else(|| {
        let file_ext = Path::new(&args.file_path).extension().and_then(OsStr::to_str).unwrap();
        match file_ext {
            "sms" | "gg" => Hardware::MasterSystem,
            "md" | "bin" => Hardware::Genesis,
            _ => {
                log::warn!("Unrecognized file extension: {file_ext} defaulting to SMS");
                Hardware::MasterSystem
            }
        }
    });

    log::info!("Running with hardware {hardware}");

    match hardware {
        Hardware::MasterSystem => run_sms(args),
        Hardware::Genesis => run_genesis(args),
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
    let keyboard_inputs = args.genesis_keyboard_config();
    let common = args.common_config(keyboard_inputs, GenesisInputConfig::default());
    let config = GenesisConfig {
        common,
        forced_timing_mode: args.genesis_timing_mode,
        forced_region: args.genesis_region,
        p1_controller_type: args.input_p1_type,
        p2_controller_type: GenesisControllerType::default(),
        aspect_ratio: args.genesis_aspect_ratio,
        adjust_aspect_ratio_in_2x_resolution: args.genesis_adjust_aspect_ratio,
    };

    let mut emulator = jgenesis_native_driver::create_genesis(config.into())?;
    while emulator.render_frame()? != NativeTickEffect::Exit {}

    Ok(())
}
