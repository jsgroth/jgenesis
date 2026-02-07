use crate::NativeEmulatorResult;
use crate::mainloop::input::ThreadedInputPoller;
use crate::mainloop::render::{DoneMessage, FrameMessage, ThreadedRenderer};
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{CreateEmulatorFn, CreatedEmulator};
use jgenesis_common::frontend::EmulatorTrait;
use jgenesis_native_config::common::WindowSize;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SendError, Sender, SyncSender};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum RunnerCommand {}

#[derive(Debug, Clone, Copy)]
pub enum RunState {
    RunNormally = 0,
    Pause = 1,
    FastForward = 2,
    Rewind = 3,
}

impl RunState {
    fn from_ordinal(ordinal: u8) -> Option<Self> {
        match ordinal {
            0 => Some(Self::RunNormally),
            1 => Some(Self::Pause),
            2 => Some(Self::FastForward),
            3 => Some(Self::Rewind),
            _ => None,
        }
    }
}

struct RunnerThreadState<Emulator: EmulatorTrait> {
    emulator: Emulator,
    renderer: ThreadedRenderer,
    input_poller: ThreadedInputPoller<Emulator::Inputs>,
    save_writer: FsSaveWriter,
    run_state: Arc<AtomicU8>,
    step_frame: Arc<AtomicBool>,
    command_receiver: Receiver<RunnerCommand>,
}

pub struct RunnerThread<Emulator: EmulatorTrait> {
    window_title: String,
    default_window_size: WindowSize,
    run_state: Arc<AtomicU8>,
    step_frame: Arc<AtomicBool>,
    command_sender: Sender<RunnerCommand>,
    frame_receiver: Receiver<FrameMessage>,
    render_done_sender: SyncSender<DoneMessage>,
    input_poller: ThreadedInputPoller<Emulator::Inputs>,
}

impl<Emulator: EmulatorTrait> RunnerThread<Emulator> {
    pub fn spawn(
        create_emulator_fn: Box<CreateEmulatorFn<Emulator>>,
        initial_inputs: Emulator::Inputs,
        mut save_writer: FsSaveWriter,
    ) -> NativeEmulatorResult<Self> {
        let run_state = Arc::new(AtomicU8::new(RunState::RunNormally as u8));
        let step_frame = Arc::new(AtomicBool::new(false));

        let (init_sender, init_receiver) = mpsc::sync_channel(0);
        let (command_sender, command_receiver) = mpsc::channel();
        let (renderer, frame_receiver, render_done_sender) = ThreadedRenderer::new();
        let input_poller = ThreadedInputPoller::new(initial_inputs);

        {
            let run_state = Arc::clone(&run_state);
            let step_frame = Arc::clone(&step_frame);
            let input_poller = input_poller.clone();

            thread::spawn(move || match create_emulator_fn(&mut save_writer) {
                Ok(CreatedEmulator { emulator, window_title, default_window_size }) => {
                    init_sender.send(Ok((window_title, default_window_size))).unwrap();

                    run_thread(RunnerThreadState {
                        emulator,
                        renderer,
                        input_poller,
                        save_writer,
                        run_state,
                        step_frame,
                        command_receiver,
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
            run_state,
            step_frame,
            command_sender,
            frame_receiver,
            render_done_sender,
            input_poller,
        })
    }

    pub fn set_run_state(&self, run_state: RunState) {
        self.run_state.store(run_state as u8, Ordering::Relaxed);
    }

    pub fn set_step_frame(&self) {
        self.step_frame.store(true, Ordering::Relaxed);
    }

    pub fn recv_frame(&self, timeout: Duration) -> Result<FrameMessage, RecvTimeoutError> {
        self.frame_receiver.recv_timeout(timeout)
    }

    pub fn send_render_done(&self, message: DoneMessage) -> Result<(), SendError<DoneMessage>> {
        self.render_done_sender.send(message)
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
    loop {}
}
