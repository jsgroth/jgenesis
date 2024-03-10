use crate::app::{App, AppConfig, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Window};
use genesis_core::{GenesisAspectRatio, GenesisRegion};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::{GenesisConfig, SegaCdConfig};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisAppConfig {
    #[serde(default)]
    forced_timing_mode: Option<TimingMode>,
    #[serde(default)]
    forced_region: Option<GenesisRegion>,
    #[serde(default)]
    aspect_ratio: GenesisAspectRatio,
    #[serde(default = "true_fn")]
    adjust_aspect_ratio_in_2x_resolution: bool,
    #[serde(default)]
    remove_sprite_limits: bool,
    #[serde(default)]
    emulate_non_linear_vdp_dac: bool,
    #[serde(default)]
    render_vertical_border: bool,
    #[serde(default)]
    render_horizontal_border: bool,
    #[serde(default = "true_fn")]
    quantize_ym2612_output: bool,
}

const fn true_fn() -> bool {
    true
}

impl Default for GenesisAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegaCdAppConfig {
    bios_path: Option<String>,
    #[serde(default = "true_fn")]
    enable_ram_cartridge: bool,
}

impl Default for SegaCdAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    pub(super) fn genesis_config(&self, path: String) -> Box<GenesisConfig> {
        Box::new(GenesisConfig {
            common: self.common_config(
                path,
                self.inputs.genesis_keyboard.clone(),
                self.inputs.genesis_joystick.clone(),
            ),
            p1_controller_type: self.inputs.genesis_p1_type,
            p2_controller_type: self.inputs.genesis_p2_type,
            forced_timing_mode: self.genesis.forced_timing_mode,
            forced_region: self.genesis.forced_region,
            aspect_ratio: self.genesis.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.genesis.adjust_aspect_ratio_in_2x_resolution,
            remove_sprite_limits: self.genesis.remove_sprite_limits,
            emulate_non_linear_vdp_dac: self.genesis.emulate_non_linear_vdp_dac,
            render_vertical_border: self.genesis.render_vertical_border,
            render_horizontal_border: self.genesis.render_horizontal_border,
            quantize_ym2612_output: self.genesis.quantize_ym2612_output,
        })
    }

    pub(super) fn sega_cd_config(&self, path: String) -> Box<SegaCdConfig> {
        Box::new(SegaCdConfig {
            genesis: *self.genesis_config(path),
            bios_file_path: self.sega_cd.bios_path.clone(),
            enable_ram_cartridge: self.sega_cd.enable_ram_cartridge,
            run_without_disc: false,
        })
    }
}

impl App {
    pub(super) fn render_genesis_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis General Settings").open(&mut open).resizable(true).show(ctx, |ui| {
            let emu_thread_status = self.emu_thread.status();
            let running_genesis = emu_thread_status != EmuThreadStatus::RunningGenesis
                && emu_thread_status != EmuThreadStatus::RunningSegaCd;

            ui.group(|ui| {
                ui.set_enabled(running_genesis);

                ui.label("Timing / display mode");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.genesis.forced_timing_mode, None, "Auto");
                    ui.radio_value(
                        &mut self.config.genesis.forced_timing_mode,
                        Some(TimingMode::Ntsc),
                        "NTSC",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.forced_timing_mode,
                        Some(TimingMode::Pal),
                        "PAL",
                    );
                });
            });

            ui.group(|ui| {
                ui.set_enabled(running_genesis);

                ui.label("Region");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.genesis.forced_region, None, "Auto");
                    ui.radio_value(
                        &mut self.config.genesis.forced_region,
                        Some(GenesisRegion::Americas),
                        "Americas",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.forced_region,
                        Some(GenesisRegion::Japan),
                        "Japan",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.forced_region,
                        Some(GenesisRegion::Europe),
                        "Europe",
                    );
                });
            });

            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.set_enabled(self.emu_thread.status() != EmuThreadStatus::RunningSegaCd);

                let bios_path_str =
                    self.config.sega_cd.bios_path.as_ref().map_or("<None>", String::as_str);
                if ui.button(bios_path_str).clicked() {
                    if let Some(bios_path) =
                        FileDialog::new().add_filter("bin", &["bin"]).pick_file()
                    {
                        self.config.sega_cd.bios_path =
                            Some(bios_path.to_string_lossy().to_string());
                    }
                }

                ui.label("Sega CD BIOS path");
            });

            ui.add_space(5.0);
            ui.checkbox(
                &mut self.config.sega_cd.enable_ram_cartridge,
                "Enable Sega CD RAM cartridge",
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisGeneral);
        }
    }

    pub(super) fn render_genesis_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::Ntsc,
                        "NTSC",
                    )
                    .on_hover_text("32:35 pixel aspect ratio in 320px mode, 8:7 in 256px mode");
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::Pal,
                        "PAL",
                    )
                    .on_hover_text("11:10 pixel aspect ratio in 320px mode, 11:8 in 256px mode");
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::SquarePixels,
                        "Square pixels",
                    )
                    .on_hover_text("1:1 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::Stretched,
                        "Stretched",
                    )
                    .on_hover_text("Stretch image to fill the screen");
                });
            });

            ui.checkbox(
                &mut self.config.genesis.adjust_aspect_ratio_in_2x_resolution,
                "Automatically double pixel aspect ratio in double vertical resolution mode",
            );

            ui.checkbox(
                &mut self.config.genesis.remove_sprite_limits,
                "Remove sprite-per-scanline and sprite-pixel-per-scanline limits",
            )
            .on_hover_text("Can reduce sprite flickering, but can also cause visual glitches");

            ui.checkbox(
                &mut self.config.genesis.emulate_non_linear_vdp_dac,
                "Emulate the VDP's non-linear color DAC",
            )
            .on_hover_text("Tends to brighten darker colors and darken brighter colors");

            ui.checkbox(&mut self.config.genesis.render_vertical_border, "Render vertical border");

            ui.checkbox(
                &mut self.config.genesis.render_horizontal_border,
                "Render horizontal border",
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisVideo);
        }
    }

    pub(super) fn render_genesis_audio_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(
                &mut self.config.genesis.quantize_ym2612_output,
                "Quantize YM2612 channel output",
            )
            .on_hover_text(
                "Quantize channel outputs from 14 bits to 9 bits to emulate the YM2612's 9-bit DAC",
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisAudio);
        }
    }
}
