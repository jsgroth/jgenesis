mod inputwindow;

use crate::emuthread::inputwindow::InputWindow;
use anyhow::anyhow;
use jgenesis_native_driver::config::{
    GameBoyConfig, GenesisConfig, NesConfig, Sega32XConfig, SegaCdConfig, SmsGgConfig, SnesConfig,
};
use jgenesis_native_driver::input::{
    AxisDirection, GamepadAction, GenericInput, HatDirection, Joysticks,
};
use jgenesis_native_driver::{
    AudioError, Native32XEmulator, NativeEmulatorResult, NativeGameBoyEmulator,
    NativeGenesisEmulator, NativeNesEmulator, NativeSegaCdEmulator, NativeSmsGgEmulator,
    NativeSnesEmulator, NativeTickEffect, SaveStateMetadata,
};
use sdl2::EventPump;
use sdl2::event::Event;
use sdl2::joystick::HatState;
use segacd_core::api::SegaCdLoadResult;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
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
    CollectInput { axis_deadzone: i16 },
    SoftReset,
    HardReset,
    OpenMemoryViewer,
    SaveState { slot: usize },
    LoadState { slot: usize },
    SegaCdRemoveDisc,
    SegaCdChangeDisc(PathBuf),
}

pub struct EmuThreadHandle {
    status: Arc<AtomicU8>,
    command_sender: Sender<EmuThreadCommand>,
    input_receiver: Receiver<Option<Vec<GenericInput>>>,
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

    pub fn poll_input_receiver(&self) -> Result<Option<Vec<GenericInput>>, TryRecvError> {
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
    input_sender: Sender<Option<Vec<GenericInput>>>,
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
            Ok(EmuThreadCommand::CollectInput { axis_deadzone }) => {
                match collect_input_not_running(axis_deadzone, ctx.pixels_per_point()) {
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

    fn event_pump_and_joysticks_mut(&mut self) -> (&mut EventPump, &mut Joysticks) {
        match_each_emulator_variant!(self, emulator => emulator.event_pump_and_joysticks_mut())
    }
}

fn run_emulator(
    mut emulator: GenericEmulator,
    command_receiver: &Receiver<EmuThreadCommand>,
    input_sender: &Sender<Option<Vec<GenericInput>>>,
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
                        EmuThreadCommand::CollectInput { axis_deadzone } => {
                            log::debug!("Received collect input command");

                            emulator.focus();
                            let (event_pump, joysticks) = emulator.event_pump_and_joysticks_mut();
                            let input = collect_input(event_pump, joysticks, axis_deadzone, None);

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
    axis_deadzone: i16,
    scale_factor: f32,
) -> anyhow::Result<Option<Vec<GenericInput>>> {
    let sdl = sdl2::init().map_err(|err| anyhow!("Error initializing SDL2: {err}"))?;
    let video =
        sdl.video().map_err(|err| anyhow!("Error initializing SDL2 video subsystem: {err}"))?;
    let joystick_subsystem = sdl
        .joystick()
        .map_err(|err| anyhow!("Error initializing SDL2 joystick subsystem: {err}"))?;
    let mut event_pump =
        sdl.event_pump().map_err(|err| anyhow!("Error initializing SDL2 event pump: {err}"))?;

    let mut sdl_window = video
        .window(
            "SDL input configuration",
            (400.0 * scale_factor).round() as u32,
            (150.0 * scale_factor).round() as u32,
        )
        .build()?;
    sdl_window.raise();
    let window = InputWindow::new(sdl_window, scale_factor)?;

    let mut joysticks = Joysticks::new(joystick_subsystem);

    let input = collect_input(&mut event_pump, &mut joysticks, axis_deadzone, Some(window));

    for _ in event_pump.poll_iter() {}

    Ok(input)
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

fn collect_input(
    event_pump: &mut EventPump,
    joysticks: &mut Joysticks,
    axis_deadzone: i16,
    mut window: Option<InputWindow>,
) -> Option<Vec<GenericInput>> {
    let mut inputs = VecSet::new();

    loop {
        for event in event_pump.poll_iter() {
            log::debug!("SDL event: {event:?}");

            if let Some(window) = &mut window {
                window.handle_sdl_event(&event);
            }

            match event {
                Event::Quit { .. } => {
                    return None;
                }
                Event::KeyDown { keycode: Some(keycode), .. } => {
                    inputs.insert(GenericInput::Keyboard(keycode));
                }
                Event::KeyUp { .. } | Event::JoyButtonUp { .. } | Event::MouseButtonUp { .. } => {
                    return Some(inputs.0);
                }
                Event::JoyDeviceAdded { which: device_id, .. } => {
                    if let Err(err) = joysticks.handle_device_added(device_id) {
                        log::error!("Error adding joystick with device id {device_id}: {err}");
                    }
                }
                Event::JoyDeviceRemoved { which: instance_id, .. } => {
                    joysticks.handle_device_removed(instance_id);
                }
                Event::JoyButtonDown { which: instance_id, button_idx, .. } => {
                    if let Some(device_id) = joysticks.map_to_device_id(instance_id) {
                        inputs.insert(GenericInput::Gamepad {
                            gamepad_idx: device_id,
                            action: GamepadAction::Button(button_idx),
                        });
                    }
                }
                Event::JoyAxisMotion { which: instance_id, axis_idx, value, .. } => {
                    let Some(gamepad_idx) = joysticks.map_to_device_id(instance_id) else {
                        continue;
                    };

                    let pressed = value.saturating_abs() > axis_deadzone;
                    if pressed {
                        let direction = AxisDirection::from_value(value);
                        inputs.insert(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Axis(axis_idx, direction),
                        });
                    } else if [AxisDirection::Positive, AxisDirection::Negative].into_iter().any(
                        |direction| {
                            inputs.0.contains(&GenericInput::Gamepad {
                                gamepad_idx,
                                action: GamepadAction::Axis(axis_idx, direction),
                            })
                        },
                    ) {
                        return Some(inputs.0);
                    }
                }
                Event::JoyHatMotion { which: instance_id, hat_idx, state, .. } => {
                    let Some(gamepad_idx) = joysticks.map_to_device_id(instance_id) else {
                        continue;
                    };

                    if state == HatState::Centered {
                        if HatDirection::ALL.into_iter().any(|direction| {
                            inputs.0.contains(&GenericInput::Gamepad {
                                gamepad_idx,
                                action: GamepadAction::Hat(hat_idx, direction),
                            })
                        }) {
                            return Some(inputs.0);
                        }

                        continue;
                    }

                    if let Some(direction) = hat_direction_for(state) {
                        inputs.insert(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Hat(hat_idx, direction),
                        });
                    }
                }
                Event::MouseButtonDown { mouse_btn, .. } => {
                    inputs.insert(GenericInput::Mouse(mouse_btn));
                }
                _ => {}
            }
        }

        if inputs.len() == jgenesis_native_driver::input::MAX_MAPPING_LEN {
            return Some(inputs.0);
        }

        if let Some(window) = &mut window {
            let result = window.update(|ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_input_window(joysticks, ui);
                });
            });
            if let Err(err) = result {
                log::error!("Error rendering input window: {err}");
            }
        }

        jgenesis_common::sleep(Duration::from_millis(10));
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

fn render_input_window(joysticks: &Joysticks, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().show(ui, |ui| {
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
            for (gamepad_idx, joystick) in devices {
                ui.label(format!("    Gamepad {gamepad_idx}: {}", joystick.name()));
            }
        }
    });
}
