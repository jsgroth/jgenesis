use crate::config::CommonConfig;
use crate::mainloop::audio::{SdlAudioOutput, SdlAudioOutputHandle};
use crate::mainloop::debug::DebuggerRunnerProcess;
use crate::mainloop::input::{ThreadedInputPoller, ThreadedInputPollerHandle};
use crate::mainloop::render::{RecvFrameError, ThreadedRenderer, ThreadedRendererHandle};
use crate::mainloop::rewind::Rewinder;
use crate::mainloop::save::{DeterminedPaths, FsSaveWriter};
use crate::mainloop::state::SaveStatePaths;
use crate::mainloop::{CreateEmulatorFn, CreatedEmulator, save, state};
use crate::{NativeEmulatorError, NativeEmulatorResult, SaveStateMetadata};
use jgenesis_common::frontend::{AudioOutput, EmulatorTrait, Renderer, SaveWriter, TickEffect};
use jgenesis_native_config::common::WindowSize;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use thiserror::Error;

// Returns new window title (if Ok)
pub type ChangeDiscFn<Emulator> =
    fn(&mut Emulator, PathBuf) -> Result<String, Box<dyn Error + Send + Sync + 'static>>;

pub type RemoveDiscFn<Emulator> = fn(&mut Emulator);

pub enum RunnerCommand<Emulator: EmulatorTrait> {
    Terminate,
    SoftReset,
    HardReset,
    ChangeDisc(PathBuf),
    RemoveDisc,
    StepFrame,
    FastForward(bool),
    Rewind(bool),
    SaveState { slot: usize },
    LoadState { slot: usize },
    ReloadConfig(Box<(CommonConfig, Emulator::Config)>),
    StartDebugger(Box<dyn DebuggerRunnerProcess<Emulator>>),
    StopDebugger,
}

#[derive(Debug)]
pub enum RunnerCommandResponse {
    SaveStateSucceeded { slot: usize },
    LoadStateSucceeded { slot: usize },
    SaveStateFailed { slot: usize, err: NativeEmulatorError },
    LoadStateFailed { slot: usize, err: NativeEmulatorError },
    ChangeDiscSucceeded { window_title: String },
    ChangeDiscFailed(Box<dyn Error + Send + Sync + 'static>),
}

struct RunnerThreadState<Emulator: EmulatorTrait> {
    emulator: Emulator,
    renderer: ThreadedRenderer,
    audio_output: SdlAudioOutput,
    input_poller: ThreadedInputPoller<Emulator::Inputs>,
    save_writer: FsSaveWriter,
    common_config: CommonConfig,
    emulator_config: Emulator::Config,
    command_receiver: Receiver<RunnerCommand<Emulator>>,
    response_sender: Sender<RunnerCommandResponse>,
    error_sender: Sender<Box<dyn Error + Send + Sync + 'static>>,
    rom_path: PathBuf,
    rom_extension: String,
    base_save_state_path: PathBuf,
    save_state_paths: SaveStatePaths,
    save_state_metadata: Arc<Mutex<SaveStateMetadata>>,
    paused: Arc<AtomicBool>,
    step_frame: bool,
    rewinder: Rewinder<Emulator>,
    change_disc_fn: ChangeDiscFn<Emulator>,
    remove_disc_fn: RemoveDiscFn<Emulator>,
    debugger_process: Option<Box<dyn DebuggerRunnerProcess<Emulator>>>,
}

impl<Emulator: EmulatorTrait> RunnerThreadState<Emulator> {
    fn update_save_paths(&mut self) -> NativeEmulatorResult<()> {
        let DeterminedPaths { save_path, save_state_path } = save::determine_save_paths(
            &self.common_config.save_path,
            &self.common_config.state_path,
            &self.rom_path,
            &self.rom_extension,
        )?;

        self.save_writer.update_path(save_path);

        if save_state_path != self.base_save_state_path {
            self.save_state_paths = state::init_paths(&save_state_path)?;
            *self.save_state_metadata.lock().unwrap() =
                SaveStateMetadata::load(&self.save_state_paths, Emulator::save_state_version());
            self.base_save_state_path = save_state_path;
        }

        Ok(())
    }

    fn reload_configs(
        &mut self,
        common_config: CommonConfig,
        emulator_config: Emulator::Config,
    ) -> NativeEmulatorResult<()> {
        self.common_config = common_config;
        self.emulator_config = emulator_config;

        self.emulator.reload_config(&self.emulator_config);
        self.audio_output.reload_config(&self.common_config);
        self.emulator.update_audio_output_frequency(self.audio_output.output_frequency());

        // In case fast forward hotkey changed
        self.audio_output.set_speed_multiplier(1);

        self.update_save_paths()?;

        self.rewinder.set_buffer_duration(Duration::from_secs(
            self.common_config.rewind_buffer_length_seconds,
        ));

        Ok(())
    }
}

pub struct RunnerSpawnArgs<'a, Emulator: EmulatorTrait> {
    pub create_emulator_fn: Box<CreateEmulatorFn<Emulator>>,
    pub change_disc_fn: ChangeDiscFn<Emulator>,
    pub remove_disc_fn: RemoveDiscFn<Emulator>,
    pub common_config: CommonConfig,
    pub emulator_config: Emulator::Config,
    pub rom_extension: String,
    pub save_state_path: PathBuf,
    pub initial_inputs: Emulator::Inputs,
    pub audio_output_handle: &'a mut SdlAudioOutputHandle,
    pub audio_output: SdlAudioOutput,
    pub save_writer: FsSaveWriter,
}

pub struct RunnerThread<Emulator: EmulatorTrait> {
    initial_window_title: String,
    default_window_size: WindowSize,
    renderer_handle: ThreadedRendererHandle,
    input_poller_handle: ThreadedInputPollerHandle<Emulator::Inputs>,
    command_sender: Sender<RunnerCommand<Emulator>>,
    response_receiver: Receiver<RunnerCommandResponse>,
    error_receiver: Receiver<Box<dyn Error + Send + Sync + 'static>>,
    save_state_metadata: Arc<Mutex<SaveStateMetadata>>,
    paused: Arc<AtomicBool>,
}

impl<Emulator: EmulatorTrait> RunnerThread<Emulator> {
    pub fn spawn(
        RunnerSpawnArgs {
            create_emulator_fn,
            change_disc_fn,
            remove_disc_fn,
            common_config,
            emulator_config,
            rom_extension,
            save_state_path,
            initial_inputs,
            audio_output_handle,
            audio_output,
            mut save_writer,
        }: RunnerSpawnArgs<'_, Emulator>,
    ) -> NativeEmulatorResult<Self> {
        let (init_sender, init_receiver) = mpsc::sync_channel(0);
        let (command_sender, command_receiver) = mpsc::channel();
        let (response_sender, response_receiver) = mpsc::channel();
        let (error_sender, error_receiver) = mpsc::channel();

        let paused = Arc::new(AtomicBool::new(false));

        let (renderer, renderer_handle) = ThreadedRenderer::new();
        let input_poller = ThreadedInputPoller::new(initial_inputs);
        let input_poller_handle = input_poller.handle();

        let save_state_paths = state::init_paths(&save_state_path)?;
        let save_state_metadata = Arc::new(Mutex::new(SaveStateMetadata::load(
            &save_state_paths,
            Emulator::save_state_version(),
        )));

        let runner_handle = {
            let paused = Arc::clone(&paused);
            let save_state_metadata = Arc::clone(&save_state_metadata);

            let common_config = common_config.clone();

            thread::spawn(move || match create_emulator_fn(&mut save_writer) {
                Ok(CreatedEmulator { emulator, window_title, default_window_size }) => {
                    init_sender.send(Ok((window_title, default_window_size))).unwrap();

                    let rewinder = Rewinder::new(Duration::from_secs(
                        common_config.rewind_buffer_length_seconds,
                    ));

                    let rom_path = common_config.rom_file_path.clone();
                    run_thread(RunnerThreadState {
                        emulator,
                        renderer,
                        audio_output,
                        input_poller,
                        save_writer,
                        common_config,
                        emulator_config,
                        command_receiver,
                        response_sender,
                        error_sender,
                        rom_path,
                        rom_extension,
                        base_save_state_path: save_state_path,
                        save_state_paths,
                        save_state_metadata,
                        paused,
                        step_frame: false,
                        rewinder,
                        change_disc_fn,
                        remove_disc_fn,
                        debugger_process: None,
                    });

                    log::info!("Runner thread has terminated");
                }
                Err(err) => {
                    init_sender.send(Err(err)).unwrap();
                }
            })
        };

        audio_output_handle.set_emulator_thread(runner_handle.thread().clone());

        let (initial_window_title, default_window_size) = init_receiver.recv().unwrap()?;

        Ok(Self {
            initial_window_title,
            default_window_size,
            renderer_handle,
            input_poller_handle,
            command_sender,
            response_receiver,
            error_receiver,
            save_state_metadata,
            paused,
        })
    }

    pub fn try_recv_frame<R: Renderer>(
        &self,
        renderer: &mut R,
        timeout: Duration,
    ) -> Result<(), RecvFrameError<R::Err>> {
        self.renderer_handle.try_recv_frame(renderer, timeout)
    }

    pub fn try_recv_error(&self) -> NativeEmulatorResult<()> {
        match self.error_receiver.try_recv() {
            Ok(err) => Err(NativeEmulatorError::Emulator(err)),
            Err(TryRecvError::Empty) => Ok(()),
            Err(TryRecvError::Disconnected) => Err(NativeEmulatorError::LostRunnerConnection),
        }
    }

    pub fn try_recv_command_response(&self) -> Option<RunnerCommandResponse> {
        self.response_receiver.try_recv().ok()
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn update_inputs(&mut self, inputs: &Emulator::Inputs) {
        self.input_poller_handle.update_inputs(inputs);
    }

    pub fn send_command(&self, command: RunnerCommand<Emulator>) -> NativeEmulatorResult<()> {
        self.command_sender.send(command).map_err(|_| NativeEmulatorError::LostRunnerConnection)
    }

    pub fn save_state_metadata(&self) -> &Arc<Mutex<SaveStateMetadata>> {
        &self.save_state_metadata
    }

    pub fn initial_window_title(&self) -> &str {
        &self.initial_window_title
    }

    pub fn default_window_size(&self) -> WindowSize {
        self.default_window_size
    }
}

fn run_thread<Emulator: EmulatorTrait>(mut state: RunnerThreadState<Emulator>) {
    loop {
        while let Ok(command) = state.command_receiver.try_recv() {
            match handle_command(&mut state, command) {
                Ok(CommandEffect::None) => {}
                Ok(CommandEffect::Terminate) => return,
                Err(CommandError::ReloadConfig(err)) => {
                    log::error!("{err}");
                }
                Err(CommandError::LostConnection) => {
                    log::error!("{}", CommandError::LostConnection);
                    return;
                }
            }
        }

        let paused = state.paused.load(Ordering::Relaxed);
        let rewinding = state.rewinder.is_rewinding();

        let should_run_emulator = !rewinding && (!paused || state.step_frame);

        if should_run_emulator {
            if let Err(err) = run_till_next_frame(&mut state) {
                let _ = state.error_sender.send(err.into());
                return;
            }

            state.rewinder.record_frame(&state.emulator);

            state.audio_output.adjust_dynamic_resampling_ratio();
            state.emulator.update_audio_output_frequency(state.audio_output.output_frequency());
        }

        state.step_frame = false;

        if rewinding
            && let Err(err) = state.rewinder.tick(
                &mut state.emulator,
                &mut state.renderer,
                &state.emulator_config,
            )
        {
            let _ = state.error_sender.send(err.into());
            return;
        }

        if let Some(debugger_process) = &mut state.debugger_process
            && (should_run_emulator || rewinding)
            && let Err(err) = debugger_process.run(&mut state.emulator)
        {
            log::error!("Error updating debugger in runner thread: {err}");
        }

        if !should_run_emulator {
            // Don't spin loop when the emulator is paused or rewinding
            thread::sleep(Duration::from_millis(1));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandEffect {
    None,
    Terminate,
}

#[derive(Debug, Error)]
enum CommandError {
    #[error("Error reloading config: {0}")]
    ReloadConfig(#[source] NativeEmulatorError),
    #[error("Lost connection to main thread")]
    LostConnection,
}

fn handle_command<Emulator: EmulatorTrait>(
    state: &mut RunnerThreadState<Emulator>,
    command: RunnerCommand<Emulator>,
) -> Result<CommandEffect, CommandError> {
    match command {
        RunnerCommand::Terminate => {
            return Ok(CommandEffect::Terminate);
        }
        RunnerCommand::SoftReset => {
            state.emulator.soft_reset();
        }
        RunnerCommand::HardReset => {
            state.emulator.hard_reset(&mut state.save_writer);
        }
        RunnerCommand::ChangeDisc(path) => {
            change_disc(state, path)?;
        }
        RunnerCommand::RemoveDisc => {
            (state.remove_disc_fn)(&mut state.emulator);
        }
        RunnerCommand::StepFrame => {
            state.step_frame = true;
        }
        RunnerCommand::FastForward(true) => {
            state.audio_output.set_speed_multiplier(state.common_config.fast_forward_multiplier);
        }
        RunnerCommand::FastForward(false) => {
            state.audio_output.set_speed_multiplier(1);
        }
        RunnerCommand::Rewind(true) => {
            state.rewinder.start_rewinding();
        }
        RunnerCommand::Rewind(false) => {
            state.rewinder.stop_rewinding();
        }
        RunnerCommand::SaveState { slot } => {
            save_state(state, slot)?;
        }
        RunnerCommand::LoadState { slot } => {
            load_state(state, slot)?;
        }
        RunnerCommand::ReloadConfig(configs) => {
            state.reload_configs(configs.0, configs.1).map_err(CommandError::ReloadConfig)?;
        }
        RunnerCommand::StartDebugger(debugger_process) => {
            state.debugger_process = Some(debugger_process);
        }
        RunnerCommand::StopDebugger => {
            state.debugger_process = None;
        }
    }

    Ok(CommandEffect::None)
}

type RunTillNextErr<Emulator> = <Emulator as EmulatorTrait>::Err<
    <ThreadedRenderer as Renderer>::Err,
    <SdlAudioOutput as AudioOutput>::Err,
    <FsSaveWriter as SaveWriter>::Err,
>;

fn run_till_next_frame<Emulator: EmulatorTrait>(
    state: &mut RunnerThreadState<Emulator>,
) -> Result<(), RunTillNextErr<Emulator>> {
    while state.emulator.tick(
        &mut state.renderer,
        &mut state.audio_output,
        &mut state.input_poller,
        &mut state.save_writer,
    )? != TickEffect::FrameRendered
    {}

    Ok(())
}

fn save_state<Emulator: EmulatorTrait>(
    state: &mut RunnerThreadState<Emulator>,
    slot: usize,
) -> Result<(), CommandError> {
    let result = {
        let mut save_state_metadata = state.save_state_metadata.lock().unwrap();
        state::save(&state.emulator, &state.save_state_paths, slot, &mut save_state_metadata)
    };

    let message = match result {
        Ok(()) => RunnerCommandResponse::SaveStateSucceeded { slot },
        Err(err) => RunnerCommandResponse::SaveStateFailed { slot, err },
    };

    state.response_sender.send(message).map_err(|_| CommandError::LostConnection)
}

fn load_state<Emulator: EmulatorTrait>(
    state: &mut RunnerThreadState<Emulator>,
    slot: usize,
) -> Result<(), CommandError> {
    let result =
        state::load(&mut state.emulator, &state.emulator_config, &state.save_state_paths, slot);

    let message = match result {
        Ok(()) => RunnerCommandResponse::LoadStateSucceeded { slot },
        Err(err) => RunnerCommandResponse::LoadStateFailed { slot, err },
    };

    state.response_sender.send(message).map_err(|_| CommandError::LostConnection)
}

fn change_disc<Emulator: EmulatorTrait>(
    state: &mut RunnerThreadState<Emulator>,
    path: PathBuf,
) -> Result<(), CommandError> {
    let result = (state.change_disc_fn)(&mut state.emulator, path.clone());

    if result.is_ok() {
        state.rom_path.clone_from(&path);

        if let Err(err) = state.update_save_paths() {
            log::error!("Error updating save paths after disc change: {err}");
        }
    }

    let message = match result {
        Ok(window_title) => RunnerCommandResponse::ChangeDiscSucceeded { window_title },
        Err(err) => RunnerCommandResponse::ChangeDiscFailed(err),
    };

    state.response_sender.send(message).map_err(|_| CommandError::LostConnection)
}
