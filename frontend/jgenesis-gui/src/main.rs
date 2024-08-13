use clap::Parser;
use eframe::NativeOptions;
use egui::{Vec2, ViewportBuilder};
use env_logger::Env;
use jgenesis_gui::app::App;
use std::path::PathBuf;

#[derive(Debug, Parser)]
struct Args {
    /// Use a specific config file path instead of the default path of 'jgenesis-config.toml'
    #[arg(long = "config")]
    config_path: Option<String>,

    /// If set, the GUI will open this file immediately after starting up, and the GUI will exit
    /// when the emulator window is closed
    #[arg(long = "file-path", short = 'f')]
    startup_file_path: Option<String>,
}

// Attempt to detect if the application is running on a Steam Deck, and if it is then override
// the winit scale factor to 1. It defaults to 4.5 on the Steam Deck which results in the GUI
// being completely unusable.
#[cfg(all(unix, not(target_os = "macos")))]
fn steam_deck_dpi_hack() {
    let Ok(mut xhandle) = xrandr::XHandle::open() else {
        return;
    };
    let Ok(monitors) = xhandle.monitors() else {
        return;
    };

    if monitors.len() != 1 {
        return;
    }

    let monitor = &monitors[0];

    if monitor.width_px != 1280 || monitor.height_px != 800 || monitor.outputs.len() != 1 {
        return;
    }

    let output = &monitor.outputs[0];

    let Some(edid) = output.properties.iter().find_map(|(_, property)| match &property.value {
        xrandr::Value::Edid(edid) => Some(edid),
        _ => None,
    }) else {
        return;
    };

    // Display name part of the EDID is always here on the Steam Deck: 'ANX7530 U<LF>'
    if edid[75..87] == [0xFC, 0x00, 0x41, 0x4E, 0x58, 0x37, 0x35, 0x33, 0x30, 0x20, 0x55, 0x0A] {
        log::info!(
            "It looks like this is a Steam Deck; overriding winit scale factor to 1 as otherwise it will default to 4.5"
        );
        std::env::set_var("WINIT_X11_SCALE_FACTOR", "1");
    }
}

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn"),
    )
    .init();

    let args = Args::parse();

    #[cfg(all(unix, not(target_os = "macos")))]
    steam_deck_dpi_hack();

    let config_path =
        args.config_path.map_or_else(jgenesis_native_config::default_config_path, PathBuf::from);
    log::info!("Using config path '{}'", config_path.display());

    if let Some(file_path) = &args.startup_file_path {
        log::info!("Will open file '{file_path}' after starting");
    }

    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size(Vec2::new(800.0, 600.0)),
        ..NativeOptions::default()
    };

    eframe::run_native(
        "jgenesis",
        options,
        Box::new(|cc| {
            Ok(Box::new(App::new(config_path, args.startup_file_path, cc.egui_ctx.clone())))
        }),
    )
}
