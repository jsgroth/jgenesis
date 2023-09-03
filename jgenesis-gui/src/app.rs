mod input;

use crate::app::input::{GenericButton, InputAppConfig};
use crate::emuthread;
use crate::emuthread::{EmuThreadCommand, EmuThreadHandle, EmuThreadStatus};
use eframe::Frame;
use egui::panel::TopBottomSide;
use egui::{
    menu, Button, Color32, Context, Key, KeyboardShortcut, Modifiers, TextEdit, TopBottomPanel,
    Widget, Window,
};
use genesis_core::GenesisAspectRatio;
use jgenesis_native_driver::config::{
    CommonConfig, GenesisConfig, GgAspectRatio, SmsAspectRatio, SmsGgConfig, WindowSize,
};
use jgenesis_native_driver::{FilterMode, PrescaleFactor, RendererConfig, VSyncMode};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsRegion, VdpVersion};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CommonAppConfig {
    #[serde(default = "true_fn")]
    audio_sync: bool,
    window_width: Option<u32>,
    window_height: Option<u32>,
    #[serde(default)]
    vsync_mode: VSyncMode,
    #[serde(default = "default_prescale_factor")]
    prescale_factor: PrescaleFactor,
    #[serde(default)]
    filter_mode: FilterMode,
}

impl CommonAppConfig {
    fn window_size(&self) -> Option<WindowSize> {
        match (self.window_width, self.window_height) {
            (Some(width), Some(height)) => Some(WindowSize { width, height }),
            _ => None,
        }
    }
}

impl Default for CommonAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn true_fn() -> bool {
    true
}

fn default_prescale_factor() -> PrescaleFactor {
    PrescaleFactor::from(NonZeroU32::new(3).unwrap())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
enum TimingMode {
    #[default]
    Ntsc,
    Pal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SmsGgAppConfig {
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
    sms_crop_vertical_border: bool,
    #[serde(default)]
    sms_crop_left_border: bool,
}

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GenesisAppConfig {
    #[serde(default)]
    aspect_ratio: GenesisAspectRatio,
    #[serde(default = "true_fn")]
    adjust_aspect_ratio_in_2x_resolution: bool,
}

impl Default for GenesisAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    common: CommonAppConfig,
    #[serde(default)]
    smsgg: SmsGgAppConfig,
    #[serde(default)]
    genesis: GenesisAppConfig,
    #[serde(default)]
    inputs: InputAppConfig,
}

impl AppConfig {
    #[allow(clippy::missing_panics_doc)]
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let config_str = fs::read_to_string(path).unwrap_or_default();
        toml::from_str(&config_str).unwrap_or_else(|err| {
            log::error!("Error deserializing app config: {err}");
            toml::from_str("").unwrap()
        })
    }

    fn common_config<KC, JC>(
        &self,
        path: String,
        keyboard_inputs: KC,
        joystick_inputs: JC,
    ) -> CommonConfig<KC, JC> {
        CommonConfig {
            rom_file_path: path,
            audio_sync: self.common.audio_sync,
            window_size: self.common.window_size(),
            renderer_config: RendererConfig {
                vsync_mode: self.common.vsync_mode,
                prescale_factor: self.common.prescale_factor,
                filter_mode: self.common.filter_mode,
            },
            keyboard_inputs,
            axis_deadzone: 8000,
            joystick_inputs,
            hotkeys: self.inputs.hotkeys.clone(),
        }
    }

    fn smsgg_config(&self, path: String) -> SmsGgConfig {
        let vdp_version = if Path::new(&path).extension().and_then(OsStr::to_str) == Some("sms") {
            match self.smsgg.sms_timing_mode {
                TimingMode::Ntsc => Some(VdpVersion::NtscMasterSystem2),
                TimingMode::Pal => Some(VdpVersion::PalMasterSystem2),
            }
        } else {
            None
        };

        SmsGgConfig {
            common: self.common_config(
                path,
                self.inputs.to_smsgg_keyboard_config(),
                self.inputs.to_smsgg_joystick_config(),
            ),
            vdp_version,
            psg_version: self.smsgg.psg_version,
            remove_sprite_limit: self.smsgg.remove_sprite_limit,
            sms_aspect_ratio: self.smsgg.sms_aspect_ratio,
            gg_aspect_ratio: self.smsgg.gg_aspect_ratio,
            sms_region: self.smsgg.sms_region,
            sms_crop_vertical_border: self.smsgg.sms_crop_vertical_border,
            sms_crop_left_border: self.smsgg.sms_crop_left_border,
        }
    }

    fn genesis_config(&self, path: String) -> GenesisConfig {
        GenesisConfig {
            common: self.common_config(
                path,
                self.inputs.to_genesis_keyboard_config(),
                self.inputs.to_genesis_joystick_config(),
            ),
            aspect_ratio: self.genesis.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.genesis.adjust_aspect_ratio_in_2x_resolution,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OpenWindow {
    SmsGgGeneral,
    CommonVideo,
    SmsGgVideo,
    GenesisVideo,
    CommonAudio,
    SmsGgKeyboard,
    SmsGgGamepad,
    GenesisKeyboard,
    GenesisGamepad,
    Hotkeys,
}

struct AppState {
    current_file_path: String,
    open_windows: HashSet<OpenWindow>,
    prescale_factor_text: String,
    prescale_factor_invalid: bool,
    axis_deadzone_text: String,
    axis_deadzone_invalid: bool,
    waiting_for_input: Option<GenericButton>,
}

impl AppState {
    fn from_config(config: &AppConfig) -> Self {
        Self {
            current_file_path: String::new(),
            open_windows: HashSet::new(),
            prescale_factor_text: config.common.prescale_factor.get().to_string(),
            prescale_factor_invalid: false,
            axis_deadzone_text: config.inputs.axis_deadzone.to_string(),
            axis_deadzone_invalid: false,
            waiting_for_input: None,
        }
    }
}

pub struct App {
    config: AppConfig,
    state: AppState,
    config_path: PathBuf,
    emu_thread: EmuThreadHandle,
}

const MAX_PRESCALE_FACTOR: u32 = 20;

impl App {
    #[must_use]
    pub fn new(config_path: PathBuf) -> Self {
        let config = AppConfig::from_file(&config_path);
        let state = AppState::from_config(&config);
        let emu_thread = emuthread::spawn();
        Self { config, state, config_path, emu_thread }
    }

    fn open_file(&mut self) {
        if self.state.waiting_for_input.is_some() {
            log::warn!("Cannot open file while configuring input");
            return;
        }

        let Some(path) =
            FileDialog::new().add_filter("sms/gg/md", &["sms", "gg", "md", "bin"]).pick_file()
        else {
            return;
        };

        let path_str = path.to_string_lossy().to_string();
        self.state.current_file_path = path_str.clone();

        match path.extension().and_then(OsStr::to_str) {
            Some("sms" | "gg") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.smsgg_config(path_str);
                self.emu_thread.send(EmuThreadCommand::RunSms(config));
            }
            Some("md" | "bin") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.genesis_config(path_str);
                self.emu_thread.send(EmuThreadCommand::RunGenesis(config));
            }
            Some(_) => todo!("unrecognized file extension"),
            None => {}
        }
    }

    fn render_smsgg_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("SMS timing / display mode");

                ui.set_enabled(self.emu_thread.status() != EmuThreadStatus::RunningSmsGg);
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
                ui.label("SMS region");

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
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgGeneral);
        }
    }

    fn render_common_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("General Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("VSync mode");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.vsync_mode,
                        VSyncMode::Enabled,
                        "Enabled",
                    );
                    ui.radio_value(
                        &mut self.config.common.vsync_mode,
                        VSyncMode::Disabled,
                        "Disabled",
                    );
                    ui.radio_value(&mut self.config.common.vsync_mode, VSyncMode::Fast, "Fast");
                });
            });

            ui.group(|ui| {
                ui.label("Filter mode");
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.filter_mode,
                        FilterMode::Nearest,
                        "Nearest neighbor",
                    );
                    ui.radio_value(
                        &mut self.config.common.filter_mode,
                        FilterMode::Linear,
                        "Linear interpolation",
                    );
                });
            });

            ui.horizontal(|ui| {
                if TextEdit::singleline(&mut self.state.prescale_factor_text)
                    .desired_width(30.0)
                    .ui(ui)
                    .changed()
                {
                    match self
                        .state
                        .prescale_factor_text
                        .parse::<u32>()
                        .ok()
                        .filter(|&n| n <= MAX_PRESCALE_FACTOR)
                        .and_then(|n| PrescaleFactor::try_from(n).ok())
                    {
                        Some(prescale_factor) => {
                            self.config.common.prescale_factor = prescale_factor;
                            self.state.prescale_factor_invalid = false;
                        }
                        None => {
                            self.state.prescale_factor_invalid = true;
                        }
                    }
                }

                ui.label("Prescale factor");
            });
            if self.state.prescale_factor_invalid {
                ui.colored_label(
                    Color32::RED,
                    format!(
                        "Prescale factor must be a non-negative integer <= {MAX_PRESCALE_FACTOR}"
                    ),
                );
            }
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::CommonVideo);
        }
    }

    fn render_smsgg_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("SMS aspect ratio");

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

    fn render_genesis_video_settings(&mut self, ctx: &Context) {
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
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisVideo);
        }
    }

    fn render_audio_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("General Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut self.config.common.audio_sync, "Audio sync enabled");

            ui.group(|ui| {
                ui.label("SMS/GG PSG version");

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
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::CommonAudio);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        let prev_config = self.config.clone();

        if let Some(button) = self.state.waiting_for_input {
            if let Ok(input) = self.emu_thread.poll_input_receiver() {
                self.state.waiting_for_input = None;

                log::info!("Received input {input:?} for button {button:?}");
                if let Some(input) = input {
                    self.config.inputs.set_input(input, button);

                    if self.emu_thread.status().is_running() {
                        self.emu_thread.reload_config(
                            self.config.smsgg_config(self.state.current_file_path.clone()),
                            self.config.genesis_config(self.state.current_file_path.clone()),
                        );
                    }
                }
            } else if self.emu_thread.status().is_running() {
                Window::new("Input Configuration").resizable(false).show(ctx, |ui| {
                    ui.colored_label(Color32::BLUE, "Use the emulator window to configure input");
                });
            }
        }

        let open_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::O);
        if ctx.input_mut(|input| input.consume_shortcut(&open_shortcut)) {
            self.open_file();
        }

        let quit_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::Q);
        if ctx.input_mut(|input| input.consume_shortcut(&quit_shortcut)) {
            frame.close();
        }

        TopBottomPanel::new(TopBottomSide::Top, "top_bottom_panel").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    let open_button =
                        Button::new("Open").shortcut_text(ctx.format_shortcut(&open_shortcut));
                    if open_button.ui(ui).clicked() {
                        self.open_file();
                        ui.close_menu();
                    }

                    let quit_button =
                        Button::new("Quit").shortcut_text(ctx.format_shortcut(&quit_shortcut));
                    if quit_button.ui(ui).clicked() {
                        frame.close();
                    }
                });

                ui.menu_button("Settings", |ui| {
                    if ui.button("SMS/GG").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgGeneral);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Video", |ui| {
                    if ui.button("General").clicked() {
                        self.state.open_windows.insert(OpenWindow::CommonVideo);
                        ui.close_menu();
                    }

                    if ui.button("SMS/GG").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgVideo);
                        ui.close_menu();
                    }

                    if ui.button("Genesis").clicked() {
                        self.state.open_windows.insert(OpenWindow::GenesisVideo);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Audio", |ui| {
                    if ui.button("General").clicked() {
                        self.state.open_windows.insert(OpenWindow::CommonAudio);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Input", |ui| {
                    if ui.button("SMS/GG Keyboard").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgKeyboard);
                        ui.close_menu();
                    }

                    if ui.button("SMS/GG Gamepad").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgGamepad);
                        ui.close_menu();
                    }

                    if ui.button("Genesis Keyboard").clicked() {
                        self.state.open_windows.insert(OpenWindow::GenesisKeyboard);
                        ui.close_menu();
                    }

                    if ui.button("Genesis Gamepad").clicked() {
                        self.state.open_windows.insert(OpenWindow::GenesisGamepad);
                        ui.close_menu();
                    }

                    if ui.button("Hotkeys").clicked() {
                        self.state.open_windows.insert(OpenWindow::Hotkeys);
                        ui.close_menu();
                    }
                });
            });
        });

        for open_window in self.state.open_windows.clone() {
            match open_window {
                OpenWindow::SmsGgGeneral => {
                    self.render_smsgg_general_settings(ctx);
                }
                OpenWindow::CommonVideo => {
                    self.render_common_video_settings(ctx);
                }
                OpenWindow::SmsGgVideo => {
                    self.render_smsgg_video_settings(ctx);
                }
                OpenWindow::GenesisVideo => {
                    self.render_genesis_video_settings(ctx);
                }
                OpenWindow::CommonAudio => {
                    self.render_audio_settings(ctx);
                }
                OpenWindow::SmsGgKeyboard => {
                    self.render_smsgg_keyboard_settings(ctx);
                }
                OpenWindow::SmsGgGamepad => {
                    self.render_smsgg_gamepad_settings(ctx);
                }
                OpenWindow::GenesisKeyboard => {
                    self.render_genesis_keyboard_settings(ctx);
                }
                OpenWindow::GenesisGamepad => {
                    self.render_genesis_gamepad_settings(ctx);
                }
                OpenWindow::Hotkeys => {
                    self.render_hotkey_settings(ctx);
                }
            }
        }

        if prev_config != self.config {
            self.emu_thread.reload_config(
                self.config.smsgg_config(self.state.current_file_path.clone()),
                self.config.genesis_config(self.state.current_file_path.clone()),
            );

            let config_str = toml::to_string_pretty(&self.config).unwrap();
            if let Err(err) = fs::write(&self.config_path, config_str) {
                log::error!("Error serializing app config: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_does_not_panic() {
        let _ = AppConfig::default();
    }
}
