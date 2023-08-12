use clap::Parser;
use env_logger::Env;
use jgsms_core::{PsgVersion, SmsGgConfig, VdpVersion};

#[derive(Parser)]
struct Args {
    /// ROM file path
    #[arg(short = 'f', long)]
    file_path: String,

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
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let config = SmsGgConfig {
        rom_file_path: args.file_path,
        vdp_version: args.vdp_version,
        psg_version: args.psg_version,
        crop_sms_vertical_border: args.crop_sms_vertical_border,
        crop_sms_left_border: args.crop_sms_left_border,
    };

    jgsms_core::run(config);
}
