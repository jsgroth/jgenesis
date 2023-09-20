use env_logger::Env;
use genesis_core::GenesisInputs;
use jgenesis_traits::frontend::{
    AudioOutput, Color, FrameSize, PixelAspectRatio, Renderer, SaveWriter, TickableEmulator,
};
use segacd_core::api::SegaCdEmulator;
use std::path::Path;
use std::{env, fs, process};

struct Null;

impl Renderer for Null {
    type Err = ();

    fn render_frame(
        &mut self,
        _frame_buffer: &[Color],
        _frame_size: FrameSize,
        _pixel_aspect_ratio: Option<PixelAspectRatio>,
    ) -> Result<(), Self::Err> {
        Ok(())
    }
}

impl AudioOutput for Null {
    type Err = ();

    fn push_sample(&mut self, _sample_l: f64, _sample_r: f64) -> Result<(), Self::Err> {
        Ok(())
    }
}

impl SaveWriter for Null {
    type Err = ();

    fn persist_save(&mut self, _save_bytes: &[u8]) -> Result<(), Self::Err> {
        Ok(())
    }
}

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
        emulator.tick(&mut Null, &mut Null, &GenesisInputs::default(), &mut Null).unwrap();
    }
}
