use bincode::{Decode, Encode};
use genesis_core::{GenesisEmulator, GenesisInputs};
use jgenesis_native_driver::config::{GenesisConfig, SmsGgConfig};
use jgenesis_native_driver::input::{GenesisButton, GetButtonField, SmsGgButton};
use jgenesis_native_driver::{NativeEmulator, NativeTickEffect, TakeRomFrom};
use jgenesis_traits::frontend::TickableEmulator;
use smsgg_core::{SmsGgEmulator, SmsGgInputs};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmuThreadStatus {
    Idle = 0,
    RunningSmsGg = 1,
    RunningGenesis = 2,
}

impl EmuThreadStatus {
    fn from_discriminant(discriminant: u8) -> Self {
        match discriminant {
            0 => Self::Idle,
            1 => Self::RunningSmsGg,
            2 => Self::RunningGenesis,
            _ => panic!("invalid status discriminant: {discriminant}"),
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, Self::RunningSmsGg | Self::RunningGenesis)
    }
}

#[derive(Debug, Clone)]
pub enum EmuThreadCommand {
    RunSms(SmsGgConfig),
    RunGenesis(GenesisConfig),
    ReloadSmsGgConfig(SmsGgConfig),
    ReloadGenesisConfig(GenesisConfig),
    StopEmulator,
}

pub struct EmuThreadHandle {
    status: Arc<AtomicU8>,
    command_sender: Sender<EmuThreadCommand>,
    command_read_signal: Arc<AtomicBool>,
}

impl EmuThreadHandle {
    pub fn send(&self, command: EmuThreadCommand) {
        self.command_sender.send(command).unwrap();
    }

    pub fn status(&self) -> EmuThreadStatus {
        EmuThreadStatus::from_discriminant(self.status.load(Ordering::Relaxed))
    }

    pub fn stop_emulator_if_running(&self) {
        if self.status().is_running() {
            self.send(EmuThreadCommand::StopEmulator);
            self.command_read_signal.store(true, Ordering::Relaxed);
        }
    }

    pub fn reload_config(&self, smsgg_config: SmsGgConfig, genesis_config: GenesisConfig) {
        match self.status() {
            EmuThreadStatus::RunningSmsGg => {
                self.send(EmuThreadCommand::ReloadSmsGgConfig(smsgg_config));
                self.command_read_signal.store(true, Ordering::Relaxed);
            }
            EmuThreadStatus::RunningGenesis => {
                self.send(EmuThreadCommand::ReloadGenesisConfig(genesis_config));
                self.command_read_signal.store(true, Ordering::Relaxed);
            }
            EmuThreadStatus::Idle => {}
        }
    }
}

pub fn spawn() -> EmuThreadHandle {
    let status_arc = Arc::new(AtomicU8::new(EmuThreadStatus::Idle as u8));
    let (command_sender, command_receiver) = mpsc::channel();
    let command_read_signal_arc = Arc::new(AtomicBool::new(false));

    let status = Arc::clone(&status_arc);
    let command_read_signal = Arc::clone(&command_read_signal_arc);
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
                            continue;
                        }
                    };
                    run_emulator(
                        emulator,
                        &command_receiver,
                        &command_read_signal,
                        smsgg_reload_handler,
                    );
                }
                Ok(EmuThreadCommand::RunGenesis(config)) => {
                    status.store(EmuThreadStatus::RunningGenesis as u8, Ordering::Relaxed);

                    let emulator = match jgenesis_native_driver::create_genesis(config) {
                        Ok(emulator) => emulator,
                        Err(err) => {
                            log::error!("Error initializing Genesis emulator: {err}");
                            continue;
                        }
                    };
                    run_emulator(
                        emulator,
                        &command_receiver,
                        &command_read_signal,
                        genesis_reload_handler,
                    );
                }
                Ok(
                    EmuThreadCommand::StopEmulator
                    | EmuThreadCommand::ReloadSmsGgConfig(_)
                    | EmuThreadCommand::ReloadGenesisConfig(_),
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
        command_read_signal: command_read_signal_arc,
    }
}

#[derive(Debug, Clone)]
enum GenericConfig {
    SmsGg(SmsGgConfig),
    Genesis(GenesisConfig),
}

fn smsgg_reload_handler(
    emulator: &mut NativeEmulator<SmsGgInputs, SmsGgButton, SmsGgEmulator>,
    config: GenericConfig,
) {
    if let GenericConfig::SmsGg(config) = config {
        emulator.reload_smsgg_config(config);
    }
}

fn genesis_reload_handler(
    emulator: &mut NativeEmulator<GenesisInputs, GenesisButton, GenesisEmulator>,
    config: GenericConfig,
) {
    if let GenericConfig::Genesis(config) = config {
        emulator.reload_genesis_config(config);
    }
}

fn run_emulator<Inputs, Button, Emulator>(
    mut emulator: NativeEmulator<Inputs, Button, Emulator>,
    command_receiver: &Receiver<EmuThreadCommand>,
    command_read_signal: &Arc<AtomicBool>,
    config_reload_handler: fn(&mut NativeEmulator<Inputs, Button, Emulator>, GenericConfig),
) where
    Inputs: Default + GetButtonField<Button>,
    Button: Copy,
    Emulator: TickableEmulator<Inputs = Inputs> + Encode + Decode + TakeRomFrom,
    anyhow::Error: From<Emulator::Err<anyhow::Error, anyhow::Error, anyhow::Error>>,
{
    loop {
        match emulator.render_frame() {
            Ok(NativeTickEffect::None) => {
                if command_read_signal.load(Ordering::Relaxed) {
                    command_read_signal.store(false, Ordering::Relaxed);

                    match command_receiver.recv().unwrap() {
                        EmuThreadCommand::ReloadSmsGgConfig(config) => {
                            config_reload_handler(&mut emulator, GenericConfig::SmsGg(config));
                        }
                        EmuThreadCommand::ReloadGenesisConfig(config) => {
                            config_reload_handler(&mut emulator, GenericConfig::Genesis(config));
                        }
                        EmuThreadCommand::StopEmulator => {
                            log::info!("Stopping emulator");
                            return;
                        }
                        _ => {}
                    }
                }
            }
            Ok(NativeTickEffect::Exit) => {
                return;
            }
            Err(err) => {
                // TODO propagate to GUI
                log::error!("Emulator terminated with an error: {err}");
                return;
            }
        }
    }
}
