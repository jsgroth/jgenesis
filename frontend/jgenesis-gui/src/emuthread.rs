use anyhow::anyhow;
use jgenesis_native_driver::config::input::{
    AxisDirection, HatDirection, JoystickAction, JoystickInput, KeyboardInput, KeyboardOrMouseInput,
};
use jgenesis_native_driver::config::{
    GameBoyConfig, GenesisConfig, NesConfig, Sega32XConfig, SegaCdConfig, SmsGgConfig, SnesConfig,
};
use jgenesis_native_driver::input::Joysticks;
use jgenesis_native_driver::{
    AudioError, Native32XEmulator, NativeEmulatorResult, NativeGameBoyEmulator,
    NativeGenesisEmulator, NativeNesEmulator, NativeSegaCdEmulator, NativeSmsGgEmulator,
    NativeSnesEmulator, NativeTickEffect, SaveStateMetadata,
};
use sdl2::event::Event;
use sdl2::joystick::HatState;
use sdl2::mouse::MouseButton;
use sdl2::pixels::Color;
use sdl2::render::WindowCanvas;
use sdl2::{EventPump, JoystickSubsystem};
use segacd_core::api::SegaCdLoadResult;
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
    Running32X = 4,
    RunningNes = 5,
    RunningSnes = 6,
    RunningGameBoy = 7,
    WaitingForFirstCommand = 8,
}

impl EmuThreadStatus {
    fn from_discriminant(discriminant: u8) -> Self {
        match discriminant {
            0 => Self::Idle,
            1 => Self::RunningSmsGg,
            2 => Self::RunningGenesis,
            3 => Self::RunningSegaCd,
            4 => Self::Running32X,
            5 => Self::RunningNes,
            6 => Self::RunningSnes,
            7 => Self::RunningGameBoy,
            8 => Self::WaitingForFirstCommand,
            _ => panic!("invalid status discriminant: {discriminant}"),
        }
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::RunningSmsGg
                | Self::RunningGenesis
                | Self::RunningSegaCd
                | Self::Running32X
                | Self::RunningNes
                | Self::RunningSnes
                | Self::RunningGameBoy
        )
    }
}

#[derive(Debug, Clone)]
pub enum EmuThreadCommand {
    RunSms(Box<SmsGgConfig>),
    RunGenesis(Box<GenesisConfig>),
    RunSegaCd(Box<SegaCdConfig>),
    Run32X(Box<Sega32XConfig>),
    RunNes(Box<NesConfig>),
    RunSnes(Box<SnesConfig>),
    RunGameBoy(Box<GameBoyConfig>),
    ReloadSmsGgConfig(Box<SmsGgConfig>),
    ReloadGenesisConfig(Box<GenesisConfig>),
    ReloadSegaCdConfig(Box<SegaCdConfig>),
    Reload32XConfig(Box<Sega32XConfig>),
    ReloadNesConfig(Box<NesConfig>),
    ReloadSnesConfig(Box<SnesConfig>),
    ReloadGameBoyConfig(Box<GameBoyConfig>),
    StopEmulator,
    CollectInput { input_type: InputType, axis_deadzone: i16 },
    SoftReset,
    HardReset,
    OpenMemoryViewer,
    SaveState { slot: usize },
    LoadState { slot: usize },
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
    save_state_metadata: Arc<Mutex<SaveStateMetadata>>,
    emulator_error: Arc<Mutex<Option<anyhow::Error>>>,
}

impl EmuThreadHandle {
    pub fn send(&self, command: EmuThreadCommand) {
        self.command_sender.send(command).unwrap();
    }

    pub fn status(&self) -> EmuThreadStatus {
        EmuThreadStatus::from_discriminant(self.status.load(Ordering::Relaxed))
    }

    pub fn save_state_metadata(&self) -> SaveStateMetadata {
        self.save_state_metadata.lock().unwrap().clone()
    }

    pub fn lock_emulator_error(&mut self) -> MutexGuard<'_, Option<anyhow::Error>> {
        self.emulator_error.lock().unwrap()
    }

    pub fn poll_input_receiver(&self) -> Result<Option<GenericInput>, TryRecvError> {
        self.input_receiver.try_recv()
    }

    pub fn clear_waiting_for_first_command(&self) {
        let _ = self.status.compare_exchange(
            EmuThreadStatus::WaitingForFirstCommand as u8,
            EmuThreadStatus::Idle as u8,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    pub fn stop_emulator_if_running(&self) {
        if self.status().is_running() {
            self.send(EmuThreadCommand::StopEmulator);
        }
    }

    // TODO fix this
    #[allow(clippy::too_many_arguments)]
    pub fn reload_config(
        &self,
        smsgg_config: Box<SmsGgConfig>,
        genesis_config: Box<GenesisConfig>,
        sega_cd_config: Box<SegaCdConfig>,
        s32x_config: Box<Sega32XConfig>,
        nes_config: Box<NesConfig>,
        snes_config: Box<SnesConfig>,
        gb_config: Box<GameBoyConfig>,
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
            EmuThreadStatus::Running32X => {
                self.send(EmuThreadCommand::Reload32XConfig(s32x_config));
            }
            EmuThreadStatus::RunningNes => {
                self.send(EmuThreadCommand::ReloadNesConfig(nes_config));
            }
            EmuThreadStatus::RunningSnes => {
                self.send(EmuThreadCommand::ReloadSnesConfig(snes_config));
            }
            EmuThreadStatus::RunningGameBoy => {
                self.send(EmuThreadCommand::ReloadGameBoyConfig(gb_config));
            }
            EmuThreadStatus::Idle | EmuThreadStatus::WaitingForFirstCommand => {}
        }
    }
}

pub fn spawn(ctx: egui::Context) -> EmuThreadHandle {
    let status = Arc::new(AtomicU8::new(EmuThreadStatus::WaitingForFirstCommand as u8));
    let (command_sender, command_receiver) = mpsc::channel();
    let (input_sender, input_receiver) = mpsc::channel();
    let save_state_metadata = Arc::new(Mutex::new(SaveStateMetadata::default()));
    let emulator_error = Arc::new(Mutex::new(None));

    {
        let status = Arc::clone(&status);
        let save_state_metadata = Arc::clone(&save_state_metadata);
        let emulator_error = Arc::clone(&emulator_error);
        thread::spawn(move || {
            thread_run(
                ctx,
                command_receiver,
                input_sender,
                status,
                save_state_metadata,
                emulator_error,
            );
        });
    }

    EmuThreadHandle { status, command_sender, input_receiver, save_state_metadata, emulator_error }
}

fn thread_run(
    ctx: egui::Context,
    command_receiver: Receiver<EmuThreadCommand>,
    input_sender: Sender<Option<GenericInput>>,
    status: Arc<AtomicU8>,
    save_state_metadata: Arc<Mutex<SaveStateMetadata>>,
    emulator_error: Arc<Mutex<Option<anyhow::Error>>>,
) {
    loop {
        if status.load(Ordering::Relaxed) != EmuThreadStatus::WaitingForFirstCommand as u8 {
            status.store(EmuThreadStatus::Idle as u8, Ordering::Relaxed);
        }

        // Force a repaint at the start of the loop so that the GUI will always repaint after an
        // emulator exits. This will immediately display the error window if there was an
        // error, and it will also force quit immediately if auto-close is enabled
        ctx.request_repaint();

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
                    &save_state_metadata,
                    &emulator_error,
                    &ctx,
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
                    &save_state_metadata,
                    &emulator_error,
                    &ctx,
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
                    &save_state_metadata,
                    &emulator_error,
                    &ctx,
                );
            }
            Ok(EmuThreadCommand::Run32X(config)) => {
                status.store(EmuThreadStatus::Running32X as u8, Ordering::Relaxed);

                let emulator = match jgenesis_native_driver::create_32x(config) {
                    Ok(emulator) => emulator,
                    Err(err) => {
                        log::error!("Error initializing 32X emulator: {err}");
                        *emulator_error.lock().unwrap() = Some(err.into());
                        continue;
                    }
                };
                run_emulator(
                    GenericEmulator::Sega32X(emulator),
                    &command_receiver,
                    &input_sender,
                    &save_state_metadata,
                    &emulator_error,
                    &ctx,
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
                    &save_state_metadata,
                    &emulator_error,
                    &ctx,
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
                    &save_state_metadata,
                    &emulator_error,
                    &ctx,
                );
            }
            Ok(EmuThreadCommand::RunGameBoy(config)) => {
                status.store(EmuThreadStatus::RunningGameBoy as u8, Ordering::Relaxed);

                let emulator = match jgenesis_native_driver::create_gb(config) {
                    Ok(emulator) => emulator,
                    Err(err) => {
                        log::error!("Error initializing Game Boy emulator: {err}");
                        *emulator_error.lock().unwrap() = Some(err.into());
                        continue;
                    }
                };
                run_emulator(
                    GenericEmulator::GameBoy(emulator),
                    &command_receiver,
                    &input_sender,
                    &save_state_metadata,
                    &emulator_error,
                    &ctx,
                );
            }
            Ok(EmuThreadCommand::CollectInput { input_type, axis_deadzone }) => {
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
                | EmuThreadCommand::Reload32XConfig(_)
                | EmuThreadCommand::ReloadNesConfig(_)
                | EmuThreadCommand::ReloadSnesConfig(_)
                | EmuThreadCommand::ReloadGameBoyConfig(_)
                | EmuThreadCommand::SoftReset
                | EmuThreadCommand::HardReset
                | EmuThreadCommand::OpenMemoryViewer
                | EmuThreadCommand::SaveState { .. }
                | EmuThreadCommand::LoadState { .. }
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
}

enum GenericEmulator {
    SmsGg(NativeSmsGgEmulator),
    Genesis(NativeGenesisEmulator),
    SegaCd(NativeSegaCdEmulator),
    Sega32X(Native32XEmulator),
    Nes(NativeNesEmulator),
    Snes(NativeSnesEmulator),
    GameBoy(NativeGameBoyEmulator),
}

macro_rules! match_each_emulator_variant {
    ($value:expr, $emulator:ident => $expr:expr) => {
        match $value {
            GenericEmulator::SmsGg($emulator) => $expr,
            GenericEmulator::Genesis($emulator) => $expr,
            GenericEmulator::SegaCd($emulator) => $expr,
            GenericEmulator::Sega32X($emulator) => $expr,
            GenericEmulator::Nes($emulator) => $expr,
            GenericEmulator::Snes($emulator) => $expr,
            GenericEmulator::GameBoy($emulator) => $expr,
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

    fn reload_32x_config(&mut self, config: Box<Sega32XConfig>) -> Result<(), AudioError> {
        if let Self::Sega32X(emulator) = self {
            emulator.reload_32x_config(config)?;
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

    fn reload_gb_config(&mut self, config: Box<GameBoyConfig>) -> Result<(), AudioError> {
        if let Self::GameBoy(emulator) = self {
            emulator.reload_gb_config(config)?;
        }

        Ok(())
    }

    fn remove_disc(&mut self) {
        if let Self::SegaCd(emulator) = self {
            emulator.remove_disc();
        }
    }

    fn change_disc(&mut self, path: PathBuf) -> SegaCdLoadResult<()> {
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

    fn save_state(&mut self, slot: usize) {
        if let Err(err) = match_each_emulator_variant!(self, emulator => emulator.save_state(slot))
        {
            log::error!("Failed to save state to slot {slot}: {err}");
        }
    }

    fn load_state(&mut self, slot: usize) {
        if let Err(err) = match_each_emulator_variant!(self, emulator => emulator.load_state(slot))
        {
            log::error!("Failed to load state from slot {slot}: {err}");
        }
    }

    fn save_state_metadata(&self) -> SaveStateMetadata {
        match_each_emulator_variant!(self, emulator => emulator.save_state_metadata().clone())
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
    save_state_metadata: &Arc<Mutex<SaveStateMetadata>>,
    emulator_error: &Arc<Mutex<Option<anyhow::Error>>>,
    ctx: &egui::Context,
) {
    loop {
        match emulator.render_frame() {
            Ok(NativeTickEffect::None) => {
                *save_state_metadata.lock().unwrap() = emulator.save_state_metadata();

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
                        EmuThreadCommand::Reload32XConfig(config) => {
                            if let Err(err) = emulator.reload_32x_config(config) {
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
                        EmuThreadCommand::ReloadGameBoyConfig(config) => {
                            if let Err(err) = emulator.reload_gb_config(config) {
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
                            ctx.request_repaint();

                            if is_none {
                                // Window was closed
                                return;
                            }
                        }
                        EmuThreadCommand::SoftReset => emulator.soft_reset(),
                        EmuThreadCommand::HardReset => emulator.hard_reset(),
                        EmuThreadCommand::OpenMemoryViewer => emulator.open_memory_viewer(),
                        EmuThreadCommand::SaveState { slot } => emulator.save_state(slot),
                        EmuThreadCommand::LoadState { slot } => emulator.load_state(slot),
                        EmuThreadCommand::SegaCdRemoveDisc => emulator.remove_disc(),
                        EmuThreadCommand::SegaCdChangeDisc(path) => {
                            if let Err(err) = emulator.change_disc(path) {
                                *emulator_error.lock().unwrap() = Some(err.into());
                                return;
                            }
                        }
                        EmuThreadCommand::RunSms(_)
                        | EmuThreadCommand::RunGenesis(_)
                        | EmuThreadCommand::RunSegaCd(_)
                        | EmuThreadCommand::Run32X(_)
                        | EmuThreadCommand::RunNes(_)
                        | EmuThreadCommand::RunSnes(_)
                        | EmuThreadCommand::RunGameBoy(_) => {}
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
