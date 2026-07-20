use clap::Parser;
use egui_sdl3_wgpu::{FrameOptions, FrameRunEffect};
use env_logger::Env;
use image::{DynamicImage, ImageFormat};
use jgenesis_gui::GuiEmulatorRunner;
use jgenesis_gui::app::{App, ConfigInfo, LoadAtStartup};
use jgenesis_native_config::AppConfig;
use jgenesis_native_config::paths::{ConfigDirs, ConfigWithPath};
use jgenesis_native_driver::SdlSubsystems;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Debug, Parser)]
struct Args {
    /// Use a specific config file path instead of the default path of 'jgenesis-config.toml'
    #[arg(long = "config")]
    config_path: Option<PathBuf>,

    /// If set, the GUI will open this file immediately after starting up, and the GUI will exit
    /// when the emulator window is closed
    #[arg(long = "file-path", short = 'f')]
    startup_file_path: Option<PathBuf>,

    /// In combination with -f, attempt to load the specified save state when launching the game.
    /// This arg has no effect if -f/--file-path is not set
    #[arg(long, value_name = "SLOT")]
    load_save_state: Option<usize>,

    /// Print version string and immediately exit
    #[arg(short = 'v', long, default_value_t = false, action = clap::ArgAction::SetTrue)]
    version: bool,
}

impl Args {
    fn fix_appimage_relative_paths(mut self) -> Self {
        if let Some(config_path) = self.config_path {
            self.config_path = Some(jgenesis_common::fix_appimage_relative_path(config_path));
        }

        if let Some(startup_file_path) = self.startup_file_path {
            self.startup_file_path =
                Some(jgenesis_common::fix_appimage_relative_path(startup_file_path));
        }

        self
    }

    fn load_at_startup(&self) -> Option<LoadAtStartup> {
        self.startup_file_path.as_ref().map(|file_path| LoadAtStartup {
            file_path: file_path.clone(),
            load_state_slot: self.load_save_state,
        })
    }
}

fn initial_gui_size(config: &AppConfig) -> (f32, f32) {
    (
        f32_max(jgenesis_native_config::DEFAULT_GUI_WIDTH, config.gui_window_width),
        f32_max(jgenesis_native_config::DEFAULT_GUI_HEIGHT, config.gui_window_height),
    )
}

fn f32_max(value: f32, max: f32) -> f32 {
    if value < max { max } else { value }
}

fn load_icon() -> DynamicImage {
    const ICON: &[u8] = include_bytes!("../../256x256.png");

    image::load_from_memory_with_format(ICON, ImageFormat::Png).expect("Failed to load GUI icon")
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or(jgenesis_common::DEFAULT_LOG_FILTER),
    )
    .init();

    let args = Args::parse().fix_appimage_relative_paths();

    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config_dirs = ConfigDirs::new();
    let config_dir_type = config_dirs.default_dir_type(args.config_path.clone());
    let config_with_path = ConfigWithPath::load_from_dir_or_default(
        &config_dirs,
        &config_dir_type,
        AppConfig::default,
    );

    if let Some(file_path) = &args.startup_file_path {
        log::info!("Will open file '{}' after starting", file_path.display());
    }

    let (gui_width, gui_height) = initial_gui_size(&config_with_path.config);

    let icon = load_icon();

    let config_info = ConfigInfo {
        initial_config: config_with_path.config,
        config_path: config_with_path.path,
        config_dirs,
        config_dir_type,
    };
    let load_at_startup = args.load_at_startup();

    let sdl = SdlSubsystems::init()?;

    let options = FrameOptions {
        window_width: gui_width.round() as u32,
        window_height: gui_height.round() as u32,
        icon: Some(icon),
        ..FrameOptions::default()
    };

    let mut frame = egui_sdl3_wgpu::Frame::new("jgenesis", &sdl.video.borrow(), options)?;

    let (mut emu_runner, emu_runner_handle) =
        GuiEmulatorRunner::new(sdl.clone(), frame.egui_ctx().clone());

    let mut app =
        App::new(config_info, load_at_startup, frame.egui_ctx().clone(), &sdl, emu_runner_handle);

    loop {
        emu_runner.run(|event| {
            frame.handle_sdl_event(event);
            app.handle_sdl_event(event, frame.egui_ctx(), frame.window_id());
        });

        if frame.run(|ui, _ctx| app.ui(ui))? == FrameRunEffect::Closed {
            return Ok(());
        }

        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_icon_does_not_panic() {
        let _ = load_icon();
    }
}
