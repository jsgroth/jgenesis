use crate::app::{App, AppConfig, OpenWindow};
use egui::{Context, Window};
use gb_core::api::GbPalette;
use jgenesis_native_driver::config::GameBoyConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBoyAppConfig {
    #[serde(default)]
    force_dmg_mode: bool,
    #[serde(default)]
    gb_palette: GbPalette,
}

impl Default for GameBoyAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    pub(super) fn gb_config(&self, path: String) -> Box<GameBoyConfig> {
        Box::new(GameBoyConfig {
            common: self.common_config(
                path,
                self.inputs.to_gb_keyboard_config(),
                self.inputs.gb_joystick.clone(),
            ),
            force_dmg_mode: self.game_boy.force_dmg_mode,
            gb_palette: self.game_boy.gb_palette,
        })
    }
}

impl App {
    pub(super) fn render_gb_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Game Boy General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(
                &mut self.config.game_boy.force_dmg_mode,
                "Force DMG mode in software with CGB support",
            )
            .on_hover_text("DMG = original Game Boy, CGB = Game Boy Color");
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GameBoyGeneral);
        }
    }

    pub(super) fn render_gb_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Game Boy Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("GB color palette");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.game_boy.gb_palette,
                        GbPalette::BlackAndWhite,
                        "Black and white",
                    );
                    ui.radio_value(
                        &mut self.config.game_boy.gb_palette,
                        GbPalette::GreenTint,
                        "Green tint",
                    );
                    ui.radio_value(
                        &mut self.config.game_boy.gb_palette,
                        GbPalette::LimeGreen,
                        "Lime green",
                    );
                });
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GameBoyVideo);
        }
    }
}