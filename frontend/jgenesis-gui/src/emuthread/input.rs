use crate::app::GenericButton;
use crate::emuthread::GenericEmulator;
use anyhow::anyhow;
use egui::RichText;
use egui_wgpu::ScreenDescriptor;
use jgenesis_native_config::input::{
    AxisDirection, GamepadAction, GenericInput, HatDirection, KeyboardInput,
};
use jgenesis_native_driver::input::Joysticks;
use sdl3::EventPump;
use sdl3::event::{Event, WindowEvent};
use sdl3::joystick::{HatState, Joystick};
use sdl3::keyboard::{Keycode, Scancode};
use sdl3::video::Window;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime};
use std::{iter, thread};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectInputsResult {
    None,
    WindowClosed,
}

pub fn collect_input_not_running(
    buttons: Vec<GenericButton>,
    scale_factor: f32,
    input_sender: &Sender<Option<Vec<GenericInput>>>,
) -> anyhow::Result<()> {
    let sdl = sdl3::init().map_err(|err| anyhow!("Error initializing SDL3: {err}"))?;
    let video =
        sdl.video().map_err(|err| anyhow!("Error initializing SDL3 video subsystem: {err}"))?;
    let joystick_subsystem = sdl
        .joystick()
        .map_err(|err| anyhow!("Error initializing SDL3 joystick subsystem: {err}"))?;
    let mut event_pump =
        sdl.event_pump().map_err(|err| anyhow!("Error initializing SDL3 event pump: {err}"))?;

    let mut sdl_window = video
        .window(
            "SDL input configuration",
            (400.0 * scale_factor).round() as u32,
            (150.0 * scale_factor).round() as u32,
        )
        .build()?;
    sdl_window.raise();
    let mut window = InputWindow::new(sdl_window, scale_factor)?;

    let mut joysticks = Joysticks::new(joystick_subsystem);

    collect_inputs(
        &buttons,
        CollectInputWindow::Gui {
            window: &mut window,
            joysticks: &mut joysticks,
            event_pump: &mut event_pump,
        },
        input_sender,
    );

    for _ in event_pump.poll_iter() {}

    Ok(())
}

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

pub enum CollectInputWindow<'a> {
    Gui { window: &'a mut InputWindow, joysticks: &'a mut Joysticks, event_pump: &'a mut EventPump },
    Emulator(&'a mut GenericEmulator),
}

impl CollectInputWindow<'_> {
    fn joysticks(&mut self) -> &mut Joysticks {
        match self {
            Self::Gui { joysticks, .. } => joysticks,
            Self::Emulator(emulator) => emulator.joysticks(),
        }
    }

    fn event_pump(&mut self) -> &mut EventPump {
        match self {
            Self::Gui { event_pump, .. } => event_pump,
            Self::Emulator(emulator) => emulator.event_pump(),
        }
    }

    fn handle_sdl_event(&mut self, event: &Event) {
        match self {
            Self::Gui { window, .. } => window.handle_sdl_event(event),
            Self::Emulator(emulator) => {
                if let Event::Window { win_event, window_id, .. } = event
                    && let Err(err) = emulator.handle_window_event(win_event, *window_id)
                {
                    log::error!("Error handling SDL window event: {err}");
                }
            }
        }
    }
}

pub fn collect_inputs(
    buttons: &[GenericButton],
    mut window: CollectInputWindow<'_>,
    input_sender: &Sender<Option<Vec<GenericInput>>>,
) -> CollectInputsResult {
    for &button in buttons {
        let input = collect_input(button, &mut window);
        let is_none = input.is_none();
        let _ = input_sender.send(input);

        if is_none {
            return CollectInputsResult::WindowClosed;
        }
    }

    CollectInputsResult::None
}

fn collect_input(
    button: GenericButton,
    window: &mut CollectInputWindow<'_>,
) -> Option<Vec<GenericInput>> {
    // Use a fairly high deadzone for detecting axis directions to make it harder to accidentally
    // input the wrong direction
    const AXIS_DEADZONE: i16 = 27000;

    let mut inputs = CollectedInputs::new(window.joysticks(), AXIS_DEADZONE);

    loop {
        while let Some(event) = window.event_pump().poll_event() {
            log::debug!("SDL event: {event:?}");

            window.handle_sdl_event(&event);

            let joysticks = window.joysticks();

            match event {
                Event::Quit { .. } => {
                    return None;
                }
                Event::KeyDown { keycode, scancode, .. }
                    if let Some(key) = keyboard_input_for(keycode, scancode)
                        && inputs.insert(GenericInput::Keyboard(key)) == CollectionDone::Yes =>
                {
                    return Some(inputs.consume());
                }
                Event::KeyUp { keycode, scancode, .. }
                    if let Some(key) = keyboard_input_for(keycode, scancode)
                        && inputs.contains(GenericInput::Keyboard(key)) =>
                {
                    return Some(inputs.consume());
                }
                Event::JoyDeviceAdded { which: joystick_id, .. } => {
                    if let Err(err) = joysticks.handle_device_added(joystick_id) {
                        log::error!("Error adding joystick with joystick id {joystick_id}: {err}");
                    }

                    if let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id)
                        && let Some(joystick) = joysticks.device(gamepad_idx)
                    {
                        inputs.add_device(joysticks, gamepad_idx, joystick, AXIS_DEADZONE);
                    }
                }
                Event::JoyDeviceRemoved { which: joystick_id, .. } => {
                    if let Err(err) = joysticks.handle_device_removed(joystick_id) {
                        log::error!(
                            "Error removing joystick with joystick id {joystick_id}: {err}"
                        );
                    }
                }
                Event::JoyButtonDown { which: joystick_id, button_idx, .. } => {
                    if let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id)
                        && inputs.insert(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Button(button_idx),
                        }) == CollectionDone::Yes
                    {
                        return Some(inputs.consume());
                    }
                }
                Event::JoyButtonUp { which: joystick_id, button_idx, .. } => {
                    if let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id)
                        && inputs.contains(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Button(button_idx),
                        })
                    {
                        return Some(inputs.consume());
                    }
                }
                Event::JoyAxisMotion { which: joystick_id, axis_idx, value, .. } => {
                    let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id) else {
                        continue;
                    };

                    let pressed = value.saturating_abs() > AXIS_DEADZONE;
                    if pressed {
                        let direction = AxisDirection::from_value(value);
                        if inputs.insert(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Axis(axis_idx, direction),
                        }) == CollectionDone::Yes
                        {
                            return Some(inputs.consume());
                        }
                    } else if inputs.axis_zero(gamepad_idx, axis_idx) == CollectionDone::Yes {
                        return Some(inputs.consume());
                    }
                }
                Event::JoyHatMotion { which: joystick_id, hat_idx, state, .. } => {
                    let Some(gamepad_idx) = joysticks.map_to_device_id(joystick_id) else {
                        continue;
                    };

                    if state == HatState::Centered {
                        if HatDirection::ALL.into_iter().any(|direction| {
                            inputs.contains(GenericInput::Gamepad {
                                gamepad_idx,
                                action: GamepadAction::Hat(hat_idx, direction),
                            })
                        }) {
                            return Some(inputs.consume());
                        }

                        continue;
                    }

                    if let Some(direction) = hat_direction_for(state)
                        && inputs.insert(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Hat(hat_idx, direction),
                        }) == CollectionDone::Yes
                    {
                        return Some(inputs.consume());
                    }
                }
                Event::MouseButtonDown { mouse_btn, .. }
                    if inputs.insert(GenericInput::Mouse(mouse_btn)) == CollectionDone::Yes =>
                {
                    return Some(inputs.consume());
                }
                Event::MouseButtonUp { mouse_btn, .. }
                    if inputs.contains(GenericInput::Mouse(mouse_btn)) =>
                {
                    return Some(inputs.consume());
                }
                _ => {}
            }
        }

        match window {
            CollectInputWindow::Gui { window, joysticks, .. } => {
                let result = window.update(|ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        render_input_window(joysticks, button, ui);
                    });
                });
                if let Err(err) = result {
                    log::error!("Error rendering input window: {err}");
                }
            }
            CollectInputWindow::Emulator(emulator) => {
                if let Err(err) = emulator.force_render() {
                    log::error!("Error rendering frame while collecting input: {err}");

                    // Any error encountered during rendering is generally fatal; abort
                    return None;
                }
            }
        }

        thread::sleep(Duration::from_millis(10));
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

pub struct InputWindow {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    platform: egui_sdl3_platform::Platform,
    renderer: egui_wgpu::Renderer,
    start_time: SystemTime,
    // SAFETY: The window must be declared after the surface so that the surface is dropped first
    window: Window,
}

impl InputWindow {
    pub fn new(window: Window, scale_factor: f32) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        // SAFETY: The returned surface must not outlive the window
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_display_and_window(
                &window, &window,
            )?)
        }?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..wgpu::RequestAdapterOptions::default()
        }))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        let (width, height) = window.size();

        // egui prefers non-sRGB-aware surface formats
        let surface_capabilities = surface.get_capabilities(&adapter);
        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|&format| !format.is_srgb())
            .unwrap_or(surface_capabilities.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::default(),
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let platform = egui_sdl3_platform::Platform::new(&window, scale_factor);
        let start_time = SystemTime::now();

        let renderer = egui_wgpu::Renderer::new(
            &device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

        Ok(Self { surface, surface_config, device, queue, platform, renderer, start_time, window })
    }

    pub fn update(&mut self, render_fn: impl FnMut(&mut egui::Ui)) -> anyhow::Result<()> {
        let egui_input = self.platform.take_raw_input(
            SystemTime::now().duration_since(self.start_time).unwrap_or_default().as_secs_f64(),
        );

        let full_output = self.platform.context().run_ui(egui_input, render_fn);

        let mut suboptimal_surface = false;
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output) => output,
            wgpu::CurrentSurfaceTexture::Suboptimal(output) => {
                suboptimal_surface = true;
                output
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                log::warn!("Skipping input window frame because wgpu surface timed out");
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::warn!("Skipping input window frame because wgpu surface is outdated");
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                log::debug!("Skipping input window frame because wgpu surface is occluded");
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => {
                return Err(anyhow!("wgpu surface was lost or failed validation"));
            }
        };
        let output_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let paint_jobs =
            self.platform.context().tessellate(full_output.shapes, full_output.pixels_per_point);

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: "debugger_encoder".into(),
        });

        self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..wgpu::RenderPassDescriptor::default()
            });

            // egui-wgpu requires a RenderPass with static lifetime
            let mut render_pass = render_pass.forget_lifetime();

            self.renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }

        if suboptimal_surface {
            self.surface.configure(&self.device, &self.surface_config);
        }

        Ok(())
    }

    pub fn handle_sdl_event(&mut self, event: &Event) {
        match event {
            Event::Window {
                window_id,
                win_event: WindowEvent::Resized(..) | WindowEvent::PixelSizeChanged(..),
                ..
            } if *window_id == self.window.id() => {
                let (width, height) = self.window.size();
                self.surface_config.width = width;
                self.surface_config.height = height;
                self.surface.configure(&self.device, &self.surface_config);
            }
            _ => {}
        }

        self.platform.handle_event(event);
    }
}
