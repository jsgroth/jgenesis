mod helptext;

use crate::app::widgets::NumericTextEdit;
use crate::app::{App, OpenWindow, WaitingForInput};
use crate::emuthread::{EmuThreadCommand, EmuThreadHandle};
use egui::{Button, Color32, ComboBox, Context, Grid, ScrollArea, Slider, Ui, Window};
use gb_config::GameBoyButton;
use gba_config::GbaButton;
use genesis_config::{GenesisButton, GenesisControllerType};
use jgenesis_common::input::Player;
use jgenesis_native_config::input::InputAppConfig;
use jgenesis_native_config::input::mappings::{
    GameBoyInputMapping, GbaInputMapping, GbaJoypadMapping, GbaSolarMapping,
    GenesisControllerMapping, GenesisInputMapping, HotkeyMapping, NesControllerMapping,
    NesControllerType, NesInputMapping, NesZapperMapping, SmsGgControllerMapping,
    SmsGgInputMapping, SnesControllerMapping, SnesControllerType, SnesInputMapping,
    SnesSuperScopeMapping,
};
use jgenesis_native_config::input::mappings::{PceInputMapping, PceJoypadMapping};
use jgenesis_native_config::input::{GenericInput, Hotkey};
use nes_config::NesButton;
use pce_config::{PceButton, PceInputDevice};
use polonius_the_crab::{polonius, polonius_return};
use smsgg_config::SmsGgButton;
use snes_config::SnesButton;
use std::mem;
use std::sync::LazyLock;

const ALLOW_OPPOSING_DIRECTIONS_LABEL: &str = "Allow simultaneous opposing gamepad directions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMappingSet {
    #[default]
    One,
    Two,
}

impl InputMappingSet {
    fn label(self) -> &'static str {
        match self {
            Self::One => "Input Mapping #1",
            Self::Two => "Input Mapping #2",
        }
    }

    fn smsgg(self, config: &mut InputAppConfig) -> &mut SmsGgInputMapping {
        match self {
            Self::One => &mut config.smsgg.mapping_1,
            Self::Two => &mut config.smsgg.mapping_2,
        }
    }

    fn genesis(self, config: &mut InputAppConfig) -> &mut GenesisInputMapping {
        match self {
            Self::One => &mut config.genesis.mapping_1,
            Self::Two => &mut config.genesis.mapping_2,
        }
    }

    fn nes(self, config: &mut InputAppConfig) -> &mut NesInputMapping {
        match self {
            Self::One => &mut config.nes.mapping_1,
            Self::Two => &mut config.nes.mapping_2,
        }
    }

    fn snes(self, config: &mut InputAppConfig) -> &mut SnesInputMapping {
        match self {
            Self::One => &mut config.snes.mapping_1,
            Self::Two => &mut config.snes.mapping_2,
        }
    }

    fn gb(self, config: &mut InputAppConfig, turbo: bool) -> &mut GameBoyInputMapping {
        match (self, turbo) {
            (Self::One, false) => &mut config.game_boy.mapping_1,
            (Self::One, true) => &mut config.game_boy.mapping_1_turbo,
            (Self::Two, false) => &mut config.game_boy.mapping_2,
            (Self::Two, true) => &mut config.game_boy.mapping_2_turbo,
        }
    }

    fn gba(self, config: &mut InputAppConfig, turbo: bool) -> &mut GbaInputMapping {
        match (self, turbo) {
            (Self::One, false) => &mut config.game_boy_advance.mapping_1,
            (Self::One, true) => &mut config.game_boy_advance.mapping_1_turbo,
            (Self::Two, false) => &mut config.game_boy_advance.mapping_2,
            (Self::Two, true) => &mut config.game_boy_advance.mapping_2_turbo,
        }
    }

    fn pce(self, config: &mut InputAppConfig) -> &mut PceInputMapping {
        match self {
            Self::One => &mut config.pc_engine.mapping_1,
            Self::Two => &mut config.pc_engine.mapping_2,
        }
    }

    fn hotkey(self, config: &mut InputAppConfig) -> &mut HotkeyMapping {
        match self {
            Self::One => &mut config.hotkeys.mapping_1,
            Self::Two => &mut config.hotkeys.mapping_2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericButton {
    SmsGg(SmsGgButton, Player),
    Genesis(GenesisButton, Player),
    Nes(NesButton, Player),
    Snes(SnesButton, Player),
    GameBoy(GameBoyButton),
    Gba(GbaButton),
    Pce(PceButton, Player),
    Hotkey(Hotkey),
}

impl GenericButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::SmsGg(button, _) => button.label(),
            Self::Genesis(button, _) => button.label(),
            Self::Nes(button, _) => button.label(),
            Self::Snes(button, _) => button.label(),
            Self::GameBoy(button) => button.label(),
            Self::Gba(button) => button.label(),
            Self::Pce(button, _) => button.label(),
            Self::Hotkey(hotkey) => hotkey.label(),
        }
    }

    pub fn access_value(
        self,
        mapping: InputMappingSet,
        config: &mut InputAppConfig,
    ) -> Option<&mut Option<Vec<GenericInput>>> {
        match self {
            Self::SmsGg(button, player) => {
                access_smsgg_value(mapping, button, player, false, config)
            }
            Self::Genesis(button, player) => {
                access_genesis_value(mapping, button, player, false, config)
            }
            Self::Nes(button, player) => access_nes_value(mapping, button, player, false, config),
            Self::Snes(button, player) => access_snes_value(mapping, button, player, false, config),
            Self::GameBoy(button) => access_gb_value(mapping, button, false, config),
            Self::Gba(button) => access_gba_value(mapping, button, false, config),
            Self::Pce(button, player) => access_pce_value(mapping, button, player, false, config),
            Self::Hotkey(hotkey) => Some(access_hotkey(mapping, hotkey, config)),
        }
    }

    pub fn access_value_turbo(
        self,
        mapping: InputMappingSet,
        config: &mut InputAppConfig,
    ) -> Option<&mut Option<Vec<GenericInput>>> {
        match self {
            Self::SmsGg(button @ (SmsGgButton::Button1 | SmsGgButton::Button2), player) => {
                access_smsgg_value(mapping, button, player, true, config)
            }
            Self::Genesis(
                button @ (GenesisButton::A
                | GenesisButton::B
                | GenesisButton::C
                | GenesisButton::X
                | GenesisButton::Y
                | GenesisButton::Z
                | GenesisButton::Xe1apA
                | GenesisButton::Xe1apB
                | GenesisButton::Xe1apC
                | GenesisButton::Xe1apD
                | GenesisButton::Xe1apE1
                | GenesisButton::Xe1apE2
                | GenesisButton::Xe1apAp
                | GenesisButton::Xe1apBp),
                player,
            ) => access_genesis_value(mapping, button, player, true, config),
            Self::Nes(button @ (NesButton::A | NesButton::B), player) => {
                access_nes_value(mapping, button, player, true, config)
            }
            Self::Snes(
                button @ (SnesButton::A
                | SnesButton::B
                | SnesButton::X
                | SnesButton::Y
                | SnesButton::L
                | SnesButton::R),
                player,
            ) => access_snes_value(mapping, button, player, true, config),
            Self::GameBoy(button @ (GameBoyButton::A | GameBoyButton::B)) => {
                access_gb_value(mapping, button, true, config)
            }
            Self::Gba(button @ (GbaButton::A | GbaButton::B | GbaButton::L | GbaButton::R)) => {
                access_gba_value(mapping, button, true, config)
            }
            Self::Pce(button @ (PceButton::Button1 | PceButton::Button2), player) => {
                access_pce_value(mapping, button, player, true, config)
            }
            _ => None,
        }
    }

    pub fn access_value_maybe_turbo(
        self,
        mapping: InputMappingSet,
        config: &mut InputAppConfig,
        turbo: bool,
    ) -> Option<&mut Option<Vec<GenericInput>>> {
        if turbo {
            self.access_value_turbo(mapping, config)
        } else {
            self.access_value(mapping, config)
        }
    }
}

fn access_smsgg_value(
    mapping: InputMappingSet,
    button: SmsGgButton,
    player: Player,
    turbo: bool,
    config: &mut InputAppConfig,
) -> Option<&mut Option<Vec<GenericInput>>> {
    let mapping_config = mapping.smsgg(config);

    if button == SmsGgButton::Pause {
        return Some(&mut mapping_config.pause);
    }

    let player_config = mapping_config.player_mapping(player, turbo)?;
    player_config.access_value(button)
}

fn access_genesis_value(
    mapping: InputMappingSet,
    button: GenesisButton,
    player: Player,
    turbo: bool,
    config: &mut InputAppConfig,
) -> Option<&mut Option<Vec<GenericInput>>> {
    let mapping_config = mapping.genesis(config);
    let player_config = mapping_config.player_mapping(player, turbo)?;
    player_config.access_value(button)
}

fn access_nes_value(
    mapping: InputMappingSet,
    button: NesButton,
    player: Player,
    turbo: bool,
    config: &mut InputAppConfig,
) -> Option<&mut Option<Vec<GenericInput>>> {
    let mut mapping_config = mapping.nes(config);

    polonius!(|mapping_config| -> Option<&'polonius mut Option<Vec<GenericInput>>> {
        if let Some(value) = mapping_config.zapper.access_value(button) {
            polonius_return!(Some(value));
        }
    });

    let player_config = mapping_config.player_mapping(player, turbo)?;
    player_config.access_value(button)
}

fn access_snes_value(
    mapping: InputMappingSet,
    button: SnesButton,
    player: Player,
    turbo: bool,
    config: &mut InputAppConfig,
) -> Option<&mut Option<Vec<GenericInput>>> {
    let mut mapping_config = mapping.snes(config);

    polonius!(|mapping_config| -> Option<&'polonius mut Option<Vec<GenericInput>>> {
        if let Some(value) = mapping_config.super_scope.access_value(button) {
            polonius_return!(Some(value));
        }
    });

    let player_config = mapping_config.player_mapping(player, turbo)?;
    player_config.access_value(button)
}

fn access_gb_value(
    mapping: InputMappingSet,
    button: GameBoyButton,
    turbo: bool,
    config: &mut InputAppConfig,
) -> Option<&mut Option<Vec<GenericInput>>> {
    let mapping_config = mapping.gb(config, turbo);
    mapping_config.access_value(button)
}

fn access_gba_value(
    mapping: InputMappingSet,
    button: GbaButton,
    turbo: bool,
    config: &mut InputAppConfig,
) -> Option<&mut Option<Vec<GenericInput>>> {
    let mapping_config = mapping.gba(config, turbo);
    mapping_config.joypad.access_value(button).or_else(|| mapping_config.solar.access_value(button))
}

fn access_pce_value(
    mapping: InputMappingSet,
    button: PceButton,
    player: Player,
    turbo: bool,
    config: &mut InputAppConfig,
) -> Option<&mut Option<Vec<GenericInput>>> {
    let mapping_config = mapping.pce(config);
    let player_config = mapping_config.player_mapping(player, turbo)?;
    player_config.access_value(button)
}

fn access_hotkey(
    mapping: InputMappingSet,
    hotkey: Hotkey,
    config: &mut InputAppConfig,
) -> &mut Option<Vec<GenericInput>> {
    let mapping_config = mapping.hotkey(config);
    mapping_config.access_value(hotkey)
}

impl App {
    pub(super) fn render_general_input_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::GeneralInput.title()).open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Gamepad joystick axis deadzone:");
                ui.add(Slider::new(&mut self.config.input.axis_deadzone, 0..=i16::MAX));
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GeneralInput);
        }
    }

    pub(super) fn render_smsgg_input_settings(&mut self, ctx: &Context) {
        static P1_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            SmsGgButton::ALL
                .into_iter()
                .filter_map(|button| {
                    (button != SmsGgButton::Pause)
                        .then_some(GenericButton::SmsGg(button, Player::One))
                })
                .collect()
        });
        static P2_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            SmsGgButton::ALL
                .into_iter()
                .filter_map(|button| {
                    (button != SmsGgButton::Pause)
                        .then_some(GenericButton::SmsGg(button, Player::Two))
                })
                .collect()
        });

        let mut open = true;
        Window::new(OpenWindow::SmsGgInput.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.checkbox(
                &mut self.config.smsgg.allow_opposing_joypad_directions,
                ALLOW_OPPOSING_DIRECTIONS_LABEL,
            );
            ui.separator();

            let mapping = self.render_mapping_set_selector(OpenWindow::SmsGgInput, ui);
            ui.separator();

            Grid::new("smsgg_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                ui.heading("Player 1");
                ui.heading("Player 2");
                ui.end_row();

                self.render_input_buttons("smsgg_p1_input_settings", mapping, &P1_BUTTONS, ui);
                self.render_input_buttons("smsgg_p2_input_settings", mapping, &P2_BUTTONS, ui);
                ui.end_row();

                self.render_configure_all_button(&P1_BUTTONS, mapping, ui);
                self.render_configure_all_button(&P2_BUTTONS, mapping, ui);
                ui.end_row();
            });

            ui.add_space(15.0);

            self.render_input_buttons(
                "smsgg_pause_input",
                mapping,
                &[GenericButton::SmsGg(SmsGgButton::Pause, Player::One)],
                ui,
            );

            ui.add_space(15.0);

            let mapping_config = mapping.smsgg(&mut self.config.input);
            ui.horizontal(|ui| {
                ComboBox::new("smsgg_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            mapping_config.p1 = SmsGgControllerMapping::keyboard_arrows();
                            mapping_config.p1_turbo = SmsGgControllerMapping::default();
                            mapping_config.pause = SmsGgControllerMapping::keyboard_pause();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping_config.p1 = SmsGgControllerMapping::keyboard_wasd();
                            mapping_config.p1_turbo = SmsGgControllerMapping::default();
                            mapping_config.pause = SmsGgControllerMapping::keyboard_pause();
                        }
                    },
                );

                if ui.button("Clear All P1").clicked() {
                    mapping_config.p1 = SmsGgControllerMapping::default();
                    mapping_config.p1_turbo = SmsGgControllerMapping::default();
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config.p2 = SmsGgControllerMapping::default();
                    mapping_config.p2_turbo = SmsGgControllerMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgInput);
        }
    }

    pub(super) fn render_genesis_input_settings(&mut self, ctx: &Context) {
        fn genesis_buttons(
            player: Player,
            filter: impl Fn(GenesisButton) -> bool,
        ) -> Vec<GenericButton> {
            GenesisButton::ALL
                .into_iter()
                .filter_map(|button| {
                    filter(button).then_some(GenericButton::Genesis(button, player))
                })
                .collect()
        }

        static P1_GAMEPAD: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| genesis_buttons(Player::One, GenesisButton::is_gamepad));
        static P2_GAMEPAD: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| genesis_buttons(Player::Two, GenesisButton::is_gamepad));
        static P1_XE1AP: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| genesis_buttons(Player::One, GenesisButton::is_xe1ap));
        static P2_XE1AP: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| genesis_buttons(Player::Two, GenesisButton::is_xe1ap));

        let mut open = true;
        Window::new(OpenWindow::GenesisInput.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.horizontal(|ui| {
                for (label, controller_type_field) in [
                    ("Player 1 controller type", &mut self.config.input.genesis.p1_type),
                    ("Player 2 controller type", &mut self.config.input.genesis.p2_type),
                ] {
                    ui.vertical(|ui| {
                        ui.group(|ui| {
                            ui.label(label);

                            ui.horizontal(|ui| {
                                for (value, label) in [
                                    (GenesisControllerType::ThreeButton, "3-button"),
                                    (GenesisControllerType::SixButton, "6-button"),
                                    (GenesisControllerType::Xe1ap, "XE-1 AP"),
                                    (GenesisControllerType::None, "None"),
                                ] {
                                    ui.radio_value(controller_type_field, value, label);
                                }
                            });
                        });
                    });
                }
            });

            ui.checkbox(
                &mut self.config.genesis.allow_opposing_joypad_directions,
                ALLOW_OPPOSING_DIRECTIONS_LABEL,
            );
            ui.checkbox(
                &mut self.config.genesis.auto_3_button_mode,
                "Automatically force 3-button mode in 6-button-incompatible games",
            );
            ui.separator();

            let mapping = self.render_mapping_set_selector(OpenWindow::GenesisInput, ui);
            ui.separator();

            let p1_type = self.config.input.genesis.p1_type;
            let p2_type = self.config.input.genesis.p2_type;

            Grid::new("genesis_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                let (p1_heading, p1_buttons) = match p1_type {
                    GenesisControllerType::Xe1ap => ("Player 1 - XE-1 AP", &P1_XE1AP),
                    _ => ("Player 1 - Gamepad", &P1_GAMEPAD),
                };
                let (p2_heading, p2_buttons) = match p2_type {
                    GenesisControllerType::Xe1ap => ("Player 2 - XE-1 AP", &P2_XE1AP),
                    _ => ("Player 2 - Gamepad", &P2_GAMEPAD),
                };

                ui.heading(p1_heading);
                ui.heading(p2_heading);
                ui.end_row();

                self.render_input_buttons("genesis_p1_input_settings", mapping, p1_buttons, ui);
                self.render_input_buttons("genesis_p2_input_settings", mapping, p2_buttons, ui);
                ui.end_row();

                self.render_configure_all_button(p1_buttons, mapping, ui);
                self.render_configure_all_button(p2_buttons, mapping, ui);
                ui.end_row();
            });

            ui.add_space(15.0);

            let mapping_config = mapping.genesis(&mut self.config.input);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(p1_type != GenesisControllerType::Xe1ap, |ui| {
                    ComboBox::new("genesis_presets", "").selected_text("Apply preset...").show_ui(
                        ui,
                        |ui| {
                            if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                                mapping_config.p1.clone_from_type(
                                    &GenesisControllerMapping::keyboard_arrows(),
                                    p1_type,
                                );
                                mapping_config
                                    .p1_turbo
                                    .clone_from_type(&GenesisControllerMapping::default(), p1_type);
                            }

                            if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                                mapping_config.p1.clone_from_type(
                                    &GenesisControllerMapping::keyboard_wasd(),
                                    p1_type,
                                );
                                mapping_config
                                    .p1_turbo
                                    .clone_from_type(&GenesisControllerMapping::default(), p1_type);
                            }
                        },
                    );
                });

                if ui.button("Clear All P1").clicked() {
                    mapping_config
                        .p1
                        .clone_from_type(&GenesisControllerMapping::default(), p1_type);
                    mapping_config
                        .p1_turbo
                        .clone_from_type(&GenesisControllerMapping::default(), p1_type);
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config
                        .p2
                        .clone_from_type(&GenesisControllerMapping::default(), p2_type);
                    mapping_config
                        .p2_turbo
                        .clone_from_type(&GenesisControllerMapping::default(), p2_type);
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisInput);
        }
    }

    pub(super) fn render_nes_input_settings(&mut self, ctx: &Context) {
        static P1_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            NesButton::ALL
                .into_iter()
                .filter_map(|button| {
                    (!button.is_zapper()).then_some(GenericButton::Nes(button, Player::One))
                })
                .collect()
        });
        static P2_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            NesButton::ALL
                .into_iter()
                .filter_map(|button| {
                    (!button.is_zapper()).then_some(GenericButton::Nes(button, Player::Two))
                })
                .collect()
        });

        let mut open = true;
        Window::new(OpenWindow::NesInput.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let rect = ui
                .checkbox(
                    &mut self.config.nes.allow_opposing_joypad_directions,
                    ALLOW_OPPOSING_DIRECTIONS_LABEL,
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state
                    .help_text
                    .insert(OpenWindow::NesInput, helptext::NES_OPPOSING_JOYPAD_DIRECTIONS);
            }
            ui.separator();

            let mapping = self.render_mapping_set_selector(OpenWindow::NesInput, ui);
            ui.separator();

            Grid::new("nes_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                ui.heading("Player 1");
                ui.heading("Player 2");
                ui.end_row();

                self.render_input_buttons("nes_p1_inputs", mapping, &P1_BUTTONS, ui);
                self.render_input_buttons("nes_p2_inputs", mapping, &P2_BUTTONS, ui);
                ui.end_row();

                self.render_configure_all_button(&P1_BUTTONS, mapping, ui);
                self.render_configure_all_button(&P2_BUTTONS, mapping, ui);
                ui.end_row();
            });

            ui.add_space(15.0);

            let mapping_config = mapping.nes(&mut self.config.input);
            ui.horizontal(|ui| {
                ComboBox::new("nes_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            mapping_config.p1 = NesControllerMapping::keyboard_arrows();
                            mapping_config.p1_turbo = NesControllerMapping::default();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping_config.p1 = NesControllerMapping::keyboard_wasd();
                            mapping_config.p1_turbo = NesControllerMapping::default();
                        }
                    },
                );

                if ui.button("Clear All P1").clicked() {
                    mapping_config.p1 = NesControllerMapping::default();
                    mapping_config.p1_turbo = NesControllerMapping::default();
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config.p2 = NesControllerMapping::default();
                    mapping_config.p2_turbo = NesControllerMapping::default();
                }
            });

            self.render_help_text(ui, OpenWindow::NesInput);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::NesInput);
        }
    }

    pub(super) fn render_nes_peripheral_settings(&mut self, ctx: &Context) {
        static ZAPPER_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            NesButton::ALL
                .into_iter()
                .filter_map(|button| {
                    button.is_zapper().then_some(GenericButton::Nes(button, Player::One))
                })
                .collect()
        });

        let mut open = true;
        Window::new(OpenWindow::NesPeripherals.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.group(|ui| {
                ui.label("Player 2 device");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.input.nes.p2_type,
                        NesControllerType::Gamepad,
                        "Gamepad",
                    );
                    ui.radio_value(
                        &mut self.config.input.nes.p2_type,
                        NesControllerType::Zapper,
                        "Zapper",
                    );
                });
            });

            ui.separator();
            let mapping = self.render_mapping_set_selector(OpenWindow::NesPeripherals, ui);
            ui.separator();

            ui.heading("Zapper");

            ui.add_space(5.0);

            self.render_input_buttons("nes_zapper_inputs", mapping, &ZAPPER_BUTTONS, ui);

            ui.add_space(15.0);

            let mapping_config = mapping.nes(&mut self.config.input);
            ui.horizontal(|ui| {
                if ui.button("Restore Defaults").clicked() {
                    mapping_config.zapper = NesZapperMapping::mouse();
                }

                if ui.button("Clear All").clicked() {
                    mapping_config.zapper = NesZapperMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::NesPeripherals);
        }
    }

    pub(super) fn render_snes_input_settings(&mut self, ctx: &Context) {
        static P1_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            SnesButton::ALL
                .into_iter()
                .filter_map(|button| {
                    button
                        .to_super_scope()
                        .is_none()
                        .then_some(GenericButton::Snes(button, Player::One))
                })
                .collect()
        });
        static P2_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            SnesButton::ALL
                .into_iter()
                .filter_map(|button| {
                    button
                        .to_super_scope()
                        .is_none()
                        .then_some(GenericButton::Snes(button, Player::Two))
                })
                .collect()
        });

        let mut open = true;
        Window::new(OpenWindow::SnesInput.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.checkbox(
                &mut self.config.snes.allow_opposing_joypad_directions,
                ALLOW_OPPOSING_DIRECTIONS_LABEL,
            );
            ui.separator();

            let mapping = self.render_mapping_set_selector(OpenWindow::SnesInput, ui);
            ui.separator();

            Grid::new("snes_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                ui.heading("Player 1");
                ui.heading("Player 2");
                ui.end_row();

                self.render_input_buttons("snes_p1_inputs", mapping, &P1_BUTTONS, ui);
                self.render_input_buttons("snes_p2_inputs", mapping, &P2_BUTTONS, ui);
                ui.end_row();

                self.render_configure_all_button(&P1_BUTTONS, mapping, ui);
                self.render_configure_all_button(&P2_BUTTONS, mapping, ui);
                ui.end_row();
            });

            ui.add_space(15.0);

            let mapping_config = mapping.snes(&mut self.config.input);
            ui.horizontal(|ui| {
                ComboBox::new("snes_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            mapping_config.p1 = SnesControllerMapping::keyboard_arrows();
                            mapping_config.p1_turbo = SnesControllerMapping::default();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping_config.p1 = SnesControllerMapping::keyboard_wasd();
                            mapping_config.p1_turbo = SnesControllerMapping::default();
                        }
                    },
                );

                if ui.button("Clear All P1").clicked() {
                    mapping_config.p1 = SnesControllerMapping::default();
                    mapping_config.p1_turbo = SnesControllerMapping::default();
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config.p2 = SnesControllerMapping::default();
                    mapping_config.p2_turbo = SnesControllerMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesInput);
        }
    }

    pub(super) fn render_snes_peripheral_settings(&mut self, ctx: &Context) {
        static SUPER_SCOPE_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            SnesButton::ALL
                .into_iter()
                .filter_map(|button| {
                    button.to_super_scope().map(|_| GenericButton::Snes(button, Player::One))
                })
                .collect()
        });

        let mut open = true;
        Window::new(OpenWindow::SnesPeripherals.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.group(|ui| {
                ui.label("Player 2 device");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.input.snes.p2_type,
                        SnesControllerType::Gamepad,
                        "Gamepad",
                    );
                    ui.radio_value(
                        &mut self.config.input.snes.p2_type,
                        SnesControllerType::SuperScope,
                        "Super Scope",
                    );
                });
            });

            ui.separator();
            let mapping = self.render_mapping_set_selector(OpenWindow::SnesPeripherals, ui);
            ui.separator();

            ui.heading("Super Scope");

            ui.add_space(5.0);

            self.render_input_buttons("super_scope_inputs", mapping, &SUPER_SCOPE_BUTTONS, ui);

            ui.add_space(15.0);

            let mapping_config = mapping.snes(&mut self.config.input);
            ui.horizontal(|ui| {
                if ui.button("Restore Defaults").clicked() {
                    mapping_config.super_scope = SnesSuperScopeMapping::mouse();
                }

                if ui.button("Clear All").clicked() {
                    mapping_config.super_scope = SnesSuperScopeMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesPeripherals);
        }
    }

    pub(super) fn render_gb_input_settings(&mut self, ctx: &Context) {
        static BUTTONS: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| GameBoyButton::ALL.into_iter().map(GenericButton::GameBoy).collect());

        let mut open = true;
        Window::new(OpenWindow::GameBoyInput.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.checkbox(
                &mut self.config.game_boy.allow_opposing_joypad_directions,
                ALLOW_OPPOSING_DIRECTIONS_LABEL,
            );
            ui.separator();

            let mapping = self.render_mapping_set_selector(OpenWindow::GameBoyInput, ui);
            ui.separator();

            self.render_input_buttons("gb_inputs", mapping, &BUTTONS, ui);

            self.render_configure_all_button(&BUTTONS, mapping, ui);

            ui.add_space(15.0);

            ui.horizontal(|ui| {
                ComboBox::new("gb_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            *mapping.gb(&mut self.config.input, false) =
                                GameBoyInputMapping::keyboard_arrows();
                            *mapping.gb(&mut self.config.input, true) =
                                GameBoyInputMapping::default();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            *mapping.gb(&mut self.config.input, false) =
                                GameBoyInputMapping::keyboard_wasd();
                            *mapping.gb(&mut self.config.input, true) =
                                GameBoyInputMapping::default();
                        }
                    },
                );

                if ui.button("Clear All").clicked() {
                    *mapping.gb(&mut self.config.input, false) = GameBoyInputMapping::default();
                    *mapping.gb(&mut self.config.input, true) = GameBoyInputMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GameBoyInput);
        }
    }

    pub(super) fn render_gba_input_settings(&mut self, ctx: &Context) {
        static BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            GbaButton::ALL
                .into_iter()
                .filter_map(|button| button.is_joypad().then_some(GenericButton::Gba(button)))
                .collect()
        });

        let mut open = true;
        Window::new(OpenWindow::GbaInput.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.checkbox(
                &mut self.config.game_boy_advance.allow_opposing_joypad_directions,
                ALLOW_OPPOSING_DIRECTIONS_LABEL,
            );
            ui.separator();

            let mapping = self.render_mapping_set_selector(OpenWindow::GbaInput, ui);
            ui.separator();

            self.render_input_buttons("gba_inputs", mapping, &BUTTONS, ui);

            self.render_configure_all_button(&BUTTONS, mapping, ui);

            ui.add_space(15.0);

            ui.horizontal(|ui| {
                ComboBox::new("gba_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            mapping.gba(&mut self.config.input, false).joypad =
                                GbaJoypadMapping::keyboard_arrows();
                            mapping.gba(&mut self.config.input, true).joypad =
                                GbaJoypadMapping::default();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping.gba(&mut self.config.input, false).joypad =
                                GbaJoypadMapping::keyboard_wasd();
                            mapping.gba(&mut self.config.input, true).joypad =
                                GbaJoypadMapping::default();
                        }
                    },
                );

                if ui.button("Clear All").clicked() {
                    mapping.gba(&mut self.config.input, false).joypad = GbaJoypadMapping::default();
                    mapping.gba(&mut self.config.input, true).joypad = GbaJoypadMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GbaInput);
        }
    }

    pub(super) fn render_gba_peripheral_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GbaPeripherals;

        static SOLAR_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            GbaButton::ALL
                .into_iter()
                .filter_map(|button| button.is_solar_sensor().then_some(GenericButton::Gba(button)))
                .collect()
        });

        let mut open = true;
        Window::new(OpenWindow::GbaPeripherals.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(WINDOW, ui);
            ui.separator();

            ui.heading("Solar Sensor");

            self.render_input_buttons("solar_sensor_inputs", mapping, &SOLAR_BUTTONS, ui);

            ui.add_space(15.0);

            let mapping_config = mapping.gba(&mut self.config.input, false);
            if ui.button("Clear All").clicked() {
                mapping_config.solar = GbaSolarMapping::default();
            }

            ui.add_space(15.0);

            let prev_slider_width = mem::replace(&mut ui.style_mut().spacing.slider_width, 200.0);

            Grid::new("gba_solar_grid").show(ui, |ui| {
                ui.label("Brightness step");

                ui.add(Slider::new(
                    &mut self.config.game_boy_advance.solar_brightness_step,
                    1..=255,
                ));

                if ui.button("Default").clicked() {
                    self.config.game_boy_advance.solar_brightness_step =
                        gba_config::DEFAULT_SOLAR_BRIGHTNESS_STEP;
                }

                ui.end_row();

                ui.label("Minimum brightness");

                ui.add(Slider::new(
                    &mut self.config.game_boy_advance.solar_min_brightness,
                    0..=255,
                ));

                if ui.button("Default").clicked() {
                    self.config.game_boy_advance.solar_min_brightness =
                        gba_config::DEFAULT_SOLAR_MIN_BRIGHTNESS;
                }

                ui.end_row();

                ui.label("Maximum brightness");

                ui.add(Slider::new(
                    &mut self.config.game_boy_advance.solar_max_brightness,
                    0..=255,
                ));

                if ui.button("Default").clicked() {
                    self.config.game_boy_advance.solar_max_brightness =
                        gba_config::DEFAULT_SOLAR_MAX_BRIGHTNESS;
                }

                ui.end_row();
            });

            ui.style_mut().spacing.slider_width = prev_slider_width;
        });

        if !open {
            self.state.open_windows.remove(&OpenWindow::GbaPeripherals);
        }
    }

    pub(super) fn render_pce_input_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::PceInput.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            ui.group(|ui| {
                ui.label("Controller port");

                ui.horizontal(|ui| {
                    for (value, label) in [
                        (PceInputDevice::TwoButtonGamepad, "Gamepad"),
                        (PceInputDevice::TurboTap, "Turbo Tap"),
                    ] {
                        ui.radio_value(&mut self.config.pc_engine.input_device, value, label);
                    }
                });

                ui.horizontal(|ui| {
                    ui.add_enabled_ui(
                        self.config.pc_engine.input_device == PceInputDevice::TurboTap,
                        |ui| {
                            ui.label("Turbo Tap gamepads:");

                            for (idx, connected) in
                                self.config.pc_engine.turbo_tap_connected.iter_mut().enumerate()
                            {
                                ui.checkbox(connected, (idx + 1).to_string());
                            }
                        },
                    );
                });
            });

            let rect = ui
                .checkbox(
                    &mut self.config.pc_engine.allow_opposing_joypad_directions,
                    ALLOW_OPPOSING_DIRECTIONS_LABEL,
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state
                    .help_text
                    .insert(OpenWindow::PceInput, helptext::OPPOSING_JOYPAD_DIRECTIONS);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.pc_engine.allow_simultaneous_run_select,
                    "Allow pressing Run+Select simultaneously",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state
                    .help_text
                    .insert(OpenWindow::PceInput, helptext::PCE_SIMULTANEOUS_RUN_SELECT);
            }

            ui.separator();

            let mapping = self.render_mapping_set_selector(OpenWindow::PceInput, ui);
            ui.separator();

            let using_turbo_tap = self.config.pc_engine.input_device == PceInputDevice::TurboTap;
            if !using_turbo_tap {
                self.state.pce_selected_player_idx = 0;
            }

            ui.add_enabled_ui(using_turbo_tap, |ui| {
                ComboBox::new("pce_player_combo", "").show_index(
                    ui,
                    &mut self.state.pce_selected_player_idx,
                    5,
                    |player_idx| format!("Player {}", player_idx + 1),
                );
            });
            ui.add_space(5.0);

            let player = [Player::One, Player::Two, Player::Three, Player::Four, Player::Five]
                [self.state.pce_selected_player_idx];

            Grid::new("pce_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                let buttons = PceButton::ALL
                    .into_iter()
                    .map(|button| GenericButton::Pce(button, player))
                    .collect::<Vec<_>>();

                self.render_input_buttons("pce_p1_inputs", mapping, &buttons, ui);
                ui.end_row();

                self.render_configure_all_button(&buttons, mapping, ui);
                ui.end_row();
            });

            ui.add_space(15.0);

            let mapping_config = mapping.pce(&mut self.config.input);
            let (mapping, turbo_mapping) = match player {
                Player::One => (&mut mapping_config.p1, &mut mapping_config.p1_turbo),
                Player::Two => (&mut mapping_config.p2, &mut mapping_config.p2_turbo),
                Player::Three => (&mut mapping_config.p3, &mut mapping_config.p3_turbo),
                Player::Four => (&mut mapping_config.p4, &mut mapping_config.p4_turbo),
                Player::Five => (&mut mapping_config.p5, &mut mapping_config.p5_turbo),
                _ => unreachable!("player is always 1-5"),
            };

            ui.horizontal(|ui| {
                ComboBox::new("pce_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            *mapping = PceJoypadMapping::keyboard_arrows();
                            *turbo_mapping = PceJoypadMapping::default();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            *mapping = PceJoypadMapping::keyboard_wasd();
                            *turbo_mapping = PceJoypadMapping::default();
                        }
                    },
                );

                if ui.button("Clear All").clicked() {
                    *mapping = PceJoypadMapping::default();
                    *turbo_mapping = PceJoypadMapping::default();
                }
            });

            self.render_help_text(ui, OpenWindow::PceInput);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::PceInput);
        }
    }

    pub(super) fn render_hotkey_settings(&mut self, ctx: &Context) {
        static GENERAL_HOTKEYS: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| hotkey_vec(HotkeyCategory::General));
        static STATE_HOTKEYS: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| hotkey_vec(HotkeyCategory::SaveState));

        let mut open = true;
        Window::new(OpenWindow::Hotkeys.title()).open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(OpenWindow::Hotkeys, ui);
            ui.separator();

            ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(ctx.content_rect().height() * 0.5)
                .show(ui, |ui| {
                    ui.heading("General");
                    self.render_input_buttons("general_hotkeys", mapping, &GENERAL_HOTKEYS, ui);

                    ui.separator();

                    ui.heading("Save States");
                    self.render_input_buttons("state_hotkeys", mapping, &STATE_HOTKEYS, ui);
                });

            ui.add_space(15.0);

            let mapping_config = mapping.hotkey(&mut self.config.input);
            ui.horizontal(|ui| {
                if ui.button("Restore Defaults").clicked() {
                    *mapping_config = HotkeyMapping::default_keyboard();
                }

                if ui.button("Clear All").clicked() {
                    *mapping_config = HotkeyMapping::default();
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.ff_multiplier_text,
                        &mut self.config.common.fast_forward_multiplier,
                        &mut self.state.ff_multiplier_invalid,
                    )
                    .with_validation(|value| value != 0)
                    .desired_width(30.0),
                );

                ui.label("Fast forward multiplier");
            });
            if self.state.ff_multiplier_invalid {
                ui.colored_label(
                    Color32::RED,
                    "Fast forward multiplier must be a positive integer",
                );
            }

            ui.horizontal(|ui| {
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.rewind_buffer_len_text,
                        &mut self.config.common.rewind_buffer_length_seconds,
                        &mut self.state.rewind_buffer_len_invalid,
                    )
                    .desired_width(30.0),
                );

                ui.label("Rewind buffer length in seconds");
            });
            if self.state.rewind_buffer_len_invalid {
                ui.colored_label(
                    Color32::RED,
                    "Rewind buffer length must be a non-negative integer",
                );
            }

            ui.checkbox(
                &mut self.config.common.load_recent_state_at_launch,
                "Load most recent save state at launch",
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::Hotkeys);
        }
    }

    fn disable_if_waiting_for_input(&self, ui: &mut Ui) {
        if self.state.waiting_for_input.is_some() {
            ui.disable();
        }
    }

    fn render_input_buttons(
        &mut self,
        id: &str,
        mapping: InputMappingSet,
        buttons: &[GenericButton],
        ui: &mut Ui,
    ) {
        Grid::new(id).show(ui, |ui| {
            for button in buttons {
                ui.label(format!("{}:", button.label()));

                let Some(current_value) = button.access_value(mapping, &mut self.config.input)
                else {
                    continue;
                };

                let current_value_str = format_input_str(current_value.as_ref());
                if ui.button(current_value_str).clicked() {
                    send_collect_input_request(
                        &self.emu_thread,
                        &mut self.state.waiting_for_input,
                        vec![*button],
                        mapping,
                        false,
                    );
                }

                if ui.button("Clear").clicked() {
                    *current_value = None;
                }

                if let Some(turbo_value) =
                    button.access_value_turbo(mapping, &mut self.config.input)
                {
                    ui.label("Turbo");

                    let turbo_value_str = format_input_str(turbo_value.as_ref());
                    if ui.button(turbo_value_str).clicked() {
                        send_collect_input_request(
                            &self.emu_thread,
                            &mut self.state.waiting_for_input,
                            vec![*button],
                            mapping,
                            true,
                        );
                    }

                    if ui.button("Clear").clicked() {
                        *turbo_value = None;
                    }
                }

                ui.end_row();
            }
        });
    }

    fn render_mapping_set_selector(&mut self, window: OpenWindow, ui: &mut Ui) -> InputMappingSet {
        let field = self.state.input_mapping_sets.entry(window).or_default();

        ui.horizontal(|ui| {
            for set in [InputMappingSet::One, InputMappingSet::Two] {
                let button = Button::new(set.label()).selected(*field == set);
                if ui.add(button).clicked() {
                    *field = set;
                }
            }
        });

        *field
    }

    fn render_configure_all_button(
        &mut self,
        buttons: &[GenericButton],
        mapping: InputMappingSet,
        ui: &mut Ui,
    ) {
        ui.add_enabled_ui(!self.emu_thread.status().is_running(), |ui| {
            if ui.button("Configure all").clicked() {
                send_collect_input_request(
                    &self.emu_thread,
                    &mut self.state.waiting_for_input,
                    buttons.to_vec(),
                    mapping,
                    false,
                );
            }
        });
    }
}

fn send_collect_input_request(
    emu_thread: &EmuThreadHandle,
    waiting_for_input: &mut Option<WaitingForInput>,
    buttons: Vec<GenericButton>,
    mapping: InputMappingSet,
    turbo: bool,
) {
    emu_thread.send(EmuThreadCommand::CollectInput(buttons.clone()));
    *waiting_for_input = Some(WaitingForInput { buttons, mapping, turbo });
}

fn format_input_str(value: Option<&Vec<GenericInput>>) -> String {
    let none = || "<None>".into();

    let Some(value) = value else {
        return none();
    };

    if value.is_empty() {
        return none();
    }

    let s: Vec<_> = value.iter().map(|&input| input.to_string()).collect();

    s.join(" + ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyCategory {
    General,
    SaveState,
}

trait HotkeyExt {
    fn category(self) -> HotkeyCategory;
}

impl HotkeyExt for Hotkey {
    fn category(self) -> HotkeyCategory {
        use Hotkey::*;

        match self {
            PowerOff | Exit | ToggleFullscreen | SoftReset | HardReset | Pause | StepFrame
            | FastForward | Rewind | ToggleOverclocking | OpenDebugger => HotkeyCategory::General,
            SaveState | LoadState | NextSaveStateSlot | PrevSaveStateSlot | SaveStateSlot0
            | SaveStateSlot1 | SaveStateSlot2 | SaveStateSlot3 | SaveStateSlot4
            | SaveStateSlot5 | SaveStateSlot6 | SaveStateSlot7 | SaveStateSlot8
            | SaveStateSlot9 | LoadStateSlot0 | LoadStateSlot1 | LoadStateSlot2
            | LoadStateSlot3 | LoadStateSlot4 | LoadStateSlot5 | LoadStateSlot6
            | LoadStateSlot7 | LoadStateSlot8 | LoadStateSlot9 => HotkeyCategory::SaveState,
        }
    }
}

fn hotkey_vec(category: HotkeyCategory) -> Vec<GenericButton> {
    Hotkey::ALL
        .into_iter()
        .filter_map(|hotkey| {
            (hotkey.category() == category).then_some(GenericButton::Hotkey(hotkey))
        })
        .collect()
}
