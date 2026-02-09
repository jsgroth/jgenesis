mod inputwindow;

use crate::emuthread::inputwindow::InputWindow;
use anyhow::anyhow;
use jgenesis_native_config::AppConfig;
use jgenesis_native_config::input::{AxisDirection, GamepadAction, GenericInput, HatDirection};
use jgenesis_native_driver::config::AppConfigExt;
use jgenesis_native_driver::extensions::Console;
use jgenesis_native_driver::input::Joysticks;
use jgenesis_native_driver::{
    Native32XEmulator, NativeEmulatorError, NativeEmulatorResult, NativeGameBoyEmulator,
    NativeGbaEmulator, NativeGenesisEmulator, NativeNesEmulator, NativeSegaCdEmulator,
    NativeSmsGgEmulator, NativeSnesEmulator, NativeTickEffect, SaveStateMetadata,
};
use jgenesis_proc_macros::MatchEachVariantMacro;
use sdl3::EventPump;
use sdl3::event::Event;
use sdl3::joystick::{HatState, Joystick};
use smsgg_core::SmsGgHardware;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::SendError;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, mpsc};
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
    RunningGba = 8,
    WaitingForFirstCommand = 9,
    Terminated = 10,
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
            8 => Self::RunningGba,
            9 => Self::WaitingForFirstCommand,
            10 => Self::Terminated,
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
                | Self::RunningGba
        )
    }
}

trait ConsoleExt {
    fn running_status(self) -> EmuThreadStatus;
}

impl ConsoleExt for Console {
    fn running_status(self) -> EmuThreadStatus {
        match self {
            Self::MasterSystem | Self::GameGear => EmuThreadStatus::RunningSmsGg,
            Self::Genesis => EmuThreadStatus::RunningGenesis,
            Self::SegaCd => EmuThreadStatus::RunningSegaCd,
            Self::Sega32X => EmuThreadStatus::Running32X,
            Self::Nes => EmuThreadStatus::RunningNes,
            Self::Snes => EmuThreadStatus::RunningSnes,
            Self::GameBoy | Self::GameBoyColor => EmuThreadStatus::RunningGameBoy,
            Self::GameBoyAdvance => EmuThreadStatus::RunningGba,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EmuThreadCommand {
    Run { console: Console, config: Box<AppConfig>, file_path: PathBuf },
    RunBios { console: Console, config: Box<AppConfig> },
    ReloadConfig(Box<AppConfig>, PathBuf),
    StopEmulator,
    Terminate,
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
    gui_focused: Arc<AtomicBool>,
    emulator_error: Arc<Mutex<Option<NativeEmulatorError>>>,
    exit_signal: Arc<AtomicBool>,
}

impl EmuThreadHandle {
    pub fn send(&self, command: EmuThreadCommand) {
        self.command_sender.send(command).unwrap();
    }

    pub fn try_send(&self, command: EmuThreadCommand) -> Result<(), SendError<EmuThreadCommand>> {
        self.command_sender.send(command)
    }

    pub fn status(&self) -> EmuThreadStatus {
        EmuThreadStatus::from_discriminant(self.status.load(Ordering::Relaxed))
    }

    pub fn save_state_metadata(&self) -> SaveStateMetadata {
        self.save_state_metadata.lock().unwrap().clone()
    }

    pub fn update_gui_focused(&self, gui_focused: bool) {
        self.gui_focused.store(gui_focused, Ordering::Relaxed);
    }

    pub fn emulator_error(&self) -> Arc<Mutex<Option<NativeEmulatorError>>> {
        Arc::clone(&self.emulator_error)
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

    pub fn exit_signal(&self) -> bool {
        self.exit_signal.load(Ordering::Relaxed)
    }
}

pub fn spawn(egui_ctx: egui::Context) -> EmuThreadHandle {
    let status = Arc::new(AtomicU8::new(EmuThreadStatus::WaitingForFirstCommand as u8));
    let (command_sender, command_receiver) = mpsc::channel();
    let (input_sender, input_receiver) = mpsc::channel();
    let save_state_metadata = Arc::new(Mutex::new(SaveStateMetadata::default()));
    let gui_focused = Arc::new(AtomicBool::new(false));
    let emulator_error = Arc::new(Mutex::new(None));
    let exit_signal = Arc::new(AtomicBool::new(false));

    {
        let status = Arc::clone(&status);
        let save_state_metadata = Arc::clone(&save_state_metadata);
        let gui_focused = Arc::clone(&gui_focused);
        let emulator_error = Arc::clone(&emulator_error);
        let exit_signal = Arc::clone(&exit_signal);
        thread::spawn(move || {
            thread_run(EmuThreadContext {
                egui_ctx,
                command_receiver,
                input_sender,
                status,
                save_state_metadata,
                gui_focused,
                emulator_error,
                exit_signal,
            });
        });
    }

    EmuThreadHandle {
        status,
        command_sender,
        input_receiver,
        save_state_metadata,
        gui_focused,
        emulator_error,
        exit_signal,
    }
}

struct EmuThreadContext {
    egui_ctx: egui::Context,
    command_receiver: Receiver<EmuThreadCommand>,
    input_sender: Sender<Option<Vec<GenericInput>>>,
    status: Arc<AtomicU8>,
    save_state_metadata: Arc<Mutex<SaveStateMetadata>>,
    gui_focused: Arc<AtomicBool>,
    emulator_error: Arc<Mutex<Option<NativeEmulatorError>>>,
    exit_signal: Arc<AtomicBool>,
}

fn thread_run(ctx: EmuThreadContext) {
    loop {
        if ctx.status.load(Ordering::Relaxed) != EmuThreadStatus::WaitingForFirstCommand as u8 {
            ctx.status.store(EmuThreadStatus::Idle as u8, Ordering::Relaxed);
        }

        // Force a repaint at the start of the loop so that the GUI will always repaint after an
        // emulator exits. This will immediately display the error window if there was an
        // error, and it will also force quit immediately if auto-close is enabled
        ctx.egui_ctx.request_repaint();

        match ctx.command_receiver.recv() {
            Ok(EmuThreadCommand::Run { console, mut config, file_path }) => {
                ctx.status.store(console.running_status() as u8, Ordering::Relaxed);

                if let Some(native_ppi) = ctx.egui_ctx.native_pixels_per_point() {
                    log::info!("Setting emulator window scale factor to {native_ppi}");
                    config.common.window_scale_factor = Some(native_ppi);
                }

                let emulator = match GenericEmulator::create(console, config, file_path) {
                    Ok(emulator) => emulator,
                    Err(err) => {
                        log::error!("Error initializing emulator: {err}");
                        *ctx.emulator_error.lock().unwrap() = Some(err);
                        ctx.egui_ctx.request_repaint();
                        continue;
                    }
                };
                let run_result = run_emulator(emulator, &ctx);

                if run_result == RunEmuResult::Terminate {
                    ctx.status.store(EmuThreadStatus::Terminated as u8, Ordering::Relaxed);
                    ctx.egui_ctx.request_repaint();
                    return;
                }
            }
            Ok(EmuThreadCommand::RunBios { console, mut config }) => {
                ctx.status.store(console.running_status() as u8, Ordering::Relaxed);

                if let Some(native_ppi) = ctx.egui_ctx.native_pixels_per_point() {
                    log::info!("Setting emulator window scale factor to {native_ppi}");
                    config.common.window_scale_factor = Some(native_ppi);
                }

                let emulator = match GenericEmulator::create_run_bios(console, config) {
                    Ok(Some(emulator)) => emulator,
                    Ok(None) => continue,
                    Err(err) => {
                        log::error!("Error initializing emulator: {err}");
                        *ctx.emulator_error.lock().unwrap() = Some(err);
                        ctx.egui_ctx.request_repaint();
                        continue;
                    }
                };
                let run_result = run_emulator(emulator, &ctx);

                if run_result == RunEmuResult::Terminate {
                    ctx.status.store(EmuThreadStatus::Terminated as u8, Ordering::Relaxed);
                    ctx.egui_ctx.request_repaint();
                    return;
                }
            }
            Ok(EmuThreadCommand::CollectInput { axis_deadzone }) => {
                match collect_input_not_running(axis_deadzone, ctx.egui_ctx.pixels_per_point()) {
                    Ok(input) => {
                        ctx.input_sender.send(input).unwrap();
                        ctx.egui_ctx.request_repaint();
                    }
                    Err(err) => {
                        log::error!("Error collecting SDL3 input: {err}");
                    }
                }
            }
            Ok(EmuThreadCommand::Terminate) => {
                log::info!("Terminating emulation thread");
                ctx.status.store(EmuThreadStatus::Terminated as u8, Ordering::Relaxed);
                return;
            }
            Ok(
                EmuThreadCommand::StopEmulator
                | EmuThreadCommand::ReloadConfig(..)
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

#[derive(MatchEachVariantMacro)]
enum GenericEmulator {
    SmsGg(Box<NativeSmsGgEmulator>),
    Genesis(Box<NativeGenesisEmulator>),
    SegaCd(Box<NativeSegaCdEmulator>),
    Sega32X(Box<Native32XEmulator>),
    Nes(Box<NativeNesEmulator>),
    Snes(Box<NativeSnesEmulator>),
    GameBoy(Box<NativeGameBoyEmulator>),
    GameBoyAdvance(Box<NativeGbaEmulator>),
}

impl GenericEmulator {
    fn create(
        console: Console,
        config: Box<AppConfig>,
        path: PathBuf,
    ) -> NativeEmulatorResult<Self> {
        let emulator = match console {
            Console::MasterSystem => Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(
                config.smsgg_config(path, Some(SmsGgHardware::MasterSystem)),
            )?)),
            Console::GameGear => Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(
                config.smsgg_config(path, Some(SmsGgHardware::GameGear)),
            )?)),
            Console::Genesis => Self::Genesis(Box::new(jgenesis_native_driver::create_genesis(
                config.genesis_config(path),
            )?)),
            Console::SegaCd => Self::SegaCd(Box::new(jgenesis_native_driver::create_sega_cd(
                config.sega_cd_config(path),
            )?)),
            Console::Sega32X => Self::Sega32X(Box::new(jgenesis_native_driver::create_32x(
                config.sega_32x_config(path),
            )?)),
            Console::Nes => {
                Self::Nes(Box::new(jgenesis_native_driver::create_nes(config.nes_config(path))?))
            }
            Console::Snes => {
                Self::Snes(Box::new(jgenesis_native_driver::create_snes(config.snes_config(path))?))
            }
            Console::GameBoy | Console::GameBoyColor => {
                Self::GameBoy(Box::new(jgenesis_native_driver::create_gb(config.gb_config(path))?))
            }
            Console::GameBoyAdvance => Self::GameBoyAdvance(Box::new(
                jgenesis_native_driver::create_gba(config.gba_config(path))?,
            )),
        };

        Ok(emulator)
    }

    fn create_run_bios(
        console: Console,
        config: Box<AppConfig>,
    ) -> NativeEmulatorResult<Option<Self>> {
        let emulator = match console {
            Console::MasterSystem => {
                let mut sms_config =
                    config.smsgg_config(PathBuf::new(), Some(SmsGgHardware::MasterSystem));
                sms_config.sms_boot_from_bios = true;
                sms_config.run_without_cartridge = true;

                Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(sms_config)?))
            }
            Console::GameGear => {
                let mut gg_config =
                    config.smsgg_config(PathBuf::new(), Some(SmsGgHardware::GameGear));
                gg_config.gg_boot_from_bios = true;
                gg_config.run_without_cartridge = true;

                Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(gg_config)?))
            }
            Console::SegaCd => {
                let mut scd_config = config.sega_cd_config(PathBuf::new());
                scd_config.run_without_disc = true;

                Self::SegaCd(Box::new(jgenesis_native_driver::create_sega_cd(scd_config)?))
            }
            _ => return Ok(None),
        };

        Ok(Some(emulator))
    }

    fn reload_config(&mut self, config: Box<AppConfig>, path: PathBuf) -> NativeEmulatorResult<()> {
        match self {
            Self::SmsGg(emulator) => emulator.reload_smsgg_config(config.smsgg_config(path, None)),
            Self::Genesis(emulator) => emulator.reload_genesis_config(config.genesis_config(path)),
            Self::SegaCd(emulator) => emulator.reload_sega_cd_config(config.sega_cd_config(path)),
            Self::Sega32X(emulator) => emulator.reload_32x_config(config.sega_32x_config(path)),
            Self::Nes(emulator) => emulator.reload_nes_config(config.nes_config(path)),
            Self::Snes(emulator) => emulator.reload_snes_config(config.snes_config(path)),
            Self::GameBoy(emulator) => emulator.reload_gb_config(config.gb_config(path)),
            Self::GameBoyAdvance(emulator) => emulator.reload_gba_config(config.gba_config(path)),
        }
    }

    fn remove_disc(&mut self) -> NativeEmulatorResult<()> {
        if let Self::SegaCd(emulator) = self {
            emulator.remove_disc()?;
        }

        Ok(())
    }

    fn change_disc(&mut self, path: PathBuf) -> NativeEmulatorResult<()> {
        if let Self::SegaCd(emulator) = self {
            emulator.change_disc(path)?;
        }

        Ok(())
    }

    fn render_frame(&mut self) -> NativeEmulatorResult<Option<NativeTickEffect>> {
        match_each_variant!(self, emulator => emulator.run())
    }

    fn soft_reset(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.soft_reset())
    }

    fn hard_reset(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.hard_reset())
    }

    fn open_memory_viewer(&mut self) {
        match_each_variant!(self, emulator => emulator.open_memory_viewer());
    }

    fn save_state(&mut self, slot: usize) {
        if let Err(err) = match_each_variant!(self, emulator => emulator.save_state(slot)) {
            log::error!("Failed to save state to slot {slot}: {err}");
        }
    }

    fn load_state(&mut self, slot: usize) {
        if let Err(err) = match_each_variant!(self, emulator => emulator.load_state(slot)) {
            log::error!("Failed to load state from slot {slot}: {err}");
        }
    }

    fn save_state_metadata(&self) -> SaveStateMetadata {
        match_each_variant!(self, emulator => emulator.save_state_metadata())
    }

    fn update_gui_focused(&mut self, gui_focused: bool) {
        match_each_variant!(self, emulator => emulator.update_gui_focused(gui_focused));
    }

    fn focus(&mut self) {
        match_each_variant!(self, emulator => emulator.focus());
    }

    fn event_pump_and_joysticks_mut(&mut self) -> (&mut EventPump, &mut Joysticks) {
        match_each_variant!(self, emulator => emulator.event_pump_and_joysticks_mut())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunEmuResult {
    None,
    Terminate,
}

#[must_use]
fn run_emulator(mut emulator: GenericEmulator, ctx: &EmuThreadContext) -> RunEmuResult {
    loop {
        match emulator.render_frame() {
            Ok(None) => {
                *ctx.save_state_metadata.lock().unwrap() = emulator.save_state_metadata();
                emulator.update_gui_focused(ctx.gui_focused.load(Ordering::Relaxed));

                while let Ok(command) = ctx.command_receiver.try_recv() {
                    match handle_command(&mut emulator, ctx, command) {
                        Ok(None) => {}
                        Ok(Some(result)) => return result,
                        Err(err) => {
                            *ctx.emulator_error.lock().unwrap() = Some(err);
                            return RunEmuResult::None;
                        }
                    }
                }
            }
            Ok(Some(NativeTickEffect::PowerOff)) => {
                return RunEmuResult::None;
            }
            Ok(Some(NativeTickEffect::Exit)) => {
                ctx.exit_signal.store(true, Ordering::Relaxed);
                return RunEmuResult::Terminate;
            }
            Err(err) => {
                log::error!("Emulator terminated with an error: {err}");
                *ctx.emulator_error.lock().unwrap() = Some(err);
                return RunEmuResult::None;
            }
        }
    }
}

fn handle_command(
    emulator: &mut GenericEmulator,
    ctx: &EmuThreadContext,
    command: EmuThreadCommand,
) -> NativeEmulatorResult<Option<RunEmuResult>> {
    match command {
        EmuThreadCommand::ReloadConfig(config, path) => {
            emulator.reload_config(config, path)?;
        }
        EmuThreadCommand::StopEmulator => {
            log::info!("Stopping emulator");
            return Ok(Some(RunEmuResult::None));
        }
        EmuThreadCommand::Terminate => {
            log::info!("Terminating emulation thread");
            return Ok(Some(RunEmuResult::Terminate));
        }
        EmuThreadCommand::CollectInput { axis_deadzone } => {
            log::debug!("Received collect input command");

            emulator.focus();
            let (event_pump, joysticks) = emulator.event_pump_and_joysticks_mut();
            let input = collect_input(event_pump, joysticks, axis_deadzone, None);

            let is_none = input.is_none();

            log::debug!("Sending collect input result {input:?}");
            ctx.input_sender.send(input).unwrap();
            ctx.egui_ctx.request_repaint();

            if is_none {
                // Window was closed
                return Ok(Some(RunEmuResult::None));
            }
        }
        EmuThreadCommand::SoftReset => emulator.soft_reset()?,
        EmuThreadCommand::HardReset => emulator.hard_reset()?,
        EmuThreadCommand::OpenMemoryViewer => emulator.open_memory_viewer(),
        EmuThreadCommand::SaveState { slot } => emulator.save_state(slot),
        EmuThreadCommand::LoadState { slot } => emulator.load_state(slot),
        EmuThreadCommand::SegaCdRemoveDisc => emulator.remove_disc()?,
        EmuThreadCommand::SegaCdChangeDisc(path) => emulator.change_disc(path)?,
        EmuThreadCommand::Run { .. } | EmuThreadCommand::RunBios { .. } => {}
    }

    Ok(None)
}

fn collect_input_not_running(
    axis_deadzone: i16,
    scale_factor: f32,
) -> anyhow::Result<Option<Vec<GenericInput>>> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollectionDone {
    No,
    Yes,
}

struct CollectedInputs {
    inputs: VecSet,
    gamepad_starting_states: HashSet<GenericInput>,
}

impl CollectedInputs {
    fn new(joysticks: &Joysticks, axis_deadzone: i16) -> Self {
        let gamepad_starting_states = joysticks
            .all_devices()
            .flat_map(|(device_id, joystick)| {
                joystick_starting_state(device_id, joystick, axis_deadzone)
            })
            .collect();

        Self { inputs: VecSet::new(), gamepad_starting_states }
    }

    fn add_device(&mut self, device_id: u32, joystick: &Joystick, axis_deadzone: i16) {
        self.gamepad_starting_states.extend(joystick_starting_state(
            device_id,
            joystick,
            axis_deadzone,
        ));
    }

    fn contains(&self, input: GenericInput) -> bool {
        self.inputs.0.contains(&input)
    }

    fn consume(self) -> Vec<GenericInput> {
        self.inputs.0
    }

    #[must_use]
    fn insert(&mut self, input: GenericInput) -> CollectionDone {
        if self.gamepad_starting_states.remove(&input) {
            return CollectionDone::No;
        }

        if let Some(opposite) = opposite_input(input)
            && self.inputs.0.contains(&opposite)
        {
            return CollectionDone::Yes;
        }

        self.inputs.insert(input);
        if self.inputs.len() == jgenesis_native_driver::input::MAX_MAPPING_LEN {
            CollectionDone::Yes
        } else {
            CollectionDone::No
        }
    }
}

fn opposite_input(input: GenericInput) -> Option<GenericInput> {
    match input {
        GenericInput::Gamepad { gamepad_idx, action: GamepadAction::Axis(axis_idx, direction) } => {
            Some(GenericInput::Gamepad {
                gamepad_idx,
                action: GamepadAction::Axis(axis_idx, direction.inverse()),
            })
        }
        _ => None,
    }
}

fn collect_input(
    event_pump: &mut EventPump,
    joysticks: &mut Joysticks,
    axis_deadzone: i16,
    mut window: Option<InputWindow>,
) -> Option<Vec<GenericInput>> {
    let mut inputs = CollectedInputs::new(joysticks, axis_deadzone);

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
                    if inputs.insert(GenericInput::Keyboard(keycode)) == CollectionDone::Yes {
                        return Some(inputs.consume());
                    }
                }
                Event::KeyUp { .. } | Event::JoyButtonUp { .. } | Event::MouseButtonUp { .. } => {
                    return Some(inputs.consume());
                }
                Event::JoyDeviceAdded { which: joystick_id, .. } => {
                    if let Err(err) = joysticks.handle_device_added(joystick_id) {
                        log::error!("Error adding joystick with joystick id {joystick_id}: {err}");
                    }

                    if let Some(joystick) = joysticks.device(joystick_id) {
                        inputs.add_device(joystick_id, joystick, axis_deadzone);
                    }
                }
                Event::JoyDeviceRemoved { which: joystick_id, .. } => {
                    if let Err(err) = joysticks.handle_device_removed(joystick_id) {
                        log::error!(
                            "Error removing joystick with joystick id {joystick_id}: {err}"
                        );
                    }
                }
                Event::JoyButtonDown { which: instance_id, button_idx, .. } => {
                    if let Some(device_id) = joysticks.map_to_device_id(instance_id)
                        && inputs.insert(GenericInput::Gamepad {
                            gamepad_idx: device_id,
                            action: GamepadAction::Button(button_idx),
                        }) == CollectionDone::Yes
                    {
                        return Some(inputs.consume());
                    }
                }
                Event::JoyAxisMotion { which: instance_id, axis_idx, value, .. } => {
                    let Some(gamepad_idx) = joysticks.map_to_device_id(instance_id) else {
                        continue;
                    };

                    let pressed = value.saturating_abs() > axis_deadzone;
                    if pressed {
                        let direction = AxisDirection::from_value(value);
                        if inputs.insert(GenericInput::Gamepad {
                            gamepad_idx,
                            action: GamepadAction::Axis(axis_idx, direction),
                        }) == CollectionDone::Yes
                        {
                            return Some(inputs.consume());
                        }
                    } else if [AxisDirection::Positive, AxisDirection::Negative].into_iter().any(
                        |direction| {
                            inputs.contains(GenericInput::Gamepad {
                                gamepad_idx,
                                action: GamepadAction::Axis(axis_idx, direction),
                            })
                        },
                    ) {
                        return Some(inputs.consume());
                    }
                }
                Event::JoyHatMotion { which: instance_id, hat_idx, state, .. } => {
                    let Some(gamepad_idx) = joysticks.map_to_device_id(instance_id) else {
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
                Event::MouseButtonDown { mouse_btn, .. } => {
                    if inputs.insert(GenericInput::Mouse(mouse_btn)) == CollectionDone::Yes {
                        return Some(inputs.consume());
                    }
                }
                _ => {}
            }
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

        thread::sleep(Duration::from_millis(10));
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
    (0..joystick.num_buttons()).filter_map(move |button_idx| {
        let pressed = joystick.button(button_idx).ok()?;
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
    (0..joystick.num_axes()).filter_map(move |axis_idx| {
        let axis_value = joystick.axis(axis_idx).ok()?;
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
    (0..joystick.num_hats()).filter_map(move |hat_idx| {
        let state = joystick.hat(hat_idx).ok()?;
        hat_direction_for(state).map(|hat_direction| GenericInput::Gamepad {
            gamepad_idx,
            action: GamepadAction::Hat(hat_idx as u8, hat_direction),
        })
    })
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
