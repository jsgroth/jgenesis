use crate::config::input::{
    AxisDirection, GenesisInputConfig, HatDirection, HotkeyConfig, JoystickAction,
    JoystickDeviceId, JoystickInput, KeyboardInput, SmsGgInputConfig,
};
use anyhow::anyhow;
use genesis_core::GenesisInputs;
use sdl2::event::Event;
use sdl2::joystick::{HatState, Joystick};
use sdl2::keyboard::Keycode;
use sdl2::JoystickSubsystem;
use smsgg_core::SmsGgInputs;
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
    Start(Player),
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
            | Self::Start(player) => player,
        }
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
        match button {
            SmsGgButton::Up(Player::One) => &mut self.p1.up,
            SmsGgButton::Left(Player::One) => &mut self.p1.left,
            SmsGgButton::Right(Player::One) => &mut self.p1.right,
            SmsGgButton::Down(Player::One) => &mut self.p1.down,
            SmsGgButton::Button1(Player::One) => &mut self.p1.button_1,
            SmsGgButton::Button2(Player::One) => &mut self.p1.button_2,
            SmsGgButton::Up(Player::Two) => &mut self.p2.up,
            SmsGgButton::Left(Player::Two) => &mut self.p2.left,
            SmsGgButton::Right(Player::Two) => &mut self.p2.right,
            SmsGgButton::Down(Player::Two) => &mut self.p2.down,
            SmsGgButton::Button1(Player::Two) => &mut self.p2.button_1,
            SmsGgButton::Button2(Player::Two) => &mut self.p2.button_2,
            SmsGgButton::Pause => &mut self.pause,
        }
    }
}

impl GetButtonField<GenesisButton> for GenesisInputs {
    fn get_field(&mut self, button: GenesisButton) -> &mut bool {
        match button {
            GenesisButton::Up(Player::One) => &mut self.p1.up,
            GenesisButton::Left(Player::One) => &mut self.p1.left,
            GenesisButton::Right(Player::One) => &mut self.p1.right,
            GenesisButton::Down(Player::One) => &mut self.p1.down,
            GenesisButton::A(Player::One) => &mut self.p1.a,
            GenesisButton::B(Player::One) => &mut self.p1.b,
            GenesisButton::C(Player::One) => &mut self.p1.c,
            GenesisButton::Start(Player::One) => &mut self.p1.start,
            GenesisButton::Up(Player::Two) => &mut self.p2.up,
            GenesisButton::Left(Player::Two) => &mut self.p2.left,
            GenesisButton::Right(Player::Two) => &mut self.p2.right,
            GenesisButton::Down(Player::Two) => &mut self.p2.down,
            GenesisButton::A(Player::Two) => &mut self.p2.a,
            GenesisButton::B(Player::Two) => &mut self.p2.b,
            GenesisButton::C(Player::Two) => &mut self.p2.c,
            GenesisButton::Start(Player::Two) => &mut self.p2.start,
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
    ) -> anyhow::Result<()> {
        let joystick = joystick_subsystem.open(device_id)?;
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

impl<Inputs: Default, Button> InputMapper<Inputs, Button> {
    fn new(
        joystick_subsystem: JoystickSubsystem,
        keyboard_mapping: HashMap<Keycode, Vec<Button>>,
        joystick_mapping: HashMap<JoystickInput, Vec<Button>>,
        axis_deadzone: i16,
    ) -> Self {
        Self {
            inputs: Inputs::default(),
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

impl InputMapper<SmsGgInputs, SmsGgButton> {
    pub(crate) fn new_smsgg(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        joystick_inputs: SmsGgInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> anyhow::Result<Self> {
        let keyboard_mapping = generate_smsgg_keyboard_mapping(keyboard_inputs)?;
        let joystick_mapping = generate_smsgg_joystick_mapping(joystick_inputs);

        Ok(Self::new(joystick_subsystem, keyboard_mapping, joystick_mapping, axis_deadzone))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        joystick_inputs: SmsGgInputConfig<JoystickInput>,
    ) -> anyhow::Result<()> {
        self.keyboard_mapping = generate_smsgg_keyboard_mapping(keyboard_inputs)?;
        self.raw_joystick_mapping = generate_smsgg_joystick_mapping(joystick_inputs);

        self.update_input_mapping();

        Ok(())
    }
}

fn generate_smsgg_keyboard_mapping(
    keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
) -> anyhow::Result<HashMap<Keycode, Vec<SmsGgButton>>> {
    let mut keyboard_mapping: HashMap<Keycode, Vec<SmsGgButton>> = HashMap::new();
    for (input, button) in smsgg_input_array!(keyboard_inputs.p1, keyboard_inputs.p2) {
        if let Some(KeyboardInput { keycode }) = input {
            let keycode = Keycode::from_name(&keycode)
                .ok_or_else(|| anyhow!("invalid SDL2 keycode: {keycode}"))?;
            keyboard_mapping.entry(keycode).or_default().push(button);
        }
    }

    if let Some(KeyboardInput { keycode }) = keyboard_inputs.p1.pause {
        let keycode = Keycode::from_name(&keycode)
            .ok_or_else(|| anyhow!("invalid SDL2 keycode: {keycode}"))?;
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
            start -> GenesisButton::Start,
        ])
    }
}

impl InputMapper<GenesisInputs, GenesisButton> {
    pub(crate) fn new_genesis(
        joystick_subsystem: JoystickSubsystem,
        keyboard_inputs: GenesisInputConfig<KeyboardInput>,
        joystick_inputs: GenesisInputConfig<JoystickInput>,
        axis_deadzone: i16,
    ) -> anyhow::Result<Self> {
        let keyboard_mapping = generate_genesis_keyboard_mapping(keyboard_inputs)?;
        let joystick_mapping = generate_genesis_joystick_mapping(joystick_inputs);

        Ok(Self::new(joystick_subsystem, keyboard_mapping, joystick_mapping, axis_deadzone))
    }

    pub(crate) fn reload_config(
        &mut self,
        keyboard_inputs: GenesisInputConfig<KeyboardInput>,
        joystick_inputs: GenesisInputConfig<JoystickInput>,
    ) -> anyhow::Result<()> {
        self.keyboard_mapping = generate_genesis_keyboard_mapping(keyboard_inputs)?;
        self.raw_joystick_mapping = generate_genesis_joystick_mapping(joystick_inputs);

        self.update_input_mapping();

        Ok(())
    }
}

fn generate_genesis_keyboard_mapping(
    keyboard_inputs: GenesisInputConfig<KeyboardInput>,
) -> anyhow::Result<HashMap<Keycode, Vec<GenesisButton>>> {
    let mut keyboard_mapping: HashMap<Keycode, Vec<GenesisButton>> = HashMap::new();
    for (input, button) in genesis_input_array!(keyboard_inputs.p1, keyboard_inputs.p2) {
        if let Some(KeyboardInput { keycode }) = input {
            let keycode = Keycode::from_name(&keycode)
                .ok_or_else(|| anyhow!("invalid SDL2 keycode: {keycode}"))?;
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

impl<Inputs, Button> InputMapper<Inputs, Button>
where
    Inputs: Default + GetButtonField<Button>,
    Button: Copy,
{
    pub(crate) fn device_added(&mut self, device_id: u32) -> anyhow::Result<()> {
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

    pub(crate) fn handle_event(&mut self, event: &Event) -> anyhow::Result<()> {
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
    pub fn from_config(config: &HotkeyConfig) -> anyhow::Result<Self> {
        let mut mapping: HashMap<Keycode, Vec<Hotkey>> = HashMap::new();
        for (input, hotkey) in [
            (&config.quit, Hotkey::Quit),
            (&config.toggle_fullscreen, Hotkey::ToggleFullscreen),
            (&config.save_state, Hotkey::SaveState),
            (&config.load_state, Hotkey::LoadState),
            (&config.soft_reset, Hotkey::SoftReset),
            (&config.hard_reset, Hotkey::HardReset),
        ] {
            if let Some(input) = input {
                let keycode = Keycode::from_name(&input.keycode)
                    .ok_or_else(|| anyhow!("Invalid SDL2 keycode: {}", input.keycode))?;
                mapping.entry(keycode).or_default().push(hotkey);
            }
        }

        Ok(Self { mapping })
    }

    #[must_use]
    pub fn check_for_hotkeys(&self, event: &Event) -> &Vec<Hotkey> {
        match event {
            Event::KeyDown { keycode: Some(keycode), .. } => {
                self.mapping.get(keycode).unwrap_or(EMPTY_VEC)
            }
            _ => EMPTY_VEC,
        }
    }
}
