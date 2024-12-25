mod serialize;

use arrayvec::ArrayVec;
use jgenesis_common::frontend::{DisplayArea, FrameSize, MappableInputs};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};
use rustc_hash::{FxHashMap, FxHashSet};
use sdl2::event::{Event, WindowEvent};
use sdl2::joystick::{HatState, Joystick};
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::{IntegerOrSdlError, JoystickSubsystem};
use std::array;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::rc::Rc;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisDirection {
    Positive,
    Negative,
}

impl AxisDirection {
    #[inline]
    #[must_use]
    pub fn from_value(value: i16) -> Self {
        if value >= 0 { Self::Positive } else { Self::Negative }
    }

    #[inline]
    #[must_use]
    pub fn inverse(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }
}

impl Display for AxisDirection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Positive => write!(f, "+"),
            Self::Negative => write!(f, "-"),
        }
    }
}

impl FromStr for AxisDirection {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "+" => Ok(Self::Positive),
            "-" => Ok(Self::Negative),
            _ => Err(format!("Invalid AxisDirection string: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumDisplay, EnumFromStr)]
pub enum HatDirection {
    Up,
    Left,
    Right,
    Down,
}

impl HatDirection {
    pub const ALL: [Self; 4] = [Self::Up, Self::Left, Self::Right, Self::Down];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadAction {
    Button(u8),
    Axis(u8, AxisDirection),
    Hat(u8, HatDirection),
}

impl Display for GamepadAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Button(idx) => write!(f, "Button {idx}"),
            Self::Axis(idx, direction) => write!(f, "Axis {idx} {direction}"),
            Self::Hat(idx, direction) => write!(f, "Hat {idx} {direction}"),
        }
    }
}

impl FromStr for GamepadAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err_fn = || format!("Invalid gamepad action string: {s}");

        let mut split = s.split_ascii_whitespace();
        let Some(input_type) = split.next() else {
            return Err(err_fn());
        };

        let Some(idx) = split.next().and_then(|idx| idx.parse().ok()) else {
            return Err(err_fn());
        };

        match input_type {
            "Button" | "button" => Ok(Self::Button(idx)),
            "Axis" | "axis" => {
                let Some(direction) = split.next().and_then(|direction| direction.parse().ok())
                else {
                    return Err(err_fn());
                };

                Ok(Self::Axis(idx, direction))
            }
            "Hat" | "hat" => {
                let Some(direction) = split.next().and_then(|direction| direction.parse().ok())
                else {
                    return Err(err_fn());
                };

                Ok(Self::Hat(idx, direction))
            }
            _ => Err(err_fn()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericInput {
    Keyboard(Keycode),
    Gamepad { gamepad_idx: u32, action: GamepadAction },
    Mouse(MouseButton),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CanonicalInput(GenericInput);

impl GenericInput {
    pub(crate) fn canonicalize(self) -> CanonicalInput {
        match self {
            Self::Keyboard(keycode) => {
                CanonicalInput(Self::Keyboard(canonicalize_keycode(keycode)))
            }
            _ => CanonicalInput(self),
        }
    }
}

fn canonicalize_keycode(keycode: Keycode) -> Keycode {
    match keycode {
        Keycode::RShift => Keycode::LShift,
        Keycode::RCtrl => Keycode::LCtrl,
        Keycode::RAlt => Keycode::LAlt,
        _ => keycode,
    }
}

impl CanonicalInput {
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

impl Display for GenericInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            &Self::Keyboard(keycode) => write!(f, "Key: {}", keycode_to_str(keycode)),
            Self::Gamepad { gamepad_idx, action } => write!(f, "Gamepad {gamepad_idx}: {action}"),
            Self::Mouse(mouse_button) => write!(f, "Mouse: {mouse_button:?}"),
        }
    }
}

fn keycode_to_str(keycode: Keycode) -> Cow<'static, str> {
    match keycode {
        Keycode::LShift | Keycode::RShift => "Shift".into(),
        Keycode::LCtrl | Keycode::RCtrl => "Ctrl".into(),
        Keycode::LAlt | Keycode::RAlt => "Alt".into(),
        _ => keycode.name().into(),
    }
}

fn keycode_from_str(s: &str) -> Option<Keycode> {
    match s {
        "Shift" => Some(Keycode::LShift),
        "Ctrl" => Some(Keycode::LCtrl),
        "Alt" => Some(Keycode::LAlt),
        _ => {
            if s == Keycode::RShift.name().as_str() {
                Some(Keycode::LShift)
            } else if s == Keycode::RCtrl.name().as_str() {
                Some(Keycode::LCtrl)
            } else if s == Keycode::RAlt.name().as_str() {
                Some(Keycode::LAlt)
            } else {
                Keycode::from_name(s)
            }
        }
    }
}

pub const MAX_MAPPING_LEN: usize = 3;
type MappingArrayVec = ArrayVec<CanonicalInput, MAX_MAPPING_LEN>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumDisplay, EnumAll)]
pub enum Hotkey {
    Exit,
    ToggleFullscreen,
    SoftReset,
    HardReset,
    PowerOff,
    Pause,
    StepFrame,
    FastForward,
    Rewind,
    OpenDebugger,
    SaveState,
    LoadState,
    NextSaveStateSlot,
    PrevSaveStateSlot,
    SaveStateSlot0,
    LoadStateSlot0,
    SaveStateSlot1,
    LoadStateSlot1,
    SaveStateSlot2,
    LoadStateSlot2,
    SaveStateSlot3,
    LoadStateSlot3,
    SaveStateSlot4,
    LoadStateSlot4,
    SaveStateSlot5,
    LoadStateSlot5,
    SaveStateSlot6,
    LoadStateSlot6,
    SaveStateSlot7,
    LoadStateSlot7,
    SaveStateSlot8,
    LoadStateSlot8,
    SaveStateSlot9,
    LoadStateSlot9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactHotkey {
    PowerOff,
    Exit,
    ToggleFullscreen,
    SaveState,
    LoadState,
    SaveStateSlot(usize),
    LoadStateSlot(usize),
    NextSaveStateSlot,
    PrevSaveStateSlot,
    SoftReset,
    HardReset,
    Pause,
    StepFrame,
    FastForward,
    Rewind,
    OpenDebugger,
}

impl Hotkey {
    pub(crate) fn to_compact(self) -> CompactHotkey {
        match self {
            Self::PowerOff => CompactHotkey::PowerOff,
            Self::Exit => CompactHotkey::Exit,
            Self::ToggleFullscreen => CompactHotkey::ToggleFullscreen,
            Self::SaveState => CompactHotkey::SaveState,
            Self::LoadState => CompactHotkey::LoadState,
            Self::NextSaveStateSlot => CompactHotkey::NextSaveStateSlot,
            Self::PrevSaveStateSlot => CompactHotkey::PrevSaveStateSlot,
            Self::SoftReset => CompactHotkey::SoftReset,
            Self::HardReset => CompactHotkey::HardReset,
            Self::Pause => CompactHotkey::Pause,
            Self::StepFrame => CompactHotkey::StepFrame,
            Self::FastForward => CompactHotkey::FastForward,
            Self::Rewind => CompactHotkey::Rewind,
            Self::OpenDebugger => CompactHotkey::OpenDebugger,
            Self::SaveStateSlot0 => CompactHotkey::SaveStateSlot(0),
            Self::SaveStateSlot1 => CompactHotkey::SaveStateSlot(1),
            Self::SaveStateSlot2 => CompactHotkey::SaveStateSlot(2),
            Self::SaveStateSlot3 => CompactHotkey::SaveStateSlot(3),
            Self::SaveStateSlot4 => CompactHotkey::SaveStateSlot(4),
            Self::SaveStateSlot5 => CompactHotkey::SaveStateSlot(5),
            Self::SaveStateSlot6 => CompactHotkey::SaveStateSlot(6),
            Self::SaveStateSlot7 => CompactHotkey::SaveStateSlot(7),
            Self::SaveStateSlot8 => CompactHotkey::SaveStateSlot(8),
            Self::SaveStateSlot9 => CompactHotkey::SaveStateSlot(9),
            Self::LoadStateSlot0 => CompactHotkey::LoadStateSlot(0),
            Self::LoadStateSlot1 => CompactHotkey::LoadStateSlot(1),
            Self::LoadStateSlot2 => CompactHotkey::LoadStateSlot(2),
            Self::LoadStateSlot3 => CompactHotkey::LoadStateSlot(3),
            Self::LoadStateSlot4 => CompactHotkey::LoadStateSlot(4),
            Self::LoadStateSlot5 => CompactHotkey::LoadStateSlot(5),
            Self::LoadStateSlot6 => CompactHotkey::LoadStateSlot(6),
            Self::LoadStateSlot7 => CompactHotkey::LoadStateSlot(7),
            Self::LoadStateSlot8 => CompactHotkey::LoadStateSlot(8),
            Self::LoadStateSlot9 => CompactHotkey::LoadStateSlot(9),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericButton<Button> {
    Button(Button, Player),
    Hotkey(Hotkey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyEvent {
    Pressed(Hotkey),
    Released(Hotkey),
}

pub struct Joysticks {
    subsystem: JoystickSubsystem,
    devices: BTreeMap<u32, Joystick>,
    instance_id_to_device_id: FxHashMap<u32, u32>,
}

impl Joysticks {
    #[must_use]
    pub fn new(subsystem: JoystickSubsystem) -> Self {
        Self { subsystem, devices: BTreeMap::new(), instance_id_to_device_id: FxHashMap::default() }
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn handle_device_added(&mut self, joystick_idx: u32) -> Result<(), IntegerOrSdlError> {
        let joystick = self.subsystem.open(joystick_idx)?;

        log::info!("Added joystick {joystick_idx}: '{}'", joystick.name());

        self.instance_id_to_device_id.insert(joystick.instance_id(), joystick_idx);
        self.devices.insert(joystick_idx, joystick);

        Ok(())
    }

    pub fn handle_device_removed(&mut self, instance_id: u32) -> Option<u32> {
        let device_id = self.instance_id_to_device_id.remove(&instance_id)?;
        let Some(_) = self.devices.remove(&device_id) else { return Some(device_id) };

        log::info!("Removed joystick {device_id}");

        Some(device_id)
    }

    #[must_use]
    pub fn map_to_device_id(&mut self, instance_id: u32) -> Option<u32> {
        self.instance_id_to_device_id.get(&instance_id).copied()
    }

    #[must_use]
    pub fn subsystem(&self) -> &JoystickSubsystem {
        &self.subsystem
    }

    #[must_use]
    pub fn device(&self, device_id: u32) -> Option<&Joystick> {
        self.devices.get(&device_id)
    }

    pub fn all_devices(&self) -> impl Iterator<Item = (u32, &'_ Joystick)> + '_ {
        self.devices.iter().map(|(&device_id, joystick)| (device_id, joystick))
    }
}

struct InputMapperState<Inputs, Button> {
    inputs: Inputs,
    hotkey_events: Rc<RefCell<Vec<HotkeyEvent>>>,
    mappings: FxHashMap<GenericButton<Button>, Vec<MappingArrayVec>>,
    inputs_to_buttons: FxHashMap<CanonicalInput, Vec<GenericButton<Button>>>,
    active_inputs: FxHashSet<GenericInput>,
    active_canonical_inputs: FxHashSet<CanonicalInput>,
    active_hotkeys: FxHashSet<Hotkey>,
    changed_button_buffers: [Vec<GenericButton<Button>>; MAX_MAPPING_LEN + 1],
}

impl<Inputs, Button> InputMapperState<Inputs, Button>
where
    Button: Debug + Copy + Hash + Eq,
    Inputs: MappableInputs<Button>,
{
    fn new(initial_inputs: Inputs) -> Self {
        Self {
            inputs: initial_inputs,
            hotkey_events: Rc::new(RefCell::new(Vec::with_capacity(10))),
            mappings: FxHashMap::default(),
            inputs_to_buttons: FxHashMap::default(),
            active_inputs: FxHashSet::default(),
            active_canonical_inputs: FxHashSet::default(),
            active_hotkeys: FxHashSet::default(),
            changed_button_buffers: array::from_fn(|_| Vec::with_capacity(10)),
        }
    }

    fn update_mappings(
        &mut self,
        button_mappings: &[((Button, Player), &Vec<GenericInput>)],
        hotkey_mappings: &[(Hotkey, &Vec<GenericInput>)],
    ) {
        self.mappings.clear();
        self.inputs_to_buttons.clear();
        self.active_inputs.clear();
        self.active_hotkeys.clear();

        for &((button, player), mapping) in button_mappings {
            if mapping.len() > MAX_MAPPING_LEN {
                log::error!("Ignoring mapping, too many inputs: {mapping:?}");
                continue;
            }

            let generic_button = GenericButton::Button(button, player);
            self.mappings
                .entry(generic_button)
                .or_default()
                .push(mapping.iter().copied().map(GenericInput::canonicalize).collect());

            for &mapping_input in mapping {
                self.inputs_to_buttons
                    .entry(mapping_input.canonicalize())
                    .or_default()
                    .push(generic_button);
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
                .push(mapping.iter().copied().map(GenericInput::canonicalize).collect());

            for &mapping_input in mapping {
                self.inputs_to_buttons
                    .entry(mapping_input.canonicalize())
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

        let input = raw_input.canonicalize();
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
                        self.inputs.set_field(button, player, pressed);
                    }
                    GenericButton::Hotkey(hotkey) => {
                        if pressed && self.active_hotkeys.insert(hotkey) {
                            self.hotkey_events.borrow_mut().push(HotkeyEvent::Pressed(hotkey));
                        } else if !pressed && self.active_hotkeys.remove(&hotkey) {
                            self.hotkey_events.borrow_mut().push(HotkeyEvent::Released(hotkey));
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

    fn unset_all_gamepad_inputs(&mut self, idx: u32) {
        // Allocation to avoid borrow checker issues is fine, this won't be called frequently
        let gamepad_inputs: Vec<_> = self
            .inputs_to_buttons
            .keys()
            .copied()
            .filter(|&input| match input.0 {
                GenericInput::Gamepad { gamepad_idx, .. } => gamepad_idx == idx,
                _ => false,
            })
            .collect();

        for input in gamepad_inputs {
            self.handle_input(input.0, false);
        }
    }
}

pub struct InputMapper<Inputs, Button> {
    joysticks: Joysticks,
    axis_deadzone: i16,
    state: InputMapperState<Inputs, Button>,
}

impl<Inputs, Button> InputMapper<Inputs, Button>
where
    Button: Debug + Copy + Hash + Eq,
    Inputs: MappableInputs<Button>,
{
    pub fn new(
        initial_inputs: Inputs,
        joystick_subsystem: JoystickSubsystem,
        axis_deadzone: i16,
        button_mappings: &[((Button, Player), &Vec<GenericInput>)],
        hotkey_mappings: &[(Hotkey, &Vec<GenericInput>)],
    ) -> Self {
        let joysticks = Joysticks::new(joystick_subsystem);

        let mut state = InputMapperState::new(initial_inputs);
        state.update_mappings(button_mappings, hotkey_mappings);

        Self { joysticks, axis_deadzone, state }
    }

    pub fn inputs_mut(&mut self) -> &mut Inputs {
        &mut self.state.inputs
    }

    pub fn update_mappings(
        &mut self,
        axis_deadzone: i16,
        button_mappings: &[((Button, Player), &Vec<GenericInput>)],
        hotkey_mappings: &[(Hotkey, &Vec<GenericInput>)],
    ) {
        self.axis_deadzone = axis_deadzone;
        self.state.update_mappings(button_mappings, hotkey_mappings);
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
                    self.state.inputs.handle_mouse_motion(x, y, frame_size, display_area);
                }
            }
            Event::Window { win_event: WindowEvent::Leave, window_id, .. }
                if window_id == emulator_window_id =>
            {
                self.state.inputs.handle_mouse_leave();
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
                    log::error!("Error opening joystick with device id {which}: {err}");
                }
            }
            Event::JoyDeviceRemoved { which, .. } => {
                let Some(gamepad_idx) = self.joysticks.handle_device_removed(which) else { return };
                self.state.unset_all_gamepad_inputs(gamepad_idx);
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

    pub fn hotkey_events(&self) -> Rc<RefCell<Vec<HotkeyEvent>>> {
        Rc::clone(&self.state.hotkey_events)
    }
}

impl<Inputs, Button> InputMapper<Inputs, Button> {
    pub fn inputs(&self) -> &Inputs {
        &self.state.inputs
    }

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
    use smsgg_core::{SmsGgButton, SmsGgInputs};
    use std::mem;

    fn take_hotkey_events<I, B>(state: &mut InputMapperState<I, B>) -> Vec<HotkeyEvent> {
        mem::take(&mut *state.hotkey_events.borrow_mut())
    }

    fn into_hash_set<T: Hash + Eq>(v: impl IntoIterator<Item = T>) -> FxHashSet<T> {
        v.into_iter().collect()
    }

    #[test]
    fn basic_mapping() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Button1, Player::Two), &vec![GenericInput::Keyboard(Keycode::G)]),
                ((SmsGgButton::Button2, Player::One), &vec![GenericInput::Keyboard(Keycode::Up)]),
            ],
            &[(Hotkey::FastForward, &vec![GenericInput::Keyboard(Keycode::H)])],
        );

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);
        assert_eq!(*state.hotkey_events.borrow(), vec![]);

        state.handle_input(GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs);
        assert_eq!(*state.hotkey_events.borrow(), vec![]);

        state.handle_input(GenericInput::Keyboard(Keycode::G), true);
        expected.p2.button1 = true;
        assert_eq!(expected, state.inputs);
        assert_eq!(*state.hotkey_events.borrow(), vec![]);

        state.handle_input(GenericInput::Keyboard(Keycode::F), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs);
        assert_eq!(*state.hotkey_events.borrow(), vec![]);

        state.handle_input(GenericInput::Keyboard(Keycode::H), true);
        assert_eq!(expected, state.inputs);
        assert_eq!(*state.hotkey_events.borrow(), vec![HotkeyEvent::Pressed(Hotkey::FastForward)]);

        state.handle_input(GenericInput::Keyboard(Keycode::H), false);
        assert_eq!(expected, state.inputs);
        assert_eq!(*state.hotkey_events.borrow(), vec![
            HotkeyEvent::Pressed(Hotkey::FastForward),
            HotkeyEvent::Released(Hotkey::FastForward),
        ]);
    }

    #[test]
    fn one_mapping_button_and_hotkey() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(
            &[((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)])],
            &[(Hotkey::SaveState, &vec![GenericInput::Keyboard(Keycode::F)])],
        );

        let mut expected_inputs = SmsGgInputs::default();
        let mut expected_hotkeys: Vec<HotkeyEvent> = vec![];
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, take_hotkey_events(&mut state));

        state.handle_input(GenericInput::Keyboard(Keycode::F), true);
        expected_inputs.p1.button1 = true;
        expected_hotkeys = vec![HotkeyEvent::Pressed(Hotkey::SaveState)];
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, take_hotkey_events(&mut state));

        state.handle_input(GenericInput::Keyboard(Keycode::F), false);
        expected_inputs.p1.button1 = false;
        expected_hotkeys = vec![HotkeyEvent::Released(Hotkey::SaveState)];
        assert_eq!(expected_inputs, state.inputs);
        assert_eq!(expected_hotkeys, take_hotkey_events(&mut state));
    }

    #[test]
    fn two_mappings_same_button() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::G)]),
            ],
            &[],
        );

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "one mapping pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::G), true);
        assert_eq!(expected, state.inputs, "two mappings pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::G), false);
        assert_eq!(expected, state.inputs, "one mapping released, one still pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::F), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "both mappings released");
    }

    #[test]
    fn one_mapping_three_buttons() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Button2, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
                ((SmsGgButton::Pause, Player::One), &vec![GenericInput::Keyboard(Keycode::F)]),
            ],
            &[],
        );

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        expected.pause = true;
        assert_eq!(expected, state.inputs, "mapping pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::F), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        expected.pause = false;
        assert_eq!(expected, state.inputs, "mapping released");
    }

    #[test]
    fn combination_mapping() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(
            &[((SmsGgButton::Button1, Player::One), &vec![
                GenericInput::Keyboard(Keycode::F),
                GenericInput::Keyboard(Keycode::G),
                GenericInput::Keyboard(Keycode::H),
            ])],
            &[],
        );

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(GenericInput::Keyboard(Keycode::F), true);
        assert_eq!(expected, state.inputs, "1/3 pressed (1)");

        state.handle_input(GenericInput::Keyboard(Keycode::H), true);
        assert_eq!(expected, state.inputs, "2/3 pressed (2)");

        state.handle_input(GenericInput::Keyboard(Keycode::F), false);
        assert_eq!(expected, state.inputs, "1/3 pressed (3)");

        state.handle_input(GenericInput::Keyboard(Keycode::G), true);
        assert_eq!(expected, state.inputs, "2/3 pressed (4)");

        state.handle_input(GenericInput::Keyboard(Keycode::F), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "3/3 pressed (5)");

        state.handle_input(GenericInput::Keyboard(Keycode::H), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "2/3 pressed (6)");

        state.handle_input(GenericInput::Keyboard(Keycode::H), true);
        expected.p1.button1 = true;
        assert_eq!(expected, state.inputs, "3/3 pressed (7)");

        state.handle_input(GenericInput::Keyboard(Keycode::G), false);
        expected.p1.button1 = false;
        assert_eq!(expected, state.inputs, "2/3 pressed (8)");
    }

    #[test]
    fn combination_length_priority_basic() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(&[], &[
            (Hotkey::SaveState, &vec![
                GenericInput::Keyboard(Keycode::LShift),
                GenericInput::Keyboard(Keycode::F1),
            ]),
            (Hotkey::LoadState, &vec![GenericInput::Keyboard(Keycode::F1)]),
        ]);

        let mut expected: Vec<HotkeyEvent> = vec![];
        assert_eq!(expected, take_hotkey_events(&mut state));

        state.handle_input(GenericInput::Keyboard(Keycode::F1), true);
        expected = vec![HotkeyEvent::Pressed(Hotkey::LoadState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "single key pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::F1), false);
        expected = vec![HotkeyEvent::Released(Hotkey::LoadState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "single key pressed & released");

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), true);
        expected = vec![];
        assert_eq!(expected, take_hotkey_events(&mut state), "1/2 pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::F1), true);
        expected = vec![HotkeyEvent::Pressed(Hotkey::SaveState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "2/2 pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::F1), false);
        expected = vec![HotkeyEvent::Released(Hotkey::SaveState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "1/2 released");

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), false);
        expected = vec![];
        assert_eq!(expected, take_hotkey_events(&mut state), "2/2 released");
    }

    #[test]
    fn combination_length_priority_weird() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(&[], &[
            (Hotkey::SaveState, &vec![
                GenericInput::Keyboard(Keycode::LShift),
                GenericInput::Keyboard(Keycode::F1),
            ]),
            (Hotkey::LoadState, &vec![GenericInput::Keyboard(Keycode::F1)]),
        ]);

        let mut expected: Vec<HotkeyEvent> = vec![];
        assert_eq!(expected, take_hotkey_events(&mut state));

        state.handle_input(GenericInput::Keyboard(Keycode::F1), true);
        expected = vec![HotkeyEvent::Pressed(Hotkey::LoadState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "single key pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), true);
        expected = vec![HotkeyEvent::Pressed(Hotkey::SaveState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "combination secondary key pressed");

        state.handle_input(GenericInput::Keyboard(Keycode::F1), false);
        expected = vec![
            HotkeyEvent::Released(Hotkey::SaveState),
            HotkeyEvent::Released(Hotkey::LoadState),
        ];
        assert_eq!(
            into_hash_set(expected),
            into_hash_set(take_hotkey_events(&mut state)),
            "single key + combination released"
        );

        state.handle_input(GenericInput::Keyboard(Keycode::F1), true);
        expected = vec![HotkeyEvent::Pressed(Hotkey::SaveState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "combination pressed second time");

        state.handle_input(GenericInput::Keyboard(Keycode::F1), false);
        expected = vec![HotkeyEvent::Released(Hotkey::SaveState)];
        assert_eq!(expected, take_hotkey_events(&mut state), "combination released second time");

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), false);
        expected = vec![];
        assert_eq!(expected, take_hotkey_events(&mut state), "combination secondary key released");
    }

    #[test]
    fn shift_canonicalization_basic() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(
                    Keycode::RShift,
                )]),
                ((SmsGgButton::Button2, Player::One), &vec![GenericInput::Keyboard(
                    Keycode::LShift,
                )]),
            ],
            &[],
        );

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs, "Pressing LShift should trigger both Shift mappings");

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(expected, state.inputs, "Releasing LShift should trigger both Shift mappings");

        state.handle_input(GenericInput::Keyboard(Keycode::RShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs, "Pressing RShift should trigger both Shift mappings");

        state.handle_input(GenericInput::Keyboard(Keycode::RShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(expected, state.inputs, "Releasing RShift should trigger both Shift mappings");
    }

    #[test]
    fn shift_canonicalization_simultaneous() {
        let mut state = InputMapperState::new(SmsGgInputs::default());
        state.update_mappings(
            &[
                ((SmsGgButton::Button1, Player::One), &vec![GenericInput::Keyboard(
                    Keycode::RShift,
                )]),
                ((SmsGgButton::Button2, Player::One), &vec![GenericInput::Keyboard(
                    Keycode::LShift,
                )]),
            ],
            &[],
        );

        let mut expected = SmsGgInputs::default();
        assert_eq!(expected, state.inputs);

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), true);
        state.handle_input(GenericInput::Keyboard(Keycode::RShift), true);
        expected.p1.button1 = true;
        expected.p1.button2 = true;
        assert_eq!(expected, state.inputs);

        state.handle_input(GenericInput::Keyboard(Keycode::RShift), false);
        assert_eq!(
            expected, state.inputs,
            "Releasing RShift while LShift is held should not change mapping"
        );

        state.handle_input(GenericInput::Keyboard(Keycode::RShift), true);
        assert_eq!(
            expected, state.inputs,
            "Pressing RShift while LShift is held should not change mapping"
        );

        state.handle_input(GenericInput::Keyboard(Keycode::LShift), false);
        assert_eq!(
            expected, state.inputs,
            "Releasing LShift while RShift is held should not change mapping"
        );

        state.handle_input(GenericInput::Keyboard(Keycode::RShift), false);
        expected.p1.button1 = false;
        expected.p1.button2 = false;
        assert_eq!(
            expected, state.inputs,
            "Releasing RShift while LShift is not held should change mapping"
        );
    }
}
