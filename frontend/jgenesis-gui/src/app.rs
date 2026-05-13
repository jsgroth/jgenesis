mod common;
mod gb;
mod gba;
mod genesis;
mod input;
mod nes;
#[cfg(feature = "unstable-cores")]
mod pce;
mod romlist;
mod smsgg;
mod snes;
mod widgets;

use crate::app::genesis::{GenesisVolumeState, S32XPriorityState};
use crate::app::input::{GenericButton, InputMappingSet};
use crate::app::nes::{NesPaletteState, OverscanState};
use crate::app::romlist::{RomListThreadHandle, RomMetadata};
use crate::app::snes::HandledError;
use crate::app::widgets::RenderErrorEffect;
use crate::emuthread;
use crate::emuthread::{EmuThreadCommand, EmuThreadHandle, EmuThreadStatus};
use crate::widgets::SavePathSelect;
use eframe::Frame;
use egui::panel::TopBottomSide;
use egui::{
    Align, Button, CentralPanel, Color32, Context, Grid, Key, KeyboardShortcut, LayerId, Layout,
    Modifiers, Order, TextEdit, ThemePreference, TopBottomPanel, Ui, UiKind, Vec2, ViewportCommand,
    Widget, Window,
};
use egui_extras::{Column, TableBuilder};
use emath::Pos2;
use jgenesis_native_config::common::{HideMouseCursor, PauseEmulator};
use jgenesis_native_config::paths::{ConfigDirType, ConfigDirs, ConfigWithPath};
use jgenesis_native_config::{AppConfig, EguiTheme, ListFilters, RecentOpen};
use jgenesis_native_driver::extensions::Console;
use jgenesis_native_driver::{NativeEmulatorError, extensions};
use nes_config::NesPalette;
use rfd::FileDialog;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
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
            self.sg_1000.then_some(Console::Sg1000),
            self.genesis.then_some(Console::Genesis),
            self.sega_cd.then_some(Console::SegaCd),
            self.sega_32x.then_some(Console::Sega32X),
            self.nes.then_some(Console::Nes),
            self.snes.then_some(Console::Snes),
            self.game_boy.then_some(Console::GameBoy),
            self.game_boy_color.then_some(Console::GameBoyColor),
            self.game_boy_advance.then_some(Console::GameBoyAdvance),
            #[cfg(feature = "unstable-cores")]
            self.pc_engine.then_some(Console::PcEngine),
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
    GbaGeneral,
    #[cfg(feature = "unstable-cores")]
    PceGeneral,
    Synchronization,
    Paths,
    Interface,
    CommonVideo,
    CommonFilter,
    SmsGgVideo,
    GenesisVideo,
    NesVideo,
    SnesVideo,
    GameBoyVideo,
    GbaVideo,
    #[cfg(feature = "unstable-cores")]
    PceVideo,
    CommonAudio,
    SmsGgAudio,
    GenesisAudio,
    NesAudio,
    SnesAudio,
    GameBoyAudio,
    GbaAudio,
    GeneralInput,
    SmsGgInput,
    GenesisInput,
    NesInput,
    NesPeripherals,
    SnesInput,
    SnesPeripherals,
    GameBoyInput,
    GbaInput,
    GbaPeripherals,
    #[cfg(feature = "unstable-cores")]
    PceInput,
    Hotkeys,
    SmsGgOverclock,
    GenesisOverclock,
    SnesOverclock,
    About,
}

impl OpenWindow {
    fn title(self) -> &'static str {
        match self {
            OpenWindow::SmsGgGeneral => "SMS/GG General Settings",
            OpenWindow::GenesisGeneral => "Genesis General Settings",
            OpenWindow::NesGeneral => "NES General Settings",
            OpenWindow::SnesGeneral => "SNES General Settings",
            OpenWindow::GameBoyGeneral => "Game Boy General Settings",
            OpenWindow::GbaGeneral => "GBA General Settings",
            #[cfg(feature = "unstable-cores")]
            OpenWindow::PceGeneral => "PC Engine General Settings",
            OpenWindow::Synchronization => "Synchronization Settings",
            OpenWindow::Paths => "Path Settings",
            OpenWindow::Interface => "Interface Settings",
            OpenWindow::CommonVideo => "General Video Settings",
            OpenWindow::CommonFilter => "Video Filtering Settings",
            OpenWindow::SmsGgVideo => "SMS/GG Video Settings",
            OpenWindow::GenesisVideo => "Genesis Video Settings",
            OpenWindow::NesVideo => "NES Video Settings",
            OpenWindow::SnesVideo => "SNES Video Settings",
            OpenWindow::GameBoyVideo => "Game Boy Video Settings",
            OpenWindow::GbaVideo => "GBA Video Settings",
            #[cfg(feature = "unstable-cores")]
            OpenWindow::PceVideo => "PC Engine Video Settings",
            OpenWindow::CommonAudio => "General Audio Settings",
            OpenWindow::SmsGgAudio => "SMS/GG Audio Settings",
            OpenWindow::GenesisAudio => "Genesis Audio Settings",
            OpenWindow::NesAudio => "NES Audio Settings",
            OpenWindow::SnesAudio => "SNES Audio Settings",
            OpenWindow::GameBoyAudio => "Game Boy Audio Settings",
            OpenWindow::GbaAudio => "GBA Audio Settings",
            OpenWindow::GeneralInput => "General Input Settings",
            OpenWindow::SmsGgInput => "SMS/GG Input Settings",
            OpenWindow::GenesisInput => "Genesis Input Settings",
            OpenWindow::NesInput => "NES Input Settings",
            OpenWindow::NesPeripherals => "NES Peripheral Settings",
            OpenWindow::SnesInput => "SNES Input Settings",
            OpenWindow::SnesPeripherals => "SNES Peripheral Settings",
            OpenWindow::GameBoyInput => "Game Boy Input Settings",
            OpenWindow::GbaInput => "GBA Input Settings",
            OpenWindow::GbaPeripherals => "GBA Peripheral Settings",
            #[cfg(feature = "unstable-cores")]
            OpenWindow::PceInput => "PC Engine Input Settings",
            OpenWindow::Hotkeys => "Hotkey Settings",
            OpenWindow::SmsGgOverclock => "SMS/GG Overclocking Settings",
            OpenWindow::GenesisOverclock => "Genesis Overclocking Settings",
            OpenWindow::SnesOverclock => "SNES Overclocking Settings",
            OpenWindow::About => "About",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HelpText {
    heading: &'static str,
    text: &'static [&'static str],
}

struct WaitingForInput {
    button: GenericButton,
    mapping: InputMappingSet,
    turbo: bool,
}

struct AppState {
    current_file_path: PathBuf,
    open_windows: HashSet<OpenWindow>,
    help_text: HashMap<OpenWindow, HelpText>,
    input_mapping_sets: HashMap<OpenWindow, InputMappingSet>,
    error_window_open: bool,
    prescale_width_raw: u32,
    prescale_height_raw: u32,
    ff_multiplier_text: String,
    ff_multiplier_invalid: bool,
    rewind_buffer_len_text: String,
    rewind_buffer_len_invalid: bool,
    audio_buffer_size_text: String,
    audio_buffer_size_invalid: bool,
    audio_hardware_queue_size_text: String,
    audio_hardware_queue_size_invalid: bool,
    audio_gain_text: String,
    audio_gain_invalid: bool,
    nes_palette: NesPaletteState,
    genesis_volume: GenesisVolumeState,
    s32x_priority: S32XPriorityState,
    overscan: OverscanState,
    waiting_for_input: Option<WaitingForInput>,
    rom_list: Arc<Mutex<Vec<RomMetadata>>>,
    filtered_rom_list: Rc<[RomMetadata]>,
    rom_list_refresh_needed: bool,
    recent_open_list: Vec<RomMetadata>,
    disc_change_options: Vec<(String, PathBuf)>,
    title_match: String,
    title_match_lowercase: Rc<str>,
    rendered_first_frame: bool,
    close_on_emulator_exit: bool,
}

impl AppState {
    fn new(ctx: &Context) -> Self {
        Self {
            current_file_path: PathBuf::new(),
            open_windows: HashSet::new(),
            help_text: HashMap::new(),
            input_mapping_sets: HashMap::new(),
            error_window_open: false,
            prescale_width_raw: 1,
            prescale_height_raw: 1,
            ff_multiplier_text: String::new(),
            ff_multiplier_invalid: false,
            rewind_buffer_len_text: String::new(),
            rewind_buffer_len_invalid: false,
            audio_buffer_size_text: String::new(),
            audio_buffer_size_invalid: false,
            audio_hardware_queue_size_text: String::new(),
            audio_hardware_queue_size_invalid: false,
            audio_gain_text: String::new(),
            audio_gain_invalid: false,
            nes_palette: NesPaletteState::create(ctx, &NesPalette::default()),
            genesis_volume: GenesisVolumeState::default(),
            s32x_priority: S32XPriorityState::default(),
            overscan: OverscanState::default(),
            waiting_for_input: None,
            rom_list: Arc::new(Mutex::new(vec![])),
            filtered_rom_list: vec![].into(),
            rom_list_refresh_needed: true,
            title_match: String::new(),
            title_match_lowercase: Rc::from(String::new()),
            recent_open_list: vec![],
            disc_change_options: Vec::new(),
            rendered_first_frame: false,
            close_on_emulator_exit: false,
        }
    }

    fn from_config(config: &AppConfig, ctx: &Context) -> Self {
        let mut state = Self::new(ctx);
        state.update_config_derived_fields(config, ctx);

        state
    }

    fn update_config_derived_fields(&mut self, config: &AppConfig, ctx: &Context) {
        self.prescale_width_raw = config.common.prescale_width.get();
        self.prescale_height_raw = config.common.prescale_height.get();
        self.ff_multiplier_text = config.common.fast_forward_multiplier.to_string();
        self.ff_multiplier_invalid = false;
        self.rewind_buffer_len_text = config.common.rewind_buffer_length_seconds.to_string();
        self.rewind_buffer_len_invalid = false;
        self.audio_buffer_size_text = config.common.audio_buffer_size.to_string();
        self.audio_buffer_size_invalid = false;
        self.audio_hardware_queue_size_text = config.common.audio_hardware_queue_size.to_string();
        self.audio_hardware_queue_size_invalid = false;
        self.audio_gain_text = format!("{:.1}", config.common.audio_gain_db);
        self.audio_gain_invalid = false;
        self.nes_palette = NesPaletteState::create(ctx, &config.nes.palette);
        self.genesis_volume = GenesisVolumeState::from_config(config);
        self.s32x_priority = S32XPriorityState::from_config(&config.sega_32x);
        self.overscan = config.nes.overscan().into();
        self.recent_open_list = romlist::from_recent_opens(&config.recent_open_list);
    }

    fn open_window(&mut self, ctx: &Context, window: OpenWindow) {
        self.open_windows.insert(window);
        ctx.move_to_top(LayerId::new(Order::Middle, window.title().into()));
    }
}

#[derive(Debug, Clone)]
pub struct LoadAtStartup {
    pub file_path: PathBuf,
    pub load_state_slot: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ConfigInfo {
    pub initial_config: AppConfig,
    pub config_path: PathBuf,
    pub config_dirs: ConfigDirs,
    pub config_dir_type: ConfigDirType,
}

pub struct App {
    config: AppConfig,
    state: AppState,
    config_path: PathBuf,
    config_dirs: ConfigDirs,
    config_dir_type: ConfigDirType,
    emu_thread: EmuThreadHandle,
    rom_list_thread: RomListThreadHandle,
    load_at_startup: Option<LoadAtStartup>,
    initial_focused: bool,
}

impl App {
    #[must_use]
    pub fn new(
        config_info: ConfigInfo,
        load_at_startup: Option<LoadAtStartup>,
        ctx: Context,
    ) -> Self {
        let config = config_info.initial_config;

        let state = AppState::from_config(&config, &ctx);
        let emu_thread = emuthread::spawn(ctx.clone());

        let rom_list_thread = RomListThreadHandle::spawn(Arc::clone(&state.rom_list), ctx);
        rom_list_thread.request_scan(config.rom_search_dirs.clone());

        Self {
            config,
            state,
            config_path: config_info.config_path,
            config_dirs: config_info.config_dirs,
            config_dir_type: config_info.config_dir_type,
            emu_thread,
            rom_list_thread,
            load_at_startup,
            initial_focused: false,
        }
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

        self.state.disc_change_options = romlist::find_all_disc_paths(&path);

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
        self.request_rom_list_scan();
    }

    fn request_rom_list_scan(&mut self) {
        self.rom_list_thread.request_scan(self.config.rom_search_dirs.clone());
        self.state.rom_list_refresh_needed = true;
    }

    fn render_path_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::Paths.title()).open(&mut open).default_width(500.0).show(
            ctx,
            |ui| {
                let using_override = matches!(self.config_dir_type, ConfigDirType::Override { .. });
                ui.add_enabled_ui(!using_override, |ui| {
                    let prev_config_dir_type = self.config_dir_type.clone();

                    ui.group(|ui| {
                        ui.label("Settings path");

                        ui.add_enabled_ui(self.config_dirs.user_profile_dir.is_some(), |ui| {
                            ui.radio_value(
                                &mut self.config_dir_type,
                                ConfigDirType::UserProfile,
                                "User profile directory",
                            );
                        });

                        ui.add_enabled_ui(self.config_dirs.emulator_dir.is_some(), |ui| {
                            ui.radio_value(
                                &mut self.config_dir_type,
                                ConfigDirType::EmulatorDirectory,
                                "Emulator directory (Portable)",
                            );
                        });

                        ui.label(format!("  {}", self.config_path.display()));
                    });

                    if self.config_dir_type != prev_config_dir_type {
                        self.handle_config_dir_type_change(ctx);
                    }
                });

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
                                self.request_rom_list_scan();
                            }

                            ui.end_row();
                        }
                    });

                    if ui.button("Add").clicked() {
                        self.add_rom_search_directory();
                    }
                });
            },
        );
        if !open {
            self.state.open_windows.remove(&OpenWindow::Paths);
        }
    }

    fn handle_config_dir_type_change(&mut self, ctx: &Context) {
        match &self.config_dir_type {
            ConfigDirType::EmulatorDirectory => {
                if let Err(err) = self.config_dirs.create_portable_txt() {
                    log::error!("Error creating portable.txt file: {err}");
                }
            }
            _ => {
                if let Err(err) = self.config_dirs.delete_portable_txt() {
                    log::error!("Error deleting portable.txt file: {err}");
                }
            }
        }

        let prev_rom_search_dirs = self.config.rom_search_dirs.clone();
        let prev_list_filters = self.config.list_filters.clone();

        // Try to load config from new path, but keep config unchanged if unable to load (e.g. file does not exist)
        let new_config = ConfigWithPath::load_from_dir_or_default(
            &self.config_dirs,
            &self.config_dir_type,
            || self.config.clone(),
        );

        log::info!("Config path changed to '{}'", new_config.path.display());

        if !new_config.path.exists() {
            log::info!(
                "Saving current settings to new config path '{}'",
                new_config.path.display()
            );
            if let Err(err) = new_config.save_config() {
                log::error!(
                    "Error saving current settings to '{}': {err}",
                    new_config.path.display()
                );
            }
        }

        self.config = new_config.config;
        self.config_path = new_config.path;

        self.state.update_config_derived_fields(&self.config, ctx);

        if prev_rom_search_dirs != self.config.rom_search_dirs
            || prev_list_filters != self.config.list_filters
        {
            self.request_rom_list_scan();
        }
    }

    fn render_interface_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::Interface.title()).open(&mut open).resizable(false).show(
            ctx,
            |ui| {
                ui.group(|ui| {
                    ui.label("Pause emulator automatically");

                    for (option, label) in [
                        (PauseEmulator::Never, "Never"),
                        (
                            PauseEmulator::EmulatorLosesFocus,
                            "When emulator window is in background",
                        ),
                        (
                            PauseEmulator::ApplicationLosesFocus,
                            "When entire application is in background",
                        ),
                    ] {
                        ui.radio_value(&mut self.config.common.pause_emulator, option, label);
                    }
                });

                ui.add_space(5.0);

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
            },
        );
        if !open {
            self.state.open_windows.remove(&OpenWindow::Interface);
        }
    }

    fn render_about(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::About.title()).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.heading("jgenesis");

            ui.add_space(10.0);
            ui.label(format!("Version: {}", env!("CARGO_PKG_VERSION")));

            ui.add_space(15.0);
            ui.label("Copyright © 2023-2026 James Groth");

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
            egui::MenuBar::new().ui(ui, |ui| {
                ui.add_enabled_ui(!self.state.error_window_open, |ui| {
                    self.render_file_menu(ctx, ui);
                    self.render_emulation_menu(ui);
                    self.render_settings_menu(ui);
                    self.render_video_menu(ui);
                    self.render_audio_menu(ui);
                    self.render_input_menu(ui);
                    self.render_overclock_menu(ui);
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
                            ui.close_kind(UiKind::Menu);
                        }

                        ui.add_space(5.0);
                    }

                    ui.separator();

                    if ui.button("Clear List").clicked() {
                        self.config.recent_open_list.clear();
                        self.state.recent_open_list.clear();
                        ui.close_kind(UiKind::Menu);
                    }
                });

                let open_most_recent_button = Button::new("Open Most Recent")
                    .shortcut_text(ctx.format_shortcut(&open_most_recent_shortcut));
                if ui.add(open_most_recent_button).clicked() {
                    self.open_most_recent_file();
                    ui.close_kind(UiKind::Menu);
                }
            });

            ui.add_space(10.0);

            ui.menu_button("Open Using", |ui| {
                for console in Console::ALL {
                    if console == Console::GameBoyColor {
                        // Game Boy backend doesn't support GB vs. GBC boot option via API parameters,
                        // only via config
                        continue;
                    }
                    self.render_open_using_button(console, ui);
                }
            });

            ui.add_space(10.0);

            ui.menu_button("Run BIOS", |ui| {
                for (label, console, has_bios) in [
                    (
                        "Master System",
                        Console::MasterSystem,
                        self.config.smsgg.sms_bios_path.is_some(),
                    ),
                    ("Sega CD", Console::SegaCd, self.config.sega_cd.bios_path.is_some()),
                ] {
                    ui.add_enabled_ui(has_bios, |ui| {
                        if ui.button(label).clicked() {
                            self.emu_thread.stop_emulator_if_running();
                            self.emu_thread.send(EmuThreadCommand::RunBios {
                                console,
                                config: Box::new(self.config.clone()),
                            });
                            self.state.current_file_path.clear();
                            ui.close_kind(UiKind::Menu);
                        }
                    });
                }
            });

            ui.add_space(10.0);

            let open_button =
                Button::new("Open").shortcut_text(ctx.format_shortcut(&open_shortcut));
            if open_button.ui(ui).clicked() {
                self.open_file(None);
                ui.close_kind(UiKind::Menu);
            }

            let quit_button =
                Button::new("Quit").shortcut_text(ctx.format_shortcut(&quit_shortcut));
            if quit_button.ui(ui).clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        });
    }

    fn render_open_using_button(&mut self, console: Console, ui: &mut Ui) {
        if ui.button(console.display_str()).clicked() {
            self.open_file(Some(console));
            ui.close_kind(UiKind::Menu);
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
                                    ui.close_kind(UiKind::Menu);
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
                            ui.close_kind(UiKind::Menu);
                        }
                    }
                });

                ui.add_space(15.0);

                if ui.button("Open Memory Viewer").clicked() {
                    self.emu_thread.send(EmuThreadCommand::OpenMemoryViewer);
                    ui.close_kind(UiKind::Menu);
                }

                ui.add_space(15.0);

                let show_soft_reset = !matches!(
                    self.emu_thread.status(),
                    EmuThreadStatus::RunningGameBoy | EmuThreadStatus::RunningGba
                );
                ui.add_enabled_ui(show_soft_reset, |ui| {
                    if ui.button("Soft Reset").clicked() {
                        self.emu_thread.send(EmuThreadCommand::SoftReset);
                        ui.close_kind(UiKind::Menu);
                    }
                });

                if ui.button("Hard Reset").clicked() {
                    self.emu_thread.send(EmuThreadCommand::HardReset);
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("Power Off").clicked() {
                    self.emu_thread.send(EmuThreadCommand::StopEmulator);
                    ui.close_kind(UiKind::Menu);
                }

                ui.add_space(15.0);

                ui.add_enabled_ui(
                    self.emu_thread.status() == EmuThreadStatus::RunningSegaCd,
                    |ui| {
                        ui.menu_button("Change Disc", |ui| {
                            if !self.state.disc_change_options.is_empty() {
                                for (name, path) in &self.state.disc_change_options {
                                    let enabled = path != &self.state.current_file_path;
                                    ui.add_enabled_ui(enabled, |ui| {
                                        if ui.button(name).clicked() {
                                            self.state.current_file_path.clone_from(path);
                                            self.emu_thread.send(
                                                EmuThreadCommand::SegaCdChangeDisc(path.clone()),
                                            );
                                            ui.close_kind(UiKind::Menu);
                                        }
                                    });
                                }

                                ui.separator();
                            }

                            if ui.button("Select file...").clicked() {
                                if let Some(path) = FileDialog::new()
                                    .add_filter("cue/chd", &["cue", "chd"])
                                    .pick_file()
                                {
                                    self.state.current_file_path.clone_from(&path);
                                    self.emu_thread.send(EmuThreadCommand::SegaCdChangeDisc(path));
                                }

                                ui.close_kind(UiKind::Menu);
                            }
                        });

                        if ui.button("Remove Disc").clicked() {
                            self.emu_thread.send(EmuThreadCommand::SegaCdRemoveDisc);
                            self.state.current_file_path.clear();
                            ui.close_kind(UiKind::Menu);
                        }
                    },
                );
            });
        });
    }

    fn render_settings_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Settings", |ui| {
            for (label, window) in [
                ("SMS / Game Gear / SG", OpenWindow::SmsGgGeneral),
                ("Genesis / Sega CD / 32X", OpenWindow::GenesisGeneral),
                ("NES", OpenWindow::NesGeneral),
                ("SNES", OpenWindow::SnesGeneral),
                ("Game Boy", OpenWindow::GameBoyGeneral),
                ("Game Boy Advance", OpenWindow::GbaGeneral),
                #[cfg(feature = "unstable-cores")]
                ("PC Engine", OpenWindow::PceGeneral),
            ] {
                if ui.button(label).clicked() {
                    self.state.open_window(ui.ctx(), window);
                    ui.close_kind(UiKind::Menu);
                }
            }

            ui.separator();

            if ui.button("Synchronization").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::Synchronization);
                ui.close_kind(UiKind::Menu);
            }

            if ui.button("Paths").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::Paths);
                ui.close_kind(UiKind::Menu);
            }

            if ui.button("Interface").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::Interface);
                ui.close_kind(UiKind::Menu);
            }
        });
    }

    fn render_video_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Video", |ui| {
            if ui.button("General").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::CommonVideo);
                ui.close_kind(UiKind::Menu);
            }

            if ui.button("Filtering").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::CommonFilter);
                ui.close_kind(UiKind::Menu);
            }

            ui.separator();

            for (label, window) in [
                ("SMS / Game Gear / SG", OpenWindow::SmsGgVideo),
                ("Genesis / Sega CD / 32X", OpenWindow::GenesisVideo),
                ("NES", OpenWindow::NesVideo),
                ("SNES", OpenWindow::SnesVideo),
                ("Game Boy", OpenWindow::GameBoyVideo),
                ("Game Boy Advance", OpenWindow::GbaVideo),
                #[cfg(feature = "unstable-cores")]
                ("PC Engine", OpenWindow::PceVideo),
            ] {
                if ui.button(label).clicked() {
                    self.state.open_window(ui.ctx(), window);
                    ui.close_kind(UiKind::Menu);
                }
            }
        });
    }

    fn render_audio_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Audio", |ui| {
            if ui.button("General").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::CommonAudio);
                ui.close_kind(UiKind::Menu);
            }

            ui.separator();

            for (label, window) in [
                ("SMS / Game Gear / SG", OpenWindow::SmsGgAudio),
                ("Genesis / Sega CD / 32X", OpenWindow::GenesisAudio),
                ("NES", OpenWindow::NesAudio),
                ("SNES", OpenWindow::SnesAudio),
                ("Game Boy", OpenWindow::GameBoyAudio),
                ("Game Boy Advance", OpenWindow::GbaAudio),
            ] {
                if ui.button(label).clicked() {
                    self.state.open_window(ui.ctx(), window);
                    ui.close_kind(UiKind::Menu);
                }
            }
        });
    }

    fn render_input_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Input", |ui| {
            if ui.button("General").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::GeneralInput);
                ui.close_kind(UiKind::Menu);
            }

            ui.separator();

            if ui.button("SMS / Game Gear / SG").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::SmsGgInput);
                ui.close_kind(UiKind::Menu);
            }

            if ui.button("Genesis / Sega CD / 32X").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::GenesisInput);
                ui.close_kind(UiKind::Menu);
            }

            ui.menu_button("NES", |ui| {
                if ui.button("Gamepads").clicked() {
                    self.state.open_window(ui.ctx(), OpenWindow::NesInput);
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("Peripherals").clicked() {
                    self.state.open_window(ui.ctx(), OpenWindow::NesPeripherals);
                    ui.close_kind(UiKind::Menu);
                }
            });

            ui.menu_button("SNES", |ui| {
                if ui.button("Gamepads").clicked() {
                    self.state.open_window(ui.ctx(), OpenWindow::SnesInput);
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("Peripherals").clicked() {
                    self.state.open_window(ui.ctx(), OpenWindow::SnesPeripherals);
                    ui.close_kind(UiKind::Menu);
                }
            });

            if ui.button("Game Boy").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::GameBoyInput);
                ui.close_kind(UiKind::Menu);
            }

            ui.menu_button("Game Boy Advance", |ui| {
                if ui.button("Gamepad").clicked() {
                    self.state.open_window(ui.ctx(), OpenWindow::GbaInput);
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("Peripherals").clicked() {
                    self.state.open_window(ui.ctx(), OpenWindow::GbaPeripherals);
                    ui.close_kind(UiKind::Menu);
                }
            });

            #[cfg(feature = "unstable-cores")]
            if ui.button("PC Engine").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::PceInput);
                ui.close_kind(UiKind::Menu);
            }

            ui.separator();

            if ui.button("Hotkeys").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::Hotkeys);
                ui.close_kind(UiKind::Menu);
            }
        });
    }

    fn render_overclock_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Overclocking", |ui| {
            for (label, window) in [
                ("SMS / Game Gear / SG", OpenWindow::SmsGgOverclock),
                ("Genesis / Sega CD / 32X", OpenWindow::GenesisOverclock),
                ("SNES", OpenWindow::SnesOverclock),
            ] {
                if ui.button(label).clicked() {
                    self.state.open_window(ui.ctx(), window);
                    ui.close_kind(UiKind::Menu);
                }
            }
        });
    }

    fn render_help_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Help", |ui| {
            if ui.button("About").clicked() {
                self.state.open_window(ui.ctx(), OpenWindow::About);
                ui.close_kind(UiKind::Menu);
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
                .desired_width(225.0);
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
            ui.checkbox(&mut self.config.list_filters.sg_1000, "SG");
            ui.checkbox(&mut self.config.list_filters.genesis, "GEN");
            ui.checkbox(&mut self.config.list_filters.sega_cd, "SCD");
            ui.checkbox(&mut self.config.list_filters.sega_32x, "32X");
            ui.checkbox(&mut self.config.list_filters.nes, "NES");
            ui.checkbox(&mut self.config.list_filters.snes, "SNES");
            ui.checkbox(&mut self.config.list_filters.game_boy, "GB");
            ui.checkbox(&mut self.config.list_filters.game_boy_color, "GBC");
            ui.checkbox(&mut self.config.list_filters.game_boy_advance, "GBA");
            #[cfg(feature = "unstable-cores")]
            ui.checkbox(&mut self.config.list_filters.pc_engine, "PCE");

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
                OpenWindow::GbaGeneral => self.render_gba_general_settings(ctx),
                #[cfg(feature = "unstable-cores")]
                OpenWindow::PceGeneral => self.render_pce_general_settings(ctx),
                OpenWindow::Synchronization => self.render_sync_settings(ctx),
                OpenWindow::Paths => self.render_path_settings(ctx),
                OpenWindow::Interface => self.render_interface_settings(ctx),
                OpenWindow::CommonVideo => self.render_common_video_settings(ctx),
                OpenWindow::CommonFilter => self.render_video_filtering_settings(ctx),
                OpenWindow::SmsGgVideo => self.render_smsgg_video_settings(ctx),
                OpenWindow::GenesisVideo => self.render_genesis_video_settings(ctx),
                OpenWindow::NesVideo => self.render_nes_video_settings(ctx),
                OpenWindow::SnesVideo => self.render_snes_video_settings(ctx),
                OpenWindow::GameBoyVideo => self.render_gb_video_settings(ctx),
                OpenWindow::GbaVideo => self.render_gba_video_settings(ctx),
                #[cfg(feature = "unstable-cores")]
                OpenWindow::PceVideo => self.render_pce_video_settings(ctx),
                OpenWindow::CommonAudio => self.render_common_audio_settings(ctx),
                OpenWindow::SmsGgAudio => self.render_smsgg_audio_settings(ctx),
                OpenWindow::GenesisAudio => self.render_genesis_audio_settings(ctx),
                OpenWindow::NesAudio => self.render_nes_audio_settings(ctx),
                OpenWindow::SnesAudio => self.render_snes_audio_settings(ctx),
                OpenWindow::GameBoyAudio => self.render_gb_audio_settings(ctx),
                OpenWindow::GbaAudio => self.render_gba_audio_settings(ctx),
                OpenWindow::GeneralInput => self.render_general_input_settings(ctx),
                OpenWindow::SmsGgInput => self.render_smsgg_input_settings(ctx),
                OpenWindow::GenesisInput => self.render_genesis_input_settings(ctx),
                OpenWindow::NesInput => self.render_nes_input_settings(ctx),
                OpenWindow::NesPeripherals => self.render_nes_peripheral_settings(ctx),
                OpenWindow::SnesInput => self.render_snes_input_settings(ctx),
                OpenWindow::SnesPeripherals => self.render_snes_peripheral_settings(ctx),
                OpenWindow::GameBoyInput => self.render_gb_input_settings(ctx),
                OpenWindow::GbaInput => self.render_gba_input_settings(ctx),
                OpenWindow::GbaPeripherals => self.render_gba_peripheral_settings(ctx),
                #[cfg(feature = "unstable-cores")]
                OpenWindow::PceInput => self.render_pce_input_settings(ctx),
                OpenWindow::Hotkeys => self.render_hotkey_settings(ctx),
                OpenWindow::SmsGgOverclock => self.render_smsgg_overclock_settings(ctx),
                OpenWindow::GenesisOverclock => self.render_genesis_overclock_settings(ctx),
                OpenWindow::SnesOverclock => self.render_snes_overclock_settings(ctx),
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
            let render_effect = match err {
                NativeEmulatorError::SmsNoBios => self.render_sms_bios_error(ctx, &mut open),
                NativeEmulatorError::GgNoBios => self.render_gg_bios_error(ctx, &mut open),
                &NativeEmulatorError::SegaCdNoBios(region) => {
                    self.render_scd_bios_error(ctx, &mut open, region)
                }
                NativeEmulatorError::GbNoDmgBootRom => {
                    self.render_dmg_boot_rom_error(ctx, &mut open)
                }
                NativeEmulatorError::GbNoCgbBootRom => {
                    self.render_cgb_boot_rom_error(ctx, &mut open)
                }
                NativeEmulatorError::SnesLoad(snes_load_err) => {
                    match self.render_snes_load_error(ctx, snes_load_err, &mut open) {
                        HandledError::Yes(effect) => effect,
                        HandledError::No => Self::render_generic_error_window(ctx, err, &mut open),
                    }
                }
                NativeEmulatorError::GbaNoBios => self.render_gba_bios_error(ctx, &mut open),
                _ => Self::render_generic_error_window(ctx, err, &mut open),
            };

            match render_effect {
                RenderErrorEffect::LaunchEmulator(console) => {
                    self.launch_emulator(self.state.current_file_path.clone(), Some(console));
                }
                RenderErrorEffect::None => {}
            }

            if !open {
                *error_lock = None;
            }
        }
    }

    fn render_generic_error_window(
        ctx: &Context,
        err: &NativeEmulatorError,
        open: &mut bool,
    ) -> RenderErrorEffect {
        Window::new("Emulator Error").open(open).resizable(false).show(ctx, |ui| {
            ui.label("Emulator terminated with error:");
            ui.add_space(10.0);
            ui.colored_label(Color32::RED, err.to_string());
        });

        RenderErrorEffect::None
    }

    fn check_waiting_for_input(&mut self, ctx: &Context) {
        if let Some(WaitingForInput { button, mapping, turbo }) = self.state.waiting_for_input {
            if let Ok(input) = self.emu_thread.poll_input_receiver() {
                self.state.waiting_for_input = None;

                log::info!("Received input {input:?} for button {button:?}");
                if let Some(input) = input
                    && !input.is_empty()
                {
                    if turbo {
                        if let Some(value) =
                            button.access_value_turbo(mapping, &mut self.config.input)
                        {
                            *value = Some(input);
                        }
                    } else {
                        *button.access_value(mapping, &mut self.config.input) = Some(input);
                    }
                }
            } else if self.emu_thread.status().is_running() {
                Window::new("Input Configuration").resizable(false).show(ctx, |ui| {
                    ui.colored_label(Color32::GREEN, "Use the emulator window to configure input");
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

            // Don't auto-focus the GUI window if -f/--file-path arg was set
            if !self.initial_focused && !self.state.close_on_emulator_exit {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                self.initial_focused = ctx.input(|input| input.raw.focused);
            }
        }

        let gui_focused = ctx.input(|input| input.raw.focused);
        self.emu_thread.update_gui_focused(gui_focused);

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
            if should_reload_config(&prev_config, &self.config) {
                self.reload_config();
            }

            let config_str = toml::to_string_pretty(&self.config).unwrap();
            if let Err(err) = fs::write(&self.config_path, config_str) {
                log::error!("Error serializing app config: {err}");
            }

            nes::update_palette_textures(ctx, &self.state.nes_palette, &self.config.nes.palette);
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
        gui_window_width: jgenesis_native_config::DEFAULT_GUI_WIDTH,
        gui_window_height: jgenesis_native_config::DEFAULT_GUI_HEIGHT,
        ..prev_config.clone()
    };

    let new_no_ui_settings = AppConfig {
        list_filters: ListFilters::default(),
        rom_search_dirs: vec![],
        recent_open_list: vec![],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_nanos_format_is_valid() {
        assert!(format_time_nanos(1_000_000_000).is_some());
    }
}
