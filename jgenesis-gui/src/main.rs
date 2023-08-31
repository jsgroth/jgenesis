use eframe::NativeOptions;
use egui::Vec2;
use env_logger::Env;
use jgenesis_gui::app::App;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("info,wgpu_core::device::global=warn"),
    )
    .init();

    let config_path = PathBuf::from("jgenesis-config.toml");

    let options = NativeOptions {
        initial_window_size: Some(Vec2::new(800.0, 600.0)),
        ..NativeOptions::default()
    };

    eframe::run_native("jgenesis", options, Box::new(|_cc| Box::new(App::new(config_path))))
}
