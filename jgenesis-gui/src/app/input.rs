use crate::app::{App, OpenWindow};
use crate::emuthread::{EmuThreadCommand, GenericInput, InputType};
use egui::{Context, Ui, Window};
use jgenesis_native_driver::config::input::{
    GenesisControllerConfig, GenesisInputConfig, JoystickInput, KeyboardInput,
    SmsGgControllerConfig, SmsGgInputConfig,
};
use jgenesis_native_driver::input::{GenesisButton, Player, SmsGgButton};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericButton {
    SmsGg(SmsGgButton),
    Genesis(GenesisButton),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAppConfig {
    #[serde(default = "default_smsgg_p1_keyboard_config")]
    smsgg_p1_keyboard: SmsGgControllerConfig<String>,
    #[serde(default)]
    smsgg_p2_keyboard: SmsGgControllerConfig<String>,
    #[serde(default)]
    smsgg_p1_joystick: SmsGgControllerConfig<JoystickInput>,
    #[serde(default)]
    smsgg_p2_joystick: SmsGgControllerConfig<JoystickInput>,
    #[serde(default = "default_genesis_p1_keyboard_config")]
    genesis_p1_keyboard: GenesisControllerConfig<String>,
    #[serde(default)]
    genesis_p2_keyboard: GenesisControllerConfig<String>,
    #[serde(default)]
    genesis_p1_joystick: GenesisControllerConfig<JoystickInput>,
    #[serde(default)]
    genesis_p2_joystick: GenesisControllerConfig<JoystickInput>,
    #[serde(default = "default_axis_deadzone")]
    axis_deadzone: i16,
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
            GenericButton::SmsGg(smsgg_button) => match smsgg_button {
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
                    set_input!(
                        input,
                        self.smsgg_p1_keyboard.button_1,
                        self.smsgg_p1_joystick.button_1
                    );
                }
                SmsGgButton::Button2(Player::One) => {
                    set_input!(
                        input,
                        self.smsgg_p1_keyboard.button_2,
                        self.smsgg_p1_joystick.button_2
                    );
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
                    set_input!(
                        input,
                        self.smsgg_p2_keyboard.button_1,
                        self.smsgg_p2_joystick.button_1
                    );
                }
                SmsGgButton::Button2(Player::Two) => {
                    set_input!(
                        input,
                        self.smsgg_p2_keyboard.button_2,
                        self.smsgg_p2_joystick.button_2
                    );
                }
                SmsGgButton::Pause => {
                    set_input!(input, self.smsgg_p1_keyboard.pause, self.smsgg_p1_joystick.pause);
                }
            },
            GenericButton::Genesis(genesis_button) => match genesis_button {
                GenesisButton::Up(Player::One) => {
                    set_input!(input, self.genesis_p1_keyboard.up, self.genesis_p1_joystick.up);
                }
                GenesisButton::Left(Player::One) => {
                    set_input!(input, self.genesis_p1_keyboard.left, self.genesis_p1_joystick.left);
                }
                GenesisButton::Right(Player::One) => {
                    set_input!(
                        input,
                        self.genesis_p1_keyboard.right,
                        self.genesis_p1_joystick.right
                    );
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
                GenesisButton::Start(Player::One) => {
                    set_input!(
                        input,
                        self.genesis_p1_keyboard.start,
                        self.genesis_p1_joystick.start
                    );
                }
                GenesisButton::Up(Player::Two) => {
                    set_input!(input, self.genesis_p2_keyboard.up, self.genesis_p2_joystick.up);
                }
                GenesisButton::Left(Player::Two) => {
                    set_input!(input, self.genesis_p2_keyboard.left, self.genesis_p2_joystick.left);
                }
                GenesisButton::Right(Player::Two) => {
                    set_input!(
                        input,
                        self.genesis_p2_keyboard.right,
                        self.genesis_p2_joystick.right
                    );
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
                GenesisButton::Start(Player::Two) => {
                    set_input!(
                        input,
                        self.genesis_p2_keyboard.start,
                        self.genesis_p2_joystick.start
                    );
                }
            },
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
}

macro_rules! to_keyboard_input_config {
    ($config:expr, $t:ident, [$($field:ident),*$(,)?]) => {
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
        [up, left, right, down, a, b, c, start]
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
        start: default.start.map(|key| key.keycode),
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
            start: "Start" -> GenericButton::Genesis(GenesisButton::Start($player)),
        ], $ui);
    }
}

impl App {
    pub(super) fn render_smsgg_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            self.keyboard_input_button(
                self.config.inputs.smsgg_p1_keyboard.pause.clone(),
                "Start/Pause",
                GenericButton::SmsGg(SmsGgButton::Pause),
                ui,
            );

            ui.add_space(25.0);

            ui.heading("Player 1");
            render_smsgg_input!(
                self,
                keyboard_input_button,
                self.config.inputs.smsgg_p1_keyboard,
                Player::One,
                ui
            );

            ui.add_space(25.0);

            ui.heading("Player 2");
            render_smsgg_input!(
                self,
                keyboard_input_button,
                self.config.inputs.smsgg_p2_keyboard,
                Player::Two,
                ui
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgKeyboard);
        }
    }

    pub(super) fn render_smsgg_gamepad_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SMS/GG Gamepad Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            self.gamepad_input_button(
                self.config.inputs.smsgg_p1_joystick.pause.clone(),
                "Start/Pause",
                GenericButton::SmsGg(SmsGgButton::Pause),
                ui,
            );

            ui.add_space(25.0);

            ui.heading("Player 1");
            render_smsgg_input!(
                self,
                gamepad_input_button,
                self.config.inputs.smsgg_p1_joystick,
                Player::One,
                ui
            );

            ui.add_space(25.0);

            ui.heading("Player 2");
            render_smsgg_input!(
                self,
                gamepad_input_button,
                self.config.inputs.smsgg_p2_joystick,
                Player::Two,
                ui
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SmsGgGamepad);
        }
    }

    pub(super) fn render_genesis_keyboard_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Keyboard Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            ui.heading("Player 1");
            render_genesis_input!(
                self,
                keyboard_input_button,
                self.config.inputs.genesis_p1_keyboard,
                Player::One,
                ui
            );

            ui.add_space(25.0);

            ui.heading("Player 2");
            render_genesis_input!(
                self,
                keyboard_input_button,
                self.config.inputs.genesis_p2_keyboard,
                Player::Two,
                ui
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisKeyboard);
        }
    }

    pub(super) fn render_genesis_gamepad_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Gamepad Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_enabled(self.state.waiting_for_input.is_none());

            ui.heading("Player 1");
            render_genesis_input!(
                self,
                gamepad_input_button,
                self.config.inputs.genesis_p1_joystick,
                Player::One,
                ui
            );

            ui.add_space(25.0);

            ui.heading("Player 2");
            render_genesis_input!(
                self,
                gamepad_input_button,
                self.config.inputs.genesis_p2_joystick,
                Player::Two,
                ui
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisGamepad);
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
        ui.horizontal(|ui| {
            if ui.button(text).clicked() {
                log::debug!("Sending collect input command for button {button:?}");
                self.emu_thread.send(EmuThreadCommand::CollectInput {
                    input_type,
                    axis_deadzone: self.config.inputs.axis_deadzone,
                });
                if self.emu_thread.status().is_running() {
                    log::debug!("Setting read signal");
                    self.emu_thread.set_command_read_signal();
                }

                self.state.waiting_for_input = Some(button);
            }
            ui.label(label);
        });
    }
}
