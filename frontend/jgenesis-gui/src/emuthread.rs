use anyhow::anyhow;
use jgenesis_native_driver::config::input::{
    AxisDirection, HatDirection, JoystickAction, JoystickInput, KeyboardInput, KeyboardOrMouseInput,
};
use jgenesis_native_driver::config::{
    GenesisConfig, NesConfig, SegaCdConfig, SmsGgConfig, SnesConfig,
};
use jgenesis_native_driver::input::Joysticks;
use jgenesis_native_driver::{
    AudioError, NativeEmulatorResult, NativeGenesisEmulator, NativeNesEmulator,
    NativeSegaCdEmulator, NativeSmsGgEmulator, NativeSnesEmulator, NativeTickEffect,
};
use sdl2::event::Event;
use sdl2::joystick::HatState;
use sdl2::mouse::MouseButton;
use sdl2::pixels::Color;
use sdl2::render::WindowCanvas;
use sdl2::{EventPump, JoystickSubsystem};
use segacd_core::api::DiscResult;
use std::path::PathBuf;
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
    RunningNes = 4,
    RunningSnes = 5,
}

impl EmuThreadStatus {
    fn from_discriminant(discriminant: u8) -> Self {
        match discriminant {
            0 => Self::Idle,
            1 => Self::RunningSmsGg,
            2 => Self::RunningGenesis,
            3 => Self::RunningSegaCd,
            4 => Self::RunningNes,
            5 => Self::RunningSnes,
            _ => panic!("invalid status discriminant: {discriminant}"),
        }
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::RunningSmsGg
                | Self::RunningGenesis
                | Self::RunningSegaCd
                | Self::RunningNes
                | Self::RunningSnes
        )
    }
}

#[derive(Debug, Clone)]
pub enum EmuThreadCommand {
    RunSms(Box<SmsGgConfig>),
    RunGenesis(Box<GenesisConfig>),
    RunSegaCd(Box<SegaCdConfig>),
    RunNes(Box<NesConfig>),
    RunSnes(Box<SnesConfig>),
    ReloadSmsGgConfig(Box<SmsGgConfig>),
    ReloadGenesisConfig(Box<GenesisConfig>),
    ReloadSegaCdConfig(Box<SegaCdConfig>),
    ReloadNesConfig(Box<NesConfig>),
    ReloadSnesConfig(Box<SnesConfig>),
    StopEmulator,
    CollectInput { input_type: InputType, axis_deadzone: i16, ctx: egui::Context },
    SoftReset,
    HardReset,
    OpenMemoryViewer,
    SegaCdRemoveDisc,
    SegaCdChangeDisc(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    Keyboard,
    Joystick,
    KeyboardOrMouse,
}

impl InputType {
    fn accepts_keyboard(self) -> bool {
        matches!(self, Self::Keyboard | Self::KeyboardOrMouse)
    }
}

#[derive(Debug, Clone)]
pub enum GenericInput {
    Keyboard(KeyboardInput),
    Joystick(JoystickInput),
    KeyboardOrMouse(KeyboardOrMouseInput),
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
        nes_config: Box<NesConfig>,
        snes_config: Box<SnesConfig>,
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
            EmuThreadStatus::RunningNes => {
                self.send(EmuThreadCommand::ReloadNesConfig(nes_config));
            }
            EmuThreadStatus::RunningSnes => {
                self.send(EmuThreadCommand::ReloadSnesConfig(snes_config));
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
                        GenericEmulator::SmsGg(emulator),
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
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
                        GenericEmulator::Genesis(emulator),
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
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
                        GenericEmulator::SegaCd(emulator),
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
                    );
                }
                Ok(EmuThreadCommand::RunNes(config)) => {
                    status.store(EmuThreadStatus::RunningNes as u8, Ordering::Relaxed);

                    let emulator = match jgenesis_native_driver::create_nes(config) {
                        Ok(emulator) => emulator,
                        Err(err) => {
                            log::error!("Error initializing NES emulator: {err}");
                            *emulator_error.lock().unwrap() = Some(err.into());
                            continue;
                        }
                    };
                    run_emulator(
                        GenericEmulator::Nes(emulator),
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
                    );
                }
                Ok(EmuThreadCommand::RunSnes(config)) => {
                    status.store(EmuThreadStatus::RunningSnes as u8, Ordering::Relaxed);

                    let emulator = match jgenesis_native_driver::create_snes(config) {
                        Ok(emulator) => emulator,
                        Err(err) => {
                            log::error!("Error initializing SNES emulator: {err}");
                            *emulator_error.lock().unwrap() = Some(err.into());
                            continue;
                        }
                    };
                    run_emulator(
                        GenericEmulator::Snes(emulator),
                        &command_receiver,
                        &input_sender,
                        &emulator_error,
                    );
                }
                Ok(EmuThreadCommand::CollectInput { input_type, axis_deadzone, ctx }) => {
                    match collect_input_not_running(input_type, axis_deadzone) {
                        Ok(input) => {
                            input_sender.send(input).unwrap();
                            ctx.request_repaint();
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
                    | EmuThreadCommand::ReloadNesConfig(_)
                    | EmuThreadCommand::ReloadSnesConfig(_)
                    | EmuThreadCommand::SoftReset
                    | EmuThreadCommand::HardReset
                    | EmuThreadCommand::OpenMemoryViewer
                    | EmuThreadCommand::SegaCdRemoveDisc
                    | EmuThreadCommand::SegaCdChangeDisc(_),
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

enum GenericEmulator {
    SmsGg(NativeSmsGgEmulator),
    Genesis(NativeGenesisEmulator),
    SegaCd(NativeSegaCdEmulator),
    Nes(NativeNesEmulator),
    Snes(NativeSnesEmulator),
}

macro_rules! match_each_emulator_variant {
    ($value:expr, $emulator:ident => $expr:expr) => {
        match $value {
            GenericEmulator::SmsGg($emulator) => $expr,
            GenericEmulator::Genesis($emulator) => $expr,
            GenericEmulator::SegaCd($emulator) => $expr,
            GenericEmulator::Nes($emulator) => $expr,
            GenericEmulator::Snes($emulator) => $expr,
        }
    };
}

impl GenericEmulator {
    fn reload_smsgg_config(&mut self, config: Box<SmsGgConfig>) -> Result<(), AudioError> {
        if let Self::SmsGg(emulator) = self {
            emulator.reload_smsgg_config(config)?;
        }

        Ok(())
    }

    fn reload_genesis_config(&mut self, config: Box<GenesisConfig>) -> Result<(), AudioError> {
        if let Self::Genesis(emulator) = self {
            emulator.reload_genesis_config(config)?;
        }

        Ok(())
    }

    fn reload_sega_cd_config(&mut self, config: Box<SegaCdConfig>) -> Result<(), AudioError> {
        if let Self::SegaCd(emulator) = self {
            emulator.reload_sega_cd_config(config)?;
        }

        Ok(())
    }

    fn reload_nes_config(&mut self, config: Box<NesConfig>) -> Result<(), AudioError> {
        if let Self::Nes(emulator) = self {
            emulator.reload_nes_config(config)?;
        }

        Ok(())
    }

    fn reload_snes_config(&mut self, config: Box<SnesConfig>) -> Result<(), AudioError> {
        if let Self::Snes(emulator) = self {
            emulator.reload_snes_config(config)?;
        }

        Ok(())
    }

    fn remove_disc(&mut self) {
        if let Self::SegaCd(emulator) = self {
            emulator.remove_disc();
        }
    }

    fn change_disc(&mut self, path: PathBuf) -> DiscResult<()> {
        if let Self::SegaCd(emulator) = self {
            emulator.change_disc(path)?;
        }

        Ok(())
    }

    fn render_frame(&mut self) -> NativeEmulatorResult<NativeTickEffect> {
        match_each_emulator_variant!(self, emulator => emulator.render_frame())
    }

    fn soft_reset(&mut self) {
        match_each_emulator_variant!(self, emulator => emulator.soft_reset());
    }

    fn hard_reset(&mut self) {
        match_each_emulator_variant!(self, emulator => emulator.hard_reset());
    }

    fn open_memory_viewer(&mut self) {
        match_each_emulator_variant!(self, emulator => emulator.open_memory_viewer());
    }

    fn focus(&mut self) {
        match_each_emulator_variant!(self, emulator => emulator.focus());
    }

    fn event_pump_and_joysticks_mut(
        &mut self,
    ) -> (&mut EventPump, &mut Joysticks, &JoystickSubsystem) {
        match_each_emulator_variant!(self, emulator => emulator.event_pump_and_joysticks_mut())
    }
}

fn run_emulator(
    mut emulator: GenericEmulator,
    command_receiver: &Receiver<EmuThreadCommand>,
    input_sender: &Sender<Option<GenericInput>>,
    emulator_error: &Arc<Mutex<Option<anyhow::Error>>>,
) {
    loop {
        match emulator.render_frame() {
            Ok(NativeTickEffect::None) => {
                while let Ok(command) = command_receiver.try_recv() {
                    match command {
                        EmuThreadCommand::ReloadSmsGgConfig(config) => {
                            if let Err(err) = emulator.reload_smsgg_config(config) {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::ReloadGenesisConfig(config) => {
                            if let Err(err) = emulator.reload_genesis_config(config) {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::ReloadSegaCdConfig(config) => {
                            if let Err(err) = emulator.reload_sega_cd_config(config) {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::ReloadNesConfig(config) => {
                            if let Err(err) = emulator.reload_nes_config(config) {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::ReloadSnesConfig(config) => {
                            if let Err(err) = emulator.reload_snes_config(config) {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::StopEmulator => {
                            log::info!("Stopping emulator");
                            return;
                        }
                        EmuThreadCommand::CollectInput { input_type, axis_deadzone, ctx } => {
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
                            ctx.request_repaint();

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
                        EmuThreadCommand::OpenMemoryViewer => {
                            emulator.open_memory_viewer();
                        }
                        EmuThreadCommand::SegaCdRemoveDisc => {
                            emulator.remove_disc();
                        }
                        EmuThreadCommand::SegaCdChangeDisc(path) => {
                            if let Err(err) = emulator.change_disc(path) {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
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
                Event::KeyDown { keycode: Some(keycode), .. } if input_type.accepts_keyboard() => {
                    return Some(match input_type {
                        InputType::Keyboard => {
                            GenericInput::Keyboard(KeyboardInput { keycode: keycode.name() })
                        }
                        InputType::KeyboardOrMouse => GenericInput::KeyboardOrMouse(
                            KeyboardOrMouseInput::Keyboard(keycode.name()),
                        ),
                        InputType::Joystick => unreachable!("nested match arms"),
                    });
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
                Event::MouseButtonDown { mouse_btn, .. }
                    if input_type == InputType::KeyboardOrMouse =>
                {
                    match mouse_btn {
                        MouseButton::Left => {
                            return Some(GenericInput::KeyboardOrMouse(
                                KeyboardOrMouseInput::MouseLeft,
                            ));
                        }
                        MouseButton::Right => {
                            return Some(GenericInput::KeyboardOrMouse(
                                KeyboardOrMouseInput::MouseRight,
                            ));
                        }
                        MouseButton::Middle => {
                            return Some(GenericInput::KeyboardOrMouse(
                                KeyboardOrMouseInput::MouseMiddle,
                            ));
                        }
                        MouseButton::X1 => {
                            return Some(GenericInput::KeyboardOrMouse(
                                KeyboardOrMouseInput::MouseX1,
                            ));
                        }
                        MouseButton::X2 => {
                            return Some(GenericInput::KeyboardOrMouse(
                                KeyboardOrMouseInput::MouseX2,
                            ));
                        }
                        MouseButton::Unknown => {}
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
