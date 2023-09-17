mod input;
mod romlist;

use crate::app::input::{GenericButton, InputAppConfig};
use crate::app::romlist::RomMetadata;
use crate::emuthread;
use crate::emuthread::{EmuThreadCommand, EmuThreadHandle, EmuThreadStatus};
use eframe::Frame;
use egui::panel::TopBottomSide;
use egui::{
    menu, Align, Button, CentralPanel, Color32, Context, Key, KeyboardShortcut, Layout, Modifiers,
    TextEdit, TopBottomPanel, Vec2, Widget, Window,
};
use egui_extras::{Column, TableBuilder};
use genesis_core::{GenesisAspectRatio, GenesisRegion};
use jgenesis_native_driver::config::{
    CommonConfig, GenesisConfig, GgAspectRatio, SmsAspectRatio, SmsGgConfig, WindowSize,
};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, RendererConfig, Scanlines, VSyncMode, WgpuBackend,
};
use jgenesis_traits::frontend::TimingMode;
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
    launch_in_fullscreen: bool,
    #[serde(default)]
    wgpu_backend: WgpuBackend,
    #[serde(default)]
    vsync_mode: VSyncMode,
    #[serde(default = "default_prescale_factor")]
    prescale_factor: PrescaleFactor,
    #[serde(default)]
    scanlines: Scanlines,
    #[serde(default)]
    force_integer_height_scaling: bool,
    #[serde(default)]
    filter_mode: FilterMode,
    #[serde(default)]
    preprocess_shader: PreprocessShader,
    #[serde(default = "default_fast_forward_multiplier")]
    fast_forward_multiplier: u64,
    #[serde(default = "default_rewind_buffer_length")]
    rewind_buffer_length_seconds: u64,
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

fn default_fast_forward_multiplier() -> u64 {
    2
}

fn default_rewind_buffer_length() -> u64 {
    10
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
enum SmsModel {
    Sms1,
    #[default]
    Sms2,
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

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GenesisAppConfig {
    #[serde(default)]
    forced_timing_mode: Option<TimingMode>,
    #[serde(default)]
    forced_region: Option<GenesisRegion>,
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
    rom_search_dir: Option<String>,
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
                wgpu_backend: self.common.wgpu_backend,
                vsync_mode: self.common.vsync_mode,
                prescale_factor: self.common.prescale_factor,
                scanlines: self.common.scanlines,
                force_integer_height_scaling: self.common.force_integer_height_scaling,
                filter_mode: self.common.filter_mode,
                preprocess_shader: self.common.preprocess_shader,
                use_webgl2_limits: false,
            },
            fast_forward_multiplier: self.common.fast_forward_multiplier,
            rewind_buffer_length_seconds: self.common.rewind_buffer_length_seconds,
            launch_in_fullscreen: self.common.launch_in_fullscreen,
            keyboard_inputs,
            axis_deadzone: self.inputs.axis_deadzone,
            joystick_inputs,
            hotkeys: self.inputs.hotkeys.clone(),
        }
    }

    fn smsgg_config(&self, path: String) -> Box<SmsGgConfig> {
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
            fm_sound_unit_enabled: self.smsgg.fm_sound_unit_enabled,
            overclock_z80: self.smsgg.overclock_z80,
        })
    }

    fn genesis_config(&self, path: String) -> Box<GenesisConfig> {
        Box::new(GenesisConfig {
            common: self.common_config(
                path,
                self.inputs.to_genesis_keyboard_config(),
                self.inputs.to_genesis_joystick_config(),
            ),
            p1_controller_type: self.inputs.genesis_p1_type,
            p2_controller_type: self.inputs.genesis_p2_type,
            forced_timing_mode: self.genesis.forced_timing_mode,
            forced_region: self.genesis.forced_region,
            aspect_ratio: self.genesis.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.genesis.adjust_aspect_ratio_in_2x_resolution,
        })
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
    GenesisGeneral,
    Interface,
    CommonVideo,
    SmsGgVideo,
    GenesisVideo,
    CommonAudio,
    SmsGgAudio,
    SmsGgKeyboard,
    SmsGgGamepad,
    GenesisKeyboard,
    GenesisGamepad,
    Hotkeys,
    About,
}

struct AppState {
    current_file_path: String,
    open_windows: HashSet<OpenWindow>,
    error_window_open: bool,
    prescale_factor_text: String,
    prescale_factor_invalid: bool,
    axis_deadzone_text: String,
    axis_deadzone_invalid: bool,
    ff_multiplier_text: String,
    ff_multiplier_invalid: bool,
    rewind_buffer_len_text: String,
    rewind_buffer_len_invalid: bool,
    display_scanlines_warning: bool,
    waiting_for_input: Option<GenericButton>,
    rom_list: Vec<RomMetadata>,
}

impl AppState {
    fn from_config(config: &AppConfig) -> Self {
        let rom_list = romlist::build(config.rom_search_dir.as_ref());
        Self {
            current_file_path: String::new(),
            open_windows: HashSet::new(),
            error_window_open: false,
            prescale_factor_text: config.common.prescale_factor.get().to_string(),
            prescale_factor_invalid: false,
            axis_deadzone_text: config.inputs.axis_deadzone.to_string(),
            axis_deadzone_invalid: false,
            ff_multiplier_text: config.common.fast_forward_multiplier.to_string(),
            ff_multiplier_invalid: false,
            rewind_buffer_len_text: config.common.rewind_buffer_length_seconds.to_string(),
            rewind_buffer_len_invalid: false,
            display_scanlines_warning: should_display_scanlines_warning(config),
            waiting_for_input: None,
            rom_list,
        }
    }
}

fn should_display_scanlines_warning(config: &AppConfig) -> bool {
    config.common.scanlines != Scanlines::None
        && (config.common.prescale_factor.get() % 2 != 0
            || !config.common.force_integer_height_scaling)
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

        let mut file_dialog =
            FileDialog::new().add_filter("sms/gg/md", &["sms", "gg", "md", "bin"]);
        if let Some(dir) = &self.config.rom_search_dir {
            file_dialog = file_dialog.set_directory(Path::new(dir));
        }
        let Some(path) = file_dialog.pick_file() else { return };

        let Some(path_str) = path.to_str().map(String::from) else { return };
        self.launch_emulator(path_str);
    }

    fn launch_emulator(&mut self, path: String) {
        self.state.current_file_path = path.clone();

        match Path::new(&path).extension().and_then(OsStr::to_str) {
            Some("sms" | "gg") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.smsgg_config(path);
                self.emu_thread.send(EmuThreadCommand::RunSms(config));
            }
            Some("md" | "bin") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.genesis_config(path);
                self.emu_thread.send(EmuThreadCommand::RunGenesis(config));
            }
            Some(_) => todo!("unrecognized file extension"),
            None => {}
        }
    }

    fn configure_rom_search_directory(&mut self) {
        let Some(dir) = FileDialog::new().pick_folder() else { return };
        let Some(dir) = dir.to_str() else { return };

        self.config.rom_search_dir = Some(dir.into());
        self.state.rom_list = romlist::build(self.config.rom_search_dir.as_ref());
    }

    fn render_smsgg_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.set_enabled(self.emu_thread.status() != EmuThreadStatus::RunningSmsGg);

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
                ui.set_enabled(self.emu_thread.status() != EmuThreadStatus::RunningSmsGg);

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

    fn render_genesis_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis General Settings").open(&mut open).resizable(true).show(ctx, |ui| {
            ui.set_enabled(self.emu_thread.status() != EmuThreadStatus::RunningGenesis);

            ui.group(|ui| {
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
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisGeneral);
        }
    }

    fn render_interface_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("UI Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rom_search_dir = self.config.rom_search_dir.clone().unwrap_or("<None>".into());
            ui.horizontal(|ui| {
                if ui.button(&rom_search_dir).clicked() {
                    self.configure_rom_search_directory();
                }

                ui.label("ROM search directory");

                if ui.button("Clear").clicked() {
                    self.config.rom_search_dir = None;
                    self.state.rom_list = Vec::new();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::Interface);
        }
    }

    fn render_common_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("General Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut self.config.common.launch_in_fullscreen, "Launch in fullscreen");

            ui.group(|ui| {
                ui.set_enabled(!self.emu_thread.status().is_running());

                ui.label("wgpu backend");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.common.wgpu_backend, WgpuBackend::Auto, "Auto");
                    ui.radio_value(
                        &mut self.config.common.wgpu_backend,
                        WgpuBackend::Vulkan,
                        "Vulkan",
                    );
                    ui.radio_value(
                        &mut self.config.common.wgpu_backend,
                        WgpuBackend::DirectX12,
                        "DirectX 12",
                    );
                    ui.radio_value(
                        &mut self.config.common.wgpu_backend,
                        WgpuBackend::Metal,
                        "Metal",
                    );
                    ui.radio_value(
                        &mut self.config.common.wgpu_backend,
                        WgpuBackend::OpenGl,
                        "OpenGL",
                    );
                });
            });

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

            ui.group(|ui| {
                ui.label("Preprocess shader");

                ui.radio_value(
                    &mut self.config.common.preprocess_shader,
                    PreprocessShader::None,
                    "None",
                );

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::HorizontalBlurTwoPixels,
                        "Horizontal blur (2px)",
                    );
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::HorizontalBlurThreePixels,
                        "Horizontal blur (3px)",
                    );
                });

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::AntiDitherWeak,
                        "Anti-dither (conservative)"
                    );
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::AntiDitherStrong,
                        "Anti-dither (aggressive)"
                    );
                });
            });

            ui.group(|ui| {
                ui.label("Scanlines");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::None, "None");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Dim, "Dim");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Black, "Black");
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

            ui.checkbox(
                &mut self.config.common.force_integer_height_scaling,
                "Force integer height scaling",
            ).on_hover_text("Display area will be the largest possible integer multiple of native height that preserves aspect ratio");

            if self.state.display_scanlines_warning {
                ui.colored_label(Color32::RED, "Integer height scaling + even-numbered prescale factor strongly recommended when scanlines are enabled");
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
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisVideo);
        }
    }

    fn render_audio_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("General Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut self.config.common.audio_sync, "Audio sync enabled");
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::CommonAudio);
        }
    }

    fn render_smsgg_audio_settings(&mut self, ctx: &Context) {
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

    fn render_about(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("About").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.heading("jgenesis");

            ui.add_space(10.0);
            ui.label(format!("Version: {}", env!("CARGO_PKG_VERSION")));

            ui.add_space(15.0);
            ui.label("Copyright © 2023 James Groth");

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label("Source code:");
                ui.hyperlink("https://github.com/jsgroth/jgenesis");
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::About);
        }
    }

    fn render_menu(&mut self, ctx: &Context, frame: &mut Frame) {
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
                ui.set_enabled(!self.state.error_window_open);

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

                ui.menu_button("Emulation", |ui| {
                    ui.set_enabled(self.emu_thread.status().is_running());

                    if ui.button("Soft Reset").clicked() {
                        self.emu_thread.send(EmuThreadCommand::SoftReset);
                    }

                    if ui.button("Hard Reset").clicked() {
                        self.emu_thread.send(EmuThreadCommand::HardReset);
                    }

                    if ui.button("Power Off").clicked() {
                        self.emu_thread.send(EmuThreadCommand::StopEmulator);
                    }
                });

                ui.menu_button("Settings", |ui| {
                    if ui.button("SMS/GG").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgGeneral);
                        ui.close_menu();
                    }

                    if ui.button("Genesis").clicked() {
                        self.state.open_windows.insert(OpenWindow::GenesisGeneral);
                        ui.close_menu();
                    }

                    if ui.button("Interface").clicked() {
                        self.state.open_windows.insert(OpenWindow::Interface);
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

                    if ui.button("SMS/GG").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgAudio);
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

                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        self.state.open_windows.insert(OpenWindow::About);
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn render_central_panel(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.set_enabled(!self.state.error_window_open);

            if self.state.rom_list.is_empty() {
                ui.centered_and_justified(|ui| {
                    if ui.selectable_label(false, "Configure ROM search directory").clicked() {
                        self.configure_rom_search_directory();
                    }
                });
            } else {
                ui.set_enabled(self.state.waiting_for_input.is_none());

                TableBuilder::new(ui)
                    .auto_shrink([false; 2])
                    .striped(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::auto().at_most(300.0))
                    .columns(Column::auto(), 2)
                    .column(Column::remainder())
                    .header(30.0, |mut row| {
                        row.col(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Name");
                            });
                        });

                        row.col(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Console");
                            });
                        });

                        row.col(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Size");
                            });
                        });

                        // Blank column to make stripes extend to the right
                        row.col(|_ui| {});
                    })
                    .body(|mut body| {
                        for metadata in self.state.rom_list.clone() {
                            body.row(40.0, |mut row| {
                                row.col(|ui| {
                                    if Button::new(&metadata.file_name_no_ext)
                                        .min_size(Vec2::new(300.0, 30.0))
                                        .wrap(true)
                                        .ui(ui)
                                        .clicked()
                                    {
                                        self.emu_thread.stop_emulator_if_running();
                                        self.launch_emulator(metadata.full_path.clone());
                                    }
                                });

                                row.col(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(metadata.console.to_str());
                                    });
                                });

                                row.col(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        let size_kb = metadata.file_size / 1024;
                                        ui.label(format!("{size_kb}KB"));
                                    });
                                });

                                // Blank column to make stripes extend to the right
                                row.col(|_ui| {});
                            });
                        }
                    });
            }
        });
    }

    fn check_emulator_error(&mut self, ctx: &Context) {
        let mut error_lock = self.emu_thread.lock_emulator_error();
        self.state.error_window_open = error_lock.is_some();

        if let Some(error) = error_lock.as_ref() {
            let mut open = true;
            Window::new("Emulator Error").open(&mut open).resizable(false).show(ctx, |ui| {
                ui.colored_label(Color32::RED, format!("Emulator terminated with error: {error}"));
            });
            if !open {
                *error_lock = None;
            }
        }
    }

    fn check_waiting_for_input(&mut self, ctx: &Context) {
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
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        let prev_config = self.config.clone();

        self.check_emulator_error(ctx);
        self.check_waiting_for_input(ctx);

        self.render_menu(ctx, frame);
        self.render_central_panel(ctx);

        for open_window in self.state.open_windows.clone() {
            match open_window {
                OpenWindow::SmsGgGeneral => {
                    self.render_smsgg_general_settings(ctx);
                }
                OpenWindow::GenesisGeneral => {
                    self.render_genesis_general_settings(ctx);
                }
                OpenWindow::Interface => {
                    self.render_interface_settings(ctx);
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
                OpenWindow::SmsGgAudio => {
                    self.render_smsgg_audio_settings(ctx);
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
                OpenWindow::About => {
                    self.render_about(ctx);
                }
            }
        }

        if prev_config != self.config {
            self.state.display_scanlines_warning = should_display_scanlines_warning(&self.config);

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
