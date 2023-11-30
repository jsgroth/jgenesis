use crate::app::{App, OpenWindow};
use crate::emuthread::{EmuThreadCommand, GenericInput, InputType};
use egui::{Color32, Context, Grid, TextEdit, Ui, Widget, Window};
use genesis_core::GenesisControllerType;
use jgenesis_native_driver::config::input::{
    GenesisControllerConfig, GenesisInputConfig, HotkeyConfig, JoystickInput, KeyboardInput,
    SmsGgControllerConfig, SmsGgInputConfig, SnesControllerConfig, SnesInputConfig,
};
use jgenesis_native_driver::input::{GenesisButton, Hotkey, Player, SmsGgButton, SnesButton};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericButton {
    SmsGg(SmsGgButton),
    Genesis(GenesisButton),
    Snes(SnesButton),
    Hotkey(Hotkey),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAppConfig {
    #[serde(default = "default_smsgg_p1_keyboard_config")]
    pub smsgg_p1_keyboard: SmsGgControllerConfig<String>,
    #[serde(default)]
    pub smsgg_p2_keyboard: SmsGgControllerConfig<String>,
    #[serde(default)]
    pub smsgg_p1_joystick: SmsGgControllerConfig<JoystickInput>,
    #[serde(default)]
    pub smsgg_p2_joystick: SmsGgControllerConfig<JoystickInput>,
    #[serde(default)]
    pub genesis_p1_type: GenesisControllerType,
    #[serde(default)]
    pub genesis_p2_type: GenesisControllerType,
    #[serde(default = "default_genesis_p1_keyboard_config")]
    pub genesis_p1_keyboard: GenesisControllerConfig<String>,
    #[serde(default)]
    pub genesis_p2_keyboard: GenesisControllerConfig<String>,
    #[serde(default)]
    pub genesis_p1_joystick: GenesisControllerConfig<JoystickInput>,
    #[serde(default)]
    pub genesis_p2_joystick: GenesisControllerConfig<JoystickInput>,
    #[serde(default = "default_snes_p1_keyboard_config")]
    pub snes_p1_keyboard: SnesControllerConfig<String>,
    #[serde(default)]
    pub snes_p2_keyboard: SnesControllerConfig<String>,
    #[serde(default)]
    pub snes_p1_joystick: SnesControllerConfig<JoystickInput>,
    #[serde(default)]
    pub snes_p2_joystick: SnesControllerConfig<JoystickInput>,
    #[serde(default = "default_axis_deadzone")]
    pub axis_deadzone: i16,
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
}

macro_rules! set_input {
    ($input:expr, $keyboard_field:expr, $joystick_field:expr) => {
        match $input {
            GenericInput::Keyboard(KeyboardInput { keycode }) => {
                $keyboard_field = Some(keycode);
            }
            GenericInput::Joystick(input) => {
                $joystick_field = Some(input);
            }
        }
    };
}

impl InputAppConfig {
    pub fn set_input(&mut self, input: GenericInput, button: GenericButton) {
        match button {
            GenericButton::SmsGg(smsgg_button) => {
                self.set_smsgg_button(input, smsgg_button);
            }
            GenericButton::Genesis(genesis_button) => {
                self.set_genesis_button(input, genesis_button);
            }
            GenericButton::Snes(snes_button) => {
                self.set_snes_button(input, snes_button);
            }
            GenericButton::Hotkey(hotkey) => {
                if let GenericInput::Keyboard(input) = input {
                    self.set_hotkey(input, hotkey);
                }
            }
        }
    }

    fn set_smsgg_button(&mut self, input: GenericInput, smsgg_button: SmsGgButton) {
        match smsgg_button {
            SmsGgButton::Up(Player::One) => {
                set_input!(input, self.smsgg_p1_keyboard.up, self.smsgg_p1_joystick.up);
            }
            SmsGgButton::Left(Player::One) => {
                set_input!(input, self.smsgg_p1_keyboard.left, self.smsgg_p1_joystick.left);
            }
            SmsGgButton::Right(Player::One) => {
                set_input!(input, self.smsgg_p1_keyboard.right, self.smsgg_p1_joystick.right);
            }
            SmsGgButton::Down(Player::One) => {
                set_input!(input, self.smsgg_p1_keyboard.down, self.smsgg_p1_joystick.down);
            }
            SmsGgButton::Button1(Player::One) => {
                set_input!(input, self.smsgg_p1_keyboard.button_1, self.smsgg_p1_joystick.button_1);
            }
            SmsGgButton::Button2(Player::One) => {
                set_input!(input, self.smsgg_p1_keyboard.button_2, self.smsgg_p1_joystick.button_2);
            }
            SmsGgButton::Up(Player::Two) => {
                set_input!(input, self.smsgg_p2_keyboard.up, self.smsgg_p2_joystick.up);
            }
            SmsGgButton::Left(Player::Two) => {
                set_input!(input, self.smsgg_p2_keyboard.left, self.smsgg_p2_joystick.left);
            }
            SmsGgButton::Right(Player::Two) => {
                set_input!(input, self.smsgg_p2_keyboard.right, self.smsgg_p2_joystick.right);
            }
            SmsGgButton::Down(Player::Two) => {
                set_input!(input, self.smsgg_p2_keyboard.down, self.smsgg_p2_joystick.down);
            }
            SmsGgButton::Button1(Player::Two) => {
                set_input!(input, self.smsgg_p2_keyboard.button_1, self.smsgg_p2_joystick.button_1);
            }
            SmsGgButton::Button2(Player::Two) => {
                set_input!(input, self.smsgg_p2_keyboard.button_2, self.smsgg_p2_joystick.button_2);
            }
            SmsGgButton::Pause => {
                set_input!(input, self.smsgg_p1_keyboard.pause, self.smsgg_p1_joystick.pause);
            }
        }
    }

    fn set_genesis_button(&mut self, input: GenericInput, genesis_button: GenesisButton) {
        match genesis_button {
            GenesisButton::Up(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.up, self.genesis_p1_joystick.up);
            }
            GenesisButton::Left(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.left, self.genesis_p1_joystick.left);
            }
            GenesisButton::Right(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.right, self.genesis_p1_joystick.right);
            }
            GenesisButton::Down(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.down, self.genesis_p1_joystick.down);
            }
            GenesisButton::A(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.a, self.genesis_p1_joystick.a);
            }
            GenesisButton::B(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.b, self.genesis_p1_joystick.b);
            }
            GenesisButton::C(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.c, self.genesis_p1_joystick.c);
            }
            GenesisButton::X(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.x, self.genesis_p1_joystick.x);
            }
            GenesisButton::Y(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.y, self.genesis_p1_joystick.y);
            }
            GenesisButton::Z(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.z, self.genesis_p1_joystick.z);
            }
            GenesisButton::Start(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.start, self.genesis_p1_joystick.start);
            }
            GenesisButton::Mode(Player::One) => {
                set_input!(input, self.genesis_p1_keyboard.mode, self.genesis_p1_joystick.mode);
            }
            GenesisButton::Up(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.up, self.genesis_p2_joystick.up);
            }
            GenesisButton::Left(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.left, self.genesis_p2_joystick.left);
            }
            GenesisButton::Right(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.right, self.genesis_p2_joystick.right);
            }
            GenesisButton::Down(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.down, self.genesis_p2_joystick.down);
            }
            GenesisButton::A(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.a, self.genesis_p2_joystick.a);
            }
            GenesisButton::B(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.b, self.genesis_p2_joystick.b);
            }
            GenesisButton::C(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.c, self.genesis_p2_joystick.c);
            }
            GenesisButton::X(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.x, self.genesis_p2_joystick.x);
            }
            GenesisButton::Y(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.y, self.genesis_p2_joystick.y);
            }
            GenesisButton::Z(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.z, self.genesis_p2_joystick.z);
            }
            GenesisButton::Start(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.start, self.genesis_p2_joystick.start);
            }
            GenesisButton::Mode(Player::Two) => {
                set_input!(input, self.genesis_p2_keyboard.mode, self.genesis_p2_joystick.mode);
            }
        }
    }

    fn set_snes_button(&mut self, input: GenericInput, snes_button: SnesButton) {
        let (keyboard, joystick) = match snes_button.player() {
            Player::One => (&mut self.snes_p1_keyboard, &mut self.snes_p1_joystick),
            Player::Two => (&mut self.snes_p2_keyboard, &mut self.snes_p2_joystick),
        };

        match snes_button {
            SnesButton::Up(_) => {
                set_input!(input, keyboard.up, joystick.up);
            }
            SnesButton::Left(_) => {
                set_input!(input, keyboard.left, joystick.left);
            }
            SnesButton::Right(_) => {
                set_input!(input, keyboard.right, joystick.right);
            }
            SnesButton::Down(_) => {
                set_input!(input, keyboard.down, joystick.down);
            }
            SnesButton::A(_) => {
                set_input!(input, keyboard.a, joystick.a);
            }
            SnesButton::B(_) => {
                set_input!(input, keyboard.b, joystick.b);
            }
            SnesButton::X(_) => {
                set_input!(input, keyboard.x, joystick.x);
            }
            SnesButton::Y(_) => {
                set_input!(input, keyboard.y, joystick.y);
            }
            SnesButton::L(_) => {
                set_input!(input, keyboard.l, joystick.l);
            }
            SnesButton::R(_) => {
                set_input!(input, keyboard.r, joystick.r);
            }
            SnesButton::Start(_) => {
                set_input!(input, keyboard.start, joystick.start);
            }
            SnesButton::Select(_) => {
                set_input!(input, keyboard.select, joystick.select);
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

    pub fn to_smsgg_keyboard_config(&self) -> SmsGgInputConfig<KeyboardInput> {
        SmsGgInputConfig {
            p1: convert_smsgg_keyboard_config(self.smsgg_p1_keyboard.clone()),
            p2: convert_smsgg_keyboard_config(self.smsgg_p2_keyboard.clone()),
        }
    }

    pub fn to_smsgg_joystick_config(&self) -> SmsGgInputConfig<JoystickInput> {
        SmsGgInputConfig { p1: self.smsgg_p1_joystick.clone(), p2: self.smsgg_p2_joystick.clone() }
    }

    pub fn to_genesis_keyboard_config(&self) -> GenesisInputConfig<KeyboardInput> {
        GenesisInputConfig {
            p1: convert_genesis_keyboard_config(self.genesis_p1_keyboard.clone()),
            p2: convert_genesis_keyboard_config(self.genesis_p2_keyboard.clone()),
        }
    }

    pub fn to_genesis_joystick_config(&self) -> GenesisInputConfig<JoystickInput> {
        GenesisInputConfig {
            p1: self.genesis_p1_joystick.clone(),
            p2: self.genesis_p2_joystick.clone(),
        }
    }

    pub fn to_snes_keyboard_config(&self) -> SnesInputConfig<KeyboardInput> {
        SnesInputConfig {
            p1: convert_snes_keyboard_config(self.snes_p1_keyboard.clone()),
            p2: convert_snes_keyboard_config(self.snes_p2_keyboard.clone()),
        }
    }

    pub fn to_snes_joystick_config(&self) -> SnesInputConfig<JoystickInput> {
        SnesInputConfig { p1: self.snes_p1_joystick.clone(), p2: self.snes_p2_joystick.clone() }
    }
}

macro_rules! to_keyboard_input_config {
    ($config:expr, $t:ident, [$($field:ident),* $(,)?]) => {
        $t {
            $(
                $field: $config.$field.map(to_keyboard_input),
            )*
        }
    }
}

fn convert_smsgg_keyboard_config(
    config: SmsGgControllerConfig<String>,
) -> SmsGgControllerConfig<KeyboardInput> {
    to_keyboard_input_config!(
        config,
        SmsGgControllerConfig,
        [up, left, right, down, button_1, button_2, pause]
    )
}

fn convert_genesis_keyboard_config(
    config: GenesisControllerConfig<String>,
) -> GenesisControllerConfig<KeyboardInput> {
    to_keyboard_input_config!(
        config,
        GenesisControllerConfig,
        [up, left, right, down, a, b, c, x, y, z, start, mode]
    )
}

fn convert_snes_keyboard_config(
    config: SnesControllerConfig<String>,
) -> SnesControllerConfig<KeyboardInput> {
    to_keyboard_input_config!(
        config,
        SnesControllerConfig,
        [up, left, right, down, a, b, x, y, l, r, start, select]
    )
}

fn to_keyboard_input(s: String) -> KeyboardInput {
    KeyboardInput { keycode: s }
}

impl Default for InputAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn default_smsgg_p1_keyboard_config() -> SmsGgControllerConfig<String> {
    let default = SmsGgInputConfig::<KeyboardInput>::default().p1;
    SmsGgControllerConfig {
        up: default.up.map(|key| key.keycode),
        left: default.left.map(|key| key.keycode),
        right: default.right.map(|key| key.keycode),
        down: default.down.map(|key| key.keycode),
        button_1: default.button_1.map(|key| key.keycode),
        button_2: default.button_2.map(|key| key.keycode),
        pause: default.pause.map(|key| key.keycode),
    }
}

fn default_genesis_p1_keyboard_config() -> GenesisControllerConfig<String> {
    let default = GenesisInputConfig::<KeyboardInput>::default().p1;
    GenesisControllerConfig {
        up: default.up.map(|key| key.keycode),
        left: default.left.map(|key| key.keycode),
        right: default.right.map(|key| key.keycode),
        down: default.down.map(|key| key.keycode),
        a: default.a.map(|key| key.keycode),
        b: default.b.map(|key| key.keycode),
        c: default.c.map(|key| key.keycode),
        x: default.x.map(|key| key.keycode),
        y: default.y.map(|key| key.keycode),
        z: default.z.map(|key| key.keycode),
        start: default.start.map(|key| key.keycode),
        mode: default.mode.map(|key| key.keycode),
    }
}

fn default_snes_p1_keyboard_config() -> SnesControllerConfig<String> {
    let default = SnesInputConfig::<KeyboardInput>::default().p1;
    let keycode_fn = |key: KeyboardInput| key.keycode;
    SnesControllerConfig {
        up: default.up.map(keycode_fn),
        left: default.left.map(keycode_fn),
        right: default.right.map(keycode_fn),
        down: default.down.map(keycode_fn),
        a: default.a.map(keycode_fn),
        b: default.b.map(keycode_fn),
        x: default.x.map(keycode_fn),
        y: default.y.map(keycode_fn),
        l: default.l.map(keycode_fn),
        r: default.r.map(keycode_fn),
        start: default.start.map(keycode_fn),
        select: default.select.map(keycode_fn),
    }
}

fn default_axis_deadzone() -> i16 {
    8000
}

macro_rules! render_buttons {
    ($self:expr, $button_fn:ident, $config:expr, [$($field:ident: $label:literal -> $button:expr),*$(,)?], $ui:expr) => {
        $(
            $self.$button_fn($config.$field.clone(), $label, $button, $ui);
        )*
    }
}

macro_rules! render_smsgg_input {
    ($self:expr, $button_fn:ident, $config:expr, $player:expr, $ui:expr) => {
        render_buttons!($self, $button_fn, $config, [
            up: "Up" -> GenericButton::SmsGg(SmsGgButton::Up($player)),
            left: "Left" -> GenericButton::SmsGg(SmsGgButton::Left($player)),
            right: "Right" -> GenericButton::SmsGg(SmsGgButton::Right($player)),
            down: "Down" -> GenericButton::SmsGg(SmsGgButton::Down($player)),
            button_1: "Button 1" -> GenericButton::SmsGg(SmsGgButton::Button1($player)),
            button_2: "Button 2" -> GenericButton::SmsGg(SmsGgButton::Button2($player)),
        ], $ui);
    }
}

macro_rules! render_genesis_input {
    ($self:expr, $button_fn:ident, $config:expr, $player:expr, $ui:expr) => {
        render_buttons!($self, $button_fn, $config, [
            up: "Up" -> GenericButton::Genesis(GenesisButton::Up($player)),
            left: "Left" -> GenericButton::Genesis(GenesisButton::Left($player)),
            right: "Right" -> GenericButton::Genesis(GenesisButton::Right($player)),
            down: "Down" -> GenericButton::Genesis(GenesisButton::Down($player)),
            a: "A" -> GenericButton::Genesis(GenesisButton::A($player)),
            b: "B" -> GenericButton::Genesis(GenesisButton::B($player)),
            c: "C" -> GenericButton::Genesis(GenesisButton::C($player)),
            x: "X" -> GenericButton::Genesis(GenesisButton::X($player)),
            y: "Y" -> GenericButton::Genesis(GenesisButton::Y($player)),
            z: "Z" -> GenericButton::Genesis(GenesisButton::Z($player)),
            start: "Start" -> GenericButton::Genesis(GenesisButton::Start($player)),
            mode: "Mode" -> GenericButton::Genesis(GenesisButton::Mode($player)),
        ], $ui);
    }
}

macro_rules! render_snes_input {
    ($self:expr, $button_fn:ident, $config:expr, $player:expr, $ui:expr) => {
        render_buttons!($self, $button_fn, $config, [
            up: "Up" -> GenericButton::Snes(SnesButton::Up($player)),
            left: "Left" -> GenericButton::Snes(SnesButton::Left($player)),
            right: "Right" -> GenericButton::Snes(SnesButton::Right($player)),
            down: "Down" -> GenericButton::Snes(SnesButton::Down($player)),
            a: "A" -> GenericButton::Snes(SnesButton::A($player)),
            b: "B" -> GenericButton::Snes(SnesButton::B($player)),
            x: "X" -> GenericButton::Snes(SnesButton::X($player)),
            y: "Y" -> GenericButton::Snes(SnesButton::Y($player)),
            l: "L" -> GenericButton::Snes(SnesButton::L($player)),
            r: "R" -> GenericButton::Snes(SnesButton::R($player)),
            start: "Start" -> GenericButton::Snes(SnesButton::Start($player)),
            select: "Select" -> GenericButton::Snes(SnesButton::Select($player)),
        ], $ui);
    }
}

impl App {
    pub(super) fn render_smsgg_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("smsgg_keyboard_grid").show(ui, |ui| {
                Grid::new("smsgg_p1_keyboard_grid").show(ui, |ui| {
                    ui.heading("Player 1");
                    ui.end_row();

                    render_smsgg_input!(
                        self,
                        keyboard_input_button,
                        self.config.inputs.smsgg_p1_keyboard,
                        Player::One,
                        ui
                    );
                });

                ui.add_space(20.0);

                Grid::new("smsgg_p2_keyboard_grid").show(ui, |ui| {
                    ui.heading("Player 2");
                    ui.end_row();

                    render_smsgg_input!(
                        self,
                        keyboard_input_button,
                        self.config.inputs.smsgg_p2_keyboard,
                        Player::Two,
                        ui
                    );
                });
            });

            ui.add_space(20.0);

            Grid::new("smsgg_pause_keyboard_grid").show(ui, |ui| {
                self.keyboard_input_button(
                    self.config.inputs.smsgg_p1_keyboard.pause.clone(),
                    "Start/Pause",
                    GenericButton::SmsGg(SmsGgButton::Pause),
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
                Grid::new("smsgg_p1_gamepad_grid").show(ui, |ui| {
                    ui.heading("Player 1");
                    ui.end_row();

                    render_smsgg_input!(
                        self,
                        gamepad_input_button,
                        self.config.inputs.smsgg_p1_joystick,
                        Player::One,
                        ui
                    );
                });

                ui.add_space(20.0);

                Grid::new("smsgg_p2_gamepad_grid").show(ui, |ui| {
                    ui.heading("Player 2");
                    ui.end_row();

                    render_smsgg_input!(
                        self,
                        gamepad_input_button,
                        self.config.inputs.smsgg_p2_joystick,
                        Player::Two,
                        ui
                    );
                });
            });

            ui.add_space(20.0);

            Grid::new("smsgg_pause_gamepad_grid").show(ui, |ui| {
                self.gamepad_input_button(
                    self.config.inputs.smsgg_p1_joystick.pause.clone(),
                    "Start/Pause",
                    GenericButton::SmsGg(SmsGgButton::Pause),
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
                Grid::new("genesis_p1_keyboard_grid").show(ui, |ui| {
                    ui.heading("Player 1");
                    ui.end_row();

                    render_genesis_input!(
                        self,
                        keyboard_input_button,
                        self.config.inputs.genesis_p1_keyboard,
                        Player::One,
                        ui
                    );
                });

                ui.add_space(50.0);

                Grid::new("genesis_p2_keyboard_grid").show(ui, |ui| {
                    ui.heading("Player 2");
                    ui.end_row();

                    render_genesis_input!(
                        self,
                        keyboard_input_button,
                        self.config.inputs.genesis_p2_keyboard,
                        Player::Two,
                        ui
                    );
                });
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
                Grid::new("genesis_p1_gamepad_grid").show(ui, |ui| {
                    ui.heading("Player 1");
                    ui.end_row();

                    render_genesis_input!(
                        self,
                        gamepad_input_button,
                        self.config.inputs.genesis_p1_joystick,
                        Player::One,
                        ui
                    );
                });

                ui.add_space(50.0);

                Grid::new("genesis_p2_gamepad_grid").show(ui, |ui| {
                    ui.heading("Player 2");
                    ui.end_row();

                    render_genesis_input!(
                        self,
                        gamepad_input_button,
                        self.config.inputs.genesis_p2_joystick,
                        Player::Two,
                        ui
                    );
                });
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

    pub(super) fn render_snes_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SNES Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            Grid::new("snes_keyboard_grid").show(ui, |ui| {
                Grid::new("snes_p1_keyboard_grid").show(ui, |ui| {
                    ui.heading("Player 1");
                    ui.end_row();

                    render_snes_input!(
                        self,
                        keyboard_input_button,
                        self.config.inputs.snes_p1_keyboard,
                        Player::One,
                        ui
                    );
                });

                ui.add_space(50.0);

                Grid::new("snes_p2_keyboard_grid").show(ui, |ui| {
                    ui.heading("Player 2");
                    ui.end_row();

                    render_snes_input!(
                        self,
                        keyboard_input_button,
                        self.config.inputs.snes_p2_keyboard,
                        Player::Two,
                        ui
                    );
                });
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
                Grid::new("snes_p1_gamepad_grid").show(ui, |ui| {
                    ui.heading("Player 1");
                    ui.end_row();

                    render_snes_input!(
                        self,
                        gamepad_input_button,
                        self.config.inputs.snes_p1_joystick,
                        Player::One,
                        ui
                    );
                });

                ui.add_space(50.0);

                Grid::new("snes_p2_gamepad_grid").show(ui, |ui| {
                    ui.heading("Player 2");
                    ui.end_row();

                    render_snes_input!(
                        self,
                        gamepad_input_button,
                        self.config.inputs.snes_p2_joystick,
                        Player::Two,
                        ui
                    );
                });
            });

            ui.add_space(30.0);

            self.render_axis_deadzone_input(ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesGamepad);
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
                if TextEdit::singleline(&mut self.state.ff_multiplier_text)
                    .desired_width(30.0)
                    .ui(ui)
                    .changed()
                {
                    match self.state.ff_multiplier_text.parse::<u64>() {
                        Ok(ff_multiplier) if ff_multiplier != 0 => {
                            self.config.common.fast_forward_multiplier = ff_multiplier;
                            self.state.ff_multiplier_invalid = false;
                        }
                        _ => {
                            self.state.ff_multiplier_invalid = true;
                        }
                    }
                }

                ui.label("Fast forward multiplier");
            });
            if self.state.ff_multiplier_invalid {
                ui.colored_label(
                    Color32::RED,
                    "Fast forward multiplier must be a positive integer",
                );
            }

            ui.horizontal(|ui| {
                if TextEdit::singleline(&mut self.state.rewind_buffer_len_text)
                    .desired_width(30.0)
                    .ui(ui)
                    .changed()
                {
                    match self.state.rewind_buffer_len_text.parse::<u64>() {
                        Ok(rewind_buffer_len) => {
                            self.config.common.rewind_buffer_length_seconds = rewind_buffer_len;
                            self.state.rewind_buffer_len_invalid = false;
                        }
                        Err(_) => {
                            self.state.rewind_buffer_len_invalid = true;
                        }
                    }
                }

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
            if TextEdit::singleline(&mut self.state.axis_deadzone_text)
                .desired_width(50.0)
                .ui(ui)
                .changed()
            {
                match self.state.axis_deadzone_text.parse::<i16>() {
                    Ok(value) if (0..=i16::MAX).contains(&value) => {
                        self.config.inputs.axis_deadzone = value;
                        self.state.axis_deadzone_invalid = false;
                    }
                    _ => {
                        self.state.axis_deadzone_invalid = true;
                    }
                }
            }

            ui.label("Joystick axis deadzone (0-32767)");
        });
        if self.state.axis_deadzone_invalid {
            ui.colored_label(Color32::RED, "Axis dead zone must be an integer between 0 and 32767");
        }
    }

    fn keyboard_input_button(
        &mut self,
        current_value: Option<String>,
        label: &str,
        button: GenericButton,
        ui: &mut Ui,
    ) {
        let text = current_value.unwrap_or("<None>".into());
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
            GenericButton::SmsGg(button) => match (input_type, button.player()) {
                (InputType::Keyboard, Player::One) => {
                    clear_smsgg_button(&mut self.config.inputs.smsgg_p1_keyboard, button);
                }
                (InputType::Joystick, Player::One) => {
                    clear_smsgg_button(&mut self.config.inputs.smsgg_p1_joystick, button);
                }
                (InputType::Keyboard, Player::Two) => {
                    clear_smsgg_button(&mut self.config.inputs.smsgg_p2_keyboard, button);
                }
                (InputType::Joystick, Player::Two) => {
                    clear_smsgg_button(&mut self.config.inputs.smsgg_p2_joystick, button);
                }
            },
            GenericButton::Genesis(button) => match (input_type, button.player()) {
                (InputType::Keyboard, Player::One) => {
                    clear_genesis_button(&mut self.config.inputs.genesis_p1_keyboard, button);
                }
                (InputType::Joystick, Player::One) => {
                    clear_genesis_button(&mut self.config.inputs.genesis_p1_joystick, button);
                }
                (InputType::Keyboard, Player::Two) => {
                    clear_genesis_button(&mut self.config.inputs.genesis_p2_keyboard, button);
                }
                (InputType::Joystick, Player::Two) => {
                    clear_genesis_button(&mut self.config.inputs.genesis_p2_joystick, button);
                }
            },
            GenericButton::Snes(button) => match (input_type, button.player()) {
                (InputType::Keyboard, Player::One) => {
                    clear_snes_button(&mut self.config.inputs.snes_p1_keyboard, button);
                }
                (InputType::Joystick, Player::One) => {
                    clear_snes_button(&mut self.config.inputs.snes_p1_joystick, button);
                }
                (InputType::Keyboard, Player::Two) => {
                    clear_snes_button(&mut self.config.inputs.snes_p2_keyboard, button);
                }
                (InputType::Joystick, Player::Two) => {
                    clear_snes_button(&mut self.config.inputs.snes_p2_joystick, button);
                }
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
            });
            self.state.waiting_for_input = Some(GenericButton::Hotkey(hotkey));
        }

        if ui.button("Clear").clicked() {
            self.clear_button_in_config(GenericButton::Hotkey(hotkey), InputType::Keyboard);
        }

        ui.end_row();
    }
}

fn clear_smsgg_button<T>(config: &mut SmsGgControllerConfig<T>, button: SmsGgButton) {
    let field = match button {
        SmsGgButton::Up(_) => &mut config.up,
        SmsGgButton::Left(_) => &mut config.left,
        SmsGgButton::Right(_) => &mut config.right,
        SmsGgButton::Down(_) => &mut config.down,
        SmsGgButton::Button1(_) => &mut config.button_1,
        SmsGgButton::Button2(_) => &mut config.button_2,
        SmsGgButton::Pause => &mut config.pause,
    };

    *field = None;
}

fn clear_genesis_button<T>(config: &mut GenesisControllerConfig<T>, button: GenesisButton) {
    let field = match button {
        GenesisButton::Up(_) => &mut config.up,
        GenesisButton::Left(_) => &mut config.left,
        GenesisButton::Right(_) => &mut config.right,
        GenesisButton::Down(_) => &mut config.down,
        GenesisButton::A(_) => &mut config.a,
        GenesisButton::B(_) => &mut config.b,
        GenesisButton::C(_) => &mut config.c,
        GenesisButton::X(_) => &mut config.x,
        GenesisButton::Y(_) => &mut config.y,
        GenesisButton::Z(_) => &mut config.z,
        GenesisButton::Start(_) => &mut config.start,
        GenesisButton::Mode(_) => &mut config.mode,
    };

    *field = None;
}

fn clear_snes_button<T>(config: &mut SnesControllerConfig<T>, button: SnesButton) {
    let field = match button {
        SnesButton::Up(_) => &mut config.up,
        SnesButton::Left(_) => &mut config.left,
        SnesButton::Right(_) => &mut config.right,
        SnesButton::Down(_) => &mut config.down,
        SnesButton::A(_) => &mut config.a,
        SnesButton::B(_) => &mut config.b,
        SnesButton::X(_) => &mut config.x,
        SnesButton::Y(_) => &mut config.y,
        SnesButton::L(_) => &mut config.l,
        SnesButton::R(_) => &mut config.r,
        SnesButton::Start(_) => &mut config.start,
        SnesButton::Select(_) => &mut config.select,
    };

    *field = None;
}
