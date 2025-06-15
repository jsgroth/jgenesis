mod common;
mod gb;
mod genesis;
mod input;
mod nes;
mod romlist;
mod smsgg;
mod snes;

use crate::app::input::{GenericButton, InputMappingSet};
use crate::app::nes::OverscanState;
use crate::app::romlist::{RomListThreadHandle, RomMetadata};
use crate::app::snes::HandledError;
use crate::emuthread;
use crate::emuthread::{EmuThreadCommand, EmuThreadHandle, EmuThreadStatus};
use crate::widgets::SavePathSelect;
use eframe::Frame;
use egui::panel::TopBottomSide;
use egui::scroll_area::ScrollAreaOutput;
use egui::style::ScrollStyle;
use egui::{
    Align, Button, CentralPanel, Color32, Context, Grid, Key, KeyboardShortcut, Layout, Modifiers,
    Response, ScrollArea, TextEdit, ThemePreference, TopBottomPanel, Ui, Vec2, ViewportCommand,
    Widget, Window, menu,
};
use egui_extras::{Column, TableBuilder};
use emath::Pos2;
use jgenesis_native_config::common::HideMouseCursor;
use jgenesis_native_config::{AppConfig, EguiTheme, ListFilters, RecentOpen};
use jgenesis_native_driver::extensions::Console;
use jgenesis_native_driver::{NativeEmulatorError, extensions};
use jgenesis_renderer::config::Scanlines;
use rfd::FileDialog;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};
use time::{OffsetDateTime, UtcOffset, format_description};

const RESERVED_HELP_TEXT_HEIGHT: f32 = 150.0;

trait ListFiltersExt {
    fn to_console_vec(&self) -> Vec<Console>;

    fn apply<'metadata>(
        &self,
        rom_list: &'metadata [RomMetadata],
        title_match: &'metadata str,
    ) -> impl Iterator<Item = &'metadata RomMetadata> + 'metadata;
}

impl ListFiltersExt for ListFilters {
    fn to_console_vec(&self) -> Vec<Console> {
        [
            self.master_system.then_some(Console::MasterSystem),
            self.game_gear.then_some(Console::GameGear),
            self.genesis.then_some(Console::Genesis),
            self.sega_cd.then_some(Console::SegaCd),
            self.sega_32x.then_some(Console::Sega32X),
            self.nes.then_some(Console::Nes),
            self.snes.then_some(Console::Snes),
            self.game_boy.then_some(Console::GameBoy),
            self.game_boy_color.then_some(Console::GameBoyColor),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn apply<'metadata>(
        &self,
        rom_list: &'metadata [RomMetadata],
        title_match_lowercase: &'metadata str,
    ) -> impl Iterator<Item = &'metadata RomMetadata> + 'metadata {
        debug_assert!(
            title_match_lowercase.chars().all(|c| c.is_lowercase() || !c.is_alphabetic())
        );

        let filters = self.to_console_vec();
        rom_list.iter().filter(move |metadata| {
            filters.contains(&metadata.console)
                && (title_match_lowercase.is_empty()
                    || metadata.file_name_no_ext.to_lowercase().contains(title_match_lowercase))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OpenWindow {
    SmsGgGeneral,
    GenesisGeneral,
    NesGeneral,
    SnesGeneral,
    GameBoyGeneral,
    Synchronization,
    Paths,
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
    GameBoyAudio,
    GeneralInput,
    SmsGgInput,
    GenesisInput,
    NesInput,
    NesPeripherals,
    SnesInput,
    SnesPeripherals,
    GameBoyInput,
    Hotkeys,
    About,
}

#[derive(Debug, Clone, Copy)]
struct HelpText {
    heading: &'static str,
    text: &'static [&'static str],
}

struct AppState {
    current_file_path: PathBuf,
    open_windows: HashSet<OpenWindow>,
    help_text: HashMap<OpenWindow, HelpText>,
    input_mapping_sets: HashMap<OpenWindow, InputMappingSet>,
    error_window_open: bool,
    prescale_factor_raw: u32,
    ff_multiplier_text: String,
    ff_multiplier_invalid: bool,
    rewind_buffer_len_text: String,
    rewind_buffer_len_invalid: bool,
    audio_hardware_queue_size_text: String,
    audio_hardware_queue_size_invalid: bool,
    audio_buffer_size_text: String,
    audio_buffer_size_invalid: bool,
    audio_gain_text: String,
    audio_gain_invalid: bool,
    display_scanlines_warning: bool,
    overscan: OverscanState,
    waiting_for_input: Option<(GenericButton, InputMappingSet)>,
    rom_list: Arc<Mutex<Vec<RomMetadata>>>,
    filtered_rom_list: Rc<[RomMetadata]>,
    rom_list_refresh_needed: bool,
    recent_open_list: Vec<RomMetadata>,
    title_match: String,
    title_match_lowercase: Rc<str>,
    rendered_first_frame: bool,
    close_on_emulator_exit: bool,
}

impl AppState {
    fn from_config(config: &AppConfig) -> Self {
        let recent_open_list = romlist::from_recent_opens(&config.recent_open_list);
        Self {
            current_file_path: PathBuf::new(),
            open_windows: HashSet::new(),
            help_text: HashMap::new(),
            input_mapping_sets: HashMap::new(),
            error_window_open: false,
            prescale_factor_raw: config.common.prescale_factor.get(),
            ff_multiplier_text: config.common.fast_forward_multiplier.to_string(),
            ff_multiplier_invalid: false,
            rewind_buffer_len_text: config.common.rewind_buffer_length_seconds.to_string(),
            rewind_buffer_len_invalid: false,
            audio_hardware_queue_size_text: config.common.audio_hardware_queue_size.to_string(),
            audio_hardware_queue_size_invalid: false,
            audio_buffer_size_text: config.common.audio_buffer_size.to_string(),
            audio_buffer_size_invalid: false,
            audio_gain_text: format!("{:.1}", config.common.audio_gain_db),
            audio_gain_invalid: false,
            overscan: config.nes.overscan().into(),
            display_scanlines_warning: should_display_scanlines_warning(config),
            waiting_for_input: None,
            rom_list: Arc::new(Mutex::new(vec![])),
            filtered_rom_list: vec![].into(),
            rom_list_refresh_needed: true,
            title_match: String::new(),
            title_match_lowercase: Rc::from(String::new()),
            recent_open_list,
            rendered_first_frame: false,
            close_on_emulator_exit: false,
        }
    }
}

fn should_display_scanlines_warning(config: &AppConfig) -> bool {
    let prescale_odd = !config.common.auto_prescale && config.common.prescale_factor.get() % 2 != 0;

    config.common.scanlines != Scanlines::None
        && (prescale_odd || !config.common.force_integer_height_scaling)
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

impl<T: Copy + FromStr> Widget for NumericTextEdit<'_, T> {
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

#[derive(Debug, Clone)]
pub struct LoadAtStartup {
    pub file_path: PathBuf,
    pub load_state_slot: Option<usize>,
}

pub struct App {
    config: AppConfig,
    state: AppState,
    config_path: PathBuf,
    emu_thread: EmuThreadHandle,
    rom_list_thread: RomListThreadHandle,
    load_at_startup: Option<LoadAtStartup>,
}

fn load_app_config(config_path: &Path) -> AppConfig {
    let config_str = fs::read_to_string(config_path).unwrap_or_default();
    let mut config = toml::from_str(&config_str).unwrap_or_else(|err| {
        log::error!("Error deserializing app config at '{}': {err}", config_path.display());
        AppConfig::default()
    });

    if let Some(migrated_config) = jgenesis_native_config::migrate_config(&config, &config_str) {
        if config != migrated_config {
            config = migrated_config;
            if let Err(err) = fs::write(config_path, toml::to_string_pretty(&config).unwrap()) {
                log::error!("Error serializing app config: {err}");
            }
        }
    }

    config
}

impl App {
    #[must_use]
    pub fn new(config_path: PathBuf, load_at_startup: Option<LoadAtStartup>, ctx: Context) -> Self {
        let config = load_app_config(&config_path);

        let state = AppState::from_config(&config);
        let emu_thread = emuthread::spawn(ctx.clone());

        let rom_list_thread = RomListThreadHandle::spawn(Arc::clone(&state.rom_list), ctx);
        rom_list_thread.request_scan(config.rom_search_dirs.clone());

        Self { config, state, config_path, emu_thread, rom_list_thread, load_at_startup }
    }

    fn open_file(&mut self, console: Option<Console>) {
        if self.state.waiting_for_input.is_some() {
            log::warn!("Cannot open file while configuring input");
            return;
        }

        let mut file_dialog = FileDialog::new();

        file_dialog = match console {
            Some(console) => {
                let extensions: Vec<_> =
                    console.supported_extensions().iter().copied().chain(["zip", "7z"]).collect();
                file_dialog.add_filter(console.display_str(), &extensions)
            }
            None => file_dialog.add_filter("Supported Files", &extensions::ALL_PLUS_ARCHIVES),
        };

        file_dialog = file_dialog.add_filter("All Files", &["*"]);

        if let Some(dir) = self.config.rom_search_dirs.first() {
            file_dialog = file_dialog.set_directory(Path::new(dir));
        }
        let Some(path) = file_dialog.pick_file() else { return };

        self.launch_emulator(path, console);
    }

    fn open_most_recent_file(&mut self) {
        let Some(recent_open) = self.state.recent_open_list.first() else { return };

        self.launch_emulator(recent_open.full_path.clone(), Some(recent_open.console));
    }

    fn launch_emulator(&mut self, path: PathBuf, console: Option<Console>) {
        self.state.current_file_path.clone_from(&path);

        let console = match console {
            Some(console) => console,
            None => {
                let Some(metadata) = romlist::read_metadata(Path::new(&path)) else {
                    log::error!("Unable to detect compatible file at path: '{}'", path.display());
                    self.emu_thread.clear_waiting_for_first_command();
                    return;
                };
                metadata.console
            }
        };

        // Update Open Recent contents
        let console_str = console.to_string();
        self.config
            .recent_open_list
            .retain(|open| open.path != path || open.console != console_str);
        self.config
            .recent_open_list
            .insert(0, RecentOpen { console: console_str, path: path.clone() });
        self.config.recent_open_list.truncate(10);
        self.state.recent_open_list = romlist::from_recent_opens(&self.config.recent_open_list);

        self.emu_thread.stop_emulator_if_running();
        self.emu_thread.send(EmuThreadCommand::Run {
            console,
            config: Box::new(self.config.clone()),
            file_path: path,
        });
    }

    fn add_rom_search_directory(&mut self) {
        let Some(dir) = FileDialog::new().pick_folder() else { return };
        let Some(dir) = dir.to_str() else { return };

        self.config.rom_search_dirs.push(dir.into());
        self.rom_list_thread.request_scan(self.config.rom_search_dirs.clone());
        self.state.rom_list_refresh_needed = true;
    }

    fn render_path_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Path Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.add(SavePathSelect::new(
                "Game save file path",
                &mut self.config.common.save_path,
                &mut self.config.common.custom_save_path,
            ));

            ui.add(SavePathSelect::new(
                "Save state path",
                &mut self.config.common.state_path,
                &mut self.config.common.custom_state_path,
            ));

            ui.add_space(10.0);

            ui.group(|ui| {
                ui.heading("ROM search directories");

                ui.add_space(5.0);

                Grid::new("rom_search_dirs").show(ui, |ui| {
                    for (i, rom_search_dir) in
                        self.config.rom_search_dirs.clone().into_iter().enumerate()
                    {
                        ui.label(&rom_search_dir);

                        if ui.button("Remove").clicked() {
                            self.config.rom_search_dirs.remove(i);
                            self.rom_list_thread.request_scan(self.config.rom_search_dirs.clone());
                            self.state.rom_list_refresh_needed = true;
                        }

                        ui.end_row();
                    }
                });

                if ui.button("Add").clicked() {
                    self.add_rom_search_directory();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::Paths);
        }
    }

    fn render_interface_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("UI Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Hide mouse cursor over emulator window");

                ui.radio_value(
                    &mut self.config.common.hide_mouse_cursor,
                    HideMouseCursor::Fullscreen,
                    "Only when fullscreen",
                );
                ui.radio_value(
                    &mut self.config.common.hide_mouse_cursor,
                    HideMouseCursor::Always,
                    "Always",
                );
                ui.radio_value(
                    &mut self.config.common.hide_mouse_cursor,
                    HideMouseCursor::Never,
                    "Never",
                );
            });

            ui.add_space(5.0);

            ui.group(|ui| {
                ui.label("UI theme");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.egui_theme,
                        EguiTheme::SystemDefault,
                        "System default",
                    );
                    ui.radio_value(&mut self.config.egui_theme, EguiTheme::Dark, "Dark");
                    ui.radio_value(&mut self.config.egui_theme, EguiTheme::Light, "Light");
                });
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
            ui.label("Copyright © 2024 James Groth");

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

    fn render_menu(&mut self, ctx: &Context) {
        TopBottomPanel::new(TopBottomSide::Top, "top_bottom_panel").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                ui.add_enabled_ui(!self.state.error_window_open, |ui| {
                    self.render_file_menu(ctx, ui);
                    self.render_emulation_menu(ui);
                    self.render_settings_menu(ui);
                    self.render_video_menu(ui);
                    self.render_audio_menu(ui);
                    self.render_input_menu(ui);
                    self.render_help_menu(ui);
                });
            });
        });
    }

    fn render_file_menu(&mut self, ctx: &Context, ui: &mut Ui) {
        let open_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::O);
        if ctx.input_mut(|input| input.consume_shortcut(&open_shortcut)) {
            self.open_file(None);
        }

        let open_most_recent_shortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);
        if ctx.input_mut(|input| input.consume_shortcut(&open_most_recent_shortcut)) {
            self.open_most_recent_file();
        }

        let quit_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::Q);
        if ctx.input_mut(|input| input.consume_shortcut(&quit_shortcut)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        ui.menu_button("File", |ui| {
            ui.add_enabled_ui(!self.state.recent_open_list.is_empty(), |ui| {
                ui.menu_button("Open Recent", |ui| {
                    ui.set_min_width(300.0);
                    ui.set_max_width(500.0);

                    for recent_open in self.state.recent_open_list.clone() {
                        let label = format!(
                            "{} [{}]",
                            recent_open.file_name_no_ext,
                            recent_open.console.display_str()
                        );
                        if ui.button(label).clicked() {
                            self.launch_emulator(recent_open.full_path, Some(recent_open.console));
                            ui.close_menu();
                        }

                        ui.add_space(5.0);
                    }

                    ui.separator();

                    if ui.button("Clear List").clicked() {
                        self.config.recent_open_list.clear();
                        self.state.recent_open_list.clear();
                        ui.close_menu();
                    }
                });

                let open_most_recent_button = Button::new("Open Most Recent")
                    .shortcut_text(ctx.format_shortcut(&open_most_recent_shortcut));
                if ui.add(open_most_recent_button).clicked() {
                    self.open_most_recent_file();
                    ui.close_menu();
                }
            });

            ui.add_space(10.0);

            ui.menu_button("Open Using", |ui| {
                for console in [
                    Console::MasterSystem,
                    Console::Genesis,
                    Console::SegaCd,
                    Console::Sega32X,
                    Console::Nes,
                    Console::Snes,
                    Console::GameBoy,
                ] {
                    self.render_open_using_button(console, ui);
                }
            });

            ui.add_space(10.0);

            ui.menu_button("Run BIOS", |ui| {
                for (label, console, has_bios) in [
                    ("Master System", Console::MasterSystem, self.config.smsgg.bios_path.is_some()),
                    ("Sega CD", Console::SegaCd, self.config.sega_cd.bios_path.is_some()),
                ] {
                    ui.add_enabled_ui(has_bios, |ui| {
                        if ui.button(label).clicked() {
                            self.emu_thread.stop_emulator_if_running();
                            self.emu_thread.send(EmuThreadCommand::RunBios {
                                console,
                                config: Box::new(self.config.clone()),
                            });
                            ui.close_menu();
                        }
                    });
                }
            });

            ui.add_space(10.0);

            let open_button =
                Button::new("Open").shortcut_text(ctx.format_shortcut(&open_shortcut));
            if open_button.ui(ui).clicked() {
                self.open_file(None);
                ui.close_menu();
            }

            let quit_button =
                Button::new("Quit").shortcut_text(ctx.format_shortcut(&quit_shortcut));
            if quit_button.ui(ui).clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        });
    }

    fn render_open_using_button(&mut self, console: Console, ui: &mut Ui) {
        let label = match console {
            Console::MasterSystem => "SMS / Game Gear",
            _ => console.display_str(),
        };

        if ui.button(label).clicked() {
            self.open_file(Some(console));
            ui.close_menu();
        }
    }

    fn render_emulation_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Emulation", |ui| {
            ui.add_enabled_ui(self.emu_thread.status().is_running(), |ui| {
                let save_state_metadata = self.emu_thread.save_state_metadata();

                ui.menu_button("Load State", |ui| {
                    ui.set_min_width(200.0);

                    for slot in 0..jgenesis_native_driver::SAVE_STATE_SLOTS {
                        match save_state_metadata.times_nanos[slot] {
                            Some(time_nanos) => {
                                let formatted_time = format_time_nanos(time_nanos)
                                    .unwrap_or_else(|| "Unknown".into());
                                let label = format!("Slot {slot} - {formatted_time}");
                                if ui.button(label).clicked() {
                                    self.emu_thread.send(EmuThreadCommand::LoadState { slot });
                                    ui.close_menu();
                                }
                            }
                            None => {
                                ui.add_enabled_ui(false, |ui| {
                                    let _ = ui.button(format!("Slot {slot} - Empty"));
                                });
                            }
                        }
                    }
                });

                ui.menu_button("Save State", |ui| {
                    ui.set_min_width(200.0);

                    for slot in 0..jgenesis_native_driver::SAVE_STATE_SLOTS {
                        let label = match save_state_metadata.times_nanos[slot] {
                            Some(time_nanos) => {
                                let formatted_time = format_time_nanos(time_nanos)
                                    .unwrap_or_else(|| "Unknown".into());
                                format!("Slot {slot} - {formatted_time}")
                            }
                            None => format!("Slot {slot} - Empty"),
                        };

                        if ui.button(label).clicked() {
                            self.emu_thread.send(EmuThreadCommand::SaveState { slot });
                            ui.close_menu();
                        }
                    }
                });

                ui.add_space(15.0);

                if ui.button("Open Memory Viewer").clicked() {
                    self.emu_thread.send(EmuThreadCommand::OpenMemoryViewer);
                    ui.close_menu();
                }

                ui.add_space(15.0);

                let running_gb = self.emu_thread.status() == EmuThreadStatus::RunningGameBoy;
                ui.add_enabled_ui(!running_gb, |ui| {
                    if ui.button("Soft Reset").clicked() {
                        self.emu_thread.send(EmuThreadCommand::SoftReset);
                        ui.close_menu();
                    }
                });

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
                            if let Some(path) =
                                FileDialog::new().add_filter("cue/chd", &["cue", "chd"]).pick_file()
                            {
                                self.emu_thread.send(EmuThreadCommand::SegaCdChangeDisc(path));
                            }

                            ui.close_menu();
                        }
                    },
                );
            });
        });
    }

    fn render_settings_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Settings", |ui| {
            if ui.button("SMS / Game Gear").clicked() {
                self.state.open_windows.insert(OpenWindow::SmsGgGeneral);
                ui.close_menu();
            }

            if ui.button("Genesis / Sega CD / 32X").clicked() {
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

            ui.separator();

            if ui.button("Synchronization").clicked() {
                self.state.open_windows.insert(OpenWindow::Synchronization);
                ui.close_menu();
            }

            if ui.button("Paths").clicked() {
                self.state.open_windows.insert(OpenWindow::Paths);
                ui.close_menu();
            }

            if ui.button("Interface").clicked() {
                self.state.open_windows.insert(OpenWindow::Interface);
                ui.close_menu();
            }
        });
    }

    fn render_video_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Video", |ui| {
            if ui.button("General").clicked() {
                self.state.open_windows.insert(OpenWindow::CommonVideo);
                ui.close_menu();
            }

            ui.separator();

            if ui.button("SMS / Game Gear").clicked() {
                self.state.open_windows.insert(OpenWindow::SmsGgVideo);
                ui.close_menu();
            }

            if ui.button("Genesis / Sega CD / 32X").clicked() {
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
    }

    fn render_audio_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Audio", |ui| {
            if ui.button("General").clicked() {
                self.state.open_windows.insert(OpenWindow::CommonAudio);
                ui.close_menu();
            }

            ui.separator();

            if ui.button("SMS / Game Gear").clicked() {
                self.state.open_windows.insert(OpenWindow::SmsGgAudio);
                ui.close_menu();
            }

            if ui.button("Genesis / Sega CD / 32X").clicked() {
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

            if ui.button("Game Boy").clicked() {
                self.state.open_windows.insert(OpenWindow::GameBoyAudio);
                ui.close_menu();
            }
        });
    }

    fn render_input_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Input", |ui| {
            if ui.button("General").clicked() {
                self.state.open_windows.insert(OpenWindow::GeneralInput);
                ui.close_menu();
            }

            ui.separator();

            if ui.button("SMS / Game Gear").clicked() {
                self.state.open_windows.insert(OpenWindow::SmsGgInput);
                ui.close_menu();
            }

            if ui.button("Genesis / Sega CD / 32X").clicked() {
                self.state.open_windows.insert(OpenWindow::GenesisInput);
                ui.close_menu();
            }

            ui.menu_button("NES", |ui| {
                if ui.button("Gamepads").clicked() {
                    self.state.open_windows.insert(OpenWindow::NesInput);
                    ui.close_menu();
                }

                if ui.button("Peripherals").clicked() {
                    self.state.open_windows.insert(OpenWindow::NesPeripherals);
                    ui.close_menu();
                }
            });

            ui.menu_button("SNES", |ui| {
                if ui.button("Gamepads").clicked() {
                    self.state.open_windows.insert(OpenWindow::SnesInput);
                    ui.close_menu();
                }

                if ui.button("Peripherals").clicked() {
                    self.state.open_windows.insert(OpenWindow::SnesPeripherals);
                    ui.close_menu();
                }
            });

            if ui.button("Game Boy").clicked() {
                self.state.open_windows.insert(OpenWindow::GameBoyInput);
                ui.close_menu();
            }

            ui.separator();

            if ui.button("Hotkeys").clicked() {
                self.state.open_windows.insert(OpenWindow::Hotkeys);
                ui.close_menu();
            }
        });
    }

    fn render_help_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Help", |ui| {
            if ui.button("About").clicked() {
                self.state.open_windows.insert(OpenWindow::About);
                ui.close_menu();
            }
        });
    }

    fn render_central_panel(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(!self.state.error_window_open, |ui| {
                if self.rom_list_thread.any_scans_in_progress() {
                    ui.centered_and_justified(|ui| {
                        ui.label("Scanning search directories...");
                    });
                } else if self.state.rom_list.lock().unwrap().is_empty() {
                    ui.centered_and_justified(|ui| {
                        if ui.selectable_label(false, "Configure ROM search directory").clicked() {
                            self.add_rom_search_directory();
                        }
                    });
                } else {
                    ui.add_enabled_ui(self.state.waiting_for_input.is_none(), |ui| {
                        self.render_central_panel_filters(ui);

                        ui.add_space(15.0);

                        TableBuilder::new(ui)
                            .auto_shrink([false; 2])
                            .striped(true)
                            .max_scroll_height(3000.0)
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .column(Column::auto().at_least(300.0).at_most(400.0))
                            .column(Column::auto().at_least(125.0))
                            .column(Column::auto().at_least(50.0))
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
                            .body(|body| {
                                let rom_list = Rc::clone(&self.state.filtered_rom_list);
                                body.rows(40.0, rom_list.len(), |mut row| {
                                    let metadata = &rom_list[row.index()];

                                    row.col(|ui| {
                                        if Button::new(&metadata.file_name_no_ext)
                                            .min_size(Vec2::new(300.0, 30.0))
                                            .wrap()
                                            .ui(ui)
                                            .clicked()
                                        {
                                            self.emu_thread.stop_emulator_if_running();
                                            self.launch_emulator(metadata.full_path.clone(), None);
                                        }
                                    });

                                    row.col(|ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.label(metadata.console.display_str());
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
                            });
                    });
                }
            });
        });
    }

    fn render_central_panel_filters(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let textedit = TextEdit::singleline(&mut self.state.title_match)
                .hint_text("Filter by name")
                .desired_width(280.0);
            if ui.add(textedit).changed() {
                self.state.title_match_lowercase = Rc::from(self.state.title_match.to_lowercase());
                self.refresh_filtered_rom_list();
            }

            if ui.button("Clear").clicked() {
                self.state.title_match.clear();
                self.state.title_match_lowercase = Rc::from(String::new());
                self.refresh_filtered_rom_list();
            }

            ui.add_space(15.0);

            let prev_list_filters = self.config.list_filters.clone();

            ui.checkbox(&mut self.config.list_filters.master_system, "SMS");
            ui.checkbox(&mut self.config.list_filters.game_gear, "GG");
            ui.checkbox(&mut self.config.list_filters.genesis, "Genesis");
            ui.checkbox(&mut self.config.list_filters.sega_cd, "Sega CD");
            ui.checkbox(&mut self.config.list_filters.sega_32x, "32X");
            ui.checkbox(&mut self.config.list_filters.nes, "NES");
            ui.checkbox(&mut self.config.list_filters.snes, "SNES");
            ui.checkbox(&mut self.config.list_filters.game_boy, "GB");
            ui.checkbox(&mut self.config.list_filters.game_boy_color, "GBC");

            if prev_list_filters != self.config.list_filters {
                self.refresh_filtered_rom_list();
            }
        });
    }

    fn render_windows(&mut self, ctx: &Context) {
        let open_windows: Vec<_> = self.state.open_windows.iter().copied().collect();
        for open_window in open_windows {
            match open_window {
                OpenWindow::SmsGgGeneral => self.render_smsgg_general_settings(ctx),
                OpenWindow::GenesisGeneral => self.render_genesis_general_settings(ctx),
                OpenWindow::NesGeneral => self.render_nes_general_settings(ctx),
                OpenWindow::SnesGeneral => self.render_snes_general_settings(ctx),
                OpenWindow::GameBoyGeneral => self.render_gb_general_settings(ctx),
                OpenWindow::Synchronization => self.render_sync_settings(ctx),
                OpenWindow::Paths => self.render_path_settings(ctx),
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
                OpenWindow::GameBoyAudio => self.render_gb_audio_settings(ctx),
                OpenWindow::GeneralInput => self.render_general_input_settings(ctx),
                OpenWindow::SmsGgInput => self.render_smsgg_input_settings(ctx),
                OpenWindow::GenesisInput => self.render_genesis_input_settings(ctx),
                OpenWindow::NesInput => self.render_nes_input_settings(ctx),
                OpenWindow::NesPeripherals => self.render_nes_peripheral_settings(ctx),
                OpenWindow::SnesInput => self.render_snes_input_settings(ctx),
                OpenWindow::SnesPeripherals => self.render_snes_peripheral_settings(ctx),
                OpenWindow::GameBoyInput => self.render_gb_input_settings(ctx),
                OpenWindow::Hotkeys => self.render_hotkey_settings(ctx),
                OpenWindow::About => self.render_about(ctx),
            }
        }
    }

    fn render_help_text(&mut self, ui: &mut Ui, window: OpenWindow) {
        ui.separator();

        ui.scope(|ui| {
            ui.set_min_size([0.0, RESERVED_HELP_TEXT_HEIGHT].into());

            let Some(help_text) = self.state.help_text.get(&window) else { return };

            ui.heading(help_text.heading);

            for text in help_text.text {
                ui.add_space(7.0);
                ui.label(*text);
            }
        });
    }

    fn check_emulator_error(&mut self, ctx: &Context) {
        let emulator_error = self.emu_thread.emulator_error();
        let mut error_lock = emulator_error.lock().unwrap();
        self.state.error_window_open = error_lock.is_some();

        if let Some(err) = error_lock.as_ref() {
            let mut open = true;
            match err {
                NativeEmulatorError::SmsNoBios => self.render_sms_bios_error(ctx, &mut open),
                NativeEmulatorError::SegaCdNoBios => self.render_scd_bios_error(ctx, &mut open),
                NativeEmulatorError::SnesLoad(snes_load_err) => {
                    match self.render_snes_load_error(ctx, snes_load_err, &mut open) {
                        HandledError::Yes => {}
                        HandledError::No => Self::render_generic_error_window(ctx, err, &mut open),
                    }
                }
                _ => Self::render_generic_error_window(ctx, err, &mut open),
            }
            if !open {
                *error_lock = None;
            }
        }
    }

    fn render_generic_error_window(ctx: &Context, err: &NativeEmulatorError, open: &mut bool) {
        Window::new("Emulator Error").open(open).resizable(false).show(ctx, |ui| {
            ui.label("Emulator terminated with error:");
            ui.add_space(10.0);
            ui.colored_label(Color32::RED, err.to_string());
        });
    }

    fn check_waiting_for_input(&mut self, ctx: &Context) {
        if let Some((button, mapping)) = self.state.waiting_for_input {
            if let Ok(input) = self.emu_thread.poll_input_receiver() {
                self.state.waiting_for_input = None;

                log::info!("Received input {input:?} for button {button:?}");
                if let Some(input) = input {
                    if !input.is_empty() {
                        *button.access_value(mapping, &mut self.config.input) = Some(input);
                    }
                }
            } else if self.emu_thread.status().is_running() {
                Window::new("Input Configuration").resizable(false).show(ctx, |ui| {
                    ui.colored_label(Color32::BLUE, "Use the emulator window to configure input");
                });
            }
        }
    }

    fn check_for_close_on_emu_exit(&mut self, ctx: &Context) {
        if self.state.close_on_emulator_exit {
            let status = self.emu_thread.status();
            if !status.is_running() && status != EmuThreadStatus::WaitingForFirstCommand {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn update_egui_theme(&mut self, ctx: &Context) {
        ctx.set_theme(match self.config.egui_theme {
            EguiTheme::SystemDefault => ThemePreference::System,
            EguiTheme::Dark => ThemePreference::Dark,
            EguiTheme::Light => ThemePreference::Light,
        });
    }

    fn reload_config(&mut self) {
        self.emu_thread.send(EmuThreadCommand::ReloadConfig(
            Box::new(self.config.clone()),
            self.state.current_file_path.clone(),
        ));
    }

    fn refresh_filtered_rom_list(&mut self) {
        let rom_list = self.state.rom_list.lock().unwrap();

        self.state.filtered_rom_list = self
            .config
            .list_filters
            .apply(&rom_list, &self.state.title_match_lowercase)
            .cloned()
            .collect::<Vec<_>>()
            .into();
    }

    fn terminate_emu_thread(&self) {
        if self.emu_thread.status() == EmuThreadStatus::Terminated {
            return;
        }

        let _ = self.emu_thread.try_send(EmuThreadCommand::Terminate);

        let wait_limit = Instant::now() + Duration::from_secs(1);
        while Instant::now() < wait_limit && self.emu_thread.status() != EmuThreadStatus::Terminated
        {
            thread::sleep(Duration::from_millis(1));
        }

        if self.emu_thread.status() != EmuThreadStatus::Terminated {
            log::warn!("Failed to terminate emulation thread; exiting anyway");
        }
    }

    fn update_window_size_in_config(&mut self, ctx: &Context) {
        ctx.viewport(|vp| {
            let Pos2 { x: width, y: height } = vp.input.screen_rect.max;
            self.config.gui_window_width = width;
            self.config.gui_window_height = height;
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        if self.emu_thread.exit_signal() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        if self.state.rom_list_refresh_needed && !self.rom_list_thread.any_scans_in_progress() {
            self.state.rom_list_refresh_needed = false;
            self.refresh_filtered_rom_list();
        }

        if self.state.rendered_first_frame {
            if let Some(load_at_startup) = self.load_at_startup.take() {
                self.launch_emulator(load_at_startup.file_path, None);

                if let Some(load_state_slot) = load_at_startup.load_state_slot {
                    self.emu_thread.send(EmuThreadCommand::LoadState { slot: load_state_slot });
                }

                self.state.close_on_emulator_exit = true;
            }
        }

        let prev_config = self.config.clone();

        self.check_emulator_error(ctx);
        self.check_waiting_for_input(ctx);
        self.check_for_close_on_emu_exit(ctx);

        self.update_egui_theme(ctx);

        self.render_menu(ctx);
        self.render_central_panel(ctx);

        self.render_windows(ctx);

        self.update_window_size_in_config(ctx);

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

        self.state.rendered_first_frame = true;
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.terminate_emu_thread();
    }
}

fn should_reload_config(prev_config: &AppConfig, new_config: &AppConfig) -> bool {
    // UI-only settings changes should not trigger emulator config reloads

    let prev_no_ui_settings = AppConfig {
        list_filters: ListFilters::default(),
        rom_search_dirs: vec![],
        recent_open_list: vec![],
        egui_theme: EguiTheme::default(),
        gui_window_width: jgenesis_native_config::DEFAULT_GUI_WIDTH,
        gui_window_height: jgenesis_native_config::DEFAULT_GUI_HEIGHT,
        ..prev_config.clone()
    };

    let new_no_ui_settings = AppConfig {
        list_filters: ListFilters::default(),
        rom_search_dirs: vec![],
        recent_open_list: vec![],
        egui_theme: EguiTheme::default(),
        gui_window_width: jgenesis_native_config::DEFAULT_GUI_WIDTH,
        gui_window_height: jgenesis_native_config::DEFAULT_GUI_HEIGHT,
        ..new_config.clone()
    };

    prev_no_ui_settings != new_no_ui_settings
}

fn format_time_nanos(time_nanos: u128) -> Option<String> {
    let utc_date_time = OffsetDateTime::from_unix_timestamp_nanos(time_nanos as i128)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);

    let local_offset = UtcOffset::current_local_offset().ok()?;
    let local_date_time = utc_date_time.checked_to_offset(local_offset)?;

    let format =
        format_description::parse_borrowed::<2>("[year]-[month]-[day] [hour]:[minute]:[second]")
            .unwrap();
    local_date_time.format(&format).ok()
}

fn render_vertical_scroll_area<R>(
    ui: &mut Ui,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> ScrollAreaOutput<R> {
    let screen_height = ui.input(|i| i.screen_rect.height());

    let mut scroll_area = ScrollArea::vertical().auto_shrink([false, true]);

    let max_scroll_height = screen_height - RESERVED_HELP_TEXT_HEIGHT - 75.0;
    if max_scroll_height >= 100.0 {
        scroll_area = scroll_area.max_height(max_scroll_height);
    }

    ui.scope(|ui| {
        ui.style_mut().spacing.scroll = ScrollStyle::solid();
        scroll_area.show(ui, add_contents)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_does_not_panic() {
        let _ = AppConfig::default();
    }

    #[test]
    fn time_nanos_format_is_valid() {
        assert!(format_time_nanos(1_000_000_000).is_some());
    }
}
