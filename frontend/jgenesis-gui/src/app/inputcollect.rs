use crate::app::GenericButton;
use crate::app::input::InputMappingSet;
use egui::{RichText, Window};
use jgenesis_native_config::input::{
    AxisDirection, GamepadAction, GenericInput, HatDirection, InputAppConfig, KeyboardInput,
};
use jgenesis_native_driver::input::Joysticks;
use sdl3::event::{Event, WindowEvent};
use sdl3::joystick::{HatState, Joystick};
use sdl3::keyboard::{Keycode, Scancode};
use std::collections::{HashMap, HashSet};
use std::mem;

struct VecSet(Vec<GenericInput>);

impl VecSet {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn insert(&mut self, input: GenericInput) {
        if !self.0.contains(&input) {
            self.0.push(input);
        }
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollectionDone {
    No,
    Yes,
}

struct CollectedInputs {
    inputs: VecSet,
    gamepad_starting_states: HashSet<GenericInput>,
    initial_axis_directions: HashMap<(u32, u8), AxisDirection>,
}

impl CollectedInputs {
    fn new(joysticks: &Joysticks, axis_deadzone: i16) -> Self {
        let gamepad_starting_states = joysticks
            .all_devices()
            .flat_map(|(device_id, (joystick, _))| {
                log::debug!("Added device {device_id} '{}'", joystick.name());
                joystick_starting_state(device_id, joystick, axis_deadzone)
            })
            .collect();

        let initial_axis_directions = joysticks
            .all_devices()
            .flat_map(|(gamepad_idx, (_, initial_axis_directions))| {
                initial_axis_directions
                    .iter()
                    .map(move |&(axis_idx, direction)| ((gamepad_idx, axis_idx), direction))
            })
            .collect();

        log::debug!("Gamepad starting states: {gamepad_starting_states:?}");

        Self { inputs: VecSet::new(), gamepad_starting_states, initial_axis_directions }
    }

    fn add_device(
        &mut self,
        joysticks: &Joysticks,
        device_id: u32,
        joystick: &Joystick,
        axis_deadzone: i16,
    ) {
        log::debug!("Added device {device_id} '{}'", joystick.name());

        self.gamepad_starting_states.extend(joystick_starting_state(
            device_id,
            joystick,
            axis_deadzone,
        ));

        if let Some(initial_axis_directions) = joysticks.initial_axis_directions(device_id) {
            self.initial_axis_directions.extend(
                initial_axis_directions
                    .map(|(axis_idx, direction)| ((device_id, axis_idx), direction)),
            );
        }

        log::debug!("Gamepad starting states: {:?}", self.gamepad_starting_states);
    }

    fn remove_device(&mut self, device_id: u32) {
        self.gamepad_starting_states.retain(|input| !matches!(*input, GenericInput::Gamepad { gamepad_idx, .. } if gamepad_idx == device_id));

        self.initial_axis_directions.retain(|&(gamepad_idx, _), _| gamepad_idx != device_id);
    }

    fn contains(&self, input: GenericInput) -> bool {
        self.inputs.0.contains(&input)
    }

    fn consume(self) -> Vec<GenericInput> {
        // Don't allow axis inputs in combination with other inputs.
        // This is to work around some controllers sending analog triggers as both an axis and a
        // button (e.g. 8BitDo Pro 2), as well as to prevent accidentally inputting two axis
        // directions simultaneously
        if let Some(&axis_input) = self.inputs.0.iter().find(|input| {
            matches!(input, GenericInput::Gamepad { action: GamepadAction::Axis(..), .. })
        }) {
            return vec![axis_input];
        }

        self.inputs.0
    }

    #[must_use]
    fn insert(&mut self, input: GenericInput) -> CollectionDone {
        match input {
            GenericInput::Gamepad {
                gamepad_idx,
                action: GamepadAction::Axis(axis_idx, direction),
            } => {
                let opposite_input = GenericInput::Gamepad {
                    gamepad_idx,
                    action: GamepadAction::Axis(axis_idx, direction.inverse()),
                };
                if self.contains(opposite_input) {
                    return CollectionDone::Yes;
                }

                self.gamepad_starting_states.remove(&opposite_input);

                if self.gamepad_starting_states.contains(&input)
                    || self
                        .initial_axis_directions
                        .get(&(gamepad_idx, axis_idx))
                        .is_some_and(|&initial_direction| direction == initial_direction)
                {
                    return CollectionDone::No;
                }
            }
            _ => {
                if self.gamepad_starting_states.remove(&input) {
                    return CollectionDone::No;
                }
            }
        }

        self.inputs.insert(input);
        if self.inputs.len() == jgenesis_native_driver::input::MAX_MAPPING_LEN {
            CollectionDone::Yes
        } else {
            CollectionDone::No
        }
    }

    #[must_use]
    fn axis_zero(&mut self, gamepad_idx: u32, axis_idx: u8) -> CollectionDone {
        for direction in [AxisDirection::Positive, AxisDirection::Negative] {
            let input = GenericInput::Gamepad {
                gamepad_idx,
                action: GamepadAction::Axis(axis_idx, direction),
            };
            if self.contains(input) {
                return CollectionDone::Yes;
            }

            self.gamepad_starting_states.remove(&input);
        }

        CollectionDone::No
    }
}

pub struct InputCollectionState {
    buttons: Vec<GenericButton>,
    mapping: InputMappingSet,
    turbo: bool,
    inputs: CollectedInputs,
    aborted: bool,
    mouse_over_window: bool,
}

impl InputCollectionState {
    // Use a fairly high deadzone for detecting axis directions to make it harder to accidentally
    // input the wrong direction
    const AXIS_DEADZONE: i16 = 27000;

    pub fn new(
        joysticks: &Joysticks,
        buttons: Vec<GenericButton>,
        mapping: InputMappingSet,
        turbo: bool,
    ) -> Self {
        let collected_inputs = CollectedInputs::new(joysticks, Self::AXIS_DEADZONE);

        Self {
            buttons,
            mapping,
            turbo,
            inputs: collected_inputs,
            aborted: false,
            mouse_over_window: false,
        }
    }

    pub fn done(&self) -> bool {
        self.aborted || self.buttons.is_empty()
    }

    pub fn show_window(&mut self, ctx: &egui::Context, joysticks: &Joysticks) {
        const TITLE: &str = "Input Configuration";

        let mut open = !self.done();
        if !open {
            return;
        }

        // Prevent input configuration window from ever going below another window
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::new(TITLE)));

        Window::new(TITLE).open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            self.mouse_over_window = ui.rect_contains_pointer(ui.max_rect());
            render_input_window(joysticks, self.buttons[0], ui);
        });

        self.aborted = !open;
    }

    pub fn handle_sdl_event(
        &mut self,
        event: &Event,
        ctx: &egui::Context,
        gui_window_id: u32,
        joysticks: &mut Joysticks,
        input_config: &mut InputAppConfig,
    ) {
        if self.done() {
            return;
        }

        if matches!(
            event,
            Event::JoyDeviceAdded { .. }
                | Event::JoyDeviceRemoved { .. }
                | Event::JoyButtonUp { .. }
                | Event::JoyButtonDown { .. }
                | Event::JoyAxisMotion { .. }
                | Event::JoyHatMotion { .. }
        ) {
            ctx.request_repaint();
        }

        if let Some(input) = self.maybe_collect_input(event, gui_window_id, joysticks) {
            let button = self.buttons[0];

            log::info!("Received input {input:?} for button {button:?}");

            if !input.is_empty()
                && let Some(value) =
                    button.access_value_maybe_turbo(self.mapping, input_config, self.turbo)
            {
                *value = Some(input);
            }

            self.buttons.remove(0);
            ctx.request_repaint();
        }
    }

    fn maybe_collect_input(
        &mut self,
        event: &Event,
        gui_window_id: u32,
        joysticks: &mut Joysticks,
    ) -> Option<Vec<GenericInput>> {
        if self.aborted || self.buttons.is_empty() {
            return None;
        }

        match *event {
            Event::Quit { .. } => {
                self.aborted = true;
            }
            Event::Window { window_id, win_event: WindowEvent::CloseRequested, .. }
                if window_id == gui_window_id =>
            {
                self.aborted = true;
            }
            Event::KeyDown { keycode, scancode, window_id, .. }
                if window_id == gui_window_id
                    && let Some(key) = keyboard_input_for(keycode, scancode)
                    && self.inputs.insert(GenericInput::Keyboard(key)) == CollectionDone::Yes =>
            {
                return self.consume_collected_inputs(joysticks);
            }
            Event::KeyUp { keycode, scancode, window_id, .. }
                if window_id == gui_window_id
                    && let Some(key) = keyboard_input_for(keycode, scancode)
                    && self.inputs.contains(GenericInput::Keyboard(key)) =>
            {
                return self.consume_collected_inputs(joysticks);
            }
            Event::JoyDeviceAdded { which: joystick_id, .. } => {
                if let Err(err) = joysticks.handle_device_added(joystick_id) {
                    log::error!("Error adding joystick with joystick id {joystick_id}: {err}");
                }

                if let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id)
                    && let Some(joystick) = joysticks.device(gamepad_idx)
                {
                    self.inputs.add_device(joysticks, gamepad_idx, joystick, Self::AXIS_DEADZONE);
                }
            }
            Event::JoyDeviceRemoved { which: joystick_id, .. } => {
                if let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id) {
                    self.inputs.remove_device(gamepad_idx);
                }

                if let Err(err) = joysticks.handle_device_removed(joystick_id) {
                    log::error!("Error removing joystick with joystick id {joystick_id}: {err}");
                }
            }
            Event::JoyButtonDown { which: joystick_id, button_idx, .. } => {
                if let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id)
                    && self.inputs.insert(GenericInput::Gamepad {
                        gamepad_idx,
                        action: GamepadAction::Button(button_idx),
                    }) == CollectionDone::Yes
                {
                    return self.consume_collected_inputs(joysticks);
                }
            }
            Event::JoyButtonUp { which: joystick_id, button_idx, .. } => {
                if let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id)
                    && self.inputs.contains(GenericInput::Gamepad {
                        gamepad_idx,
                        action: GamepadAction::Button(button_idx),
                    })
                {
                    return self.consume_collected_inputs(joysticks);
                }
            }
            Event::JoyAxisMotion { which: joystick_id, axis_idx, value, .. } => {
                let gamepad_idx = joysticks.map_to_device_id(joystick_id)?;

                let pressed = value.saturating_abs() > Self::AXIS_DEADZONE;
                if pressed {
                    let direction = AxisDirection::from_value(value);
                    if self.inputs.insert(GenericInput::Gamepad {
                        gamepad_idx,
                        action: GamepadAction::Axis(axis_idx, direction),
                    }) == CollectionDone::Yes
                    {
                        return self.consume_collected_inputs(joysticks);
                    }
                } else if self.inputs.axis_zero(gamepad_idx, axis_idx) == CollectionDone::Yes {
                    return self.consume_collected_inputs(joysticks);
                }
            }
            Event::JoyHatMotion { which: joystick_id, hat_idx, state, .. } => {
                let gamepad_idx = joysticks.map_to_device_id(joystick_id)?;

                if state == HatState::Centered {
                    if HatDirection::ALL.into_iter().any(|direction| {
                        self.inputs.contains(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Hat(hat_idx, direction),
                        })
                    }) {
                        return self.consume_collected_inputs(joysticks);
                    }

                    return None;
                }

                if let Some(direction) = hat_direction_for(state)
                    && self.inputs.insert(GenericInput::Gamepad {
                        gamepad_idx,
                        action: GamepadAction::Hat(hat_idx, direction),
                    }) == CollectionDone::Yes
                {
                    return self.consume_collected_inputs(joysticks);
                }
            }
            Event::MouseButtonDown { mouse_btn, window_id, .. }
                if window_id == gui_window_id
                    && self.mouse_over_window
                    && self.inputs.insert(GenericInput::Mouse(mouse_btn))
                        == CollectionDone::Yes =>
            {
                return self.consume_collected_inputs(joysticks);
            }
            Event::MouseButtonUp { mouse_btn, window_id, .. }
                if window_id == gui_window_id
                    && self.mouse_over_window
                    && self.inputs.contains(GenericInput::Mouse(mouse_btn)) =>
            {
                return self.consume_collected_inputs(joysticks);
            }
            _ => {}
        }

        None
    }

    #[allow(clippy::unnecessary_wraps)] // Returns an Option for convenience of use
    fn consume_collected_inputs(&mut self, joysticks: &Joysticks) -> Option<Vec<GenericInput>> {
        let inputs =
            mem::replace(&mut self.inputs, CollectedInputs::new(joysticks, Self::AXIS_DEADZONE));
        Some(inputs.consume())
    }
}

fn keyboard_input_for(
    keycode: Option<Keycode>,
    scancode: Option<Scancode>,
) -> Option<KeyboardInput> {
    // Prefer keycode (virtual key) over scancode (physical key location) if both are present,
    // only using scancode if keycode is unknown (e.g. the ñ key on Spanish keyboards).
    // This is mainly to respect the keyboard layout's modifier key locations, and to make the
    // input configuration UI hopefully less confusing
    match (keycode, scancode) {
        (Some(keycode), _) => Some(KeyboardInput::Keycode(keycode)),
        (None, Some(scancode)) => Some(KeyboardInput::Scancode(scancode)),
        (None, None) => None,
    }
}

fn hat_direction_for(state: HatState) -> Option<HatDirection> {
    match state {
        HatState::Up => Some(HatDirection::Up),
        HatState::Left => Some(HatDirection::Left),
        HatState::Right => Some(HatDirection::Right),
        HatState::Down => Some(HatDirection::Down),
        // Ignore diagonals for the purpose of collecting input
        _ => None,
    }
}

fn joystick_starting_state(
    device_id: u32,
    joystick: &Joystick,
    axis_deadzone: i16,
) -> impl Iterator<Item = GenericInput> + use<'_> {
    buttons_starting_state(device_id, joystick)
        .chain(axes_starting_state(device_id, joystick, axis_deadzone))
        .chain(hats_starting_state(device_id, joystick))
}

fn buttons_starting_state(
    gamepad_idx: u32,
    joystick: &Joystick,
) -> impl Iterator<Item = GenericInput> + use<'_> {
    let num_buttons = joystick.num_buttons();
    log::debug!("  Gamepad {gamepad_idx} has {num_buttons} buttons");

    (0..num_buttons).filter_map(move |button_idx| {
        let pressed = joystick.button(button_idx).ok()?;
        log::debug!("    Button {button_idx} initial pressed: {pressed}");
        pressed.then_some(GenericInput::Gamepad {
            gamepad_idx,
            action: GamepadAction::Button(button_idx as u8),
        })
    })
}

fn axes_starting_state(
    gamepad_idx: u32,
    joystick: &Joystick,
    deadzone: i16,
) -> impl Iterator<Item = GenericInput> + use<'_> {
    let num_axes = joystick.num_axes();
    log::debug!("  Gamepad {gamepad_idx} has {num_axes} axes");

    (0..num_axes).filter_map(move |axis_idx| {
        let axis_value = joystick.axis(axis_idx).ok()?;
        log::debug!("    Axis {axis_idx} initial value: {axis_value}");

        if axis_value.saturating_abs() < deadzone {
            return None;
        }

        let direction = AxisDirection::from_value(axis_value);
        Some(GenericInput::Gamepad {
            gamepad_idx,
            action: GamepadAction::Axis(axis_idx as u8, direction),
        })
    })
}

fn hats_starting_state(
    gamepad_idx: u32,
    joystick: &Joystick,
) -> impl Iterator<Item = GenericInput> + use<'_> {
    let num_hats = joystick.num_hats();
    log::debug!("  Gamepad {gamepad_idx} has {num_hats} hats");

    (0..num_hats).filter_map(move |hat_idx| {
        let state = joystick.hat(hat_idx).ok()?;
        log::debug!("    Hat {hat_idx} initial state: {state:?}");

        hat_direction_for(state).map(|hat_direction| GenericInput::Gamepad {
            gamepad_idx,
            action: GamepadAction::Hat(hat_idx as u8, hat_direction),
        })
    })
}

fn render_input_window(joysticks: &Joysticks, button: GenericButton, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Configuring button:");
            ui.label(RichText::new(button.label()).strong());
        });

        ui.add_space(10.0);

        ui.label(
            format!(
                "Press a key, a gamepad input, or a mouse button. Mouse clicks must be on this window. Combinations of up to {} inputs simultaneously are supported.",
                jgenesis_native_driver::input::MAX_MAPPING_LEN,
            )
        );

        ui.add_space(10.0);

        ui.label("Connected gamepads:");

        let devices: Vec<_> = joysticks.all_devices().collect();
        if devices.is_empty() {
            ui.label("    (None)");
        } else {
            for (gamepad_idx, (joystick, _)) in devices {
                ui.label(format!("    Gamepad {gamepad_idx}: {}", joystick.name()));
            }
        }
    });
}
