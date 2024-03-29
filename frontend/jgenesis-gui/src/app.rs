mod common;
mod gb;
mod genesis;
mod input;
mod nes;
mod romlist;
mod smsgg;
mod snes;

use crate::app::common::CommonAppConfig;
use crate::app::gb::GameBoyAppConfig;
use crate::app::genesis::{GenesisAppConfig, SegaCdAppConfig};
use crate::app::input::{GenericButton, InputAppConfig};
use crate::app::nes::{NesAppConfig, OverscanState};
use crate::app::romlist::{Console, RomMetadata};
use crate::app::smsgg::SmsGgAppConfig;
use crate::app::snes::SnesAppConfig;
use crate::emuthread;
use crate::emuthread::{EmuThreadCommand, EmuThreadHandle, EmuThreadStatus};
use eframe::Frame;
use egui::panel::TopBottomSide;
use egui::{
    menu, Align, Button, CentralPanel, Color32, Context, Key, KeyboardShortcut, Layout, Modifiers,
    Response, TextEdit, TopBottomPanel, Ui, Vec2, ViewportCommand, Widget, Window,
};
use egui_extras::{Column, TableBuilder};
use jgenesis_renderer::config::Scanlines;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ListFilters {
    #[serde(default = "true_fn")]
    master_system: bool,
    #[serde(default = "true_fn")]
    game_gear: bool,
    #[serde(default = "true_fn")]
    genesis: bool,
    #[serde(default = "true_fn")]
    sega_cd: bool,
    #[serde(default = "true_fn")]
    nes: bool,
    #[serde(default = "true_fn")]
    snes: bool,
    #[serde(default = "true_fn")]
    game_boy: bool,
    #[serde(skip)]
    title_match: String,
}

fn true_fn() -> bool {
    true
}

impl Default for ListFilters {
    fn default() -> Self {
        Self {
            master_system: true,
            game_gear: true,
            genesis: true,
            sega_cd: true,
            nes: true,
            snes: true,
            game_boy: true,
            title_match: String::new(),
        }
    }
}

impl ListFilters {
    fn to_console_vec(&self) -> Vec<Console> {
        [
            self.master_system.then_some(Console::MasterSystem),
            self.game_gear.then_some(Console::GameGear),
            self.genesis.then_some(Console::Genesis),
            self.sega_cd.then_some(Console::SegaCd),
            self.nes.then_some(Console::Nes),
            self.snes.then_some(Console::Snes),
            self.game_boy.then_some(Console::GameBoy),
            self.game_boy.then_some(Console::GameBoyColor),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn apply<'metadata>(
        &self,
        rom_list: &'metadata [RomMetadata],
    ) -> impl Iterator<Item = &'metadata RomMetadata> {
        let filters = self.to_console_vec();
        let title_match = self.title_match.to_lowercase();
        rom_list.iter().filter(move |metadata| {
            filters.contains(&metadata.console)
                && (title_match.is_empty()
                    || metadata.file_name_no_ext.to_lowercase().contains(&title_match))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    common: CommonAppConfig,
    #[serde(default)]
    smsgg: SmsGgAppConfig,
    #[serde(default)]
    genesis: GenesisAppConfig,
    #[serde(default)]
    sega_cd: SegaCdAppConfig,
    #[serde(default)]
    nes: NesAppConfig,
    #[serde(default)]
    snes: SnesAppConfig,
    #[serde(default)]
    game_boy: GameBoyAppConfig,
    #[serde(default)]
    inputs: InputAppConfig,
    #[serde(default)]
    list_filters: ListFilters,
    #[serde(default)]
    rom_search_dirs: Vec<String>,
    #[serde(default)]
    recent_opens: Vec<String>,
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
    NesGeneral,
    SnesGeneral,
    GameBoyGeneral,
    Interface,
    CommonVideo,
    SmsGgVideo,
    GenesisVideo,
    NesVideo,
    SnesVideo,
    GameBoyVideo,
    CommonAudio,
    SmsGgAudio,
    GenesisAudio,
    NesAudio,
    SnesAudio,
    SmsGgKeyboard,
    SmsGgGamepad,
    GenesisKeyboard,
    GenesisGamepad,
    NesKeyboard,
    NesGamepad,
    SnesKeyboard,
    SnesGamepad,
    SnesPeripherals,
    GameBoyKeyboard,
    GameBoyGamepad,
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
    audio_device_queue_size_text: String,
    audio_device_queue_size_invalid: bool,
    internal_audio_buffer_size_text: String,
    internal_audio_buffer_size_invalid: bool,
    audio_sync_threshold_text: String,
    audio_sync_threshold_invalid: bool,
    audio_gain_text: String,
    audio_gain_invalid: bool,
    display_scanlines_warning: bool,
    overscan: OverscanState,
    waiting_for_input: Option<GenericButton>,
    rom_list: Rc<RefCell<Vec<RomMetadata>>>,
    recent_open_list: Vec<RomMetadata>,
}

impl AppState {
    fn from_config(config: &AppConfig) -> Self {
        let rom_list = romlist::build(&config.rom_search_dirs);
        let recent_open_list = romlist::from_recent_opens(&config.recent_opens);
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
            audio_device_queue_size_text: config.common.audio_device_queue_size.to_string(),
            audio_device_queue_size_invalid: false,
            internal_audio_buffer_size_text: config.common.internal_audio_buffer_size.to_string(),
            internal_audio_buffer_size_invalid: false,
            audio_sync_threshold_text: config.common.audio_sync_threshold.to_string(),
            audio_sync_threshold_invalid: false,
            audio_gain_text: format!("{:.1}", config.common.audio_gain_db),
            audio_gain_invalid: false,
            overscan: config.nes.overscan().into(),
            display_scanlines_warning: should_display_scanlines_warning(config),
            waiting_for_input: None,
            rom_list: Rc::new(RefCell::new(rom_list)),
            recent_open_list,
        }
    }
}

fn should_display_scanlines_warning(config: &AppConfig) -> bool {
    config.common.scanlines != Scanlines::None
        && (config.common.prescale_factor.get() % 2 != 0
            || !config.common.force_integer_height_scaling)
}

struct NumericTextEdit<'a, T> {
    text: &'a mut String,
    value: &'a mut T,
    invalid: &'a mut bool,
    validation_fn: Box<dyn Fn(T) -> bool>,
    desired_width: Option<f32>,
}

impl<'a, T> NumericTextEdit<'a, T> {
    fn new(text: &'a mut String, value: &'a mut T, invalid: &'a mut bool) -> Self {
        Self { text, value, invalid, validation_fn: Box::new(|_| true), desired_width: None }
    }

    fn with_validation(mut self, validation_fn: impl Fn(T) -> bool + 'static) -> Self {
        self.validation_fn = Box::new(validation_fn);
        self
    }

    fn desired_width(mut self, desired_width: f32) -> Self {
        self.desired_width = Some(desired_width);
        self
    }
}

impl<'a, T: Copy + FromStr> Widget for NumericTextEdit<'a, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut text_edit = TextEdit::singleline(self.text);
        if let Some(desired_width) = self.desired_width {
            text_edit = text_edit.desired_width(desired_width);
        }

        let response = text_edit.ui(ui);
        if response.changed() {
            match self.text.parse::<T>() {
                Ok(value) if (self.validation_fn)(value) => {
                    *self.value = value;
                    *self.invalid = false;
                }
                _ => {
                    *self.invalid = true;
                }
            }
        }

        response
    }
}

pub struct App {
    config: AppConfig,
    state: AppState,
    config_path: PathBuf,
    emu_thread: EmuThreadHandle,
}

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

        let mut file_dialog = FileDialog::new().add_filter(
            "Supported ROM files",
            &["sms", "gg", "md", "bin", "cue", "chd", "nes", "sfc", "smc", "gb", "gbc"],
        );
        if let Some(dir) = self.config.rom_search_dirs.first() {
            file_dialog = file_dialog.set_directory(Path::new(dir));
        }
        let Some(path) = file_dialog.pick_file() else { return };

        let Some(path_str) = path.to_str().map(String::from) else { return };
        self.launch_emulator(path_str);
    }

    fn launch_emulator(&mut self, path: String) {
        self.state.current_file_path.clone_from(&path);

        // Update Open Recent contents
        self.config.recent_opens.retain(|recent_open_path| recent_open_path != &path);
        self.config.recent_opens.insert(0, path.clone());
        self.config.recent_opens.truncate(10);
        self.state.recent_open_list = romlist::from_recent_opens(&self.config.recent_opens);

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
            Some("cue" | "chd") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.sega_cd_config(path);
                self.emu_thread.send(EmuThreadCommand::RunSegaCd(config));
            }
            Some("nes") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.nes_config(path);
                self.emu_thread.send(EmuThreadCommand::RunNes(config));
            }
            Some("sfc" | "smc") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.snes_config(path);
                self.emu_thread.send(EmuThreadCommand::RunSnes(config));
            }
            Some("gb" | "gbc") => {
                self.emu_thread.stop_emulator_if_running();

                let config = self.config.gb_config(path);
                self.emu_thread.send(EmuThreadCommand::RunGameBoy(config));
            }
            Some(extension) => {
                log::error!("Unsupported file extension: {extension}");
            }
            None => {}
        }
    }

    fn add_rom_search_directory(&mut self) {
        let Some(dir) = FileDialog::new().pick_folder() else { return };
        let Some(dir) = dir.to_str() else { return };

        self.config.rom_search_dirs.push(dir.into());
        *self.state.rom_list.borrow_mut() = romlist::build(&self.config.rom_search_dirs);
    }

    fn render_interface_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("UI Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(
                &mut self.config.common.hide_cursor_over_window,
                "Hide mouse cursor over emulator window",
            );

            ui.add_space(5.0);

            ui.group(|ui| {
                ui.label("ROM search directories");

                ui.add_space(5.0);

                for (i, rom_search_dir) in
                    self.config.rom_search_dirs.clone().into_iter().enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.label(&rom_search_dir);

                        if ui.button("Remove").clicked() {
                            self.config.rom_search_dirs.remove(i);
                            *self.state.rom_list.borrow_mut() =
                                romlist::build(&self.config.rom_search_dirs);
                        }
                    });
                }

                if ui.button("Add").clicked() {
                    self.add_rom_search_directory();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::Interface);
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

    fn render_menu(&mut self, ctx: &Context, _frame: &mut Frame) {
        let open_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::O);
        if ctx.input_mut(|input| input.consume_shortcut(&open_shortcut)) {
            self.open_file();
        }

        let quit_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::Q);
        if ctx.input_mut(|input| input.consume_shortcut(&quit_shortcut)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        TopBottomPanel::new(TopBottomSide::Top, "top_bottom_panel").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                ui.set_enabled(!self.state.error_window_open);

                ui.menu_button("File", |ui| {
                    ui.add_enabled_ui(!self.state.recent_open_list.is_empty(), |ui| {
                        ui.menu_button("Open Recent", |ui| {
                            for recent_open in self.state.recent_open_list.clone() {
                                if ui.button(&recent_open.file_name_no_ext).clicked() {
                                    self.launch_emulator(recent_open.full_path);
                                    ui.close_menu();
                                }

                                ui.add_space(5.0);
                            }
                        });
                    });

                    let open_button =
                        Button::new("Open").shortcut_text(ctx.format_shortcut(&open_shortcut));
                    if open_button.ui(ui).clicked() {
                        self.open_file();
                        ui.close_menu();
                    }

                    let quit_button =
                        Button::new("Quit").shortcut_text(ctx.format_shortcut(&quit_shortcut));
                    if quit_button.ui(ui).clicked() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                });

                ui.menu_button("Emulation", |ui| {
                    ui.set_enabled(self.emu_thread.status().is_running());

                    if ui.button("Open Memory Viewer").clicked() {
                        self.emu_thread.send(EmuThreadCommand::OpenMemoryViewer);
                        ui.close_menu();
                    }

                    ui.add_space(15.0);

                    if ui.button("Soft Reset").clicked() {
                        self.emu_thread.send(EmuThreadCommand::SoftReset);
                        ui.close_menu();
                    }

                    if ui.button("Hard Reset").clicked() {
                        self.emu_thread.send(EmuThreadCommand::HardReset);
                        ui.close_menu();
                    }

                    if ui.button("Power Off").clicked() {
                        self.emu_thread.send(EmuThreadCommand::StopEmulator);
                        ui.close_menu();
                    }

                    ui.add_space(15.0);

                    ui.add_enabled_ui(
                        self.emu_thread.status() == EmuThreadStatus::RunningSegaCd,
                        |ui| {
                            if ui.button("Remove Disc").clicked() {
                                self.emu_thread.send(EmuThreadCommand::SegaCdRemoveDisc);
                                ui.close_menu();
                            }

                            if ui.button("Change Disc").clicked() {
                                if let Some(path) = FileDialog::new()
                                    .add_filter("cue/chd", &["cue", "chd"])
                                    .pick_file()
                                {
                                    self.emu_thread.send(EmuThreadCommand::SegaCdChangeDisc(path));
                                }

                                ui.close_menu();
                            }
                        },
                    );
                });

                ui.menu_button("Settings", |ui| {
                    if ui.button("SMS / Game Gear").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgGeneral);
                        ui.close_menu();
                    }

                    if ui.button("Genesis / Sega CD").clicked() {
                        self.state.open_windows.insert(OpenWindow::GenesisGeneral);
                        ui.close_menu();
                    }

                    if ui.button("NES").clicked() {
                        self.state.open_windows.insert(OpenWindow::NesGeneral);
                        ui.close_menu();
                    }

                    if ui.button("SNES").clicked() {
                        self.state.open_windows.insert(OpenWindow::SnesGeneral);
                        ui.close_menu();
                    }

                    if ui.button("Game Boy").clicked() {
                        self.state.open_windows.insert(OpenWindow::GameBoyGeneral);
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

                    if ui.button("SMS / Game Gear").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgVideo);
                        ui.close_menu();
                    }

                    if ui.button("Genesis / Sega CD").clicked() {
                        self.state.open_windows.insert(OpenWindow::GenesisVideo);
                        ui.close_menu();
                    }

                    if ui.button("NES").clicked() {
                        self.state.open_windows.insert(OpenWindow::NesVideo);
                        ui.close_menu();
                    }

                    if ui.button("SNES").clicked() {
                        self.state.open_windows.insert(OpenWindow::SnesVideo);
                        ui.close_menu();
                    }

                    if ui.button("Game Boy").clicked() {
                        self.state.open_windows.insert(OpenWindow::GameBoyVideo);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Audio", |ui| {
                    if ui.button("General").clicked() {
                        self.state.open_windows.insert(OpenWindow::CommonAudio);
                        ui.close_menu();
                    }

                    if ui.button("SMS / Game Gear").clicked() {
                        self.state.open_windows.insert(OpenWindow::SmsGgAudio);
                        ui.close_menu();
                    }

                    if ui.button("Genesis / Sega CD").clicked() {
                        self.state.open_windows.insert(OpenWindow::GenesisAudio);
                        ui.close_menu();
                    }

                    if ui.button("NES").clicked() {
                        self.state.open_windows.insert(OpenWindow::NesAudio);
                        ui.close_menu();
                    }

                    if ui.button("SNES").clicked() {
                        self.state.open_windows.insert(OpenWindow::SnesAudio);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Input", |ui| {
                    ui.menu_button("SMS / Game Gear", |ui| {
                        if ui.button("Keyboard").clicked() {
                            self.state.open_windows.insert(OpenWindow::SmsGgKeyboard);
                            ui.close_menu();
                        }

                        if ui.button("Gamepad").clicked() {
                            self.state.open_windows.insert(OpenWindow::SmsGgGamepad);
                            ui.close_menu();
                        }
                    });

                    ui.add_space(5.0);

                    ui.menu_button("Genesis / Sega CD", |ui| {
                        if ui.button("Keyboard").clicked() {
                            self.state.open_windows.insert(OpenWindow::GenesisKeyboard);
                            ui.close_menu();
                        }

                        if ui.button("Gamepad").clicked() {
                            self.state.open_windows.insert(OpenWindow::GenesisGamepad);
                            ui.close_menu();
                        }
                    });

                    ui.add_space(5.0);

                    ui.menu_button("NES", |ui| {
                        if ui.button("Keyboard").clicked() {
                            self.state.open_windows.insert(OpenWindow::NesKeyboard);
                            ui.close_menu();
                        }

                        if ui.button("Gamepad").clicked() {
                            self.state.open_windows.insert(OpenWindow::NesGamepad);
                            ui.close_menu();
                        }
                    });

                    ui.add_space(5.0);

                    ui.menu_button("SNES", |ui| {
                        if ui.button("Keyboard").clicked() {
                            self.state.open_windows.insert(OpenWindow::SnesKeyboard);
                            ui.close_menu();
                        }

                        if ui.button("Gamepad").clicked() {
                            self.state.open_windows.insert(OpenWindow::SnesGamepad);
                            ui.close_menu();
                        }

                        if ui.button("Peripherals").clicked() {
                            self.state.open_windows.insert(OpenWindow::SnesPeripherals);
                            ui.close_menu();
                        }
                    });

                    ui.add_space(5.0);

                    ui.menu_button("Game Boy", |ui| {
                        if ui.button("Keyboard").clicked() {
                            self.state.open_windows.insert(OpenWindow::GameBoyKeyboard);
                            ui.close_menu();
                        }

                        if ui.button("Gamepad").clicked() {
                            self.state.open_windows.insert(OpenWindow::GameBoyGamepad);
                            ui.close_menu();
                        }
                    });

                    ui.add_space(5.0);

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

            if self.state.rom_list.borrow().is_empty() {
                ui.centered_and_justified(|ui| {
                    if ui.selectable_label(false, "Configure ROM search directory").clicked() {
                        self.add_rom_search_directory();
                    }
                });
            } else {
                ui.set_enabled(self.state.waiting_for_input.is_none());

                self.render_central_panel_filters(ui);

                ui.add_space(15.0);

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
                        let rom_list = Rc::clone(&self.state.rom_list);
                        for metadata in self.config.list_filters.apply(&rom_list.borrow()) {
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
                                        if metadata.file_size < 1024 * 1024 {
                                            let file_size_kb = metadata.file_size / 1024;
                                            ui.label(format!("{file_size_kb}KB"));
                                        } else {
                                            let file_size_mb = metadata.file_size / 1024 / 1024;
                                            ui.label(format!("{file_size_mb}MB"));
                                        }
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

    fn render_central_panel_filters(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.config.list_filters.title_match)
                    .hint_text("Filter by name"),
            );

            if ui.button("Clear").clicked() {
                self.config.list_filters.title_match.clear();
            }

            ui.add_space(15.0);

            ui.checkbox(&mut self.config.list_filters.master_system, "SMS");
            ui.checkbox(&mut self.config.list_filters.game_gear, "Game Gear");
            ui.checkbox(&mut self.config.list_filters.genesis, "Genesis");
            ui.checkbox(&mut self.config.list_filters.sega_cd, "Sega CD");
            ui.checkbox(&mut self.config.list_filters.nes, "NES");
            ui.checkbox(&mut self.config.list_filters.snes, "SNES");
            ui.checkbox(&mut self.config.list_filters.game_boy, "GB");
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
                        self.reload_config();
                    }
                }
            } else if self.emu_thread.status().is_running() {
                Window::new("Input Configuration").resizable(false).show(ctx, |ui| {
                    ui.colored_label(Color32::BLUE, "Use the emulator window to configure input");
                });
            }
        }
    }

    fn reload_config(&mut self) {
        self.emu_thread.reload_config(
            self.config.smsgg_config(self.state.current_file_path.clone()),
            self.config.genesis_config(self.state.current_file_path.clone()),
            self.config.sega_cd_config(self.state.current_file_path.clone()),
            self.config.nes_config(self.state.current_file_path.clone()),
            self.config.snes_config(self.state.current_file_path.clone()),
            self.config.gb_config(self.state.current_file_path.clone()),
        );
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
                OpenWindow::SmsGgGeneral => self.render_smsgg_general_settings(ctx),
                OpenWindow::GenesisGeneral => self.render_genesis_general_settings(ctx),
                OpenWindow::NesGeneral => self.render_nes_general_settings(ctx),
                OpenWindow::SnesGeneral => self.render_snes_general_settings(ctx),
                OpenWindow::GameBoyGeneral => self.render_gb_general_settings(ctx),
                OpenWindow::Interface => self.render_interface_settings(ctx),
                OpenWindow::CommonVideo => self.render_common_video_settings(ctx),
                OpenWindow::SmsGgVideo => self.render_smsgg_video_settings(ctx),
                OpenWindow::GenesisVideo => self.render_genesis_video_settings(ctx),
                OpenWindow::NesVideo => self.render_nes_video_settings(ctx),
                OpenWindow::SnesVideo => self.render_snes_video_settings(ctx),
                OpenWindow::GameBoyVideo => self.render_gb_video_settings(ctx),
                OpenWindow::CommonAudio => self.render_common_audio_settings(ctx),
                OpenWindow::SmsGgAudio => self.render_smsgg_audio_settings(ctx),
                OpenWindow::GenesisAudio => self.render_genesis_audio_settings(ctx),
                OpenWindow::NesAudio => self.render_nes_audio_settings(ctx),
                OpenWindow::SnesAudio => self.render_snes_audio_settings(ctx),
                OpenWindow::SmsGgKeyboard => self.render_smsgg_keyboard_settings(ctx),
                OpenWindow::SmsGgGamepad => self.render_smsgg_gamepad_settings(ctx),
                OpenWindow::GenesisKeyboard => self.render_genesis_keyboard_settings(ctx),
                OpenWindow::GenesisGamepad => self.render_genesis_gamepad_settings(ctx),
                OpenWindow::NesKeyboard => self.render_nes_keyboard_settings(ctx),
                OpenWindow::NesGamepad => self.render_nes_joystick_settings(ctx),
                OpenWindow::SnesKeyboard => self.render_snes_keyboard_settings(ctx),
                OpenWindow::SnesGamepad => self.render_snes_gamepad_settings(ctx),
                OpenWindow::SnesPeripherals => self.render_snes_peripheral_settings(ctx),
                OpenWindow::GameBoyKeyboard => self.render_gb_keyboard_settings(ctx),
                OpenWindow::GameBoyGamepad => self.render_gb_joystick_settings(ctx),
                OpenWindow::Hotkeys => self.render_hotkey_settings(ctx),
                OpenWindow::About => self.render_about(ctx),
            }
        }

        if prev_config != self.config {
            self.state.display_scanlines_warning = should_display_scanlines_warning(&self.config);

            if should_reload_config(&prev_config, &self.config) {
                self.reload_config();
            }

            let config_str = toml::to_string_pretty(&self.config).unwrap();
            if let Err(err) = fs::write(&self.config_path, config_str) {
                log::error!("Error serializing app config: {err}");
            }
        }
    }
}

fn should_reload_config(prev_config: &AppConfig, new_config: &AppConfig) -> bool {
    // UI-only settings changes should not trigger emulator config reloads

    let prev_no_ui_settings = AppConfig {
        list_filters: ListFilters::default(),
        rom_search_dirs: vec![],
        recent_opens: vec![],
        ..prev_config.clone()
    };

    let new_no_ui_settings = AppConfig {
        list_filters: ListFilters::default(),
        rom_search_dirs: vec![],
        recent_opens: vec![],
        ..new_config.clone()
    };

    prev_no_ui_settings != new_no_ui_settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_does_not_panic() {
        let _ = AppConfig::default();
    }
}
