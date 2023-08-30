use clap::Parser;
use env_logger::Env;
use genesis_core::GenesisAspectRatio;
use jgenesis_native_driver::config::{
    GenesisConfig, GgAspectRatio, SmsAspectRatio, SmsGgConfig, WindowSize,
};
use jgenesis_native_driver::{FilterMode, PrescaleFactor, RendererConfig, VSyncMode};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use smsgg_core::psg::PsgVersion;
use smsgg_core::VdpVersion;
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

    /// Force SMS/GG VDP version (NtscMasterSystem2 / PalMasterSystem2 / GameGear)
    #[arg(long)]
    vdp_version: Option<VdpVersion>,

    /// Force SMS/GG PSG version (MasterSystem2 / Standard)
    #[arg(long)]
    psg_version: Option<PsgVersion>,

    /// Remove SMS/GG 8-sprite-per-scanline limit which disables sprite flickering
    #[arg(long)]
    remove_sprite_limit: bool,

    /// SMS aspect ratio (Ntsc / Pal / SquarePixels / Stretched)
    #[arg(long, default_value_t)]
    sms_aspect_ratio: SmsAspectRatio,

    /// GG aspect ratio (GgLcd / SquarePixels / Stretched)
    #[arg(long, default_value_t)]
    gg_aspect_ratio: GgAspectRatio,

    /// Genesis aspect ratio (Ntsc / SquarePixels / Stretched)
    #[arg(long, default_value_t)]
    genesis_aspect_ratio: GenesisAspectRatio,

    /// Crop SMS top and bottom border; almost all games display only the background color in this area
    #[arg(long, default_value_t)]
    sms_crop_vertical_border: bool,

    /// Crop SMS left border; many games display only the background color in this area
    #[arg(long, default_value_t)]
    sms_crop_left_border: bool,

    /// Disable audio sync
    #[arg(long = "no-audio-sync", default_value_t = true, action = clap::ArgAction::SetFalse)]
    audio_sync: bool,

    /// Window width in pixels; height must also be set
    #[arg(long)]
    window_width: Option<u32>,

    /// Window height in pixels; width must also be set
    #[arg(long)]
    window_height: Option<u32>,

    /// VSync mode (Enabled / Disabled / Fast)
    #[arg(long, default_value_t = VSyncMode::Enabled)]
    vsync_mode: VSyncMode,

    /// Prescale factor; must be a positive integer
    #[arg(long, default_value_t = 3)]
    prescale_factor: u32,

    /// Filter mode (Nearest / Linear)
    #[arg(long, default_value_t = FilterMode::Linear)]
    filter_mode: FilterMode,
}

impl Args {
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
            vsync_mode: self.vsync_mode,
            prescale_factor,
            filter_mode: self.filter_mode,
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("info,wgpu_core::device::global=warn"),
    )
    .init();

    let args = Args::parse();

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
    let window_size = args.window_size();
    let renderer_config = args.renderer_config();
    let config = SmsGgConfig {
        rom_file_path: args.file_path,
        vdp_version: args.vdp_version,
        psg_version: args.psg_version,
        remove_sprite_limit: args.remove_sprite_limit,
        sms_aspect_ratio: args.sms_aspect_ratio,
        gg_aspect_ratio: args.gg_aspect_ratio,
        sms_crop_vertical_border: args.sms_crop_vertical_border,
        sms_crop_left_border: args.sms_crop_left_border,
        audio_sync: args.audio_sync,
        window_size,
        renderer_config,
    };

    jgenesis_native_driver::run_smsgg(config)
}

fn run_genesis(args: Args) -> anyhow::Result<()> {
    let window_size = args.window_size();
    let renderer_config = args.renderer_config();
    let config = GenesisConfig {
        rom_file_path: args.file_path,
        aspect_ratio: args.genesis_aspect_ratio,
        audio_sync: args.audio_sync,
        window_size,
        renderer_config,
    };

    jgenesis_native_driver::run_genesis(config)
}
