use crate::config::input::{
    AxisDirection, GenesisInputConfig, HatDirection, HotkeyConfig, JoystickAction,
    JoystickDeviceId, JoystickInput, KeyboardInput, SmsGgInputConfig, SnesInputConfig,
};
use crate::mainloop::{NativeEmulatorError, NativeEmulatorResult};
use genesis_core::{GenesisControllerType, GenesisInputs, GenesisJoypadState};
use sdl2::event::Event;
use sdl2::joystick::{HatState, Joystick};
use sdl2::keyboard::Keycode;
use sdl2::JoystickSubsystem;
use smsgg_core::SmsGgInputs;
use snes_core::input::{SnesInputs, SnesJoypadState};
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
}

impl SnesButton {
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
        }
    }
}

pub trait Clearable {
    fn clear(&mut self);
}

impl Clearable for SmsGgInputs {
    fn clear(&mut self) {
        *self = Self::default();
    }
}

impl Clearable for GenesisInputs {
    fn clear(&mut self) {
        self.p1 = GenesisJoypadState::default();
        self.p2 = GenesisJoypadState::default();
    }
}

impl Clearable for SnesInputs {
    fn clear(&mut self) {
        self.p1 = SnesJoypadState::default();
        self.p2 = SnesJoypadState::default();
    }
}

pub trait GetButtonField<Button>
where
    Button: Copy,
{
    fn get_field(&mut self, button: Button) -> &mut bool;
}

impl GetButtonField<SmsGgButton> for SmsGgInputs {
    fn get_field(&mut self, button: SmsGgButton) -> &mut bool {
        let joypad_state = match button.player() {
            Player::One => &mut self.p1,
            Player::Two => &mut self.p2,
        };

        match button {
            SmsGgButton::Up(..) => &mut joypad_state.up,
            SmsGgButton::Left(..) => &mut joypad_state.left,
            SmsGgButton::Right(..) => &mut joypad_state.right,
            SmsGgButton::Down(..) => &mut joypad_state.down,
            SmsGgButton::Button1(..) => &mut joypad_state.button_1,
            SmsGgButton::Button2(..) => &mut joypad_state.button_2,
            SmsGgButton::Pause => &mut self.pause,
        }
    }
}

impl GetButtonField<GenesisButton> for GenesisInputs {
    fn get_field(&mut self, button: GenesisButton) -> &mut bool {
        let joypad_state = match button.player() {
            Player::One => &mut self.p1,
            Player::Two => &mut self.p2,
        };

        match button {
            GenesisButton::Up(..) => &mut joypad_state.up,
            GenesisButton::Left(..) => &mut joypad_state.left,
            GenesisButton::Right(..) => &mut joypad_state.right,
            GenesisButton::Down(..) => &mut joypad_state.down,
            GenesisButton::A(..) => &mut joypad_state.a,
            GenesisButton::B(..) => &mut joypad_state.b,
            GenesisButton::C(..) => &mut joypad_state.c,
            GenesisButton::X(..) => &mut joypad_state.x,
            GenesisButton::Y(..) => &mut joypad_state.y,
            GenesisButton::Z(..) => &mut joypad_state.z,
            GenesisButton::Start(..) => &mut joypad_state.start,
            GenesisButton::Mode(..) => &mut joypad_state.mode,
        }
    }
}

impl GetButtonField<SnesButton> for SnesInputs {
    fn get_field(&mut self, button: SnesButton) -> &mut bool {
        let joypad_state = match button.player() {
            Player::One => &mut self.p1,
            Player::Two => &mut self.p2,
        };

        match button {
            SnesButton::Up(..) => &mut joypad_state.up,
            SnesButton::Left(..) => &mut joypad_state.left,
            SnesButton::Right(..) => &mut joypad_state.right,
            SnesButton::Down(..) => &mut joypad_state.down,
            SnesButton::A(..) => &mut joypad_state.a,
            SnesButton::B(..) => &mut joypad_state.b,
            SnesButton::X(..) => &mut joypad_state.x,
            SnesButton::Y(..) => &mut joypad_state.y,
            SnesButton::L(..) => &mut joypad_state.l,
            SnesButton::R(..) => &mut joypad_state.r,
            SnesButton::Start(..) => &mut joypad_state.start,
            SnesButton::Select(..) => &mut joypad_state.select,
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
        if let Some(device_id) = self.instance_id_to_device_id.remove(&instance_id) {
            if let Some(joystick) = self.joysticks.remove(&device_id) {
                log::info!("Disconnected joystick id {device_id}: {}", joystick.name());
            }

            for device_ids in self.name_to_device_ids.values_mut() {
                device_ids.retain(|&id| id != device_id);
            }
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
    ($p1_config:expr, $p2_config:expr, [$($field:ident -> $button:expr),*$(,)?]) => {
        [
            $(
                ($p1_config.$field, $button(Player::One)),
                ($p2_config.$field, $button(Player::Two)),
            )*
        ]
    }
}

macro_rules! smsgg_input_array {
    ($p1_config:expr, $p2_config:expr) => {
        inputs_array!($p1_config, $p2_config, [
            up -> SmsGgButton::Up,
            left -> SmsGgButton::Left,
            right -> SmsGgButton::Right,
            down -> SmsGgButton::Down,
            button_1 -> SmsGgButton::Button1,
            button_2 -> SmsGgButton::Button2,
        ])
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

impl InputMapper<SmsGgInputs, SmsGgButton> {
    pub(crate) fn new_smsgg(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        joystick_inputs: SmsGgInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        let keyboard_mapping = generate_smsgg_keyboard_mapping(keyboard_inputs)?;
        let joystick_mapping = generate_smsgg_joystick_mapping(joystick_inputs);

        Ok(Self::new(
            SmsGgInputs::default(),
            joystick_subsystem,
            keyboard_mapping,
            joystick_mapping,
            axis_deadzone,
        ))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        joystick_inputs: SmsGgInputConfig<JoystickInput>,
    ) -> NativeEmulatorResult<()> {
        self.keyboard_mapping = generate_smsgg_keyboard_mapping(keyboard_inputs)?;
        self.raw_joystick_mapping = generate_smsgg_joystick_mapping(joystick_inputs);

        self.update_input_mapping();

        Ok(())
    }
}

fn generate_smsgg_keyboard_mapping(
    keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
) -> NativeEmulatorResult<HashMap<Keycode, Vec<SmsGgButton>>> {
    let mut keyboard_mapping: HashMap<Keycode, Vec<SmsGgButton>> = HashMap::new();
    for (input, button) in smsgg_input_array!(keyboard_inputs.p1, keyboard_inputs.p2) {
        if let Some(KeyboardInput { keycode }) = input {
            let keycode = Keycode::from_name(&keycode)
                .ok_or_else(|| NativeEmulatorError::InvalidKeycode(keycode))?;
            keyboard_mapping.entry(keycode).or_default().push(button);
        }
    }

    if let Some(KeyboardInput { keycode }) = keyboard_inputs.p1.pause {
        let keycode = Keycode::from_name(&keycode)
            .ok_or_else(|| NativeEmulatorError::InvalidKeycode(keycode))?;
        keyboard_mapping.entry(keycode).or_default().push(SmsGgButton::Pause);
    }

    Ok(keyboard_mapping)
}

fn generate_smsgg_joystick_mapping(
    joystick_inputs: SmsGgInputConfig<JoystickInput>,
) -> HashMap<JoystickInput, Vec<SmsGgButton>> {
    let mut joystick_mapping: HashMap<JoystickInput, Vec<SmsGgButton>> = HashMap::new();
    for (input, button) in smsgg_input_array!(joystick_inputs.p1, joystick_inputs.p2) {
        if let Some(input) = input {
            joystick_mapping.entry(input).or_default().push(button);
        }
    }

    if let Some(input) = joystick_inputs.p1.pause {
        joystick_mapping.entry(input).or_default().push(SmsGgButton::Pause);
    }

    joystick_mapping
}

impl InputMapper<GenesisInputs, GenesisButton> {
    pub(crate) fn new_genesis(
        p1_type: GenesisControllerType,
        p2_type: GenesisControllerType,
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: GenesisInputConfig<KeyboardInput>,
        joystick_inputs: GenesisInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        let keyboard_mapping = generate_genesis_keyboard_mapping(keyboard_inputs)?;
        let joystick_mapping = generate_genesis_joystick_mapping(joystick_inputs);

        let inputs = GenesisInputs {
            p1_type,
            p1: GenesisJoypadState::default(),
            p2_type,
            p2: GenesisJoypadState::default(),
        };
        Ok(Self::new(inputs, joystick_subsystem, keyboard_mapping, joystick_mapping, axis_deadzone))
    }

    pub(crate) fn reload_config(
        &mut self,
        p1_type: GenesisControllerType,
        p2_type: GenesisControllerType,
        keyboard_inputs: GenesisInputConfig<KeyboardInput>,
        joystick_inputs: GenesisInputConfig<JoystickInput>,
    ) -> NativeEmulatorResult<()> {
        self.inputs.p1_type = p1_type;
        self.inputs.p2_type = p2_type;
        self.keyboard_mapping = generate_genesis_keyboard_mapping(keyboard_inputs)?;
        self.raw_joystick_mapping = generate_genesis_joystick_mapping(joystick_inputs);

        self.update_input_mapping();

        Ok(())
    }
}

fn generate_genesis_keyboard_mapping(
    keyboard_inputs: GenesisInputConfig<KeyboardInput>,
) -> NativeEmulatorResult<HashMap<Keycode, Vec<GenesisButton>>> {
    let mut keyboard_mapping: HashMap<Keycode, Vec<GenesisButton>> = HashMap::new();
    for (input, button) in genesis_input_array!(keyboard_inputs.p1, keyboard_inputs.p2) {
        if let Some(KeyboardInput { keycode }) = input {
            let keycode = Keycode::from_name(&keycode)
                .ok_or_else(|| NativeEmulatorError::InvalidKeycode(keycode))?;
            keyboard_mapping.entry(keycode).or_default().push(button);
        }
    }

    Ok(keyboard_mapping)
}

fn generate_genesis_joystick_mapping(
    joystick_inputs: GenesisInputConfig<JoystickInput>,
) -> HashMap<JoystickInput, Vec<GenesisButton>> {
    let mut joystick_mapping: HashMap<JoystickInput, Vec<GenesisButton>> = HashMap::new();
    for (input, button) in genesis_input_array!(joystick_inputs.p1, joystick_inputs.p2) {
        if let Some(input) = input {
            joystick_mapping.entry(input).or_default().push(button);
        }
    }

    joystick_mapping
}

impl InputMapper<SnesInputs, SnesButton> {
    pub(crate) fn new_snes(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: SnesInputConfig<KeyboardInput>,
        joystick_inputs: SnesInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> NativeEmulatorResult<Self> {
        let keyboard_mapping = generate_snes_keyboard_mapping(keyboard_inputs)?;
        let joystick_mapping = generate_snes_joystick_mapping(joystick_inputs)?;

        let inputs = SnesInputs::default();

        Ok(Self::new(inputs, joystick_subsystem, keyboard_mapping, joystick_mapping, axis_deadzone))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: SnesInputConfig<KeyboardInput>,
        joystick_inputs: SnesInputConfig<JoystickInput>,
    ) -> NativeEmulatorResult<()> {
        self.keyboard_mapping = generate_snes_keyboard_mapping(keyboard_inputs)?;
        self.raw_joystick_mapping = generate_snes_joystick_mapping(joystick_inputs)?;

        self.update_input_mapping();

        Ok(())
    }
}

fn generate_snes_keyboard_mapping(
    keyboard_inputs: SnesInputConfig<KeyboardInput>,
) -> NativeEmulatorResult<HashMap<Keycode, Vec<SnesButton>>> {
    let mut keyboard_mapping: HashMap<Keycode, Vec<SnesButton>> = HashMap::new();
    for (input, button) in snes_input_array!(keyboard_inputs.p1, keyboard_inputs.p2) {
        if let Some(KeyboardInput { keycode }) = input {
            let keycode = Keycode::from_name(&keycode)
                .ok_or_else(|| NativeEmulatorError::InvalidKeycode(keycode))?;
            keyboard_mapping.entry(keycode).or_default().push(button);
        }
    }

    Ok(keyboard_mapping)
}

fn generate_snes_joystick_mapping(
    keyboard_inputs: SnesInputConfig<JoystickInput>,
) -> NativeEmulatorResult<HashMap<JoystickInput, Vec<SnesButton>>> {
    let mut joystick_mapping: HashMap<JoystickInput, Vec<SnesButton>> = HashMap::new();
    for (input, button) in snes_input_array!(keyboard_inputs.p1, keyboard_inputs.p2) {
        if let Some(input) = input {
            joystick_mapping.entry(input).or_default().push(button);
        }
    }

    Ok(joystick_mapping)
}

impl<Inputs, Button> InputMapper<Inputs, Button>
where
    Inputs: Clearable + GetButtonField<Button>,
    Button: Copy,
{
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
        self.inputs.clear();

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
                *self.inputs.get_field(button) = value;
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
        if let Some(device_id) = self.joysticks.device_id_for(instance_id) {
            if let Some(buttons) =
                self.joystick_mapping.get(&(device_id, JoystickAction::Button { button_idx }))
            {
                for &button in buttons {
                    *self.inputs.get_field(button) = value;
                }
            }
        }
    }

    pub(crate) fn axis_motion(&mut self, instance_id: u32, axis_idx: u8, value: i16) {
        let negative_down = value < -self.axis_deadzone;
        let positive_down = value > self.axis_deadzone;

        if let Some(device_id) = self.joysticks.device_id_for(instance_id) {
            for (direction, value) in
                [(AxisDirection::Positive, positive_down), (AxisDirection::Negative, negative_down)]
            {
                if let Some(buttons) = self
                    .joystick_mapping
                    .get(&(device_id, JoystickAction::Axis { axis_idx, direction }))
                {
                    for &button in buttons {
                        *self.inputs.get_field(button) = value;
                    }
                }
            }
        }
    }

    pub(crate) fn hat_motion(&mut self, instance_id: u32, hat_idx: u8, state: HatState) {
        let up_pressed = matches!(state, HatState::LeftUp | HatState::Up | HatState::RightUp);
        let left_pressed = matches!(state, HatState::LeftUp | HatState::Left | HatState::LeftDown);
        let down_pressed =
            matches!(state, HatState::LeftDown | HatState::Down | HatState::RightDown);
        let right_pressed =
            matches!(state, HatState::RightUp | HatState::Right | HatState::RightDown);

        if let Some(device_id) = self.joysticks.device_id_for(instance_id) {
            for (direction, value) in [
                (HatDirection::Up, up_pressed),
                (HatDirection::Left, left_pressed),
                (HatDirection::Down, down_pressed),
                (HatDirection::Right, right_pressed),
            ] {
                if let Some(buttons) = self
                    .joystick_mapping
                    .get(&(device_id, JoystickAction::Hat { hat_idx, direction }))
                {
                    for &button in buttons {
                        *self.inputs.get_field(button) = value;
                    }
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
    OpenCramDebug,
    OpenVramDebug,
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
            (&config.open_cram_debug, Hotkey::OpenCramDebug),
            (&config.open_vram_debug, Hotkey::OpenVramDebug),
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
