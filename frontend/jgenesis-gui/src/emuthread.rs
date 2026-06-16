mod input;

use crate::app::{ActiveCheats, GenericButton};
use crate::emuthread::input::{CollectInputWindow, CollectInputsResult};
use genesis_config::cheats::GenesisCheats;
use jgenesis_native_config::AppConfig;
use jgenesis_native_config::input::GenericInput;
use jgenesis_native_driver::NativePcEngineEmulator;
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
use sdl3::event::WindowEvent;
use smsgg_config::cheats::SmsGgCheats;
use smsgg_core::SmsGgHardware;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::SendError;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(jgenesis_proc_macros::EnumAll))]
pub enum EmuThreadStatus {
    Idle = 0,
    RunningSms = 1,
    RunningGenesis = 2,
    RunningSegaCd = 3,
    Running32X = 4,
    RunningNes = 5,
    RunningSnes = 6,
    RunningGameBoy = 7,
    RunningGba = 8,
    WaitingForFirstCommand = 9,
    Terminated = 10,
    RunningGameGear = 11,
    RunningPcEngine = 12,
}

impl EmuThreadStatus {
    fn from_discriminant(discriminant: u8) -> Self {
        match discriminant {
            0 => Self::Idle,
            1 => Self::RunningSms,
            2 => Self::RunningGenesis,
            3 => Self::RunningSegaCd,
            4 => Self::Running32X,
            5 => Self::RunningNes,
            6 => Self::RunningSnes,
            7 => Self::RunningGameBoy,
            8 => Self::RunningGba,
            9 => Self::WaitingForFirstCommand,
            10 => Self::Terminated,
            11 => Self::RunningGameGear,
            12 => Self::RunningPcEngine,
            _ => panic!("invalid status discriminant: {discriminant}"),
        }
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::RunningSms
                | Self::RunningGenesis
                | Self::RunningSegaCd
                | Self::Running32X
                | Self::RunningNes
                | Self::RunningSnes
                | Self::RunningGameBoy
                | Self::RunningGba
                | Self::RunningGameGear
                | Self::RunningPcEngine
        )
    }

    pub fn is_running_smsgg(self) -> bool {
        matches!(self, Self::RunningSms | Self::RunningGameGear)
    }

    pub fn is_running_handheld(self) -> bool {
        matches!(self, Self::RunningGameGear | Self::RunningGameBoy | Self::RunningGba)
    }

    pub fn running_console(self) -> Option<Console> {
        match self {
            Self::RunningSms => Some(Console::MasterSystem),
            Self::RunningGameGear => Some(Console::GameGear),
            Self::RunningGenesis => Some(Console::Genesis),
            Self::RunningSegaCd => Some(Console::SegaCd),
            Self::Running32X => Some(Console::Sega32X),
            Self::RunningNes => Some(Console::Nes),
            Self::RunningSnes => Some(Console::Snes),
            Self::RunningGameBoy => Some(Console::GameBoy),
            Self::RunningGba => Some(Console::GameBoyAdvance),
            Self::RunningPcEngine => Some(Console::PcEngine),
            EmuThreadStatus::Idle
            | EmuThreadStatus::WaitingForFirstCommand
            | EmuThreadStatus::Terminated => None,
        }
    }
}

trait ConsoleExt {
    fn running_status(self) -> EmuThreadStatus;
}

impl ConsoleExt for Console {
    fn running_status(self) -> EmuThreadStatus {
        match self {
            Self::MasterSystem | Self::Sg1000 => EmuThreadStatus::RunningSms,
            Self::GameGear => EmuThreadStatus::RunningGameGear,
            Self::Genesis => EmuThreadStatus::RunningGenesis,
            Self::SegaCd => EmuThreadStatus::RunningSegaCd,
            Self::Sega32X => EmuThreadStatus::Running32X,
            Self::Nes => EmuThreadStatus::RunningNes,
            Self::Snes => EmuThreadStatus::RunningSnes,
            Self::GameBoy | Self::GameBoyColor => EmuThreadStatus::RunningGameBoy,
            Self::GameBoyAdvance => EmuThreadStatus::RunningGba,
            Self::PcEngine => EmuThreadStatus::RunningPcEngine,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EmulatorRunInput {
    OpenFile(PathBuf),
    RunBios,
}

#[derive(Debug, Clone)]
pub enum EmuThreadCommand {
    Run {
        console: Console,
        config: Box<AppConfig>,
        cheats: Arc<ActiveCheats>,
        input: EmulatorRunInput,
    },
    ReloadConfig(Box<AppConfig>, Arc<ActiveCheats>, PathBuf),
    StopEmulator,
    Terminate,
    CollectInput(Vec<GenericButton>),
    SoftReset,
    HardReset,
    OpenMemoryViewer,
    SaveState {
        slot: usize,
    },
    LoadState {
        slot: usize,
    },
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
            Ok(EmuThreadCommand::Run { console, mut config, cheats, input }) => {
                ctx.status.store(console.running_status() as u8, Ordering::Relaxed);

                if let Some(native_ppi) = ctx.egui_ctx.native_pixels_per_point() {
                    log::info!("Setting emulator window scale factor to {native_ppi}");
                    config.common.window_scale_factor = Some(native_ppi);
                }

                let emulator = match input {
                    EmulatorRunInput::OpenFile(file_path) => {
                        GenericEmulator::create(console, config, cheats, file_path).map(Some)
                    }
                    EmulatorRunInput::RunBios => GenericEmulator::create_run_bios(console, config),
                };

                let emulator = match emulator {
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
            Ok(EmuThreadCommand::CollectInput(buttons)) => {
                match input::collect_input_not_running(
                    buttons,
                    ctx.egui_ctx.pixels_per_point(),
                    &ctx.input_sender,
                ) {
                    Ok(()) => {
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
    PcEngine(Box<NativePcEngineEmulator>),
}

impl GenericEmulator {
    fn create(
        console: Console,
        config: Box<AppConfig>,
        cheats: Arc<ActiveCheats>,
        path: PathBuf,
    ) -> NativeEmulatorResult<Self> {
        let emulator = match console {
            Console::MasterSystem => {
                Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(config.smsgg_config(
                    path,
                    Some(SmsGgHardware::MasterSystem),
                    cheats.smsgg_or_default(),
                ))?))
            }
            Console::GameGear => Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(
                config.smsgg_config(path, Some(SmsGgHardware::GameGear), cheats.smsgg_or_default()),
            )?)),
            Console::Sg1000 => Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(
                config.smsgg_config(path, Some(SmsGgHardware::Sg1000), cheats.smsgg_or_default()),
            )?)),
            Console::Genesis => Self::Genesis(Box::new(jgenesis_native_driver::create_genesis(
                config.genesis_config(path, cheats.genesis_or_default()),
            )?)),
            Console::SegaCd => Self::SegaCd(Box::new(jgenesis_native_driver::create_sega_cd(
                config.sega_cd_config(path, cheats.genesis_or_default()),
            )?)),
            Console::Sega32X => Self::Sega32X(Box::new(jgenesis_native_driver::create_32x(
                config.sega_32x_config(path, cheats.genesis_or_default()),
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
            Console::PcEngine => Self::PcEngine(Box::new(jgenesis_native_driver::create_pce(
                config.pce_config(path),
            )?)),
        };

        Ok(emulator)
    }

    fn create_run_bios(
        console: Console,
        config: Box<AppConfig>,
    ) -> NativeEmulatorResult<Option<Self>> {
        let emulator = match console {
            Console::MasterSystem => {
                let mut sms_config = config.smsgg_config(
                    PathBuf::new(),
                    Some(SmsGgHardware::MasterSystem),
                    &SmsGgCheats::default(),
                );
                sms_config.sms_boot_from_bios = true;
                sms_config.run_without_cartridge = true;

                Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(sms_config)?))
            }
            Console::GameGear => {
                let mut gg_config = config.smsgg_config(
                    PathBuf::new(),
                    Some(SmsGgHardware::GameGear),
                    &SmsGgCheats::default(),
                );
                gg_config.gg_boot_from_bios = true;
                gg_config.run_without_cartridge = true;

                Self::SmsGg(Box::new(jgenesis_native_driver::create_smsgg(gg_config)?))
            }
            Console::SegaCd => {
                let mut scd_config =
                    config.sega_cd_config(PathBuf::new(), &GenesisCheats::default());
                scd_config.run_without_disc = true;

                Self::SegaCd(Box::new(jgenesis_native_driver::create_sega_cd(scd_config)?))
            }
            _ => return Ok(None),
        };

        Ok(Some(emulator))
    }

    fn reload_config(
        &mut self,
        config: Box<AppConfig>,
        cheats: Arc<ActiveCheats>,
        path: PathBuf,
    ) -> NativeEmulatorResult<()> {
        match self {
            Self::SmsGg(emulator) => emulator.reload_smsgg_config(config.smsgg_config(
                path,
                None,
                cheats.smsgg_or_default(),
            )),
            Self::Genesis(emulator) => emulator
                .reload_genesis_config(config.genesis_config(path, cheats.genesis_or_default())),
            Self::SegaCd(emulator) => emulator
                .reload_sega_cd_config(config.sega_cd_config(path, cheats.genesis_or_default())),
            Self::Sega32X(emulator) => emulator
                .reload_32x_config(config.sega_32x_config(path, cheats.genesis_or_default())),
            Self::Nes(emulator) => emulator.reload_nes_config(config.nes_config(path)),
            Self::Snes(emulator) => emulator.reload_snes_config(config.snes_config(path)),
            Self::GameBoy(emulator) => emulator.reload_gb_config(config.gb_config(path)),
            Self::GameBoyAdvance(emulator) => emulator.reload_gba_config(config.gba_config(path)),

            Self::PcEngine(emulator) => emulator.reload_pce_config(config.pce_config(path)),
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

    fn run(&mut self) -> NativeEmulatorResult<Option<NativeTickEffect>> {
        match_each_variant!(self, emulator => emulator.run())
    }

    fn soft_reset(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.soft_reset())
    }

    fn hard_reset(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.hard_reset())
    }

    fn open_memory_viewer(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.open_memory_viewer())
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

    fn joysticks(&mut self) -> &mut Joysticks {
        match_each_variant!(self, emulator => emulator.joysticks())
    }

    fn event_pump(&mut self) -> &mut EventPump {
        match_each_variant!(self, emulator => emulator.event_pump())
    }

    fn handle_window_event(
        &mut self,
        event: &WindowEvent,
        window_id: u32,
    ) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.handle_window_event(event, window_id))?;
        Ok(())
    }

    fn force_render(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.force_render())
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
        match emulator.run() {
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
        EmuThreadCommand::ReloadConfig(config, cheats, path) => {
            emulator.reload_config(config, cheats, path)?;
        }
        EmuThreadCommand::StopEmulator => {
            log::info!("Stopping emulator");
            return Ok(Some(RunEmuResult::None));
        }
        EmuThreadCommand::Terminate => {
            log::info!("Terminating emulation thread");
            return Ok(Some(RunEmuResult::Terminate));
        }
        EmuThreadCommand::CollectInput(buttons) => {
            log::debug!("Received collect input command");

            emulator.focus();

            let result = input::collect_inputs(
                &buttons,
                CollectInputWindow::Emulator(emulator),
                &ctx.input_sender,
            );

            ctx.egui_ctx.request_repaint();

            if result == CollectInputsResult::WindowClosed {
                return Ok(Some(RunEmuResult::None));
            }
        }
        EmuThreadCommand::SoftReset => emulator.soft_reset()?,
        EmuThreadCommand::HardReset => emulator.hard_reset()?,
        EmuThreadCommand::OpenMemoryViewer => emulator.open_memory_viewer()?,
        EmuThreadCommand::SaveState { slot } => emulator.save_state(slot),
        EmuThreadCommand::LoadState { slot } => emulator.load_state(slot),
        EmuThreadCommand::SegaCdRemoveDisc => emulator.remove_disc()?,
        EmuThreadCommand::SegaCdChangeDisc(path) => emulator.change_disc(path)?,
        EmuThreadCommand::Run { .. } => {}
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_from_discriminant() {
        for value in EmuThreadStatus::ALL {
            assert_eq!(EmuThreadStatus::from_discriminant(value as u8), value);
        }
    }
}
