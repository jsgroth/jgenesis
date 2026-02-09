use crate::NativeEmulatorResult;
use crate::config::CommonConfig;
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::input::ThreadedInputPoller;
use crate::mainloop::render::{RecvFrameError, ThreadedRenderer, ThreadedRendererHandle};
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{CreateEmulatorFn, CreatedEmulator};
use jgenesis_common::frontend::{AudioOutput, EmulatorTrait, Renderer, SaveWriter, TickEffect};
use jgenesis_native_config::common::WindowSize;
use std::error::Error;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum RunnerCommand<Config> {
    Terminate,
    Pause(bool),
    StepFrame,
    FastForward(bool),
    Rewind(bool),
    SaveState { slot: usize },
    LoadState { slot: usize },
    ReloadConfig(Box<(CommonConfig, Config)>),
}

struct RunnerThreadState<Emulator: EmulatorTrait> {
    emulator: Emulator,
    renderer: ThreadedRenderer,
    audio_output: SdlAudioOutput,
    input_poller: ThreadedInputPoller<Emulator::Inputs>,
    save_writer: FsSaveWriter,
    command_receiver: Receiver<RunnerCommand<Emulator::Config>>,
    error_sender: Sender<Box<dyn Error + Send + Sync + 'static>>,
}

pub struct RunnerThread<Emulator: EmulatorTrait> {
    window_title: String,
    default_window_size: WindowSize,
    command_sender: Sender<RunnerCommand<Emulator::Config>>,
    renderer_handle: ThreadedRendererHandle,
    input_poller: ThreadedInputPoller<Emulator::Inputs>,
    error_receiver: Receiver<Box<dyn Error + Send + Sync + 'static>>,
}

impl<Emulator: EmulatorTrait> RunnerThread<Emulator> {
    pub fn spawn(
        create_emulator_fn: Box<CreateEmulatorFn<Emulator>>,
        initial_inputs: Emulator::Inputs,
        audio_output: SdlAudioOutput,
        mut save_writer: FsSaveWriter,
    ) -> NativeEmulatorResult<Self> {
        let (init_sender, init_receiver) = mpsc::sync_channel(0);
        let (command_sender, command_receiver) = mpsc::channel();
        let (error_sender, error_receiver) = mpsc::channel();

        let (renderer, renderer_handle) = ThreadedRenderer::new();
        let input_poller = ThreadedInputPoller::new(initial_inputs);

        {
            let input_poller = input_poller.clone();

            thread::spawn(move || match create_emulator_fn(&mut save_writer) {
                Ok(CreatedEmulator { emulator, window_title, default_window_size }) => {
                    init_sender.send(Ok((window_title, default_window_size))).unwrap();

                    run_thread(RunnerThreadState {
                        emulator,
                        renderer,
                        audio_output,
                        input_poller,
                        save_writer,
                        command_receiver,
                        error_sender,
                    });
                }
                Err(err) => {
                    init_sender.send(Err(err)).unwrap();
                }
            });
        }

        let (window_title, default_window_size) = init_receiver.recv().unwrap()?;

        Ok(Self {
            window_title,
            default_window_size,
            command_sender,
            renderer_handle,
            input_poller,
            error_receiver,
        })
    }

    pub fn send_command(
        &self,
        command: RunnerCommand<Emulator::Config>,
    ) -> Result<(), SendError<RunnerCommand<Emulator::Config>>> {
        self.command_sender.send(command)
    }

    pub fn recv_frame<R: Renderer>(
        &self,
        renderer: &mut R,
        timeout: Duration,
    ) -> Result<(), RecvFrameError<R::Err>> {
        self.renderer_handle.try_recv_frame(renderer, timeout)
    }

    pub fn try_recv_error(&self) -> Result<Box<dyn Error + Send + Sync + 'static>, TryRecvError> {
        self.error_receiver.try_recv()
    }

    pub fn update_inputs(&mut self, inputs: &Emulator::Inputs) {
        self.input_poller.update_inputs(inputs);
    }

    pub fn window_title(&self) -> &str {
        &self.window_title
    }

    pub fn default_window_size(&self) -> WindowSize {
        self.default_window_size
    }
}

fn run_thread<Emulator: EmulatorTrait>(mut state: RunnerThreadState<Emulator>) {
    loop {
        if let Err(err) = run_till_next_frame(&mut state) {
            state.error_sender.send(err.into()).unwrap();
            return;
        }

        state.input_poller.check_for_updates();
    }
}

fn run_till_next_frame<Emulator: EmulatorTrait>(
    state: &mut RunnerThreadState<Emulator>,
) -> Result<
    (),
    Emulator::Err<
        <ThreadedRenderer as Renderer>::Err,
        <SdlAudioOutput as AudioOutput>::Err,
        <FsSaveWriter as SaveWriter>::Err,
    >,
> {
    while state.emulator.tick(
        &mut state.renderer,
        &mut state.audio_output,
        &mut state.input_poller,
        &mut state.save_writer,
    )? != TickEffect::FrameRendered
    {}

    Ok(())
}
