use eframe::NativeOptions;
use egui::Vec2;
use jgenesis_gui::app::{App, AppConfig};
use std::path::Path;

fn main() -> eframe::Result<()> {
    let config = AppConfig::from_file(Path::new("jgenesis-config.toml"));

    let options = NativeOptions {
        initial_window_size: Some(Vec2::new(800.0, 600.0)),
        ..NativeOptions::default()
    };

    eframe::run_native("jgenesis", options, Box::new(|_cc| Box::new(App::new(config))))
}
