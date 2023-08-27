use clap::Parser;
use env_logger::Env;
use genesis_core::GenesisConfig;
use jgenesis_native_driver::config::SmsGgConfig;
use smsgg_core::psg::PsgVersion;
use smsgg_core::VdpVersion;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hardware {
    MasterSystem,
    Genesis,
}

impl Display for Hardware {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MasterSystem => write!(f, "MasterSystem"),
            Self::Genesis => write!(f, "Genesis"),
        }
    }
}

impl FromStr for Hardware {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "MasterSystem" => Ok(Self::MasterSystem),
            "Genesis" => Ok(Self::Genesis),
            _ => Err(format!("invalid hardware string: {s}")),
        }
    }
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
    hardware: Option<Hardware>,

    /// VDP version
    #[arg(long)]
    vdp_version: Option<VdpVersion>,

    /// PSG version
    #[arg(long)]
    psg_version: Option<PsgVersion>,

    /// Crop SMS top and bottom borders (16px each); all games display only the overscan color here
    #[arg(long)]
    crop_sms_vertical_border: bool,

    /// Crop SMS left border (8px); many games hide this part of the screen to enable
    /// smooth sprite scrolling off the left edge
    #[arg(long)]
    crop_sms_left_border: bool,

    /// Remove 8-sprite-per-scanline limit which disables sprite flickering
    #[arg(long)]
    remove_sprite_limit: bool,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let hardware = args.hardware.unwrap_or_else(|| {
        let file_ext = Path::new(&args.file_path)
            .extension()
            .and_then(OsStr::to_str)
            .unwrap();
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
    let config = SmsGgConfig {
        rom_file_path: args.file_path,
        vdp_version: args.vdp_version,
        psg_version: args.psg_version,
        crop_sms_vertical_border: args.crop_sms_vertical_border,
        crop_sms_left_border: args.crop_sms_left_border,
        remove_sprite_limit: args.remove_sprite_limit,
    };

    jgenesis_native_driver::run_smsgg(config)
}

fn run_genesis(args: Args) -> anyhow::Result<()> {
    let config = GenesisConfig {
        rom_file_path: args.file_path,
    };

    genesis_core::run(config)
}
