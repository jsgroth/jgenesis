use anyhow::anyhow;
use jgenesis_native_driver::config::input::{
    AxisDirection, HatDirection, JoystickAction, JoystickInput, KeyboardInput,
};
use jgenesis_native_driver::config::{GenesisConfig, SegaCdConfig, SmsGgConfig};
use jgenesis_native_driver::input::{Clearable, GetButtonField, Joysticks};
use jgenesis_native_driver::{
    AudioError, NativeEmulator, NativeGenesisEmulator, NativeSegaCdEmulator, NativeSmsGgEmulator,
    NativeTickEffect, SaveWriteError,
};
use jgenesis_renderer::renderer::RendererError;
use jgenesis_traits::frontend::EmulatorTrait;
use sdl2::event::Event;
use sdl2::joystick::HatState;
use sdl2::pixels::Color;
use sdl2::render::WindowCanvas;
use sdl2::{EventPump, JoystickSubsystem};
use std::error::Error;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmuThreadStatus {
    Idle = 0,
    RunningSmsGg = 1,
    RunningGenesis = 2,
    RunningSegaCd = 3,
}

impl EmuThreadStatus {
    fn from_discriminant(discriminant: u8) -> Self {
        match discriminant {
            0 => Self::Idle,
            1 => Self::RunningSmsGg,
            2 => Self::RunningGenesis,
            3 => Self::RunningSegaCd,
            _ => panic!("invalid status discriminant: {discriminant}"),
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, Self::RunningSmsGg | Self::RunningGenesis | Self::RunningSegaCd)
    }
}

#[derive(Debug, Clone)]
pub enum EmuThreadCommand {
    RunSms(Box<SmsGgConfig>),
    RunGenesis(Box<GenesisConfig>),
    RunSegaCd(Box<SegaCdConfig>),
    ReloadSmsGgConfig(Box<SmsGgConfig>),
    ReloadGenesisConfig(Box<GenesisConfig>),
    ReloadSegaCdConfig(Box<SegaCdConfig>),
    StopEmulator,
    CollectInput { input_type: InputType, axis_deadzone: i16 },
    SoftReset,
    HardReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    Keyboard,
    Joystick,
}

#[derive(Debug, Clone)]
pub enum GenericInput {
    Keyboard(KeyboardInput),
    Joystick(JoystickInput),
}

pub struct EmuThreadHandle {
    status: Arc<AtomicU8>,
    command_sender: Sender<EmuThreadCommand>,
    input_receiver: Receiver<Option<GenericInput>>,
    emulator_error: Arc<Mutex<Option<anyhow::Error>>>,
}

impl EmuThreadHandle {
    pub fn send(&self, command: EmuThreadCommand) {
        self.command_sender.send(command).unwrap();
    }

    pub fn status(&self) -> EmuThreadStatus {
        EmuThreadStatus::from_discriminant(self.status.load(Ordering::Relaxed))
    }

    pub fn lock_emulator_error(&mut self) -> MutexGuard<'_, Option<anyhow::Error>> {
        self.emulator_error.lock().unwrap()
    }

    pub fn poll_input_receiver(&self) -> Result<Option<GenericInput>, TryRecvError> {
        self.input_receiver.try_recv()
    }

    pub fn stop_emulator_if_running(&self) {
        if self.status().is_running() {
            self.send(EmuThreadCommand::StopEmulator);
        }
    }

    pub fn reload_config(
        &self,
        smsgg_config: Box<SmsGgConfig>,
        genesis_config: Box<GenesisConfig>,
        sega_cd_config: Box<SegaCdConfig>,
    ) {
        match self.status() {
            EmuThreadStatus::RunningSmsGg => {
                self.send(EmuThreadCommand::ReloadSmsGgConfig(smsgg_config));
            }
            EmuThreadStatus::RunningGenesis => {
                self.send(EmuThreadCommand::ReloadGenesisConfig(genesis_config));
            }
            EmuThreadStatus::RunningSegaCd => {
                self.send(EmuThreadCommand::ReloadSegaCdConfig(sega_cd_config));
            }
            EmuThreadStatus::Idle => {}
        }
    }
}

pub fn spawn() -> EmuThreadHandle {
    let status_arc = Arc::new(AtomicU8::new(EmuThreadStatus::Idle as u8));
    let (command_sender, command_receiver) = mpsc::channel();
    let (input_sender, input_receiver) = mpsc::channel();
    let emulator_error_arc = Arc::new(Mutex::new(None));

    let status = Arc::clone(&status_arc);
    let emulator_error = Arc::clone(&emulator_error_arc);
    thread::spawn(move || {
        loop {
            status.store(EmuThreadStatus::Idle as u8, Ordering::Relaxed);

            match command_receiver.recv() {
                Ok(EmuThreadCommand::RunSms(config)) => {
                    status.store(EmuThreadStatus::RunningSmsGg as u8, Ordering::Relaxed);

                    let emulator = match jgenesis_native_driver::create_smsgg(config) {
                        Ok(emulator) => emulator,
                        Err(err) => {
                            log::error!("Error initializing SMS/GG emulator: {err}");
                            *emulator_error.lock().unwrap() = Some(err.into());
                            continue;
                        }
                    };
                    run_emulator(
                        emulator,
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
                        smsgg_reload_handler,
                    );
                }
                Ok(EmuThreadCommand::RunGenesis(config)) => {
                    status.store(EmuThreadStatus::RunningGenesis as u8, Ordering::Relaxed);

                    let emulator = match jgenesis_native_driver::create_genesis(config) {
                        Ok(emulator) => emulator,
                        Err(err) => {
                            log::error!("Error initializing Genesis emulator: {err}");
                            *emulator_error.lock().unwrap() = Some(err.into());
                            continue;
                        }
                    };
                    run_emulator(
                        emulator,
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
                        genesis_reload_handler,
                    );
                }
                Ok(EmuThreadCommand::RunSegaCd(config)) => {
                    status.store(EmuThreadStatus::RunningSegaCd as u8, Ordering::Relaxed);

                    let emulator = match jgenesis_native_driver::create_sega_cd(config) {
                        Ok(emulator) => emulator,
                        Err(err) => {
                            log::error!("Error initializing Sega CD emulator: {err}");
                            *emulator_error.lock().unwrap() = Some(err.into());
                            continue;
                        }
                    };
                    run_emulator(
                        emulator,
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
                        sega_cd_reload_handler,
                    );
                }
                Ok(EmuThreadCommand::CollectInput { input_type, axis_deadzone }) => {
                    match collect_input_not_running(input_type, axis_deadzone) {
                        Ok(input) => {
                            input_sender.send(input).unwrap();
                        }
                        Err(err) => {
                            log::error!("Error collecting SDL2 input: {err}");
                        }
                    }
                }
                Ok(
                    EmuThreadCommand::StopEmulator
                    | EmuThreadCommand::ReloadSmsGgConfig(_)
                    | EmuThreadCommand::ReloadGenesisConfig(_)
                    | EmuThreadCommand::ReloadSegaCdConfig(_)
                    | EmuThreadCommand::SoftReset
                    | EmuThreadCommand::HardReset,
                ) => {}
                Err(err) => {
                    log::info!(
                        "Error receiving command in emulation thread, probably caused by closing main window: {err}"
                    );
                    break;
                }
            }
        }
    });

    EmuThreadHandle {
        command_sender,
        status: status_arc,
        input_receiver,
        emulator_error: emulator_error_arc,
    }
}

#[derive(Debug, Clone)]
enum GenericConfig {
    SmsGg(Box<SmsGgConfig>),
    Genesis(Box<GenesisConfig>),
    SegaCd(Box<SegaCdConfig>),
}

fn smsgg_reload_handler(
    emulator: &mut NativeSmsGgEmulator,
    config: GenericConfig,
) -> Result<(), AudioError> {
    if let GenericConfig::SmsGg(config) = config {
        emulator.reload_smsgg_config(config)?;
    }

    Ok(())
}

fn genesis_reload_handler(
    emulator: &mut NativeGenesisEmulator,
    config: GenericConfig,
) -> Result<(), AudioError> {
    if let GenericConfig::Genesis(config) = config {
        emulator.reload_genesis_config(config)?;
    }

    Ok(())
}

fn sega_cd_reload_handler(
    emulator: &mut NativeSegaCdEmulator,
    config: GenericConfig,
) -> Result<(), AudioError> {
    if let GenericConfig::SegaCd(config) = config {
        emulator.reload_sega_cd_config(config)?;
    }

    Ok(())
}

type ConfigReloadHandler<Inputs, Button, Config, Emulator> = fn(
    &mut NativeEmulator<Inputs, Button, Config, Emulator>,
    GenericConfig,
) -> Result<(), AudioError>;

fn run_emulator<Inputs, Button, Config, Emulator>(
    mut emulator: NativeEmulator<Inputs, Button, Config, Emulator>,
    command_receiver: &Receiver<EmuThreadCommand>,
    input_sender: &Sender<Option<GenericInput>>,
    emulator_error: &Arc<Mutex<Option<anyhow::Error>>>,
    config_reload_handler: ConfigReloadHandler<Inputs, Button, Config, Emulator>,
) where
    Inputs: Clearable + GetButtonField<Button>,
    Button: Copy,
    Emulator: EmulatorTrait<EmulatorInputs = Inputs, EmulatorConfig = Config>,
    Emulator::Err<RendererError, AudioError, SaveWriteError>: Error + Send + Sync + 'static,
{
    loop {
        match emulator.render_frame() {
            Ok(NativeTickEffect::None) => {
                while let Ok(command) = command_receiver.try_recv() {
                    match command {
                        EmuThreadCommand::ReloadSmsGgConfig(config) => {
                            if let Err(err) =
                                config_reload_handler(&mut emulator, GenericConfig::SmsGg(config))
                            {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::ReloadGenesisConfig(config) => {
                            if let Err(err) =
                                config_reload_handler(&mut emulator, GenericConfig::Genesis(config))
                            {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::ReloadSegaCdConfig(config) => {
                            if let Err(err) =
                                config_reload_handler(&mut emulator, GenericConfig::SegaCd(config))
                            {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::StopEmulator => {
                            log::info!("Stopping emulator");
                            return;
                        }
                        EmuThreadCommand::CollectInput { input_type, axis_deadzone } => {
                            log::debug!("Received collect input command");

                            emulator.focus();
                            let (event_pump, joysticks, joystick_subsystem) =
                                emulator.event_pump_and_joysticks_mut();
                            let input = collect_input(
                                input_type,
                                event_pump,
                                joysticks,
                                joystick_subsystem,
                                axis_deadzone,
                                None,
                            );

                            let is_none = input.is_none();

                            log::debug!("Sending collect input result {input:?}");
                            input_sender.send(input).unwrap();

                            if is_none {
                                // Window was closed
                                return;
                            }
                        }
                        EmuThreadCommand::SoftReset => {
                            emulator.soft_reset();
                        }
                        EmuThreadCommand::HardReset => {
                            emulator.hard_reset();
                        }
                        _ => {}
                    }
                }
            }
            Ok(NativeTickEffect::Exit) => {
                return;
            }
            Err(err) => {
                log::error!("Emulator terminated with an error: {err}");
                *emulator_error.lock().unwrap() = Some(err.into());
                return;
            }
        }
    }
}

fn collect_input_not_running(
    input_type: InputType,
    axis_deadzone: i16,
) -> anyhow::Result<Option<GenericInput>> {
    let sdl = sdl2::init().map_err(|err| anyhow!("Error initializing SDL2: {err}"))?;
    let video =
        sdl.video().map_err(|err| anyhow!("Error initializing SDL2 video subsystem: {err}"))?;
    let joystick_subsystem = sdl
        .joystick()
        .map_err(|err| anyhow!("Error initializing SDL2 joystick subsystem: {err}"))?;
    let mut event_pump =
        sdl.event_pump().map_err(|err| anyhow!("Error initializing SDL2 event pump: {err}"))?;

    let mut canvas =
        video.window("SDL input configuration", 200, 100).build()?.into_canvas().build()?;
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas.present();

    let mut joysticks = Joysticks::new();

    let input = collect_input(
        input_type,
        &mut event_pump,
        &mut joysticks,
        &joystick_subsystem,
        axis_deadzone,
        Some(&mut canvas),
    );

    for _ in event_pump.poll_iter() {}

    Ok(input)
}

// Some gamepads report phantom inputs right after connecting; use a timestamp threshold to avoid
// collecting those
const TIMESTAMP_THRESHOLD: u32 = 1000;

fn collect_input(
    input_type: InputType,
    event_pump: &mut EventPump,
    joysticks: &mut Joysticks,
    joystick_subsystem: &JoystickSubsystem,
    axis_deadzone: i16,
    mut canvas: Option<&mut WindowCanvas>,
) -> Option<GenericInput> {
    loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    return None;
                }
                Event::KeyDown { keycode: Some(keycode), .. }
                    if input_type == InputType::Keyboard =>
                {
                    return Some(GenericInput::Keyboard(KeyboardInput { keycode: keycode.name() }));
                }
                Event::JoyDeviceAdded { which: device_id, .. } => {
                    if let Err(err) = joysticks.device_added(device_id, joystick_subsystem) {
                        log::error!("Error adding joystick with device id {device_id}: {err}");
                    }
                }
                Event::JoyDeviceRemoved { which: instance_id, .. } => {
                    joysticks.device_removed(instance_id);
                }
                Event::JoyButtonDown { which: instance_id, button_idx, timestamp }
                    if timestamp > TIMESTAMP_THRESHOLD && input_type == InputType::Joystick =>
                {
                    if let Some(device_id) = joysticks.device_id_for(instance_id) {
                        if let Some(joystick_id) = joysticks.get_joystick_id(device_id) {
                            return Some(GenericInput::Joystick(JoystickInput {
                                device: joystick_id,
                                action: JoystickAction::Button { button_idx },
                            }));
                        }
                    }
                }
                Event::JoyAxisMotion { which: instance_id, axis_idx, value, timestamp }
                    if timestamp > TIMESTAMP_THRESHOLD
                        && input_type == InputType::Joystick
                        && value.saturating_abs() > axis_deadzone =>
                {
                    if let Some(device_id) = joysticks.device_id_for(instance_id) {
                        if let Some(joystick_id) = joysticks.get_joystick_id(device_id) {
                            let direction = if value < 0 {
                                AxisDirection::Negative
                            } else {
                                AxisDirection::Positive
                            };
                            return Some(GenericInput::Joystick(JoystickInput {
                                device: joystick_id,
                                action: JoystickAction::Axis { axis_idx, direction },
                            }));
                        }
                    }
                }
                Event::JoyHatMotion { which: instance_id, hat_idx, state, timestamp }
                    if timestamp > TIMESTAMP_THRESHOLD && input_type == InputType::Joystick =>
                {
                    if let Some(direction) = hat_direction_for(state) {
                        if let Some(device_id) = joysticks.device_id_for(instance_id) {
                            if let Some(joystick_id) = joysticks.get_joystick_id(device_id) {
                                return Some(GenericInput::Joystick(JoystickInput {
                                    device: joystick_id,
                                    action: JoystickAction::Hat { hat_idx, direction },
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(canvas) = &mut canvas {
            canvas.clear();
            canvas.present();
        }

        thread::sleep(Duration::from_millis(1));
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
