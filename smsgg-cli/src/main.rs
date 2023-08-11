use clap::Parser;
use env_logger::Env;
use smsgg_core::{PsgVersion, SmsGgConfig, VdpVersion};

#[derive(Parser)]
struct Args {
    /// ROM file path
    #[arg(short = 'f', long)]
    file_path: String,

    /// VDP version
    #[arg(long, default_value_t)]
    vdp_version: VdpVersion,

    /// PSG version
    #[arg(long, default_value_t)]
    psg_version: PsgVersion,
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let config = SmsGgConfig {
        rom_file_path: args.file_path,
        vdp_version: args.vdp_version,
        psg_version: args.psg_version,
    };

    smsgg_core::run(config);
}
