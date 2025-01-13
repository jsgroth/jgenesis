use crate::app::{App, NumericTextEdit, OpenWindow};
use crate::emuthread::EmuThreadCommand;
use egui::{Button, Color32, ComboBox, Context, Grid, ScrollArea, Slider, Ui, Window};
use gb_core::inputs::GameBoyButton;
use genesis_core::GenesisControllerType;
use genesis_core::input::GenesisButton;
use jgenesis_common::input::Player;
use jgenesis_native_config::input::InputAppConfig;
use jgenesis_native_driver::config::input::{
    GameBoyInputMapping, GenesisControllerMapping, GenesisInputMapping, HotkeyMapping,
    NesControllerMapping, NesControllerType, NesInputMapping, NesZapperMapping,
    SmsGgControllerMapping, SmsGgInputMapping, SnesControllerMapping, SnesControllerType,
    SnesInputMapping, SnesSuperScopeMapping,
};
use jgenesis_native_driver::input::{GenericInput, Hotkey};
use nes_core::input::NesButton;
use smsgg_core::SmsGgButton;
use snes_core::input::SnesButton;
use std::sync::LazyLock;

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

    fn gb(self, config: &mut InputAppConfig) -> &mut GameBoyInputMapping {
        match self {
            Self::One => &mut config.game_boy.mapping_1,
            Self::Two => &mut config.game_boy.mapping_2,
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
    Hotkey(Hotkey),
}

impl GenericButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::SmsGg(button, _) => smsgg_label(button),
            Self::Genesis(button, _) => genesis_label(button),
            Self::Nes(button, _) => nes_label(button),
            Self::Snes(button, _) => snes_label(button),
            Self::GameBoy(button) => gb_label(button),
            Self::Hotkey(hotkey) => hotkey_label(hotkey),
        }
    }

    pub fn access_value(
        self,
        mapping: InputMappingSet,
        config: &mut InputAppConfig,
    ) -> &mut Option<Vec<GenericInput>> {
        match self {
            Self::SmsGg(button, player) => access_smsgg_value(mapping, button, player, config),
            Self::Genesis(button, player) => access_genesis_value(mapping, button, player, config),
            Self::Nes(button, player) => access_nes_value(mapping, button, player, config),
            Self::Snes(button, player) => access_snes_value(mapping, button, player, config),
            Self::GameBoy(button) => access_gb_value(mapping, button, config),
            Self::Hotkey(hotkey) => access_hotkey(mapping, hotkey, config),
        }
    }
}

fn smsgg_label(button: SmsGgButton) -> &'static str {
    use SmsGgButton::*;

    match button {
        Up => "Up:",
        Left => "Left:",
        Right => "Right:",
        Down => "Down:",
        Button1 => "Button 1:",
        Button2 => "Button 2:",
        Pause => "Start/Pause:",
    }
}

fn genesis_label(button: GenesisButton) -> &'static str {
    use GenesisButton::*;

    match button {
        Up => "Up:",
        Left => "Left:",
        Right => "Right:",
        Down => "Down:",
        A => "A:",
        B => "B:",
        C => "C:",
        X => "X:",
        Y => "Y:",
        Z => "Z:",
        Start => "Start:",
        Mode => "Mode:",
    }
}

fn nes_label(button: NesButton) -> &'static str {
    use NesButton::*;

    match button {
        Up => "Up:",
        Left => "Left:",
        Right => "Right:",
        Down => "Down:",
        A => "A:",
        B => "B:",
        Start => "Start:",
        Select => "Select:",
        ZapperFire => "Fire:",
        ZapperForceOffscreen => "Force offscreen (Hold):",
    }
}

fn snes_label(button: SnesButton) -> &'static str {
    use SnesButton::*;

    match button {
        Up => "Up:",
        Left => "Left:",
        Right => "Right:",
        Down => "Down:",
        A => "A:",
        B => "B:",
        X => "X:",
        Y => "Y:",
        L => "L:",
        R => "R:",
        Start => "Start:",
        Select => "Select:",
        SuperScopeFire => "Fire:",
        SuperScopeCursor => "Cursor:",
        SuperScopePause => "Pause:",
        SuperScopeTurboToggle => "Turbo (Toggle):",
    }
}

fn gb_label(button: GameBoyButton) -> &'static str {
    use GameBoyButton::*;

    match button {
        Up => "Up:",
        Left => "Left:",
        Right => "Right:",
        Down => "Down:",
        A => "A:",
        B => "B:",
        Start => "Start:",
        Select => "Select:",
    }
}

fn hotkey_label(hotkey: Hotkey) -> &'static str {
    use Hotkey::*;

    match hotkey {
        PowerOff => "Power off emulated system:",
        Exit => "Exit application:",
        ToggleFullscreen => "Toggle fullscreen:",
        SaveState => "Save state to current slot:",
        LoadState => "Load state from current slot:",
        NextSaveStateSlot => "Next save state slot:",
        PrevSaveStateSlot => "Previous save state slot:",
        SoftReset => "Soft reset:",
        HardReset => "Hard reset:",
        Pause => "Pause:",
        StepFrame => "Step to next frame:",
        FastForward => "Fast forward:",
        Rewind => "Rewind:",
        ToggleOverclocking => "Toggle overclocking enabled:",
        OpenDebugger => "Open memory viewer:",
        SaveStateSlot0 => "Save state to slot 0:",
        SaveStateSlot1 => "Save state to slot 1:",
        SaveStateSlot2 => "Save state to slot 2:",
        SaveStateSlot3 => "Save state to slot 3:",
        SaveStateSlot4 => "Save state to slot 4:",
        SaveStateSlot5 => "Save state to slot 5:",
        SaveStateSlot6 => "Save state to slot 6:",
        SaveStateSlot7 => "Save state to slot 7:",
        SaveStateSlot8 => "Save state to slot 8:",
        SaveStateSlot9 => "Save state to slot 9:",
        LoadStateSlot0 => "Load state from slot 0:",
        LoadStateSlot1 => "Load state from slot 1:",
        LoadStateSlot2 => "Load state from slot 2:",
        LoadStateSlot3 => "Load state from slot 3:",
        LoadStateSlot4 => "Load state from slot 4:",
        LoadStateSlot5 => "Load state from slot 5:",
        LoadStateSlot6 => "Load state from slot 6:",
        LoadStateSlot7 => "Load state from slot 7:",
        LoadStateSlot8 => "Load state from slot 8:",
        LoadStateSlot9 => "Load state from slot 9:",
    }
}

fn access_smsgg_value(
    mapping: InputMappingSet,
    button: SmsGgButton,
    player: Player,
    config: &mut InputAppConfig,
) -> &mut Option<Vec<GenericInput>> {
    let mapping_config = mapping.smsgg(config);

    if button == SmsGgButton::Pause {
        return &mut mapping_config.pause;
    }

    let player_config = match player {
        Player::One => &mut mapping_config.p1,
        Player::Two => &mut mapping_config.p2,
    };

    match button {
        SmsGgButton::Up => &mut player_config.up,
        SmsGgButton::Left => &mut player_config.left,
        SmsGgButton::Right => &mut player_config.right,
        SmsGgButton::Down => &mut player_config.down,
        SmsGgButton::Button1 => &mut player_config.button1,
        SmsGgButton::Button2 => &mut player_config.button2,
        SmsGgButton::Pause => unreachable!("early return for Pause"),
    }
}

fn access_genesis_value(
    mapping: InputMappingSet,
    button: GenesisButton,
    player: Player,
    config: &mut InputAppConfig,
) -> &mut Option<Vec<GenericInput>> {
    let mapping_config = mapping.genesis(config);

    let player_config = match player {
        Player::One => &mut mapping_config.p1,
        Player::Two => &mut mapping_config.p2,
    };

    match button {
        GenesisButton::Up => &mut player_config.up,
        GenesisButton::Left => &mut player_config.left,
        GenesisButton::Right => &mut player_config.right,
        GenesisButton::Down => &mut player_config.down,
        GenesisButton::A => &mut player_config.a,
        GenesisButton::B => &mut player_config.b,
        GenesisButton::C => &mut player_config.c,
        GenesisButton::X => &mut player_config.x,
        GenesisButton::Y => &mut player_config.y,
        GenesisButton::Z => &mut player_config.z,
        GenesisButton::Start => &mut player_config.start,
        GenesisButton::Mode => &mut player_config.mode,
    }
}

fn access_nes_value(
    mapping: InputMappingSet,
    button: NesButton,
    player: Player,
    config: &mut InputAppConfig,
) -> &mut Option<Vec<GenericInput>> {
    let mapping_config = mapping.nes(config);

    match button {
        NesButton::ZapperFire => return &mut mapping_config.zapper.fire,
        NesButton::ZapperForceOffscreen => return &mut mapping_config.zapper.force_offscreen,
        _ => {}
    }

    let player_config = match player {
        Player::One => &mut mapping_config.p1,
        Player::Two => &mut mapping_config.p2,
    };

    match button {
        NesButton::Up => &mut player_config.up,
        NesButton::Left => &mut player_config.left,
        NesButton::Right => &mut player_config.right,
        NesButton::Down => &mut player_config.down,
        NesButton::A => &mut player_config.a,
        NesButton::B => &mut player_config.b,
        NesButton::Start => &mut player_config.start,
        NesButton::Select => &mut player_config.select,
        NesButton::ZapperFire | NesButton::ZapperForceOffscreen => {
            unreachable!("early return for Zapper buttons")
        }
    }
}

fn access_snes_value(
    mapping: InputMappingSet,
    button: SnesButton,
    player: Player,
    config: &mut InputAppConfig,
) -> &mut Option<Vec<GenericInput>> {
    let mapping_config = mapping.snes(config);

    match button {
        SnesButton::SuperScopeFire => return &mut mapping_config.super_scope.fire,
        SnesButton::SuperScopeCursor => return &mut mapping_config.super_scope.cursor,
        SnesButton::SuperScopePause => return &mut mapping_config.super_scope.pause,
        SnesButton::SuperScopeTurboToggle => return &mut mapping_config.super_scope.turbo_toggle,
        _ => {}
    }

    let player_config = match player {
        Player::One => &mut mapping_config.p1,
        Player::Two => &mut mapping_config.p2,
    };

    match button {
        SnesButton::Up => &mut player_config.up,
        SnesButton::Left => &mut player_config.left,
        SnesButton::Right => &mut player_config.right,
        SnesButton::Down => &mut player_config.down,
        SnesButton::A => &mut player_config.a,
        SnesButton::B => &mut player_config.b,
        SnesButton::X => &mut player_config.x,
        SnesButton::Y => &mut player_config.y,
        SnesButton::L => &mut player_config.l,
        SnesButton::R => &mut player_config.r,
        SnesButton::Start => &mut player_config.start,
        SnesButton::Select => &mut player_config.select,
        SnesButton::SuperScopeFire
        | SnesButton::SuperScopeCursor
        | SnesButton::SuperScopePause
        | SnesButton::SuperScopeTurboToggle => unreachable!("early return for Super Scope buttons"),
    }
}

fn access_gb_value(
    mapping: InputMappingSet,
    button: GameBoyButton,
    config: &mut InputAppConfig,
) -> &mut Option<Vec<GenericInput>> {
    let mapping_config = mapping.gb(config);

    match button {
        GameBoyButton::Up => &mut mapping_config.up,
        GameBoyButton::Left => &mut mapping_config.left,
        GameBoyButton::Right => &mut mapping_config.right,
        GameBoyButton::Down => &mut mapping_config.down,
        GameBoyButton::A => &mut mapping_config.a,
        GameBoyButton::B => &mut mapping_config.b,
        GameBoyButton::Start => &mut mapping_config.start,
        GameBoyButton::Select => &mut mapping_config.select,
    }
}

fn access_hotkey(
    mapping: InputMappingSet,
    hotkey: Hotkey,
    config: &mut InputAppConfig,
) -> &mut Option<Vec<GenericInput>> {
    use Hotkey::*;

    let mapping_config = mapping.hotkey(config);

    match hotkey {
        PowerOff => &mut mapping_config.power_off,
        Exit => &mut mapping_config.exit,
        ToggleFullscreen => &mut mapping_config.toggle_fullscreen,
        SaveState => &mut mapping_config.save_state,
        LoadState => &mut mapping_config.load_state,
        NextSaveStateSlot => &mut mapping_config.next_save_state_slot,
        PrevSaveStateSlot => &mut mapping_config.prev_save_state_slot,
        SoftReset => &mut mapping_config.soft_reset,
        HardReset => &mut mapping_config.hard_reset,
        Pause => &mut mapping_config.pause,
        StepFrame => &mut mapping_config.step_frame,
        FastForward => &mut mapping_config.fast_forward,
        Rewind => &mut mapping_config.rewind,
        ToggleOverclocking => &mut mapping_config.toggle_overclocking,
        OpenDebugger => &mut mapping_config.open_debugger,
        SaveStateSlot0 => &mut mapping_config.save_state_slot_0,
        SaveStateSlot1 => &mut mapping_config.save_state_slot_1,
        SaveStateSlot2 => &mut mapping_config.save_state_slot_2,
        SaveStateSlot3 => &mut mapping_config.save_state_slot_3,
        SaveStateSlot4 => &mut mapping_config.save_state_slot_4,
        SaveStateSlot5 => &mut mapping_config.save_state_slot_5,
        SaveStateSlot6 => &mut mapping_config.save_state_slot_6,
        SaveStateSlot7 => &mut mapping_config.save_state_slot_7,
        SaveStateSlot8 => &mut mapping_config.save_state_slot_8,
        SaveStateSlot9 => &mut mapping_config.save_state_slot_9,
        LoadStateSlot0 => &mut mapping_config.load_state_slot_0,
        LoadStateSlot1 => &mut mapping_config.load_state_slot_1,
        LoadStateSlot2 => &mut mapping_config.load_state_slot_2,
        LoadStateSlot3 => &mut mapping_config.load_state_slot_3,
        LoadStateSlot4 => &mut mapping_config.load_state_slot_4,
        LoadStateSlot5 => &mut mapping_config.load_state_slot_5,
        LoadStateSlot6 => &mut mapping_config.load_state_slot_6,
        LoadStateSlot7 => &mut mapping_config.load_state_slot_7,
        LoadStateSlot8 => &mut mapping_config.load_state_slot_8,
        LoadStateSlot9 => &mut mapping_config.load_state_slot_9,
    }
}

impl App {
    pub(super) fn render_general_input_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("General Input Settings").open(&mut open).show(ctx, |ui| {
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
        Window::new("SMS/GG Input Settings").open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(OpenWindow::SmsGgInput, ui);
            ui.separator();

            Grid::new("smsgg_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                ui.heading("Player 1");
                ui.heading("Player 2");
                ui.end_row();

                self.render_input_buttons("smsgg_p1_input_settings", mapping, &P1_BUTTONS, ui);
                self.render_input_buttons("smsgg_p2_input_settings", mapping, &P2_BUTTONS, ui);
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
                            mapping_config.pause = SmsGgControllerMapping::keyboard_pause();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping_config.p1 = SmsGgControllerMapping::keyboard_wasd();
                            mapping_config.pause = SmsGgControllerMapping::keyboard_pause();
                        }
                    },
                );

                if ui.button("Clear All P1").clicked() {
                    mapping_config.p1 = SmsGgControllerMapping::default();
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config.p2 = SmsGgControllerMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgInput);
        }
    }

    pub(super) fn render_genesis_input_settings(&mut self, ctx: &Context) {
        static P1_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            GenesisButton::ALL
                .into_iter()
                .map(|button| GenericButton::Genesis(button, Player::One))
                .collect()
        });
        static P2_BUTTONS: LazyLock<Vec<GenericButton>> = LazyLock::new(|| {
            GenesisButton::ALL
                .into_iter()
                .map(|button| GenericButton::Genesis(button, Player::Two))
                .collect()
        });

        let mut open = true;
        Window::new("Genesis Input Settings").open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(OpenWindow::GenesisInput, ui);
            ui.separator();

            Grid::new("genesis_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                ui.heading("Player 1");
                ui.heading("Player 2");
                ui.end_row();

                self.render_input_buttons("genesis_p1_input_settings", mapping, &P1_BUTTONS, ui);
                self.render_input_buttons("genesis_p2_input_settings", mapping, &P2_BUTTONS, ui);
                ui.end_row();
            });

            ui.add_space(15.0);

            let mapping_config = mapping.genesis(&mut self.config.input);
            ui.horizontal(|ui| {
                ComboBox::new("genesis_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            mapping_config.p1 = GenesisControllerMapping::keyboard_arrows();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping_config.p1 = GenesisControllerMapping::keyboard_wasd();
                        }
                    },
                );

                if ui.button("Clear All P1").clicked() {
                    mapping_config.p1 = GenesisControllerMapping::default();
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config.p2 = GenesisControllerMapping::default();
                }
            });

            ui.separator();

            for player in [Player::One, Player::Two] {
                ui.group(|ui| {
                    let label = match player {
                        Player::One => "Player 1 controller type",
                        Player::Two => "Player 2 controller type",
                    };
                    ui.label(label);

                    let controller_type_field = match player {
                        Player::One => &mut self.config.input.genesis.p1_type,
                        Player::Two => &mut self.config.input.genesis.p2_type,
                    };

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            controller_type_field,
                            GenesisControllerType::ThreeButton,
                            "3-button",
                        );
                        ui.radio_value(
                            controller_type_field,
                            GenesisControllerType::SixButton,
                            "6-button",
                        );
                        ui.radio_value(controller_type_field, GenesisControllerType::None, "None");
                    });
                });
            }
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
        Window::new("NES Input Settings").open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(OpenWindow::NesInput, ui);
            ui.separator();

            Grid::new("nes_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                ui.heading("Player 1");
                ui.heading("Player 2");
                ui.end_row();

                self.render_input_buttons("nes_p1_inputs", mapping, &P1_BUTTONS, ui);
                self.render_input_buttons("nes_p2_inputs", mapping, &P2_BUTTONS, ui);
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
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping_config.p1 = NesControllerMapping::keyboard_wasd();
                        }
                    },
                );

                if ui.button("Clear All P1").clicked() {
                    mapping_config.p1 = NesControllerMapping::default();
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config.p2 = NesControllerMapping::default();
                }
            });
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
        Window::new("NES Peripheral Settings").open(&mut open).show(ctx, |ui| {
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
        Window::new("SNES Input Settings").open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(OpenWindow::SnesInput, ui);
            ui.separator();

            Grid::new("snes_inputs").spacing([50.0, 5.0]).show(ui, |ui| {
                ui.heading("Player 1");
                ui.heading("Player 2");
                ui.end_row();

                self.render_input_buttons("snes_p1_inputs", mapping, &P1_BUTTONS, ui);
                self.render_input_buttons("snes_p2_inputs", mapping, &P2_BUTTONS, ui);
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
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            mapping_config.p1 = SnesControllerMapping::keyboard_wasd();
                        }
                    },
                );

                if ui.button("Clear All P1").clicked() {
                    mapping_config.p1 = SnesControllerMapping::default();
                }

                if ui.button("Clear All P2").clicked() {
                    mapping_config.p2 = SnesControllerMapping::default();
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
        Window::new("SNES Peripheral Settings").open(&mut open).show(ctx, |ui| {
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
        Window::new("Game Boy Input Settings").open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(OpenWindow::GameBoyInput, ui);
            ui.separator();

            self.render_input_buttons("gb_inputs", mapping, &BUTTONS, ui);

            ui.add_space(15.0);

            let mapping_config = mapping.gb(&mut self.config.input);
            ui.horizontal(|ui| {
                ComboBox::new("gb_presets", "").selected_text("Apply preset...").show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(false, "Keyboard - Arrow movement").clicked() {
                            *mapping_config = GameBoyInputMapping::keyboard_arrows();
                        }

                        if ui.selectable_label(false, "Keyboard - WASD movement").clicked() {
                            *mapping_config = GameBoyInputMapping::keyboard_wasd();
                        }
                    },
                );

                if ui.button("Clear All").clicked() {
                    *mapping_config = GameBoyInputMapping::default();
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GameBoyInput);
        }
    }

    pub(super) fn render_hotkey_settings(&mut self, ctx: &Context) {
        static GENERAL_HOTKEYS: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| hotkey_vec(HotkeyCategory::General));
        static STATE_HOTKEYS: LazyLock<Vec<GenericButton>> =
            LazyLock::new(|| hotkey_vec(HotkeyCategory::SaveState));

        let mut open = true;
        Window::new("Hotkey Settings").open(&mut open).show(ctx, |ui| {
            self.disable_if_waiting_for_input(ui);

            let mapping = self.render_mapping_set_selector(OpenWindow::Hotkeys, ui);
            ui.separator();

            ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(ctx.screen_rect().height() * 0.5)
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
                ui.label(button.label());

                let current_value = button.access_value(mapping, &mut self.config.input);
                let current_value_str = format_input_str(current_value.as_ref());
                if ui.button(current_value_str).clicked() {
                    self.emu_thread.send(EmuThreadCommand::CollectInput {
                        axis_deadzone: self.config.input.axis_deadzone,
                    });
                    self.state.waiting_for_input = Some((*button, mapping));
                }

                if ui.button("Clear").clicked() {
                    *button.access_value(mapping, &mut self.config.input) = None;
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
