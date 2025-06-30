use crate::app::widgets::{BiosErrorStrings, RenderErrorEffect};
use crate::app::{App, widgets};
use egui::Context;
use jgenesis_native_driver::extensions::Console;
use rfd::FileDialog;
use std::path::PathBuf;

impl App {
    pub(super) fn render_gba_bios_error(
        &mut self,
        ctx: &Context,
        open: &mut bool,
    ) -> RenderErrorEffect {
        widgets::render_bios_error(
            ctx,
            open,
            BiosErrorStrings {
                title: "Missing GBA BIOS",
                text: "No GBA BIOS path is configured. A BIOS ROM is required for GBA emulation.",
                button_label: "Configure GBA BIOS path",
            },
            &mut self.config.game_boy_advance.bios_path,
            Console::GameBoyAdvance,
            pick_bios_path,
        )
    }
}

fn pick_bios_path() -> Option<PathBuf> {
    FileDialog::new().add_filter("bin", &["bin"]).add_filter("All Files", &["*"]).pick_file()
}
