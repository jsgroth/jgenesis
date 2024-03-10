use crate::config::input::{
    AxisDirection, GameBoyInputConfig, GenesisInputConfig, HatDirection, HotkeyConfig,
    JoystickAction, JoystickDeviceId, JoystickInput, KeyboardInput, KeyboardOrMouseInput,
    NesInputConfig, SmsGgInputConfig, SnesControllerType, SnesInputConfig, SuperScopeConfig,
};
use crate::mainloop::{NativeEmulatorError, NativeEmulatorResult};
use gb_core::inputs::GameBoyInputs;
use genesis_core::GenesisInputs;
use jgenesis_common::frontend::FrameSize;
use jgenesis_renderer::renderer::DisplayArea;
use nes_core::input::NesInputs;
use sdl2::event::{Event, WindowEvent};
use sdl2::joystick::{HatState, Joystick};
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::JoystickSubsystem;
use smsgg_core::SmsGgInputs;
use snes_core::input::{SnesInputDevice, SnesInputs, SnesJoypadState, SuperScopeState};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Player {
    One,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmsGgButton {
    Up(Player),
    Left(Player),
    Right(Player),
    Down(Player),
    Button1(Player),
    Button2(Player),
    Pause,
}

impl SmsGgButton {
    #[must_use]
    pub fn player(self) -> Player {
        match self {
            Self::Up(player)
            | Self::Left(player)
            | Self::Right(player)
            | Self::Down(player)
            | Self::Button1(player)
            | Self::Button2(player) => player,
            Self::Pause => Player::One,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenesisButton {
    Up(Player),
    Left(Player),
    Right(Player),
    Down(Player),
    A(Player),
    B(Player),
    C(Player),
    X(Player),
    Y(Player),
    Z(Player),
    Start(Player),
    Mode(Player),
}

impl GenesisButton {
    #[must_use]
    pub fn player(self) -> Player {
        match self {
            Self::Up(player)
            | Self::Left(player)
            | Self::Right(player)
            | Self::Down(player)
            | Self::A(player)
            | Self::B(player)
            | Self::C(player)
            | Self::X(player)
            | Self::Y(player)
            | Self::Z(player)
            | Self::Start(player)
            | Self::Mode(player) => player,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NesButton {
    Up(Player),
    Left(Player),
    Right(Player),
    Down(Player),
    A(Player),
    B(Player),
    Start(Player),
    Select(Player),
}

impl NesButton {
    #[must_use]
    pub fn player(self) -> Player {
        match self {
            Self::Up(player)
            | Self::Left(player)
            | Self::Right(player)
            | Self::Down(player)
            | Self::A(player)
            | Self::B(player)
            | Self::Start(player)
            | Self::Select(player) => player,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperScopeButton {
    Fire,
    Cursor,
    Pause,
    TurboToggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnesButton {
    Up(Player),
    Left(Player),
    Right(Player),
    Down(Player),
    A(Player),
    B(Player),
    X(Player),
    Y(Player),
    L(Player),
    R(Player),
    Start(Player),
    Select(Player),
    SuperScope(SuperScopeButton),
}

impl SnesButton {
    #[must_use]
    pub fn player(self) -> Player {
        match self {
            Self::Up(player)
            | Self::Left(player)
            | Self::Right(player)
            | Self::Down(player)
            | Self::A(player)
            | Self::B(player)
            | Self::X(player)
            | Self::Y(player)
            | Self::L(player)
            | Self::R(player)
            | Self::Start(player)
            | Self::Select(player) => player,
            Self::SuperScope(_) => Player::One,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameBoyButton {
    Up,
    Left,
    Right,
    Down,
    A,
    B,
    Start,
    Select,
}

pub trait MappableInputs<Button> {
    fn set_field(&mut self, button: Button, value: bool);

    fn handle_mouse_motion(
        &mut self,
        x: i32,
        y: i32,
        frame_size: FrameSize,
        display_area: DisplayArea,
    );

    fn handle_mouse_leave(&mut self);
}

impl MappableInputs<SmsGgButton> for SmsGgInputs {
    fn set_field(&mut self, button: SmsGgButton, value: bool) {
        let joypad_state = match button.player() {
            Player::One => &mut self.p1,
            Player::Two => &mut self.p2,
        };

        match button {
            SmsGgButton::Up(..) => joypad_state.up = value,
            SmsGgButton::Left(..) => joypad_state.left = value,
            SmsGgButton::Right(..) => joypad_state.right = value,
            SmsGgButton::Down(..) => joypad_state.down = value,
            SmsGgButton::Button1(..) => joypad_state.button1 = value,
            SmsGgButton::Button2(..) => joypad_state.button2 = value,
            SmsGgButton::Pause => self.pause = value,
        }
    }

    fn handle_mouse_motion(
        &mut self,
        _x: i32,
        _y: i32,
        _frame_size: FrameSize,
        _display_area: DisplayArea,
    ) {
    }

    fn handle_mouse_leave(&mut self) {}
}

impl MappableInputs<GenesisButton> for GenesisInputs {
    fn set_field(&mut self, button: GenesisButton, value: bool) {
        let joypad_state = match button.player() {
            Player::One => &mut self.p1,
            Player::Two => &mut self.p2,
        };

        match button {
            GenesisButton::Up(..) => joypad_state.up = value,
            GenesisButton::Left(..) => joypad_state.left = value,
            GenesisButton::Right(..) => joypad_state.right = value,
            GenesisButton::Down(..) => joypad_state.down = value,
            GenesisButton::A(..) => joypad_state.a = value,
            GenesisButton::B(..) => joypad_state.b = value,
            GenesisButton::C(..) => joypad_state.c = value,
            GenesisButton::X(..) => joypad_state.x = value,
            GenesisButton::Y(..) => joypad_state.y = value,
            GenesisButton::Z(..) => joypad_state.z = value,
            GenesisButton::Start(..) => joypad_state.start = value,
            GenesisButton::Mode(..) => joypad_state.mode = value,
        }
    }

    fn handle_mouse_motion(
        &mut self,
        _x: i32,
        _y: i32,
        _frame_size: FrameSize,
        _display_area: DisplayArea,
    ) {
    }

    fn handle_mouse_leave(&mut self) {}
}

impl MappableInputs<NesButton> for NesInputs {
    fn set_field(&mut self, button: NesButton, value: bool) {
        let joypad_state = match button.player() {
            Player::One => &mut self.p1,
            Player::Two => &mut self.p2,
        };

        match button {
            NesButton::Up(_) => joypad_state.up = value,
            NesButton::Left(_) => joypad_state.left = value,
            NesButton::Right(_) => joypad_state.right = value,
            NesButton::Down(_) => joypad_state.down = value,
            NesButton::A(_) => joypad_state.a = value,
            NesButton::B(_) => joypad_state.b = value,
            NesButton::Start(_) => joypad_state.start = value,
            NesButton::Select(_) => joypad_state.select = value,
        }
    }

    fn handle_mouse_motion(
        &mut self,
        _x: i32,
        _y: i32,
        _frame_size: FrameSize,
        _display_area: DisplayArea,
    ) {
    }

    fn handle_mouse_leave(&mut self) {}
}

impl MappableInputs<SnesButton> for SnesInputs {
    fn set_field(&mut self, button: SnesButton, value: bool) {
        if let SnesButton::SuperScope(super_scope_button) = button {
            let SnesInputDevice::SuperScope(super_scope_state) = &mut self.p2 else { return };

            match super_scope_button {
                SuperScopeButton::Fire => super_scope_state.fire = value,
                SuperScopeButton::Cursor => super_scope_state.cursor = value,
                SuperScopeButton::Pause => super_scope_state.pause = value,
                SuperScopeButton::TurboToggle => {
                    if value {
                        super_scope_state.turbo = !super_scope_state.turbo;
                    }
                }
            }

            return;
        }

        let joypad_state = match button.player() {
            Player::One => &mut self.p1,
            Player::Two => match &mut self.p2 {
                SnesInputDevice::Controller(joypad_state) => joypad_state,
                SnesInputDevice::SuperScope(..) => return,
            },
        };

        match button {
            SnesButton::Up(..) => joypad_state.up = value,
            SnesButton::Left(..) => joypad_state.left = value,
            SnesButton::Right(..) => joypad_state.right = value,
            SnesButton::Down(..) => joypad_state.down = value,
            SnesButton::A(..) => joypad_state.a = value,
            SnesButton::B(..) => joypad_state.b = value,
            SnesButton::X(..) => joypad_state.x = value,
            SnesButton::Y(..) => joypad_state.y = value,
            SnesButton::L(..) => joypad_state.l = value,
            SnesButton::R(..) => joypad_state.r = value,
            SnesButton::Start(..) => joypad_state.start = value,
            SnesButton::Select(..) => joypad_state.select = value,
            SnesButton::SuperScope(..) => unreachable!("early return if button is Super Scope"),
        }
    }

    fn handle_mouse_motion(
        &mut self,
        x: i32,
        y: i32,
        frame_size: FrameSize,
        display_area: DisplayArea,
    ) {
        let SnesInputDevice::SuperScope(super_scope_state) = &mut self.p2 else { return };

        let display_left = display_area.x as i32;
        let display_right = display_left + display_area.width as i32;
        let display_top = display_area.y as i32;
        let display_bottom = display_top + display_area.height as i32;

        if !(display_left..display_right).contains(&x)
            || !(display_top..display_bottom).contains(&y)
        {
            super_scope_state.position = None;
            return;
        }

        let x: f64 = x.into();
        let y: f64 = y.into();
        let display_left: f64 = display_left.into();
        let display_width: f64 = display_area.width.into();
        let frame_width: f64 = frame_size.width.into();
        let display_top: f64 = display_top.into();
        let display_height: f64 = display_area.height.into();
        let frame_height: f64 = frame_size.height.into();

        let snes_x = ((x - display_left) * frame_width / display_width).round() as u16;
        let snes_y = ((y - display_top) * frame_height / display_height).round() as u16;
        super_scope_state.position = Some((snes_x, snes_y));
    }

    fn handle_mouse_leave(&mut self) {
        if let SnesInputDevice::SuperScope(super_scope_state) = &mut self.p2 {
            super_scope_state.position = None;
        }
    }
}

impl MappableInputs<GameBoyButton> for GameBoyInputs {
    fn set_field(&mut self, button: GameBoyButton, value: bool) {
        use GameBoyButton::*;

        match button {
            Up => self.up = value,
            Left => self.left = value,
            Right => self.right = value,
            Down => self.down = value,
            A => self.a = value,
            B => self.b = value,
            Start => self.start = value,
            Select => self.select = value,
        }
    }

    fn handle_mouse_motion(
        &mut self,
        _x: i32,
        _y: i32,
        _frame_size: FrameSize,
        _display_area: DisplayArea,
    ) {
    }

    fn handle_mouse_leave(&mut self) {}
}

#[derive(Default)]
pub struct Joysticks {
    joysticks: HashMap<u32, Joystick>,
    instance_id_to_device_id: HashMap<u32, u32>,
    name_to_device_ids: HashMap<String, Vec<u32>>,
}

impl Joysticks {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a joystick.
    ///
    /// # Errors
    ///
    /// Will return an error if SDL2 cannot open the given device.
    pub fn device_added(
        &mut self,
        device_id: u32,
        joystick_subsystem: &JoystickSubsystem,
    ) -> NativeEmulatorResult<()> {
        let joystick = joystick_subsystem
            .open(device_id)
            .map_err(|source| NativeEmulatorError::SdlJoystickOpen { device_id, source })?;
        let name = joystick.name();
        log::info!("Opened joystick id {device_id}: {name}");

        let instance_id = joystick.instance_id();
        self.joysticks.insert(device_id, joystick);
        self.instance_id_to_device_id.insert(instance_id, device_id);

        self.name_to_device_ids
            .entry(name)
            .and_modify(|device_ids| {
                device_ids.push(device_id);
                device_ids.sort();
            })
            .or_insert_with(|| vec![device_id]);

        Ok(())
    }

    pub fn device_removed(&mut self, instance_id: u32) {
        let Some(device_id) = self.instance_id_to_device_id.remove(&instance_id) else { return };

        if let Some(joystick) = self.joysticks.remove(&device_id) {
            log::info!("Disconnected joystick id {device_id}: {}", joystick.name());
        }

        for device_ids in self.name_to_device_ids.values_mut() {
            device_ids.retain(|&id| id != device_id);
        }
    }

    #[must_use]
    pub fn joystick(&self, device_id: u32) -> Option<&Joystick> {
        self.joysticks.get(&device_id)
    }

    #[must_use]
    pub fn get_joystick_id(&self, device_id: u32) -> Option<JoystickDeviceId> {
        let joystick = self.joysticks.get(&device_id)?;

        let name = joystick.name();
        let device_ids = self.name_to_device_ids.get(&name)?;
        let (device_idx, _) =
            device_ids.iter().copied().enumerate().find(|&(_, id)| id == device_id)?;
        Some(JoystickDeviceId::new(name, device_idx as u32))
    }

    #[must_use]
    pub fn device_id_for(&self, instance_id: u32) -> Option<u32> {
        self.instance_id_to_device_id.get(&instance_id).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeycodeOrMouseButton {
    Keycode(Keycode),
    Mouse(MouseButton),
}

impl TryFrom<KeyboardOrMouseInput> for KeycodeOrMouseButton {
    type Error = NativeEmulatorError;

    fn try_from(value: KeyboardOrMouseInput) -> Result<Self, Self::Error> {
        match value {
            KeyboardOrMouseInput::Keyboard(keycode) => {
                let keycode = Keycode::from_name(&keycode)
                    .ok_or_else(|| NativeEmulatorError::InvalidKeycode(keycode))?;
                Ok(Self::Keycode(keycode))
            }
            KeyboardOrMouseInput::MouseLeft => Ok(Self::Mouse(MouseButton::Left)),
            KeyboardOrMouseInput::MouseRight => Ok(Self::Mouse(MouseButton::Right)),
            KeyboardOrMouseInput::MouseMiddle => Ok(Self::Mouse(MouseButton::Middle)),
            KeyboardOrMouseInput::MouseX1 => Ok(Self::Mouse(MouseButton::X1)),
            KeyboardOrMouseInput::MouseX2 => Ok(Self::Mouse(MouseButton::X2)),
        }
    }
}

pub(crate) struct InputMapper<Inputs, Button> {
    inputs: Inputs,
    joystick_subsystem: JoystickSubsystem,
    joysticks: Joysticks,
    axis_deadzone: i16,
    keyboard_mapping: HashMap<Keycode, Vec<Button>>,
    raw_joystick_mapping: HashMap<JoystickInput, Vec<Button>>,
    joystick_mapping: HashMap<(u32, JoystickAction), Vec<Button>>,
    key_or_mouse_mapping: HashMap<KeycodeOrMouseButton, Vec<Button>>,
}

impl<Inputs, Button> InputMapper<Inputs, Button> {
    pub(crate) fn joysticks_mut(&mut self) -> (&mut Joysticks, &JoystickSubsystem) {
        (&mut self.joysticks, &self.joystick_subsystem)
    }
}

impl<Inputs, Button> InputMapper<Inputs, Button> {
    fn new(
        inputs: Inputs,
        joystick_subsystem: JoystickSubsystem,
        keyboard_mapping: HashMap<Keycode, Vec<Button>>,
        joystick_mapping: HashMap<JoystickInput, Vec<Button>>,
        key_or_mouse_mapping: HashMap<KeycodeOrMouseButton, Vec<Button>>,
        axis_deadzone: i16,
    ) -> Self {
        Self {
            inputs,
            joystick_subsystem,
            joysticks: Joysticks::new(),
            axis_deadzone,
            keyboard_mapping,
            raw_joystick_mapping: joystick_mapping,
            joystick_mapping: HashMap::new(),
            key_or_mouse_mapping,
        }
    }
}

macro_rules! inputs_array {
    ($p1_config:expr, $p2_config:expr, [$($field:ident -> $button:expr),* $(,)?] $(, extra: $extra:tt $(,)?)?) => {
        [
            $(
                ($p1_config.$field, $button(Player::One)),
                ($p2_config.$field, $button(Player::Two)),
            )*
            $(
                $extra
            )?
        ]
    }
}

macro_rules! flat_inputs_array {
    ($config:expr, [$($field:ident -> $button:expr),* $(,)?]) => {
        [
            $(
                ($config.$field, $button),
            )*
        ]
    };
}

macro_rules! smsgg_input_array {
    ($p1_config:expr, $p2_config:expr) => {
        inputs_array!(
            $p1_config,
            $p2_config,
            [
                up -> SmsGgButton::Up,
                left -> SmsGgButton::Left,
                right -> SmsGgButton::Right,
                down -> SmsGgButton::Down,
                button_1 -> SmsGgButton::Button1,
                button_2 -> SmsGgButton::Button2,
            ],
            extra: ($p1_config.pause, SmsGgButton::Pause),
        )
    }
}

macro_rules! genesis_input_array {
    ($p1_config:expr, $p2_config:expr) => {
        inputs_array!($p1_config, $p2_config, [
            up -> GenesisButton::Up,
            left -> GenesisButton::Left,
            right -> GenesisButton::Right,
            down -> GenesisButton::Down,
            a -> GenesisButton::A,
            b -> GenesisButton::B,
            c -> GenesisButton::C,
            x -> GenesisButton::X,
            y -> GenesisButton::Y,
            z -> GenesisButton::Z,
            start -> GenesisButton::Start,
            mode -> GenesisButton::Mode,
        ])
    }
}

macro_rules! nes_input_array {
    ($p1_config:expr, $p2_config:expr) => {
        inputs_array!($p1_config, $p2_config, [
            up -> NesButton::Up,
            left -> NesButton::Left,
            right -> NesButton::Right,
            down -> NesButton::Down,
            a -> NesButton::A,
            b -> NesButton::B,
            start -> NesButton::Start,
            select -> NesButton::Select,
        ])
    }
}

macro_rules! snes_input_array {
    ($p1_config:expr, $p2_config:expr) => {
        inputs_array!($p1_config, $p2_config, [
            up -> SnesButton::Up,
            left -> SnesButton::Left,
            right -> SnesButton::Right,
            down -> SnesButton::Down,
            a -> SnesButton::A,
            b -> SnesButton::B,
            x -> SnesButton::X,
            y -> SnesButton::Y,
            l -> SnesButton::L,
            r -> SnesButton::R,
            start -> SnesButton::Start,
            select -> SnesButton::Select,
        ])
    }
}

macro_rules! gb_input_array {
    ($config:expr) => {
        flat_inputs_array!($config, [
            up -> GameBoyButton::Up,
            left -> GameBoyButton::Left,
            right -> GameBoyButton::Right,
            down -> GameBoyButton::Down,
            a -> GameBoyButton::A,
            b -> GameBoyButton::B,
            start -> GameBoyButton::Start,
            select -> GameBoyButton::Select,
        ])
    }
}

macro_rules! impl_generate_keyboard_mapping {
    ($name:ident, $config_t:ident, $button_t:ty, |$config:ident| $inputs_arr:expr $(,)?) => {
        fn $name(
            $config: $config_t<KeyboardInput>,
        ) -> NativeEmulatorResult<HashMap<Keycode, Vec<$button_t>>> {
            let mut keyboard_mapping: HashMap<Keycode, Vec<$button_t>> = HashMap::new();
            for (input, button) in $inputs_arr {
                if let Some(KeyboardInput { keycode }) = input {
                    let keycode = Keycode::from_name(&keycode)
                        .ok_or_else(|| NativeEmulatorError::InvalidKeycode(keycode))?;
                    keyboard_mapping.entry(keycode).or_default().push(button);
                }
            }

            Ok(keyboard_mapping)
        }
    };
}

macro_rules! impl_generate_joystick_mapping {
    ($name:ident, $config_t:ident, $button_t:ty, |$config:ident| $inputs_arr:expr $(,)?) => {
        fn $name($config: $config_t<JoystickInput>) -> HashMap<JoystickInput, Vec<$button_t>> {
            let mut joystick_mapping: HashMap<JoystickInput, Vec<$button_t>> = HashMap::new();
            for (input, button) in $inputs_arr {
                if let Some(input) = input {
                    joystick_mapping.entry(input).or_default().push(button);
                }
            }

            joystick_mapping
        }
    };
}

macro_rules! impl_generate_mapping_fns {
    ($keyboard_name:ident, $joystick_name:ident, $config_t:ident, $button_t:ty, |$config:ident| $inputs_arr:expr $(,)?) => {
        impl_generate_keyboard_mapping!($keyboard_name, $config_t, $button_t, |$config| {
            $inputs_arr
        });
        impl_generate_joystick_mapping!($joystick_name, $config_t, $button_t, |$config| {
            $inputs_arr
        });
    };
}

impl_generate_mapping_fns!(
    generate_smsgg_keyboard_mapping,
    generate_smsgg_joystick_mapping,
    SmsGgInputConfig,
    SmsGgButton,
    |config| smsgg_input_array!(config.p1, config.p2)
);

impl_generate_mapping_fns!(
    generate_genesis_keyboard_mapping,
    generate_genesis_joystick_mapping,
    GenesisInputConfig,
    GenesisButton,
    |config| genesis_input_array!(config.p1, config.p2)
);

impl_generate_mapping_fns!(
    generate_nes_keyboard_mapping,
    generate_nes_joystick_mapping,
    NesInputConfig,
    NesButton,
    |config| nes_input_array!(config.p1, config.p2)
);

impl_generate_mapping_fns!(
    generate_snes_keyboard_mapping,
    generate_snes_joystick_mapping,
    SnesInputConfig,
    SnesButton,
    |config| snes_input_array!(config.p1, config.p2)
);

impl_generate_mapping_fns!(
    generate_gb_keyboard_mapping,
    generate_gb_joystick_mapping,
    GameBoyInputConfig,
    GameBoyButton,
    |config| gb_input_array!(config)
);

impl InputMapper<SmsGgInputs, SmsGgButton> {
    pub(crate) fn new_smsgg(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        joystick_inputs: SmsGgInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        Ok(Self::new_generic(
            joystick_subsystem,
            generate_smsgg_keyboard_mapping(keyboard_inputs)?,
            generate_smsgg_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        joystick_inputs: SmsGgInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<()> {
        self.reload_config_generic(
            generate_smsgg_keyboard_mapping(keyboard_inputs)?,
            generate_smsgg_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        );

        Ok(())
    }
}

impl InputMapper<GenesisInputs, GenesisButton> {
    pub(crate) fn new_genesis(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: GenesisInputConfig<KeyboardInput>,
        joystick_inputs: GenesisInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        Ok(Self::new_generic(
            joystick_subsystem,
            generate_genesis_keyboard_mapping(keyboard_inputs)?,
            generate_genesis_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: GenesisInputConfig<KeyboardInput>,
        joystick_inputs: GenesisInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<()> {
        self.reload_config_generic(
            generate_genesis_keyboard_mapping(keyboard_inputs)?,
            generate_genesis_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        );

        Ok(())
    }
}

impl InputMapper<NesInputs, NesButton> {
    pub(crate) fn new_nes(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: NesInputConfig<KeyboardInput>,
        joystick_inputs: NesInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        Ok(Self::new_generic(
            joystick_subsystem,
            generate_nes_keyboard_mapping(keyboard_inputs)?,
            generate_nes_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: NesInputConfig<KeyboardInput>,
        joystick_inputs: NesInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<()> {
        self.reload_config_generic(
            generate_nes_keyboard_mapping(keyboard_inputs)?,
            generate_nes_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        );

        Ok(())
    }
}

fn generate_snes_key_or_mouse_mapping(
    super_scope_config: SuperScopeConfig,
) -> NativeEmulatorResult<HashMap<KeycodeOrMouseButton, Vec<SnesButton>>> {
    let mut map: HashMap<KeycodeOrMouseButton, Vec<SnesButton>> = HashMap::new();
    for (input, button) in [
        (super_scope_config.fire, SuperScopeButton::Fire),
        (super_scope_config.cursor, SuperScopeButton::Cursor),
        (super_scope_config.pause, SuperScopeButton::Pause),
        (super_scope_config.turbo_toggle, SuperScopeButton::TurboToggle),
    ] {
        let Some(input) = input else { continue };
        let key_or_mouse_button = input.try_into()?;
        map.entry(key_or_mouse_button).or_default().push(SnesButton::SuperScope(button));
    }

    Ok(map)
}

impl InputMapper<SnesInputs, SnesButton> {
    pub(crate) fn new_snes(
        joystick_subsystem: JoystickSubsystem,
        p2_controller_type: SnesControllerType,
        keyboard_inputs: SnesInputConfig<KeyboardInput>,
        joystick_inputs: SnesInputConfig<JoystickInput>,
        super_scope_config: SuperScopeConfig,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        let mut mapper = Self::new_generic(
            joystick_subsystem,
            generate_snes_keyboard_mapping(keyboard_inputs)?,
            generate_snes_joystick_mapping(joystick_inputs),
            generate_snes_key_or_mouse_mapping(super_scope_config)?,
            axis_deadzone,
        );
        set_default_snes_inputs(
            &mut mapper.inputs,
            p2_controller_type,
            SuperScopeState::default().turbo,
        );

        Ok(mapper)
    }

    pub(crate) fn reload_config(
        &mut self,
        p2_controller_type: SnesControllerType,
        keyboard_inputs: SnesInputConfig<KeyboardInput>,
        joystick_inputs: SnesInputConfig<JoystickInput>,
        super_scope_config: SuperScopeConfig,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<()> {
        let existing_super_scope_turbo = match self.inputs.p2 {
            SnesInputDevice::SuperScope(super_scope_state) => super_scope_state.turbo,
            SnesInputDevice::Controller(_) => SuperScopeState::default().turbo,
        };

        self.reload_config_generic(
            generate_snes_keyboard_mapping(keyboard_inputs)?,
            generate_snes_joystick_mapping(joystick_inputs),
            generate_snes_key_or_mouse_mapping(super_scope_config)?,
            axis_deadzone,
        );
        set_default_snes_inputs(&mut self.inputs, p2_controller_type, existing_super_scope_turbo);

        Ok(())
    }
}

impl InputMapper<GameBoyInputs, GameBoyButton> {
    pub(crate) fn new_gb(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: GameBoyInputConfig<KeyboardInput>,
        joystick_inputs: GameBoyInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        Ok(Self::new_generic(
            joystick_subsystem,
            generate_gb_keyboard_mapping(keyboard_inputs)?,
            generate_gb_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: GameBoyInputConfig<KeyboardInput>,
        joystick_inputs: GameBoyInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<()> {
        self.reload_config_generic(
            generate_gb_keyboard_mapping(keyboard_inputs)?,
            generate_gb_joystick_mapping(joystick_inputs),
            HashMap::new(),
            axis_deadzone,
        );

        Ok(())
    }
}

fn set_default_snes_inputs(
    inputs: &mut SnesInputs,
    p2_controller_type: SnesControllerType,
    super_scope_turbo: bool,
) {
    match p2_controller_type {
        SnesControllerType::Gamepad => {
            inputs.p2 = SnesInputDevice::Controller(SnesJoypadState::default());
        }
        SnesControllerType::SuperScope => {
            inputs.p2 = SnesInputDevice::SuperScope(SuperScopeState {
                turbo: super_scope_turbo,
                ..SuperScopeState::default()
            });
        }
    }
}

impl<Inputs, Button> InputMapper<Inputs, Button>
where
    Inputs: Default + MappableInputs<Button>,
    Button: Copy,
{
    fn new_generic(
        joystick_subsystem: JoystickSubsystem,
        keyboard_mapping: HashMap<Keycode, Vec<Button>>,
        joystick_mapping: HashMap<JoystickInput, Vec<Button>>,
        key_or_mouse_mapping: HashMap<KeycodeOrMouseButton, Vec<Button>>,
        axis_deadzone: i16,
    ) -> Self {
        Self::new(
            Inputs::default(),
            joystick_subsystem,
            keyboard_mapping,
            joystick_mapping,
            key_or_mouse_mapping,
            axis_deadzone,
        )
    }

    fn reload_config_generic(
        &mut self,
        keyboard_mapping: HashMap<Keycode, Vec<Button>>,
        joystick_mapping: HashMap<JoystickInput, Vec<Button>>,
        key_or_mouse_mapping: HashMap<KeycodeOrMouseButton, Vec<Button>>,
        axis_deadzone: i16,
    ) {
        self.keyboard_mapping = keyboard_mapping;
        self.raw_joystick_mapping = joystick_mapping;
        self.key_or_mouse_mapping = key_or_mouse_mapping;
        self.axis_deadzone = axis_deadzone;

        self.update_input_mapping();
    }

    pub(crate) fn device_added(&mut self, device_id: u32) -> NativeEmulatorResult<()> {
        self.joysticks.device_added(device_id, &self.joystick_subsystem)?;
        self.update_input_mapping();

        Ok(())
    }

    pub(crate) fn device_removed(&mut self, instance_id: u32) {
        self.joysticks.device_removed(instance_id);
        self.update_input_mapping();
    }

    fn update_input_mapping(&mut self) {
        self.joystick_mapping.clear();
        self.inputs = Inputs::default();

        for (input, buttons) in &self.raw_joystick_mapping {
            if let Some(device_ids) = self.joysticks.name_to_device_ids.get(&input.device.name) {
                if let Some(&device_id) = device_ids.get(input.device.idx as usize) {
                    self.joystick_mapping.insert((device_id, input.action), buttons.clone());
                }
            }
        }
    }

    pub(crate) fn key_down(&mut self, keycode: Keycode) {
        self.key(keycode, true);
    }

    pub(crate) fn key_up(&mut self, keycode: Keycode) {
        self.key(keycode, false);
    }

    fn key(&mut self, keycode: Keycode, value: bool) {
        if let Some(buttons) = self.keyboard_mapping.get(&keycode) {
            for &button in buttons {
                self.inputs.set_field(button, value);
            }
        }

        if let Some(buttons) =
            self.key_or_mouse_mapping.get(&KeycodeOrMouseButton::Keycode(keycode))
        {
            for &button in buttons {
                self.inputs.set_field(button, value);
            }
        }
    }

    pub(crate) fn button_down(&mut self, instance_id: u32, button_idx: u8) {
        self.button(instance_id, button_idx, true);
    }

    pub(crate) fn button_up(&mut self, instance_id: u32, button_idx: u8) {
        self.button(instance_id, button_idx, false);
    }

    fn button(&mut self, instance_id: u32, button_idx: u8, value: bool) {
        let Some(device_id) = self.joysticks.device_id_for(instance_id) else { return };

        let Some(buttons) =
            self.joystick_mapping.get(&(device_id, JoystickAction::Button { button_idx }))
        else {
            return;
        };

        for &button in buttons {
            self.inputs.set_field(button, value);
        }
    }

    pub(crate) fn axis_motion(&mut self, instance_id: u32, axis_idx: u8, value: i16) {
        let Some(device_id) = self.joysticks.device_id_for(instance_id) else { return };

        let negative_down = value < -self.axis_deadzone;
        let positive_down = value > self.axis_deadzone;

        for (direction, value) in
            [(AxisDirection::Positive, positive_down), (AxisDirection::Negative, negative_down)]
        {
            if let Some(buttons) = self
                .joystick_mapping
                .get(&(device_id, JoystickAction::Axis { axis_idx, direction }))
            {
                for &button in buttons {
                    self.inputs.set_field(button, value);
                }
            }
        }
    }

    pub(crate) fn hat_motion(&mut self, instance_id: u32, hat_idx: u8, state: HatState) {
        let Some(device_id) = self.joysticks.device_id_for(instance_id) else { return };

        let up_pressed = matches!(state, HatState::LeftUp | HatState::Up | HatState::RightUp);
        let left_pressed = matches!(state, HatState::LeftUp | HatState::Left | HatState::LeftDown);
        let down_pressed =
            matches!(state, HatState::LeftDown | HatState::Down | HatState::RightDown);
        let right_pressed =
            matches!(state, HatState::RightUp | HatState::Right | HatState::RightDown);

        for (direction, value) in [
            (HatDirection::Up, up_pressed),
            (HatDirection::Left, left_pressed),
            (HatDirection::Down, down_pressed),
            (HatDirection::Right, right_pressed),
        ] {
            if let Some(buttons) =
                self.joystick_mapping.get(&(device_id, JoystickAction::Hat { hat_idx, direction }))
            {
                for &button in buttons {
                    self.inputs.set_field(button, value);
                }
            }
        }
    }

    pub(crate) fn handle_mouse_button(&mut self, mouse_button: MouseButton, pressed: bool) {
        if let Some(buttons) =
            self.key_or_mouse_mapping.get(&KeycodeOrMouseButton::Mouse(mouse_button))
        {
            for &button in buttons {
                self.inputs.set_field(button, pressed);
            }
        }
    }

    pub(crate) fn handle_event(
        &mut self,
        event: &Event,
        emulator_window_id: u32,
        display_info: Option<(FrameSize, DisplayArea)>,
    ) -> NativeEmulatorResult<()> {
        match *event {
            Event::KeyDown { keycode: Some(keycode), .. } => {
                self.key_down(keycode);
            }
            Event::KeyUp { keycode: Some(keycode), .. } => {
                self.key_up(keycode);
            }
            Event::JoyDeviceAdded { which: device_id, .. } => {
                self.device_added(device_id)?;
            }
            Event::JoyDeviceRemoved { which: instance_id, .. } => {
                self.device_removed(instance_id);
            }
            Event::JoyButtonDown { which: instance_id, button_idx, .. } => {
                self.button_down(instance_id, button_idx);
            }
            Event::JoyButtonUp { which: instance_id, button_idx, .. } => {
                self.button_up(instance_id, button_idx);
            }
            Event::JoyAxisMotion { which: instance_id, axis_idx, value, .. } => {
                self.axis_motion(instance_id, axis_idx, value);
            }
            Event::JoyHatMotion { which: instance_id, hat_idx, state, .. } => {
                self.hat_motion(instance_id, hat_idx, state);
            }
            Event::MouseButtonDown { mouse_btn, window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.handle_mouse_button(mouse_btn, true);
            }
            Event::MouseButtonUp { mouse_btn, window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.handle_mouse_button(mouse_btn, false);
            }
            Event::MouseMotion { x, y, window_id, .. } if window_id == emulator_window_id => {
                if let Some((frame_size, display_area)) = display_info {
                    self.inputs.handle_mouse_motion(x, y, frame_size, display_area);
                }
            }
            Event::Window { win_event: WindowEvent::Leave, window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.inputs.handle_mouse_leave();
            }
            _ => {}
        }

        Ok(())
    }

    pub(crate) fn inputs(&self) -> &Inputs {
        &self.inputs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    Quit,
    ToggleFullscreen,
    SaveState,
    LoadState,
    SoftReset,
    HardReset,
    Pause,
    StepFrame,
    FastForward,
    Rewind,
    OpenDebugger,
}

pub(crate) enum HotkeyMapResult<'a> {
    None,
    Pressed(&'a Vec<Hotkey>),
    Released(&'a Vec<Hotkey>),
}

pub(crate) struct HotkeyMapper {
    mapping: HashMap<Keycode, Vec<Hotkey>>,
}

const EMPTY_VEC: &Vec<Hotkey> = &Vec::new();

impl HotkeyMapper {
    /// Build a hotkey mapper from the given config.
    ///
    /// # Errors
    ///
    /// This function will return an error if the given config contains any invalid keycodes.
    pub fn from_config(config: &HotkeyConfig) -> NativeEmulatorResult<Self> {
        let mut mapping: HashMap<Keycode, Vec<Hotkey>> = HashMap::new();
        for (input, hotkey) in [
            (&config.quit, Hotkey::Quit),
            (&config.toggle_fullscreen, Hotkey::ToggleFullscreen),
            (&config.save_state, Hotkey::SaveState),
            (&config.load_state, Hotkey::LoadState),
            (&config.soft_reset, Hotkey::SoftReset),
            (&config.hard_reset, Hotkey::HardReset),
            (&config.pause, Hotkey::Pause),
            (&config.step_frame, Hotkey::StepFrame),
            (&config.fast_forward, Hotkey::FastForward),
            (&config.rewind, Hotkey::Rewind),
            (&config.open_debugger, Hotkey::OpenDebugger),
        ] {
            if let Some(input) = input {
                let keycode = Keycode::from_name(&input.keycode)
                    .ok_or_else(|| NativeEmulatorError::InvalidKeycode(input.keycode.clone()))?;
                mapping.entry(keycode).or_default().push(hotkey);
            }
        }

        Ok(Self { mapping })
    }

    #[must_use]
    pub fn check_for_hotkeys(&self, event: &Event) -> HotkeyMapResult<'_> {
        match event {
            Event::KeyDown { keycode: Some(keycode), .. } => {
                HotkeyMapResult::Pressed(self.mapping.get(keycode).unwrap_or(EMPTY_VEC))
            }
            Event::KeyUp { keycode: Some(keycode), .. } => {
                HotkeyMapResult::Released(self.mapping.get(keycode).unwrap_or(EMPTY_VEC))
            }
            _ => HotkeyMapResult::None,
        }
    }
}
