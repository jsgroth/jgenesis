use jgenesis_common::frontend::DisplayInfo;
use jgenesis_common::input::Player;
use jgenesis_native_config::input::{
    AxisDirection, GamepadAction, GenericInput, HatDirection, Hotkey, KeyboardInput,
};
use rustc_hash::{FxHashMap, FxHashSet};
use sdl3::event::{Event, WindowEvent};
use sdl3::joystick::{HatState, Joystick};
use sdl3::keyboard::{Keycode, Scancode};
use sdl3::sys::everything::SDL_JoystickID;
use sdl3::{IntegerOrSdlError, JoystickSubsystem};
use std::array;
use std::cell::RefCell;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CanonicalInput(GenericInput);

fn canonicalize_keycode(keycode: Keycode) -> Keycode {
    match keycode {
        Keycode::RShift => Keycode::LShift,
        Keycode::RCtrl => Keycode::LCtrl,
        Keycode::RAlt => Keycode::LAlt,
        _ => keycode,
    }
}

fn canonicalize_scancode(scancode: Scancode) -> Scancode {
    match scancode {
        Scancode::RShift => Scancode::LShift,
        Scancode::RCtrl => Scancode::LCtrl,
        Scancode::RAlt => Scancode::LAlt,
        _ => scancode,
    }
}

fn canonicalize_key(key: KeyboardInput) -> KeyboardInput {
    match key {
        KeyboardInput::Keycode(keycode) => KeyboardInput::Keycode(canonicalize_keycode(keycode)),
        KeyboardInput::Scancode(scancode) => {
            KeyboardInput::Scancode(canonicalize_scancode(scancode))
        }
    }
}

impl CanonicalInput {
    pub(crate) fn canonicalize(input: GenericInput) -> Self {
        match input {
            GenericInput::Keyboard(key) => Self(GenericInput::Keyboard(canonicalize_key(key))),
            _ => Self(input),
        }
    }

    pub(crate) fn reverse_canonicalize(self) -> Option<&'static [GenericInput]> {
        match self.0 {
            GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::LShift)) => Some(&[
                GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::LShift)),
                GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::RShift)),
            ]),
            GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::LCtrl)) => Some(&[
                GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::LCtrl)),
                GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::RCtrl)),
            ]),
            GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::LAlt)) => Some(&[
                GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::LAlt)),
                GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::RAlt)),
            ]),
            GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::LShift)) => Some(&[
                GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::LShift)),
                GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::RShift)),
            ]),
            GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::LCtrl)) => Some(&[
                GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::LCtrl)),
                GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::RCtrl)),
            ]),
            GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::LAlt)) => Some(&[
                GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::LAlt)),
                GenericInput::Keyboard(KeyboardInput::Scancode(Scancode::RAlt)),
            ]),
            _ => None,
        }
    }
}

pub const MAX_MAPPING_LEN: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericButton<Button> {
    Button(Button, Player),
    TurboButton(Button, Player),
    Hotkey(Hotkey),
}

#[derive(Debug, Clone, Copy)]
pub enum InputEvent<Button> {
    Button { button: Button, player: Player, pressed: bool },
    AnalogValueChange { button: Button, player: Player, value: i16 },
    MouseMotion { x: f32, y: f32, display_info: DisplayInfo },
    MouseLeave,
    Hotkey { hotkey: Hotkey, pressed: bool },
}

pub struct InitialAxisDirections(pub Vec<(u8, AxisDirection)>);

impl Deref for InitialAxisDirections {
    type Target = Vec<(u8, AxisDirection)>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Joysticks {
    subsystem: JoystickSubsystem,
    open_joysticks: Vec<(Joystick, InitialAxisDirections)>,
    joystick_id_to_device_id: FxHashMap<u32, u32>,
    device_id_to_idx: FxHashMap<u32, usize>,
}

impl Joysticks {
    #[must_use]
    pub fn new(subsystem: JoystickSubsystem) -> Self {
        Self {
            subsystem,
            open_joysticks: Vec::new(),
            joystick_id_to_device_id: FxHashMap::default(),
            device_id_to_idx: FxHashMap::default(),
        }
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn handle_device_added(&mut self, joystick_id: u32) -> Result<(), IntegerOrSdlError> {
        let joystick = self.subsystem.open(SDL_JoystickID(joystick_id))?;

        let name = joystick.name();

        let initial_axis_directions = read_initial_axis_directions(&joystick);

        self.open_joysticks.push((joystick, initial_axis_directions));
        self.regenerate_id_maps().map_err(IntegerOrSdlError::SdlError)?;

        if let Some(&device_id) = self.joystick_id_to_device_id.get(&joystick_id) {
            log::info!("Added joystick ID {joystick_id}: '{name}' (Device ID {device_id})");
        }

        Ok(())
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn handle_device_removed(&mut self, joystick_id: u32) -> Result<Option<u32>, sdl3::Error> {
        let device_id = self.joystick_id_to_device_id.get(&joystick_id).copied();

        self.open_joysticks.retain(|(joystick, _)| joystick.id() != joystick_id);
        self.regenerate_id_maps()?;

        if let Some(device_id) = device_id {
            log::info!("Removed joystick ID {joystick_id} (Device ID {device_id})");
        }

        Ok(device_id)
    }

    fn regenerate_id_maps(&mut self) -> Result<(), sdl3::Error> {
        // Clear maps before making joysticks() call that could potentially return an error
        self.joystick_id_to_device_id.clear();
        self.device_id_to_idx.clear();

        let joystick_ids = self.subsystem.joysticks()?;
        for (device_id, joystick_id) in joystick_ids.into_iter().enumerate() {
            let Some(idx) =
                self.open_joysticks.iter().position(|(joystick, _)| joystick.id() == joystick_id.0)
            else {
                continue;
            };

            self.joystick_id_to_device_id.insert(joystick_id.0, device_id as u32);
            self.device_id_to_idx.insert(device_id as u32, idx);
        }

        Ok(())
    }

    #[must_use]
    pub fn map_to_device_id(&mut self, instance_id: u32) -> Option<u32> {
        self.joystick_id_to_device_id.get(&instance_id).copied()
    }

    #[must_use]
    pub fn subsystem(&self) -> &JoystickSubsystem {
        &self.subsystem
    }

    #[must_use]
    pub fn device(&self, device_id: u32) -> Option<&Joystick> {
        self.device_id_to_idx.get(&device_id).map(|&idx| &self.open_joysticks[idx].0)
    }

    #[must_use]
    pub fn initial_axis_directions(
        &self,
        device_id: u32,
    ) -> Option<impl Iterator<Item = (u8, AxisDirection)>> {
        self.device_id_to_idx.get(&device_id).map(|&idx| self.open_joysticks[idx].1.iter().copied())
    }

    pub fn all_devices(
        &self,
    ) -> impl Iterator<Item = (u32, &'_ (Joystick, InitialAxisDirections))> + '_ {
        self.device_id_to_idx
            .iter()
            .map(|(&device_id, &idx)| (device_id, &self.open_joysticks[idx]))
    }
}

fn read_initial_axis_directions(joystick: &Joystick) -> InitialAxisDirections {
    InitialAxisDirections(
        (0..joystick.num_axes())
            .filter_map(|axis_id| {
                let value = joystick.axis(axis_id).ok()?;
                if value > 30000 {
                    Some((axis_id as u8, AxisDirection::Positive))
                } else if value < -30000 {
                    Some((axis_id as u8, AxisDirection::Negative))
                } else {
                    None
                }
            })
            .collect(),
    )
}

struct InputMapperState<Button> {
    input_events: Rc<RefCell<Vec<InputEvent<Button>>>>,
    mappings: FxHashMap<GenericButton<Button>, Vec<Vec<CanonicalInput>>>,
    inputs_to_buttons: FxHashMap<CanonicalInput, Vec<GenericButton<Button>>>,
    active_inputs: FxHashSet<GenericInput>,
    active_canonical_inputs: FxHashSet<CanonicalInput>,
    active_turbo_buttons: FxHashMap<(Button, Player), bool>,
    active_hotkeys: FxHashSet<Hotkey>,
    changed_button_buffers: [Vec<GenericButton<Button>>; MAX_MAPPING_LEN + 1],
}

impl<Button> InputMapperState<Button>
where
    Button: Debug + Copy + Hash + Eq,
{
    fn new() -> Self {
        Self {
            input_events: Rc::new(RefCell::new(Vec::with_capacity(10))),
            mappings: FxHashMap::default(),
            inputs_to_buttons: FxHashMap::default(),
            active_inputs: FxHashSet::default(),
            active_canonical_inputs: FxHashSet::default(),
            active_turbo_buttons: FxHashMap::default(),
            active_hotkeys: FxHashSet::default(),
            changed_button_buffers: array::from_fn(|_| Vec::with_capacity(10)),
        }
    }

    fn update_mappings(
        &mut self,
        button_mappings: &[((Button, Player), &Vec<GenericInput>)],
        turbo_mappings: &[((Button, Player), &Vec<GenericInput>)],
        hotkey_mappings: &[(Hotkey, &Vec<GenericInput>)],
    ) {
        self.mappings.clear();
        self.inputs_to_buttons.clear();
        self.active_inputs.clear();
        self.active_canonical_inputs.clear();
        self.active_turbo_buttons.clear();
        self.active_hotkeys.clear();

        for (mappings, turbo) in [(button_mappings, false), (turbo_mappings, true)] {
            for &((button, player), mapping) in mappings {
                if mapping.len() > MAX_MAPPING_LEN {
                    log::error!("Ignoring mapping, too many inputs: {mapping:?}");
                    continue;
                }

                let generic_button = if turbo {
                    GenericButton::TurboButton(button, player)
                } else {
                    GenericButton::Button(button, player)
                };
                self.mappings
                    .entry(generic_button)
                    .or_default()
                    .push(mapping.iter().copied().map(CanonicalInput::canonicalize).collect());

                for &mapping_input in mapping {
                    self.inputs_to_buttons
                        .entry(CanonicalInput::canonicalize(mapping_input))
                        .or_default()
                        .push(generic_button);
                }
            }
        }

        for &(hotkey, mapping) in hotkey_mappings {
            if mapping.len() > MAX_MAPPING_LEN {
                log::error!("Ignoring mapping, too many inputs: {mapping:?}");
                continue;
            }

            let generic_button = GenericButton::Hotkey(hotkey);
            self.mappings
                .entry(generic_button)
                .or_default()
                .push(mapping.iter().copied().map(CanonicalInput::canonicalize).collect());

            for &mapping_input in mapping {
                self.inputs_to_buttons
                    .entry(CanonicalInput::canonicalize(mapping_input))
                    .or_default()
                    .push(generic_button);
            }
        }
    }

    fn handle_input(&mut self, raw_input: GenericInput, pressed: bool) {
        if pressed && !self.active_inputs.insert(raw_input) {
            // Input is already pressed
            return;
        } else if !pressed && !self.active_inputs.remove(&raw_input) {
            // Input is already released
            return;
        }

        let input = CanonicalInput::canonicalize(raw_input);
        if let Some(raw_inputs) = input.reverse_canonicalize() {
            for &other_raw_input in raw_inputs {
                if other_raw_input == raw_input {
                    continue;
                }

                if self.active_inputs.contains(&other_raw_input) {
                    // Mapping will not change as a result of this press/release
                    return;
                }
            }
        }

        if pressed {
            self.active_canonical_inputs.insert(input);
        } else {
            self.active_canonical_inputs.remove(&input);
        }

        self.handle_canonical_input(input, pressed);
    }

    fn handle_canonical_input(&mut self, input: CanonicalInput, pressed: bool) {
        let Some(buttons) = self.inputs_to_buttons.get(&input) else { return };

        log::debug!("Input {input:?}, pressed={pressed}, buttons={buttons:?}");

        for buffer in &mut self.changed_button_buffers {
            buffer.clear();
        }

        for &button in buttons {
            let Some(mappings) = self.mappings.get(&button) else { continue };

            log::debug!("Mappings: {mappings:?}");

            let other_mappings_pressed = mappings.iter().any(|mapping| {
                mapping.iter().all(|&mapping_input| {
                    mapping_input != input && self.active_canonical_inputs.contains(&mapping_input)
                })
            });
            if other_mappings_pressed {
                // Mappings that don't contain this input are still pressed; button state will not change
                continue;
            }

            for mapping in mappings {
                let mut contains_new_input = false;
                let mut all_others_pressed = true;
                for &mapping_input in mapping {
                    contains_new_input |= mapping_input == input;
                    all_others_pressed &= mapping_input == input
                        || self.active_canonical_inputs.contains(&mapping_input);
                }

                if contains_new_input && all_others_pressed {
                    // This was the only input in the mapping that was not already pressed, so
                    // button state will change
                    self.changed_button_buffers[mapping.len()].push(button);
                }
            }
        }

        // Iterate in reverse mapping length order
        for changed_buttons in self.changed_button_buffers.iter().rev() {
            if changed_buttons.is_empty() {
                continue;
            }

            for &button in changed_buttons {
                log::debug!("Button state changed! button={button:?} pressed={pressed}");

                match button {
                    GenericButton::Button(button, player) => {
                        self.input_events.borrow_mut().push(InputEvent::Button {
                            button,
                            player,
                            pressed,
                        });
                    }
                    GenericButton::TurboButton(button, player) => {
                        if pressed {
                            self.active_turbo_buttons.insert((button, player), true);
                        } else {
                            self.active_turbo_buttons.remove(&(button, player));
                        }
                        self.input_events.borrow_mut().push(InputEvent::Button {
                            button,
                            player,
                            pressed,
                        });
                    }
                    GenericButton::Hotkey(hotkey) => {
                        if pressed && self.active_hotkeys.insert(hotkey) {
                            self.input_events
                                .borrow_mut()
                                .push(InputEvent::Hotkey { hotkey, pressed: true });
                        } else if !pressed && self.active_hotkeys.remove(&hotkey) {
                            self.input_events
                                .borrow_mut()
                                .push(InputEvent::Hotkey { hotkey, pressed: false });
                        }
                    }
                }
            }

            if pressed {
                // On input presses, only count a button/hotkey as pressed if the combination length
                // is the maximum out of all combinations that changed state.
                //
                // This is to handle cases like e.g. Shift+F1 and F1 being mapped to different
                // hotkeys, where only the Shift+F1 mapping should change state when Shift is pressed
                // first and F1 is pressed second.
                //
                // There is probably a more robust way to do this - maybe making the input order significant?
                break;
            }
        }
    }

    fn toggle_turbo_states(&mut self) {
        for (&(button, player), pressed) in &mut self.active_turbo_buttons {
            self.input_events.borrow_mut().push(InputEvent::Button {
                button,
                player,
                pressed: *pressed,
            });

            *pressed = !*pressed;
        }
    }

    fn unset_all_gamepad_inputs(&mut self) {
        // Allocation to avoid borrow checker issues is fine, this won't be called frequently
        let gamepad_inputs: Vec<_> = self.inputs_to_buttons.keys().copied().collect();

        for input in gamepad_inputs {
            self.handle_input(input.0, false);
        }
    }
}

pub(crate) struct InputMapper<Button> {
    joysticks: Joysticks,
    axis_deadzone: i16,
    state: InputMapperState<Button>,
}

impl<Button> InputMapper<Button>
where
    Button: Debug + Copy + Hash + Eq,
{
    pub fn new(
        joystick_subsystem: JoystickSubsystem,
        axis_deadzone: i16,
        button_mappings: &[((Button, Player), &Vec<GenericInput>)],
        turbo_mappings: &[((Button, Player), &Vec<GenericInput>)],
        hotkey_mappings: &[(Hotkey, &Vec<GenericInput>)],
    ) -> Self {
        let joysticks = Joysticks::new(joystick_subsystem);

        let mut state = InputMapperState::new();
        state.update_mappings(button_mappings, turbo_mappings, hotkey_mappings);

        Self { joysticks, axis_deadzone, state }
    }

    pub fn update_mappings(
        &mut self,
        axis_deadzone: i16,
        button_mappings: &[((Button, Player), &Vec<GenericInput>)],
        turbo_mappings: &[((Button, Player), &Vec<GenericInput>)],
        hotkey_mappings: &[(Hotkey, &Vec<GenericInput>)],
    ) {
        self.axis_deadzone = axis_deadzone;
        self.state.update_mappings(button_mappings, turbo_mappings, hotkey_mappings);
    }

    pub fn handle_event(
        &mut self,
        event: &Event,
        emulator_window_id: u32,
        display_info: Option<DisplayInfo>,
    ) {
        log::debug!("SDL event: {event:?}");

        match *event {
            Event::KeyDown { keycode, scancode, window_id, .. }
                if window_id == emulator_window_id =>
            {
                if let Some(keycode) = keycode {
                    self.state.handle_input(
                        GenericInput::Keyboard(KeyboardInput::Keycode(keycode)),
                        true,
                    );
                }

                if let Some(scancode) = scancode {
                    self.state.handle_input(
                        GenericInput::Keyboard(KeyboardInput::Scancode(scancode)),
                        true,
                    );
                }
            }
            Event::KeyUp { keycode, scancode, window_id, .. }
                if window_id == emulator_window_id =>
            {
                if let Some(keycode) = keycode {
                    self.state.handle_input(
                        GenericInput::Keyboard(KeyboardInput::Keycode(keycode)),
                        false,
                    );
                }

                if let Some(scancode) = scancode {
                    self.state.handle_input(
                        GenericInput::Keyboard(KeyboardInput::Scancode(scancode)),
                        false,
                    );
                }
            }
            Event::MouseButtonDown { mouse_btn, window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.state.handle_input(GenericInput::Mouse(mouse_btn), true);
            }
            Event::MouseButtonUp { mouse_btn, window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.state.handle_input(GenericInput::Mouse(mouse_btn), false);
            }
            Event::MouseMotion { x, y, window_id, .. } if window_id == emulator_window_id => {
                if let Some(display_info) = display_info {
                    self.state.input_events.borrow_mut().push(InputEvent::MouseMotion {
                        x,
                        y,
                        display_info,
                    });
                }
            }
            Event::Window { win_event: WindowEvent::MouseLeave, window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.state.input_events.borrow_mut().push(InputEvent::MouseLeave);
            }
            Event::JoyButtonDown { which, button_idx, .. } => {
                let Some(gamepad_idx) = self.joysticks.map_to_device_id(which) else { return };
                self.state.handle_input(
                    GenericInput::Gamepad {
                        gamepad_idx,
                        action: GamepadAction::Button(button_idx),
                    },
                    true,
                );
            }
            Event::JoyButtonUp { which, button_idx, .. } => {
                let Some(gamepad_idx) = self.joysticks.map_to_device_id(which) else { return };
                self.state.handle_input(
                    GenericInput::Gamepad {
                        gamepad_idx,
                        action: GamepadAction::Button(button_idx),
                    },
                    false,
                );
            }
            Event::JoyAxisMotion { which, axis_idx, value, .. } => {
                let Some(gamepad_idx) = self.joysticks.map_to_device_id(which) else { return };
                self.handle_axis_input(gamepad_idx, axis_idx, value);
            }
            Event::JoyHatMotion { which, hat_idx, state, .. } => {
                let Some(gamepad_idx) = self.joysticks.map_to_device_id(which) else { return };
                self.handle_hat_input(gamepad_idx, hat_idx, state);
            }
            Event::JoyDeviceAdded { which, .. } => {
                if let Err(err) = self.joysticks.handle_device_added(which) {
                    log::error!("Error opening joystick with joystick id {which}: {err}");
                }
                self.state.unset_all_gamepad_inputs();
            }
            Event::JoyDeviceRemoved { which, .. } => {
                if let Err(err) = self.joysticks.handle_device_removed(which) {
                    log::error!("Error closing joystick with joystick id {which}: {err}");
                }
                self.state.unset_all_gamepad_inputs();
            }
            _ => {}
        }
    }

    fn handle_axis_input(&mut self, gamepad_idx: u32, axis_idx: u8, value: i16) {
        let magnitude = value.saturating_abs();
        let pressed = magnitude > self.axis_deadzone;
        let pressed_direction = AxisDirection::from_value(value);

        if pressed {
            self.state.handle_input(
                GenericInput::Gamepad {
                    gamepad_idx,
                    action: GamepadAction::Axis(axis_idx, pressed_direction.inverse()),
                },
                false,
            );
            self.state.handle_input(
                GenericInput::Gamepad {
                    gamepad_idx,
                    action: GamepadAction::Axis(axis_idx, pressed_direction),
                },
                true,
            );
        } else {
            for direction in [AxisDirection::Positive, AxisDirection::Negative] {
                self.state.handle_input(
                    GenericInput::Gamepad {
                        gamepad_idx,
                        action: GamepadAction::Axis(axis_idx, direction),
                    },
                    false,
                );
            }
        }

        // When a gamepad axis input is mapped to an emulated analog input, ensure that the emulator
        // receives analog values instead of digital pressed vs. not pressed by pushing the analog
        // value change events _after_ checking for a digital pressed change. This way the emulator's
        // input mapping code will always see the analog change events last without needing to
        // special case digital vs. analog inputs.
        //
        // For a similar reason, push an event for the inverse axis direction first in case both
        // gamepad axis directions are mapped to the same emulated analog axis.
        // TODO deadzone
        for direction in [pressed_direction.inverse(), pressed_direction] {
            let canonical_input = CanonicalInput::canonicalize(GenericInput::Gamepad {
                gamepad_idx,
                action: GamepadAction::Axis(axis_idx, direction),
            });
            let Some(buttons) = self.state.inputs_to_buttons.get(&canonical_input) else {
                continue;
            };

            let direction_magnitude = if direction == pressed_direction { magnitude } else { 0 };

            for &button in buttons {
                let GenericButton::Button(button, player) = button else { continue };

                self.state.input_events.borrow_mut().push(InputEvent::AnalogValueChange {
                    button,
                    player,
                    value: direction_magnitude,
                });
            }
        }
    }

    fn handle_hat_input(&mut self, gamepad_idx: u32, hat_idx: u8, state: HatState) {
        for direction in HatDirection::ALL {
            let pressed = is_hat_direction_pressed(direction, state);
            self.state.handle_input(
                GenericInput::Gamepad {
                    gamepad_idx,
                    action: GamepadAction::Hat(hat_idx, direction),
                },
                pressed,
            );
        }
    }

    pub fn frame_complete(&mut self) {
        self.state.toggle_turbo_states();
    }

    #[must_use]
    pub fn input_events(&self) -> Rc<RefCell<Vec<InputEvent<Button>>>> {
        Rc::clone(&self.state.input_events)
    }
}

impl<Button> InputMapper<Button> {
    pub fn joysticks_mut(&mut self) -> &mut Joysticks {
        &mut self.joysticks
    }
}

fn is_hat_direction_pressed(direction: HatDirection, state: HatState) -> bool {
    use HatDirection as HD;
    use HatState as HS;

    match direction {
        HD::Up => matches!(state, HS::Up | HS::LeftUp | HS::RightUp),
        HD::Left => matches!(state, HS::Left | HS::LeftUp | HS::LeftDown),
        HD::Right => matches!(state, HS::Right | HS::RightUp | HS::RightDown),
        HD::Down => matches!(state, HS::Down | HS::LeftDown | HS::RightDown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jgenesis_common::frontend::MappableInputs;
    use smsgg_config::{SmsGgButton, SmsGgInputs};
    use std::marker::PhantomData;

    struct TestState<B, I: MappableInputs<B>> {
        inputs: I,
        hotkeys: FxHashSet<Hotkey>,
        _marker: PhantomData<B>,
    }

    impl<B, I> TestState<B, I>
    where
        B: Debug + Copy + Eq + Hash,
        I: Default + MappableInputs<B>,
    {
        fn new() -> Self {
            Self { inputs: I::default(), hotkeys: FxHashSet::default(), _marker: PhantomData }
        }

        fn handle_input(
            &mut self,
            state: &mut InputMapperState<B>,
            input: GenericInput,
            pressed: bool,
        ) {
            state.handle_input(input, pressed);
            take_events(&mut self.inputs, &mut self.hotkeys, state);
        }
    }

    fn new_smsgg_state() -> TestState<SmsGgButton, SmsGgInputs> {
        TestState::new()
    }

    fn take_events<I, B>(
        inputs: &mut I,
        hotkeys: &mut FxHashSet<Hotkey>,
        state: &mut InputMapperState<B>,
    ) where
        I: MappableInputs<B>,
    {
        for event in state.input_events.borrow_mut().drain(..) {
            match event {
                InputEvent::Button { button, player, pressed } => {
                    inputs.set_field(button, player, pressed);
                }
                InputEvent::Hotkey { hotkey, pressed } => {
                    if pressed {
                        hotkeys.insert(hotkey);
                    } else {
                        hotkeys.remove(&hotkey);
                    }
                }
                _ => {}
            }
        }
    }

    fn into_hash_set<H: Eq + Hash>(iter: impl IntoIterator<Item = H>) -> FxHashSet<H> {
        iter.into_iter().collect()
    }

    macro_rules! key_input {
        ($keycode:ident) => {
            GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::$keycode))
        };
    }

    #[test]
    fn basic_mapping() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![key_input!(F)]),
                ((SmsGgButton::Button1, Player::Two), &vec![key_input!(G)]),
                ((SmsGgButton::Button2, Player::One), &vec![key_input!(Up)]),
            ],
            &[],
            &[(Hotkey::FastForward, &vec![key_input!(H)])],
        );

        let mut state = new_smsgg_state();
        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, key_input!(F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, key_input!(G), true);
        expected.p2.button1 = true;
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, key_input!(F), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, key_input!(H), true);
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, into_hash_set([Hotkey::FastForward]));

        state.handle_input(&mut input_state, key_input!(H), false);
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());
    }

    #[test]
    fn one_mapping_button_and_hotkey() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[((SmsGgButton::Button1, Player::One), &vec![key_input!(F)])],
            &[],
            &[(Hotkey::SaveState, &vec![key_input!(F)])],
        );

        let mut state = new_smsgg_state();

        let mut expected_inputs = SmsGgInputs::default();
        let mut expected_hotkeys: FxHashSet<Hotkey> = FxHashSet::default();
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, state.hotkeys);

        state.handle_input(&mut input_state, key_input!(F), true);
        expected_inputs.p1.button1 = true;
        expected_hotkeys.insert(Hotkey::SaveState);
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, state.hotkeys);

        state.handle_input(&mut input_state, key_input!(F), false);
        expected_inputs.p1.button1 = false;
        expected_hotkeys.remove(&Hotkey::SaveState);
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, state.hotkeys);
    }

    #[test]
    fn two_mappings_same_button() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![key_input!(F)]),
                ((SmsGgButton::Button1, Player::One), &vec![key_input!(G)]),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "one mapping pressed");

        state.handle_input(&mut input_state, key_input!(G), true);
        assert_eq!(expected, state.inputs, "two mappings pressed");

        state.handle_input(&mut input_state, key_input!(G), false);
        assert_eq!(expected, state.inputs, "one mapping released, one still pressed");

        state.handle_input(&mut input_state, key_input!(F), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "both mappings released");
    }

    #[test]
    fn one_mapping_three_buttons() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![key_input!(F)]),
                ((SmsGgButton::Button2, Player::One), &vec![key_input!(F)]),
                ((SmsGgButton::Pause, Player::One), &vec![key_input!(F)]),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(F), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        expected.pause = true;
        assert_eq!(expected, state.inputs, "mapping pressed");

        state.handle_input(&mut input_state, key_input!(F), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        expected.pause = false;
        assert_eq!(expected, state.inputs, "mapping released");
    }

    #[test]
    fn combination_mapping() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[(
                (SmsGgButton::Button1, Player::One),
                &vec![key_input!(F), key_input!(G), key_input!(H)],
            )],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(F), true);
        assert_eq!(expected, state.inputs, "1/3 pressed (1)");

        state.handle_input(&mut input_state, key_input!(H), true);
        assert_eq!(expected, state.inputs, "2/3 pressed (2)");

        state.handle_input(&mut input_state, key_input!(F), false);
        assert_eq!(expected, state.inputs, "1/3 pressed (3)");

        state.handle_input(&mut input_state, key_input!(G), true);
        assert_eq!(expected, state.inputs, "2/3 pressed (4)");

        state.handle_input(&mut input_state, key_input!(F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "3/3 pressed (5)");

        state.handle_input(&mut input_state, key_input!(H), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "2/3 pressed (6)");

        state.handle_input(&mut input_state, key_input!(H), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "3/3 pressed (7)");

        state.handle_input(&mut input_state, key_input!(G), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "2/3 pressed (8)");
    }

    #[test]
    fn combination_length_priority_basic() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[],
            &[],
            &[
                (Hotkey::SaveState, &vec![key_input!(LShift), key_input!(F1)]),
                (Hotkey::LoadState, &vec![key_input!(F1)]),
            ],
        );

        let mut state = new_smsgg_state();

        let mut expected: FxHashSet<Hotkey> = FxHashSet::default();
        assert_eq!(expected, state.hotkeys);

        state.handle_input(&mut input_state, key_input!(F1), true);
        expected.insert(Hotkey::LoadState);
        assert_eq!(expected, state.hotkeys, "single key pressed");

        state.handle_input(&mut input_state, key_input!(F1), false);
        expected.remove(&Hotkey::LoadState);
        assert_eq!(expected, state.hotkeys, "single key pressed & released");

        state.handle_input(&mut input_state, key_input!(LShift), true);
        assert_eq!(expected, state.hotkeys, "1/2 pressed");

        state.handle_input(&mut input_state, key_input!(F1), true);
        expected.insert(Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "2/2 pressed");

        state.handle_input(&mut input_state, key_input!(F1), false);
        expected.remove(&Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "1/2 released");

        state.handle_input(&mut input_state, key_input!(LShift), false);
        assert_eq!(expected, state.hotkeys, "2/2 released");
    }

    #[test]
    fn combination_length_priority_weird() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[],
            &[],
            &[
                (Hotkey::SaveState, &vec![key_input!(LShift), key_input!(F1)]),
                (Hotkey::LoadState, &vec![key_input!(F1)]),
            ],
        );

        let mut state = new_smsgg_state();

        let mut expected: FxHashSet<Hotkey> = FxHashSet::default();
        assert_eq!(expected, state.hotkeys);

        state.handle_input(&mut input_state, key_input!(F1), true);
        expected.insert(Hotkey::LoadState);
        assert_eq!(expected, state.hotkeys, "single key pressed");

        state.handle_input(&mut input_state, key_input!(LShift), true);
        expected.insert(Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "combination secondary key pressed");

        state.handle_input(&mut input_state, key_input!(F1), false);
        expected.remove(&Hotkey::LoadState);
        expected.remove(&Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "single key + combination released");

        state.handle_input(&mut input_state, key_input!(F1), true);
        expected.insert(Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "combination pressed second time");

        state.handle_input(&mut input_state, key_input!(F1), false);
        expected.remove(&Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "combination released second time");

        state.handle_input(&mut input_state, key_input!(LShift), false);
        assert_eq!(expected, state.hotkeys, "combination secondary key released");
    }

    #[test]
    fn shift_canonicalization_basic() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![key_input!(RShift)]),
                ((SmsGgButton::Button2, Player::One), &vec![key_input!(LShift)]),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(LShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs, "Pressing LShift should trigger both Shift mappings");

        state.handle_input(&mut input_state, key_input!(LShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(expected, state.inputs, "Releasing LShift should trigger both Shift mappings");

        state.handle_input(&mut input_state, key_input!(RShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs, "Pressing RShift should trigger both Shift mappings");

        state.handle_input(&mut input_state, key_input!(RShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(expected, state.inputs, "Releasing RShift should trigger both Shift mappings");
    }

    #[test]
    fn shift_canonicalization_simultaneous() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![key_input!(RShift)]),
                ((SmsGgButton::Button2, Player::One), &vec![key_input!(LShift)]),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(LShift), true);
        state.handle_input(&mut input_state, key_input!(RShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(RShift), false);
        assert_eq!(
            expected, state.inputs,
            "Releasing RShift while LShift is held should not change mapping"
        );

        state.handle_input(&mut input_state, key_input!(RShift), true);
        assert_eq!(
            expected, state.inputs,
            "Pressing RShift while LShift is held should not change mapping"
        );

        state.handle_input(&mut input_state, key_input!(LShift), false);
        assert_eq!(
            expected, state.inputs,
            "Releasing LShift while RShift is held should not change mapping"
        );

        state.handle_input(&mut input_state, key_input!(RShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(
            expected, state.inputs,
            "Releasing RShift while LShift is not held should change mapping"
        );
    }

    #[test]
    fn turbo() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[],
            &[((SmsGgButton::Button1, Player::One), &vec![key_input!(D)])],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(D), false);
        assert_eq!(expected, state.inputs);
        input_state.toggle_turbo_states();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, key_input!(D), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs);

        // First toggle won't change state
        // This is intentional - the pressed state should immediately change to true once the turbo
        // mapping is pressed, then it should remain true until the second "frame complete" event
        input_state.toggle_turbo_states();
        take_events(&mut state.inputs, &mut state.hotkeys, &mut input_state);
        assert_eq!(expected, state.inputs);

        for _ in 0..50 {
            input_state.toggle_turbo_states();
            take_events(&mut state.inputs, &mut state.hotkeys, &mut input_state);
            expected.p1.button1 = !expected.p1.button1;
            assert_eq!(expected, state.inputs);
        }

        assert_eq!(state.inputs.p1.button1, true);
        state.handle_input(&mut input_state, key_input!(D), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs);

        for _ in 0..51 {
            input_state.toggle_turbo_states();
            take_events(&mut state.inputs, &mut state.hotkeys, &mut input_state);
            assert_eq!(expected, state.inputs);
        }
    }
}
