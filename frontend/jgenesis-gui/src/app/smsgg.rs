use crate::app::{App, AppConfig, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Window};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::{GgAspectRatio, SmsAspectRatio, SmsGgConfig};
use serde::{Deserialize, Serialize};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsRegion, VdpVersion};
use std::ffi::OsStr;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SmsModel {
    Sms1,
    #[default]
    Sms2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsGgAppConfig {
    psg_version: Option<PsgVersion>,
    #[serde(default)]
    remove_sprite_limit: bool,
    #[serde(default)]
    sms_aspect_ratio: SmsAspectRatio,
    #[serde(default)]
    gg_aspect_ratio: GgAspectRatio,
    #[serde(default)]
    sms_region: SmsRegion,
    #[serde(default)]
    sms_timing_mode: TimingMode,
    #[serde(default)]
    sms_model: SmsModel,
    #[serde(default)]
    sms_crop_vertical_border: bool,
    #[serde(default)]
    sms_crop_left_border: bool,
    #[serde(default = "true_fn")]
    fm_sound_unit_enabled: bool,
    #[serde(default)]
    overclock_z80: bool,
}

const fn true_fn() -> bool {
    true
}

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    pub(super) fn smsgg_config(&self, path: String) -> Box<SmsGgConfig> {
        let vdp_version = if Path::new(&path).extension().and_then(OsStr::to_str) == Some("sms") {
            match (self.smsgg.sms_timing_mode, self.smsgg.sms_model) {
                (TimingMode::Ntsc, SmsModel::Sms2) => Some(VdpVersion::NtscMasterSystem2),
                (TimingMode::Pal, SmsModel::Sms2) => Some(VdpVersion::PalMasterSystem2),
                (TimingMode::Ntsc, SmsModel::Sms1) => Some(VdpVersion::NtscMasterSystem1),
                (TimingMode::Pal, SmsModel::Sms1) => Some(VdpVersion::PalMasterSystem1),
            }
        } else {
            None
        };

        Box::new(SmsGgConfig {
            common: self.common_config(
                path,
                self.inputs.smsgg_keyboard.clone(),
                self.inputs.smsgg_joystick.clone(),
            ),
            vdp_version,
            psg_version: self.smsgg.psg_version,
            remove_sprite_limit: self.smsgg.remove_sprite_limit,
            sms_aspect_ratio: self.smsgg.sms_aspect_ratio,
            gg_aspect_ratio: self.smsgg.gg_aspect_ratio,
            sms_region: self.smsgg.sms_region,
            sms_crop_vertical_border: self.smsgg.sms_crop_vertical_border,
            sms_crop_left_border: self.smsgg.sms_crop_left_border,
            fm_sound_unit_enabled: self.smsgg.fm_sound_unit_enabled,
            overclock_z80: self.smsgg.overclock_z80,
        })
    }
}

impl App {
    pub(super) fn render_smsgg_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Sega Master System timing / display mode");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.smsgg.sms_timing_mode,
                        TimingMode::Ntsc,
                        "NTSC",
                    );
                    ui.radio_value(&mut self.config.smsgg.sms_timing_mode, TimingMode::Pal, "PAL");
                });
            });

            ui.group(|ui| {
                ui.label("Sega Master System VDP version");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.smsgg.sms_model, SmsModel::Sms2, "SMS2");

                    ui.radio_value(&mut self.config.smsgg.sms_model, SmsModel::Sms1, "SMS1")
                        .on_hover_text("Emulates an SMS1 quirk that is required for the Japanese version of Ys");
                });
            });

            ui.group(|ui| {
                ui.label("Sega Master System region");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.smsgg.sms_region,
                        SmsRegion::International,
                        "International / Overseas",
                    );
                    ui.radio_value(
                        &mut self.config.smsgg.sms_region,
                        SmsRegion::Domestic,
                        "Domestic (Japan)",
                    );
                });
            });

            ui.checkbox(&mut self.config.smsgg.overclock_z80, "Double Z80 CPU speed")
                .on_hover_text(
                    "Can reduce slowdown in some games but can also cause major glitches",
                );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgGeneral);
        }
    }

    pub(super) fn render_smsgg_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Sega Master System aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.smsgg.sms_aspect_ratio,
                        SmsAspectRatio::Ntsc,
                        "NTSC",
                    )
                    .on_hover_text("8:7 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.smsgg.sms_aspect_ratio,
                        SmsAspectRatio::Pal,
                        "PAL",
                    )
                    .on_hover_text("11:8 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.smsgg.sms_aspect_ratio,
                        SmsAspectRatio::SquarePixels,
                        "Square pixels",
                    )
                    .on_hover_text("1:1 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.smsgg.sms_aspect_ratio,
                        SmsAspectRatio::Stretched,
                        "Stretched",
                    )
                    .on_hover_text("Stretch image to fill the screen");
                });
            });

            ui.group(|ui| {
                ui.label("Game Gear aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.smsgg.gg_aspect_ratio,
                        GgAspectRatio::GgLcd,
                        "Game Gear LCD",
                    )
                    .on_hover_text("6:5 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.smsgg.gg_aspect_ratio,
                        GgAspectRatio::SquarePixels,
                        "Square pixels",
                    )
                    .on_hover_text("1:1 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.smsgg.gg_aspect_ratio,
                        GgAspectRatio::Stretched,
                        "Stretched",
                    )
                    .on_hover_text("Stretch image to fill the screen");
                });
            });

            ui.checkbox(
                &mut self.config.smsgg.remove_sprite_limit,
                "Remove sprite-per-scanline limit",
            );

            ui.checkbox(
                &mut self.config.smsgg.sms_crop_vertical_border,
                "(SMS) Crop vertical border",
            );
            ui.checkbox(&mut self.config.smsgg.sms_crop_left_border, "(SMS) Crop left border");
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgVideo);
        }
    }

    pub(super) fn render_smsgg_audio_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("PSG version");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.smsgg.psg_version, None, "Auto").on_hover_text(
                        "SMS games will use SMS2 PSG, Game Gear games will use SMS1/GG PSG",
                    );
                    ui.radio_value(
                        &mut self.config.smsgg.psg_version,
                        Some(PsgVersion::MasterSystem2),
                        "SMS2",
                    )
                    .on_hover_text("SMS2 PSG clips high volumes");
                    ui.radio_value(
                        &mut self.config.smsgg.psg_version,
                        Some(PsgVersion::Standard),
                        "SMS1 / Game Gear",
                    )
                    .on_hover_text("SMS1 and Game Gear PSGs correctly play high volumes");
                });
            });

            ui.set_enabled(self.emu_thread.status() != EmuThreadStatus::RunningSmsGg);
            ui.checkbox(
                &mut self.config.smsgg.fm_sound_unit_enabled,
                "Sega Master System FM sound unit enabled",
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgAudio);
        }
    }
}
