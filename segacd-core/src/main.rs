use env_logger::Env;
use segacd_core::api::SegaCdEmulator;
use std::path::Path;
use std::{env, fs, process};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let mut args = env::args();
    if args.len() < 3 {
        log::error!("ARGS: <bios_path> <cue_path>");
        process::exit(1);
    }

    args.next();
    let bios_path = args.next().unwrap();
    let cue_path = args.next().unwrap();

    let bios = fs::read(Path::new(&bios_path))?;

    let mut emulator = SegaCdEmulator::create(bios, Path::new(&cue_path))?;
    loop {
        emulator.tick();
    }
}
