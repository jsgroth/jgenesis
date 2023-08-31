use crate::config::input::{
    AxisDirection, HatDirection, JoystickAction, JoystickInput, KeyboardInput, SmsGgInputConfig,
};
use anyhow::anyhow;
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

pub struct InputMapper<'joy, Inputs, Button> {
    inputs: Inputs,
    joystick_subsystem: &'joy JoystickSubsystem,
    joysticks: HashMap<u32, Joystick>,
    axis_deadzone: i16,
    instance_id_to_device_id: HashMap<u32, u32>,
    name_to_device_ids: HashMap<String, Vec<u32>>,
    keyboard_mapping: HashMap<Keycode, Vec<Button>>,
    raw_input_mapping: HashMap<JoystickInput, Vec<Button>>,
    input_mapping: HashMap<(u32, JoystickAction), Vec<Button>>,
}

impl<'joy> InputMapper<'joy, SmsGgInputs, SmsGgButton> {
    pub fn new(
        joystick_subsystem: &'joy JoystickSubsystem,
        keyboard_inputs: SmsGgInputConfig<KeyboardInput>,
        axis_deadzone: i16,
    ) -> anyhow::Result<Self> {
        let mut keyboard_mapping: HashMap<Keycode, Vec<SmsGgButton>> = HashMap::new();
        for (input, button) in [
            (keyboard_inputs.p1.up, SmsGgButton::Up(Player::One)),
            (keyboard_inputs.p1.left, SmsGgButton::Left(Player::One)),
            (keyboard_inputs.p1.right, SmsGgButton::Right(Player::One)),
            (keyboard_inputs.p1.down, SmsGgButton::Down(Player::One)),
            (keyboard_inputs.p1.button_1, SmsGgButton::Button1(Player::One)),
            (keyboard_inputs.p1.button_2, SmsGgButton::Button2(Player::One)),
            (keyboard_inputs.p1.pause, SmsGgButton::Pause),
            (keyboard_inputs.p2.up, SmsGgButton::Up(Player::Two)),
            (keyboard_inputs.p2.left, SmsGgButton::Left(Player::Two)),
            (keyboard_inputs.p2.right, SmsGgButton::Right(Player::Two)),
            (keyboard_inputs.p2.down, SmsGgButton::Down(Player::Two)),
            (keyboard_inputs.p2.button_1, SmsGgButton::Button1(Player::Two)),
            (keyboard_inputs.p2.button_2, SmsGgButton::Button2(Player::Two)),
            (keyboard_inputs.p2.pause, SmsGgButton::Pause),
        ] {
            if let Some(KeyboardInput { keycode }) = input {
                let keycode = Keycode::from_name(&keycode)
                    .ok_or_else(|| anyhow!("invalid SDL2 keycode: {keycode}"))?;
                keyboard_mapping.entry(keycode).or_default().push(button);
            }
        }

        Ok(Self {
            inputs: SmsGgInputs::default(),
            joystick_subsystem,
            joysticks: HashMap::new(),
            axis_deadzone,
            instance_id_to_device_id: HashMap::new(),
            name_to_device_ids: HashMap::new(),
            keyboard_mapping,
            // TODO joystick mappings
            raw_input_mapping: HashMap::new(),
            input_mapping: HashMap::new(),
        })
    }
}

impl<'joy, Inputs, Button> InputMapper<'joy, Inputs, Button>
where
    Inputs: Default + GetButtonField<Button>,
    Button: Copy,
{
    pub fn device_added(&mut self, device_id: u32) -> anyhow::Result<()> {
        let joystick = self.joystick_subsystem.open(device_id)?;
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

        self.update_input_mapping();

        Ok(())
    }

    pub fn device_removed(&mut self, instance_id: u32) {
        if let Some(device_id) = self.instance_id_to_device_id.remove(&instance_id) {
            if let Some(joystick) = self.joysticks.remove(&device_id) {
                log::info!("Disconnected joystick id {device_id}: {}", joystick.name());
            }
        }

        self.update_input_mapping();
    }

    fn update_input_mapping(&mut self) {
        self.input_mapping.clear();
        self.inputs = Inputs::default();

        for (input, buttons) in &self.raw_input_mapping {
            if let Some(device_ids) = self.name_to_device_ids.get(&input.device.name) {
                if let Some(&device_id) = device_ids.get(input.device.idx as usize) {
                    self.input_mapping.insert((device_id, input.action), buttons.clone());
                }
            }
        }
    }

    pub fn key_down(&mut self, keycode: Keycode) {
        self.key(keycode, true);
    }

    pub fn key_up(&mut self, keycode: Keycode) {
        self.key(keycode, false);
    }

    fn key(&mut self, keycode: Keycode, value: bool) {
        if let Some(buttons) = self.keyboard_mapping.get(&keycode) {
            for &button in buttons {
                *self.inputs.get_field(button) = value;
            }
        }
    }

    pub fn button_down(&mut self, instance_id: u32, button_idx: u8) {
        self.button(instance_id, button_idx, true);
    }

    pub fn button_up(&mut self, instance_id: u32, button_idx: u8) {
        self.button(instance_id, button_idx, false);
    }

    fn button(&mut self, instance_id: u32, button_idx: u8, value: bool) {
        if let Some(&device_id) = self.instance_id_to_device_id.get(&instance_id) {
            if let Some(buttons) =
                self.input_mapping.get(&(device_id, JoystickAction::Button { button_idx }))
            {
                for &button in buttons {
                    *self.inputs.get_field(button) = value;
                }
            }
        }
    }

    pub fn axis_motion(&mut self, instance_id: u32, axis_idx: u8, value: i16) {
        let negative_down = value <= -self.axis_deadzone;
        let positive_down = value >= self.axis_deadzone;

        if let Some(&device_id) = self.instance_id_to_device_id.get(&instance_id) {
            for (direction, value) in
                [(AxisDirection::Positive, positive_down), (AxisDirection::Negative, negative_down)]
            {
                if let Some(buttons) = self
                    .input_mapping
                    .get(&(device_id, JoystickAction::Axis { axis_idx, direction }))
                {
                    for &button in buttons {
                        *self.inputs.get_field(button) = value;
                    }
                }
            }
        }
    }

    pub fn hat_motion(&mut self, instance_id: u32, hat_idx: u8, state: HatState) {
        let up_pressed = matches!(state, HatState::LeftUp | HatState::Up | HatState::RightUp);
        let left_pressed = matches!(state, HatState::LeftUp | HatState::Left | HatState::LeftDown);
        let down_pressed =
            matches!(state, HatState::LeftDown | HatState::Down | HatState::RightDown);
        let right_pressed =
            matches!(state, HatState::RightUp | HatState::Right | HatState::RightDown);

        if let Some(&device_id) = self.instance_id_to_device_id.get(&instance_id) {
            for (direction, value) in [
                (HatDirection::Up, up_pressed),
                (HatDirection::Left, left_pressed),
                (HatDirection::Down, down_pressed),
                (HatDirection::Right, right_pressed),
            ] {
                if let Some(buttons) =
                    self.input_mapping.get(&(device_id, JoystickAction::Hat { hat_idx, direction }))
                {
                    for &button in buttons {
                        *self.inputs.get_field(button) = value;
                    }
                }
            }
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> anyhow::Result<()> {
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

    pub fn inputs(&self) -> &Inputs {
        &self.inputs
    }
}
