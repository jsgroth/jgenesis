use arrayvec::ArrayVec;
use jgenesis_common::frontend::{DisplayArea, FrameSize};
use jgenesis_common::input::Player;
use jgenesis_native_config::input::{
    AxisDirection, GamepadAction, GenericInput, HatDirection, Hotkey,
};
use rustc_hash::{FxHashMap, FxHashSet};
use sdl3::event::{Event, WindowEvent};
use sdl3::joystick::{HatState, Joystick};
use sdl3::keyboard::Keycode;
use sdl3::{IntegerOrSdlError, JoystickSubsystem};
use std::array;
use std::cell::RefCell;
use std::fmt::Debug;
use std::hash::Hash;
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

impl CanonicalInput {
    pub(crate) fn canonicalize(input: GenericInput) -> Self {
        match input {
            GenericInput::Keyboard(keycode) => {
                Self(GenericInput::Keyboard(canonicalize_keycode(keycode)))
            }
            _ => Self(input),
        }
    }

    pub(crate) fn reverse_canonicalize(self) -> Option<&'static [GenericInput]> {
        match self.0 {
            GenericInput::Keyboard(Keycode::LShift) => Some(&[
                GenericInput::Keyboard(Keycode::LShift),
                GenericInput::Keyboard(Keycode::RShift),
            ]),
            GenericInput::Keyboard(Keycode::LCtrl) => Some(&[
                GenericInput::Keyboard(Keycode::LCtrl),
                GenericInput::Keyboard(Keycode::RCtrl),
            ]),
            GenericInput::Keyboard(Keycode::LAlt) => Some(&[
                GenericInput::Keyboard(Keycode::LAlt),
                GenericInput::Keyboard(Keycode::RAlt),
            ]),
            _ => None,
        }
    }
}

pub const MAX_MAPPING_LEN: usize = 3;
type MappingArrayVec = ArrayVec<CanonicalInput, MAX_MAPPING_LEN>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericButton<Button> {
    Button(Button, Player),
    TurboButton(Button, Player),
    Hotkey(Hotkey),
}

#[derive(Debug, Clone, Copy)]
pub enum InputEvent<Button> {
    Button { button: Button, player: Player, pressed: bool },
    MouseMotion { x: f32, y: f32, frame_size: FrameSize, display_area: DisplayArea },
    MouseLeave,
    Hotkey { hotkey: Hotkey, pressed: bool },
}

pub struct Joysticks {
    subsystem: JoystickSubsystem,
    open_joysticks: Vec<Joystick>,
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
        let joystick = self.subsystem.open(joystick_id)?;

        let name = joystick.name();

        self.open_joysticks.push(joystick);
        self.regenerate_id_maps().map_err(IntegerOrSdlError::SdlError)?;

        if let Some(&device_id) = self.joystick_id_to_device_id.get(&joystick_id) {
            log::info!("Added joystick ID {joystick_id}: '{name}' (Device ID {device_id})");
        }

        Ok(())
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn handle_device_removed(&mut self, joystick_id: u32) -> Result<Option<u32>, sdl3::Error> {
        let device_id = self.joystick_id_to_device_id.get(&joystick_id).copied();

        self.open_joysticks.retain(|joystick| joystick.id() != joystick_id);
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
                self.open_joysticks.iter().position(|joystick| joystick.id() == joystick_id)
            else {
                continue;
            };

            self.joystick_id_to_device_id.insert(joystick_id, device_id as u32);
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
        self.device_id_to_idx.get(&device_id).map(|&idx| &self.open_joysticks[idx])
    }

    pub fn all_devices(&self) -> impl Iterator<Item = (u32, &'_ Joystick)> + '_ {
        self.device_id_to_idx
            .iter()
            .map(|(&device_id, &idx)| (device_id, &self.open_joysticks[idx]))
    }
}

struct InputMapperState<Button> {
    input_events: Rc<RefCell<Vec<InputEvent<Button>>>>,
    mappings: FxHashMap<GenericButton<Button>, Vec<MappingArrayVec>>,
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

pub struct InputMapper<Button> {
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
        display_info: Option<(FrameSize, DisplayArea)>,
    ) {
        log::debug!("SDL event: {event:?}");

        match *event {
            Event::KeyDown { keycode: Some(keycode), window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.state.handle_input(GenericInput::Keyboard(keycode), true);
            }
            Event::KeyUp { keycode: Some(keycode), window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.state.handle_input(GenericInput::Keyboard(keycode), false);
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
                if let Some((frame_size, display_area)) = display_info {
                    self.state.input_events.borrow_mut().push(InputEvent::MouseMotion {
                        x,
                        y,
                        frame_size,
                        display_area,
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
        let pressed = value.saturating_abs() > self.axis_deadzone;

        if pressed {
            let direction = AxisDirection::from_value(value);
            self.state.handle_input(
                GenericInput::Gamepad {
                    gamepad_idx,
                    action: GamepadAction::Axis(axis_idx, direction),
                },
                true,
            );
            self.state.handle_input(
                GenericInput::Gamepad {
                    gamepad_idx,
                    action: GamepadAction::Axis(axis_idx, direction.inverse()),
                },
                false,
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

    #[test]
    fn basic_mapping() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Button1, Player::Two), &vec![GenericInput::Keyboard(Keycode::G)]),
                ((SmsGgButton::Button2, Player::One), &vec![GenericInput::Keyboard(Keycode::Up)]),
            ],
            &[],
            &[(Hotkey::FastForward, &vec![GenericInput::Keyboard(Keycode::H)])],
        );

        let mut state = new_smsgg_state();
        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::G), true);
        expected.p2.button1 = true;
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::H), true);
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, into_hash_set([Hotkey::FastForward]));

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::H), false);
        assert_eq!(expected, state.inputs);
        assert_eq!(state.hotkeys, FxHashSet::default());
    }

    #[test]
    fn one_mapping_button_and_hotkey() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)])],
            &[],
            &[(Hotkey::SaveState, &vec![GenericInput::Keyboard(Keycode::F)])],
        );

        let mut state = new_smsgg_state();

        let mut expected_inputs = SmsGgInputs::default();
        let mut expected_hotkeys: FxHashSet<Hotkey> = FxHashSet::default();
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, state.hotkeys);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), true);
        expected_inputs.p1.button1 = true;
        expected_hotkeys.insert(Hotkey::SaveState);
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, state.hotkeys);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), false);
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
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::G)]),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "one mapping pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::G), true);
        assert_eq!(expected, state.inputs, "two mappings pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::G), false);
        assert_eq!(expected, state.inputs, "one mapping released, one still pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "both mappings released");
    }

    #[test]
    fn one_mapping_three_buttons() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Button2, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Pause, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        expected.pause = true;
        assert_eq!(expected, state.inputs, "mapping pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), false);
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
                &vec![
                    GenericInput::Keyboard(Keycode::F),
                    GenericInput::Keyboard(Keycode::G),
                    GenericInput::Keyboard(Keycode::H),
                ],
            )],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), true);
        assert_eq!(expected, state.inputs, "1/3 pressed (1)");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::H), true);
        assert_eq!(expected, state.inputs, "2/3 pressed (2)");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), false);
        assert_eq!(expected, state.inputs, "1/3 pressed (3)");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::G), true);
        assert_eq!(expected, state.inputs, "2/3 pressed (4)");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "3/3 pressed (5)");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::H), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "2/3 pressed (6)");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::H), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "3/3 pressed (7)");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::G), false);
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
                (
                    Hotkey::SaveState,
                    &vec![
                        GenericInput::Keyboard(Keycode::LShift),
                        GenericInput::Keyboard(Keycode::F1),
                    ],
                ),
                (Hotkey::LoadState, &vec![GenericInput::Keyboard(Keycode::F1)]),
            ],
        );

        let mut state = new_smsgg_state();

        let mut expected: FxHashSet<Hotkey> = FxHashSet::default();
        assert_eq!(expected, state.hotkeys);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), true);
        expected.insert(Hotkey::LoadState);
        assert_eq!(expected, state.hotkeys, "single key pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), false);
        expected.remove(&Hotkey::LoadState);
        assert_eq!(expected, state.hotkeys, "single key pressed & released");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), true);
        assert_eq!(expected, state.hotkeys, "1/2 pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), true);
        expected.insert(Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "2/2 pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), false);
        expected.remove(&Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "1/2 released");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), false);
        assert_eq!(expected, state.hotkeys, "2/2 released");
    }

    #[test]
    fn combination_length_priority_weird() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[],
            &[],
            &[
                (
                    Hotkey::SaveState,
                    &vec![
                        GenericInput::Keyboard(Keycode::LShift),
                        GenericInput::Keyboard(Keycode::F1),
                    ],
                ),
                (Hotkey::LoadState, &vec![GenericInput::Keyboard(Keycode::F1)]),
            ],
        );

        let mut state = new_smsgg_state();

        let mut expected: FxHashSet<Hotkey> = FxHashSet::default();
        assert_eq!(expected, state.hotkeys);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), true);
        expected.insert(Hotkey::LoadState);
        assert_eq!(expected, state.hotkeys, "single key pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), true);
        expected.insert(Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "combination secondary key pressed");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), false);
        expected.remove(&Hotkey::LoadState);
        expected.remove(&Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "single key + combination released");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), true);
        expected.insert(Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "combination pressed second time");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::F1), false);
        expected.remove(&Hotkey::SaveState);
        assert_eq!(expected, state.hotkeys, "combination released second time");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), false);
        assert_eq!(expected, state.hotkeys, "combination secondary key released");
    }

    #[test]
    fn shift_canonicalization_basic() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                (
                    (SmsGgButton::Button1, Player::One),
                    &vec![GenericInput::Keyboard(Keycode::RShift)],
                ),
                (
                    (SmsGgButton::Button2, Player::One),
                    &vec![GenericInput::Keyboard(Keycode::LShift)],
                ),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs, "Pressing LShift should trigger both Shift mappings");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(expected, state.inputs, "Releasing LShift should trigger both Shift mappings");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::RShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs, "Pressing RShift should trigger both Shift mappings");

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::RShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(expected, state.inputs, "Releasing RShift should trigger both Shift mappings");
    }

    #[test]
    fn shift_canonicalization_simultaneous() {
        let mut input_state = InputMapperState::new();
        input_state.update_mappings(
            &[
                (
                    (SmsGgButton::Button1, Player::One),
                    &vec![GenericInput::Keyboard(Keycode::RShift)],
                ),
                (
                    (SmsGgButton::Button2, Player::One),
                    &vec![GenericInput::Keyboard(Keycode::LShift)],
                ),
            ],
            &[],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), true);
        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::RShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::RShift), false);
        assert_eq!(
            expected, state.inputs,
            "Releasing RShift while LShift is held should not change mapping"
        );

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::RShift), true);
        assert_eq!(
            expected, state.inputs,
            "Pressing RShift while LShift is held should not change mapping"
        );

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::LShift), false);
        assert_eq!(
            expected, state.inputs,
            "Releasing LShift while RShift is held should not change mapping"
        );

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::RShift), false);
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
            &[((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::D)])],
            &[],
        );

        let mut state = new_smsgg_state();

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::D), false);
        assert_eq!(expected, state.inputs);
        input_state.toggle_turbo_states();
        assert_eq!(expected, state.inputs);

        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::D), true);
        assert_eq!(expected, state.inputs);

        for _ in 0..51 {
            input_state.toggle_turbo_states();
            take_events(&mut state.inputs, &mut state.hotkeys, &mut input_state);
            expected.p1.button1 = !expected.p1.button1;
            assert_eq!(expected, state.inputs);
        }

        assert_eq!(state.inputs.p1.button1, true);
        state.handle_input(&mut input_state, GenericInput::Keyboard(Keycode::D), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs);

        for _ in 0..51 {
            input_state.toggle_turbo_states();
            take_events(&mut state.inputs, &mut state.hotkeys, &mut input_state);
            assert_eq!(expected, state.inputs);
        }
    }
}
