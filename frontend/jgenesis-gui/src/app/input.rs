use crate::app::{App, NumericTextEdit, OpenWindow};
use crate::emuthread::{EmuThreadCommand, GenericInput, InputType};
use egui::{Color32, Context, Grid, Ui, Window};
use gb_core::inputs::GameBoyButton;
use genesis_core::input::GenesisButton;
use genesis_core::GenesisControllerType;
use jgenesis_common::input::Player;
use jgenesis_native_driver::config::input::{
    GameBoyInputConfig, GenesisInputConfig, HotkeyConfig, InputConfig, JoystickInput,
    KeyboardInput, KeyboardOrMouseInput, NesInputConfig, SmsGgInputConfig, SnesControllerType,
    SnesInputConfig, SuperScopeConfig,
};
use jgenesis_native_driver::input::Hotkey;
use nes_core::input::NesButton;
use serde::{Deserialize, Serialize};
use smsgg_core::SmsGgButton;
use snes_core::input::{SnesButton, SnesControllerButton, SuperScopeButton};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericButton {
    SmsGg(SmsGgButton, Player),
    Genesis(GenesisButton, Player),
    Nes(NesButton, Player),
    Snes(SnesButton, Player),
    GameBoy(GameBoyButton),
    Hotkey(Hotkey),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAppConfig {
    #[serde(default)]
    pub smsgg_keyboard: SmsGgInputConfig<KeyboardInput>,
    #[serde(default)]
    pub smsgg_joystick: SmsGgInputConfig<JoystickInput>,
    #[serde(default)]
    pub genesis_p1_type: GenesisControllerType,
    #[serde(default)]
    pub genesis_p2_type: GenesisControllerType,
    #[serde(default)]
    pub genesis_keyboard: GenesisInputConfig<KeyboardInput>,
    #[serde(default)]
    pub genesis_joystick: GenesisInputConfig<JoystickInput>,
    #[serde(default)]
    pub nes_keyboard: NesInputConfig<KeyboardInput>,
    #[serde(default)]
    pub nes_joystick: NesInputConfig<JoystickInput>,
    #[serde(default)]
    pub snes_keyboard: SnesInputConfig<KeyboardInput>,
    #[serde(default)]
    pub snes_joystick: SnesInputConfig<JoystickInput>,
    #[serde(default)]
    pub snes_p2_type: SnesControllerType,
    #[serde(default)]
    pub snes_super_scope: SuperScopeConfig,
    #[serde(default = "default_gb_keyboard_config")]
    pub gb_keyboard: GameBoyInputConfig<KeyboardInput>,
    #[serde(default)]
    pub gb_joystick: GameBoyInputConfig<JoystickInput>,
    #[serde(default = "default_axis_deadzone")]
    pub axis_deadzone: i16,
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
}

fn set_input<Button, KC, JC>(
    input: GenericInput,
    button: Button,
    player: Player,
    keyboard: &mut KC,
    joystick: &mut JC,
) where
    KC: InputConfig<Button = Button, Input = KeyboardInput>,
    JC: InputConfig<Button = Button, Input = JoystickInput>,
{
    match input {
        GenericInput::Keyboard(input) => {
            keyboard.set_input(button, player, input);
        }
        GenericInput::Joystick(input) => {
            joystick.set_input(button, player, input);
        }
        GenericInput::KeyboardOrMouse(_) => {
            log::error!("keyboard/mouse input set from unexpected code path");
        }
    }
}

impl InputAppConfig {
    pub fn set_input(&mut self, input: GenericInput, button: GenericButton) {
        match button {
            GenericButton::SmsGg(button, player) => {
                set_input(
                    input,
                    button,
                    player,
                    &mut self.smsgg_keyboard,
                    &mut self.smsgg_joystick,
                );
            }
            GenericButton::Genesis(button, player) => {
                set_input(
                    input,
                    button,
                    player,
                    &mut self.genesis_keyboard,
                    &mut self.genesis_joystick,
                );
            }
            GenericButton::Nes(button, player) => {
                set_input(input, button, player, &mut self.nes_keyboard, &mut self.nes_joystick);
            }
            GenericButton::Snes(button, player) => {
                match button {
                    SnesButton::Controller(button) => set_input(
                        input,
                        button,
                        player,
                        &mut self.snes_keyboard,
                        &mut self.snes_joystick,
                    ),
                    SnesButton::SuperScope(button) => {
                        if let GenericInput::KeyboardOrMouse(input) = input {
                            self.snes_super_scope.set_button(button, input);
                        }
                    }
                };
            }
            GenericButton::GameBoy(button) => {
                set_input(input, button, Player::One, &mut self.gb_keyboard, &mut self.gb_joystick);
            }
            GenericButton::Hotkey(hotkey) => {
                if let GenericInput::Keyboard(input) = input {
                    self.set_hotkey(input, hotkey);
                }
            }
        }
    }

    fn set_hotkey(&mut self, input: KeyboardInput, hotkey: Hotkey) {
        match hotkey {
            Hotkey::Quit => {
                self.hotkeys.quit = Some(input);
            }
            Hotkey::ToggleFullscreen => {
                self.hotkeys.toggle_fullscreen = Some(input);
            }
            Hotkey::SaveState => {
                self.hotkeys.save_state = Some(input);
            }
            Hotkey::LoadState => {
                self.hotkeys.load_state = Some(input);
            }
            Hotkey::SoftReset => {
                self.hotkeys.soft_reset = Some(input);
            }
            Hotkey::HardReset => {
                self.hotkeys.hard_reset = Some(input);
            }
            Hotkey::Pause => {
                self.hotkeys.pause = Some(input);
            }
            Hotkey::StepFrame => {
                self.hotkeys.step_frame = Some(input);
            }
            Hotkey::FastForward => {
                self.hotkeys.fast_forward = Some(input);
            }
            Hotkey::Rewind => {
                self.hotkeys.rewind = Some(input);
            }
            Hotkey::OpenDebugger => {
                self.hotkeys.open_debugger = Some(input);
            }
        }
    }
}

impl Default for InputAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn default_gb_keyboard_config() -> GameBoyInputConfig<KeyboardInput> {
    GameBoyInputConfig::default_p1()
}

fn default_axis_deadzone() -> i16 {
    8000
}

impl App {
    pub(super) fn render_smsgg_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("smsgg_keyboard_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("smsgg_p1_keyboard_grid", "Player 1", Player::One),
                    ("smsgg_p2_keyboard_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in SmsGgButton::ALL {
                            if button == SmsGgButton::Pause {
                                continue;
                            }

                            let current_value = self
                                .config
                                .inputs
                                .smsgg_keyboard
                                .get_input(button, player)
                                .cloned();
                            self.keyboard_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::SmsGg(button, player),
                                ui,
                            );
                        }
                    });

                    ui.add_space(20.0);
                }
            });

            ui.add_space(20.0);

            Grid::new("smsgg_pause_keyboard_grid").show(ui, |ui| {
                self.keyboard_input_button(
                    self.config.inputs.smsgg_keyboard.pause.clone(),
                    "Start/Pause",
                    GenericButton::SmsGg(SmsGgButton::Pause, Player::One),
                    ui,
                );
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgKeyboard);
        }
    }

    pub(super) fn render_smsgg_gamepad_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Gamepad Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("smsgg_gamepad_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("smsgg_p1_gamepad_grid", "Player 1", Player::One),
                    ("smsgg_p2_gamepad_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in SmsGgButton::ALL {
                            if button == SmsGgButton::Pause {
                                continue;
                            }

                            let current_value = self
                                .config
                                .inputs
                                .smsgg_joystick
                                .get_input(button, player)
                                .cloned();
                            self.gamepad_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::SmsGg(button, player),
                                ui,
                            );
                        }
                    });

                    ui.add_space(20.0);
                }
            });

            ui.add_space(20.0);

            Grid::new("smsgg_pause_gamepad_grid").show(ui, |ui| {
                self.gamepad_input_button(
                    self.config.inputs.smsgg_joystick.pause.clone(),
                    "Start/Pause",
                    GenericButton::SmsGg(SmsGgButton::Pause, Player::One),
                    ui,
                );
            });

            ui.add_space(20.0);
            self.render_axis_deadzone_input(ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgGamepad);
        }
    }

    pub(super) fn render_genesis_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("genesis_keyboard_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("genesis_p1_keyboard_grid", "Player 1", Player::One),
                    ("genesis_p2_keyboard_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in GenesisButton::ALL {
                            let current_value = self
                                .config
                                .inputs
                                .genesis_keyboard
                                .get_input(button, player)
                                .cloned();
                            self.keyboard_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::Genesis(button, player),
                                ui,
                            );
                        }
                    });

                    ui.add_space(50.0);
                }
            });

            ui.add_space(30.0);

            self.controller_type_input("Player 1 controller", Player::One, ui);
            self.controller_type_input("Player 2 controller", Player::Two, ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisKeyboard);
        }
    }

    pub(super) fn render_genesis_gamepad_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Gamepad Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("genesis_gamepad_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("genesis_p1_gamepad_grid", "Player 1", Player::One),
                    ("genesis_p2_gamepad_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in GenesisButton::ALL {
                            let current_value = self
                                .config
                                .inputs
                                .genesis_joystick
                                .get_input(button, player)
                                .cloned();
                            self.gamepad_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::Genesis(button, player),
                                ui,
                            );
                        }
                    });

                    ui.add_space(50.0);
                }
            });

            ui.add_space(30.0);

            self.render_axis_deadzone_input(ui);

            ui.add_space(20.0);

            self.controller_type_input("Player 1 controller", Player::One, ui);
            self.controller_type_input("Player 2 controller", Player::Two, ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisGamepad);
        }
    }

    pub(super) fn render_nes_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("NES Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("nes_keyboard_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("nes_p1_keyboard_grid", "Player 1", Player::One),
                    ("nes_p2_keyboard_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in NesButton::ALL {
                            let current_value =
                                self.config.inputs.nes_keyboard.get_input(button, player).cloned();
                            self.keyboard_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::Nes(button, player),
                                ui,
                            );
                        }

                        ui.add_space(50.0);
                    });
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::NesKeyboard);
        }
    }

    pub(super) fn render_nes_joystick_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("NES Gamepad Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("nes_gamepad_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("nes_p1_gamepad_grid", "Player 1", Player::One),
                    ("nes_p2_gamepad_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in NesButton::ALL {
                            let current_value =
                                self.config.inputs.nes_joystick.get_input(button, player).cloned();
                            self.gamepad_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::Nes(button, player),
                                ui,
                            );
                        }

                        ui.add_space(50.0);
                    });
                }
            });

            ui.add_space(30.0);

            self.render_axis_deadzone_input(ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::NesGamepad);
        }
    }

    pub(super) fn render_snes_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SNES Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("snes_keyboard_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("snes_p1_keyboard_grid", "Player 1", Player::One),
                    ("snes_p2_keyboard_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in SnesControllerButton::ALL {
                            let current_value =
                                self.config.inputs.snes_keyboard.get_input(button, player).cloned();
                            self.keyboard_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::Snes(SnesButton::Controller(button), player),
                                ui,
                            );
                        }
                    });

                    ui.add_space(50.0);
                }
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesKeyboard);
        }
    }

    pub(super) fn render_snes_gamepad_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SNES Gamepad Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("snes_gamepad_grid").show(ui, |ui| {
                for (grid_id, heading, player) in [
                    ("snes_p1_gamepad_grid", "Player 1", Player::One),
                    ("snes_p2_gamepad_grid", "Player 2", Player::Two),
                ] {
                    Grid::new(grid_id).show(ui, |ui| {
                        ui.heading(heading);
                        ui.end_row();

                        for button in SnesControllerButton::ALL {
                            let current_value =
                                self.config.inputs.snes_joystick.get_input(button, player).cloned();
                            self.gamepad_input_button(
                                current_value,
                                &button.to_string(),
                                GenericButton::Snes(SnesButton::Controller(button), player),
                                ui,
                            );
                        }
                    });

                    ui.add_space(50.0);
                }
            });

            ui.add_space(30.0);

            self.render_axis_deadzone_input(ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesGamepad);
        }
    }

    pub(super) fn render_snes_peripheral_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SNES Peripheral Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            ui.group(|ui| {
                ui.label("P2 input device");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.inputs.snes_p2_type,
                        SnesControllerType::Gamepad,
                        "Gamepad",
                    );
                    ui.radio_value(
                        &mut self.config.inputs.snes_p2_type,
                        SnesControllerType::SuperScope,
                        "Super Scope",
                    );
                });
            });

            ui.add_space(10.0);

            ui.heading("Super Scope");

            Grid::new("super_scope_grid").show(ui, |ui| {
                self.super_scope_button(
                    self.config.inputs.snes_super_scope.fire.clone(),
                    "Fire",
                    SuperScopeButton::Fire,
                    ui,
                );
                self.super_scope_button(
                    self.config.inputs.snes_super_scope.cursor.clone(),
                    "Cursor",
                    SuperScopeButton::Cursor,
                    ui,
                );
                self.super_scope_button(
                    self.config.inputs.snes_super_scope.pause.clone(),
                    "Pause",
                    SuperScopeButton::Pause,
                    ui,
                );
                self.super_scope_button(
                    self.config.inputs.snes_super_scope.turbo_toggle.clone(),
                    "Turbo (Toggle)",
                    SuperScopeButton::TurboToggle,
                    ui,
                );
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesPeripherals);
        }
    }

    pub(super) fn render_gb_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Game Boy Keyboard Settings").open(&mut open).resizable(false).show(
            ctx,
            |ui| {
                ui.set_enabled(self.state.waiting_for_input.is_none());

                Grid::new("gb_keyboard_grid").show(ui, |ui| {
                    for button in GameBoyButton::ALL {
                        let current_value =
                            self.config.inputs.gb_keyboard.get_input(button, Player::One).cloned();
                        self.keyboard_input_button(
                            current_value,
                            &button.to_string(),
                            GenericButton::GameBoy(button),
                            ui,
                        );
                    }
                });
            },
        );
        if !open {
            self.state.open_windows.remove(&OpenWindow::GameBoyKeyboard);
        }
    }

    pub(super) fn render_gb_joystick_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Game Boy Joystick Settings").open(&mut open).resizable(false).show(
            ctx,
            |ui| {
                ui.set_enabled(self.state.waiting_for_input.is_none());

                Grid::new("gb_joystick_grid").show(ui, |ui| {
                    for button in GameBoyButton::ALL {
                        let current_value =
                            self.config.inputs.gb_joystick.get_input(button, Player::One).cloned();
                        self.gamepad_input_button(
                            current_value,
                            &button.to_string(),
                            GenericButton::GameBoy(button),
                            ui,
                        );
                    }
                });

                ui.add_space(30.0);

                self.render_axis_deadzone_input(ui);
            },
        );
        if !open {
            self.state.open_windows.remove(&OpenWindow::GameBoyGamepad);
        }
    }

    pub(super) fn render_hotkey_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Hotkey Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("hotkeys_grid").show(ui, |ui| {
                self.hotkey_button(
                    self.config.inputs.hotkeys.quit.clone(),
                    "Quit",
                    Hotkey::Quit,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.toggle_fullscreen.clone(),
                    "Toggle fullscreen",
                    Hotkey::ToggleFullscreen,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.save_state.clone(),
                    "Save state",
                    Hotkey::SaveState,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.load_state.clone(),
                    "Load state",
                    Hotkey::LoadState,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.soft_reset.clone(),
                    "Soft reset",
                    Hotkey::SoftReset,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.hard_reset.clone(),
                    "Hard reset",
                    Hotkey::HardReset,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.pause.clone(),
                    "Pause/Unpause",
                    Hotkey::Pause,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.step_frame.clone(),
                    "Step frame while paused",
                    Hotkey::StepFrame,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.fast_forward.clone(),
                    "Fast forward",
                    Hotkey::FastForward,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.rewind.clone(),
                    "Rewind",
                    Hotkey::Rewind,
                    ui,
                );
                self.hotkey_button(
                    self.config.inputs.hotkeys.open_debugger.clone(),
                    "Open memory viewer",
                    Hotkey::OpenDebugger,
                    ui,
                );
            });

            ui.add_space(20.0);

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
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::Hotkeys);
        }
    }

    fn render_axis_deadzone_input(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(
                NumericTextEdit::new(
                    &mut self.state.axis_deadzone_text,
                    &mut self.config.inputs.axis_deadzone,
                    &mut self.state.axis_deadzone_invalid,
                )
                .with_validation(|value| value >= 0)
                .desired_width(50.0),
            );

            ui.label("Joystick axis deadzone (0-32767)");
        });
        if self.state.axis_deadzone_invalid {
            ui.colored_label(Color32::RED, "Axis dead zone must be an integer between 0 and 32767");
        }
    }

    fn keyboard_input_button(
        &mut self,
        current_value: Option<KeyboardInput>,
        label: &str,
        button: GenericButton,
        ui: &mut Ui,
    ) {
        let text = current_value.map_or_else(|| "<None>".into(), |input| input.keycode);
        self.input_button(&text, label, InputType::Keyboard, button, ui);
    }

    fn gamepad_input_button(
        &mut self,
        current_value: Option<JoystickInput>,
        label: &str,
        button: GenericButton,
        ui: &mut Ui,
    ) {
        let text = current_value.map_or_else(
            || "<None>".into(),
            |input| format!("{} ({})", input.action, input.device),
        );
        self.input_button(&text, label, InputType::Joystick, button, ui);
    }

    fn input_button(
        &mut self,
        text: &str,
        label: &str,
        input_type: InputType,
        button: GenericButton,
        ui: &mut Ui,
    ) {
        ui.label(format!("{label}:"));

        if ui.button(text).clicked() {
            log::debug!("Sending collect input command for button {button:?}");
            self.emu_thread.send(EmuThreadCommand::CollectInput {
                input_type,
                axis_deadzone: self.config.inputs.axis_deadzone,
                ctx: ui.ctx().clone(),
            });
            self.state.waiting_for_input = Some(button);
        }

        if ui.button("Clear").clicked() {
            log::debug!("Clearing button {button:?} for input_type {input_type:?}");
            self.clear_button_in_config(button, input_type);
        }

        ui.end_row();
    }

    fn clear_button_in_config(&mut self, button: GenericButton, input_type: InputType) {
        match button {
            GenericButton::SmsGg(button, player) => match input_type {
                InputType::Keyboard => {
                    self.config.inputs.smsgg_keyboard.clear_input(button, player);
                }
                InputType::Joystick => {
                    self.config.inputs.smsgg_joystick.clear_input(button, player);
                }
                InputType::KeyboardOrMouse => {}
            },
            GenericButton::Genesis(button, player) => match input_type {
                InputType::Keyboard => {
                    self.config.inputs.genesis_keyboard.clear_input(button, player);
                }
                InputType::Joystick => {
                    self.config.inputs.genesis_joystick.clear_input(button, player);
                }
                InputType::KeyboardOrMouse => {}
            },
            GenericButton::Nes(button, player) => match input_type {
                InputType::Keyboard => self.config.inputs.nes_keyboard.clear_input(button, player),
                InputType::Joystick => self.config.inputs.nes_joystick.clear_input(button, player),
                InputType::KeyboardOrMouse => {}
            },
            GenericButton::Snes(button, player) => match (input_type, button) {
                (InputType::Keyboard, SnesButton::Controller(button)) => {
                    self.config.inputs.snes_keyboard.clear_input(button, player);
                }
                (InputType::Joystick, SnesButton::Controller(button)) => {
                    self.config.inputs.snes_joystick.clear_input(button, player);
                }
                (InputType::KeyboardOrMouse, SnesButton::SuperScope(button)) => {
                    self.config.inputs.snes_super_scope.clear_button(button);
                }
                _ => {}
            },
            GenericButton::GameBoy(button) => match input_type {
                InputType::Keyboard => self.config.inputs.gb_keyboard.clear_button(button),
                InputType::Joystick => self.config.inputs.gb_joystick.clear_button(button),
                InputType::KeyboardOrMouse => {}
            },
            GenericButton::Hotkey(hotkey) => match hotkey {
                Hotkey::Quit => {
                    self.config.inputs.hotkeys.quit = None;
                }
                Hotkey::ToggleFullscreen => {
                    self.config.inputs.hotkeys.toggle_fullscreen = None;
                }
                Hotkey::SaveState => {
                    self.config.inputs.hotkeys.save_state = None;
                }
                Hotkey::LoadState => {
                    self.config.inputs.hotkeys.load_state = None;
                }
                Hotkey::SoftReset => {
                    self.config.inputs.hotkeys.soft_reset = None;
                }
                Hotkey::HardReset => {
                    self.config.inputs.hotkeys.hard_reset = None;
                }
                Hotkey::Pause => {
                    self.config.inputs.hotkeys.pause = None;
                }
                Hotkey::StepFrame => {
                    self.config.inputs.hotkeys.step_frame = None;
                }
                Hotkey::FastForward => {
                    self.config.inputs.hotkeys.fast_forward = None;
                }
                Hotkey::Rewind => {
                    self.config.inputs.hotkeys.rewind = None;
                }
                Hotkey::OpenDebugger => {
                    self.config.inputs.hotkeys.open_debugger = None;
                }
            },
        }
    }

    fn controller_type_input(&mut self, label: &str, player: Player, ui: &mut Ui) {
        ui.group(|ui| {
            ui.label(label);

            let controller_type_field = match player {
                Player::One => &mut self.config.inputs.genesis_p1_type,
                Player::Two => &mut self.config.inputs.genesis_p2_type,
            };

            ui.horizontal(|ui| {
                ui.radio_value(
                    controller_type_field,
                    GenesisControllerType::ThreeButton,
                    "3-button",
                );
                ui.radio_value(controller_type_field, GenesisControllerType::SixButton, "6-button");
            });
        });
    }

    fn hotkey_button(
        &mut self,
        current_value: Option<KeyboardInput>,
        label: &str,
        hotkey: Hotkey,
        ui: &mut Ui,
    ) {
        ui.label(format!("{label}:"));

        let text = match current_value {
            Some(value) => value.keycode,
            None => "<None>".into(),
        };
        if ui.button(text).clicked() {
            log::debug!("Sending collect input command for hotkey {hotkey:?}");
            self.emu_thread.send(EmuThreadCommand::CollectInput {
                input_type: InputType::Keyboard,
                axis_deadzone: self.config.inputs.axis_deadzone,
                ctx: ui.ctx().clone(),
            });
            self.state.waiting_for_input = Some(GenericButton::Hotkey(hotkey));
        }

        if ui.button("Clear").clicked() {
            self.clear_button_in_config(GenericButton::Hotkey(hotkey), InputType::Keyboard);
        }

        ui.end_row();
    }

    fn super_scope_button(
        &mut self,
        current_value: Option<KeyboardOrMouseInput>,
        label: &str,
        button: SuperScopeButton,
        ui: &mut Ui,
    ) {
        ui.label(format!("{label}:"));

        let text = match current_value {
            Some(value) => value.to_string(),
            None => "<None>".into(),
        };
        if ui.button(text).clicked() {
            log::debug!("Sending collect input request for Super Scope button {button:?}");
            self.emu_thread.send(EmuThreadCommand::CollectInput {
                input_type: InputType::KeyboardOrMouse,
                axis_deadzone: self.config.inputs.axis_deadzone,
                ctx: ui.ctx().clone(),
            });
            self.state.waiting_for_input =
                Some(GenericButton::Snes(SnesButton::SuperScope(button), Player::One));
        }

        if ui.button("Clear").clicked() {
            self.clear_button_in_config(
                GenericButton::Snes(SnesButton::SuperScope(button), Player::One),
                InputType::KeyboardOrMouse,
            );
        }

        ui.end_row();
    }
}
