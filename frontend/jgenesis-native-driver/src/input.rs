use crate::config::input::{
    AxisDirection, GenesisInputConfig, HatDirection, HotkeyConfig, JoystickAction,
    JoystickDeviceId, JoystickInput, KeyboardInput, SmsGgInputConfig, SnesInputConfig,
};
use crate::mainloop::{NativeEmulatorError, NativeEmulatorResult};
use genesis_core::GenesisInputs;
use sdl2::event::Event;
use sdl2::joystick::{HatState, Joystick};
use sdl2::keyboard::Keycode;
use sdl2::JoystickSubsystem;
use smsgg_core::SmsGgInputs;
use snes_core::input::{SnesInputDevice, SnesInputs};
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

pub trait SetButtonField<Button> {
    fn set_field(&mut self, button: Button, value: bool);
}

impl SetButtonField<SmsGgButton> for SmsGgInputs {
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
            SmsGgButton::Button1(..) => joypad_state.button_1 = value,
            SmsGgButton::Button2(..) => joypad_state.button_2 = value,
            SmsGgButton::Pause => self.pause = value,
        }
    }
}

impl SetButtonField<GenesisButton> for GenesisInputs {
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
}

impl SetButtonField<SnesButton> for SnesInputs {
    fn set_field(&mut self, button: SnesButton, value: bool) {
        if let SnesButton::SuperScope(super_scope_button) = button {
            let SnesInputDevice::SuperScope(super_scope_state) = &mut self.p2 else { return };

            match super_scope_button {
                SuperScopeButton::Fire => super_scope_state.fire = true,
                SuperScopeButton::Cursor => super_scope_state.cursor = true,
                SuperScopeButton::Pause => super_scope_state.pause = true,
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
            SnesButton::SuperScope(..) => {}
        }
    }
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
        let Some((device_idx, _)) =
            device_ids.iter().copied().enumerate().find(|&(_, id)| id == device_id)
        else {
            return None;
        };

        Some(JoystickDeviceId::new(name, device_idx as u32))
    }

    #[must_use]
    pub fn device_id_for(&self, instance_id: u32) -> Option<u32> {
        self.instance_id_to_device_id.get(&instance_id).copied()
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
    generate_snes_keyboard_mapping,
    generate_snes_joystick_mapping,
    SnesInputConfig,
    SnesButton,
    |config| snes_input_array!(config.p1, config.p2)
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
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        joystick_inputs: SmsGgInputConfig<JoystickInput>,
    ) -> NativeEmulatorResult<()> {
        self.reload_config_generic(
            generate_smsgg_keyboard_mapping(keyboard_inputs)?,
            generate_smsgg_joystick_mapping(joystick_inputs),
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
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: GenesisInputConfig<KeyboardInput>,
        joystick_inputs: GenesisInputConfig<JoystickInput>,
    ) -> NativeEmulatorResult<()> {
        self.reload_config_generic(
            generate_genesis_keyboard_mapping(keyboard_inputs)?,
            generate_genesis_joystick_mapping(joystick_inputs),
        );

        Ok(())
    }
}

impl InputMapper<SnesInputs, SnesButton> {
    pub(crate) fn new_snes(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: SnesInputConfig<KeyboardInput>,
        joystick_inputs: SnesInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        Ok(Self::new_generic(
            joystick_subsystem,
            generate_snes_keyboard_mapping(keyboard_inputs)?,
            generate_snes_joystick_mapping(joystick_inputs),
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: SnesInputConfig<KeyboardInput>,
        joystick_inputs: SnesInputConfig<JoystickInput>,
    ) -> NativeEmulatorResult<()> {
        self.reload_config_generic(
            generate_snes_keyboard_mapping(keyboard_inputs)?,
            generate_snes_joystick_mapping(joystick_inputs),
        );

        Ok(())
    }
}

impl<Inputs, Button> InputMapper<Inputs, Button>
where
    Inputs: Default + SetButtonField<Button>,
    Button: Copy,
{
    fn new_generic(
        joystick_subsystem: JoystickSubsystem,
        keyboard_mapping: HashMap<Keycode, Vec<Button>>,
        joystick_mapping: HashMap<JoystickInput, Vec<Button>>,
        axis_deadzone: i16,
    ) -> Self {
        Self::new(
            Inputs::default(),
            joystick_subsystem,
            keyboard_mapping,
            joystick_mapping,
            axis_deadzone,
        )
    }

    fn reload_config_generic(
        &mut self,
        keyboard_mapping: HashMap<Keycode, Vec<Button>>,
        joystick_mapping: HashMap<JoystickInput, Vec<Button>>,
    ) {
        self.keyboard_mapping = keyboard_mapping;
        self.raw_joystick_mapping = joystick_mapping;

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
        let Some(buttons) = self.keyboard_mapping.get(&keycode) else { return };

        for &button in buttons {
            self.inputs.set_field(button, value);
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

    pub(crate) fn handle_event(&mut self, event: &Event) -> NativeEmulatorResult<()> {
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
