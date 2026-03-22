use crate::DebugRenderContext;
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, InputPoller, Renderer, SaveWriter, TickEffect,
};
use jgenesis_common::sync::{SharedVarReceiver, SharedVarSender};
use std::error::Error;

pub type RunTillNextResult<Emulator, RErr, AErr, SErr> =
    Result<(), <Emulator as EmulatorTrait>::Err<RErr, AErr, SErr>>;

pub trait DebuggerRunnerProcess<Emulator, R, A, I, S>: Send + 'static
where
    Emulator: EmulatorTrait,
    R: Renderer,
    A: AudioOutput,
    I: InputPoller<Emulator::Inputs>,
    S: SaveWriter,
{
    /// Run periodic processing, e.g. copying and sending emulator state to the frontend. This will
    /// generally get called once per frame while the emulator is running.
    ///
    /// # Errors
    ///
    /// May propagate any errors encountered during processing.
    fn run(
        &mut self,
        emulator: &mut Emulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;

    /// Run the emulator until the next frame render.
    ///
    /// This exists as a hook so that, if desired, the debugger can call a different emulator entry
    /// point while the debugger is active.
    ///
    /// # Errors
    ///
    /// Should propagate any errors encountered while running the emulator.
    fn run_emulator_till_next_frame(
        &mut self,
        emulator: &mut Emulator,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
    ) -> RunTillNextResult<Emulator, R::Err, A::Err, S::Err> {
        while emulator.tick(renderer, audio_output, input_poller, save_writer)?
            != TickEffect::FrameRendered
        {}

        Ok(())
    }
}

pub trait DebuggerMainProcess {
    /// Render the debugger frontend.
    ///
    /// # Errors
    ///
    /// May return any errors encountered while processing or rendering.
    fn run(
        &mut self,
        ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;
}

pub type DebuggerProcesses<Emulator, R, A, I, S> =
    (Box<dyn DebuggerRunnerProcess<Emulator, R, A, I, S>>, Box<dyn DebuggerMainProcess>);

pub type DebugFn<Emulator, R, A, I, S> = fn() -> DebuggerProcesses<Emulator, R, A, I, S>;

pub struct NullDebugger;

impl<Emulator, R, A, I, S> DebuggerRunnerProcess<Emulator, R, A, I, S> for NullDebugger
where
    Emulator: EmulatorTrait,
    R: Renderer,
    A: AudioOutput,
    I: InputPoller<Emulator::Inputs>,
    S: SaveWriter,
{
    fn run(
        &mut self,
        _emulator: &mut Emulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        Ok(())
    }
}

impl DebuggerMainProcess for NullDebugger {
    fn run(
        &mut self,
        _ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        Ok(())
    }
}

#[must_use]
pub fn null_debug_fn<Emulator, R, A, I, S>() -> DebuggerProcesses<Emulator, R, A, I, S>
where
    Emulator: EmulatorTrait,
    R: Renderer,
    A: AudioOutput,
    I: InputPoller<Emulator::Inputs>,
    S: SaveWriter,
{
    (Box::new(NullDebugger), Box::new(NullDebugger))
}

pub struct PartialCloneRunnerProcess<Emulator> {
    emulator_sender: SharedVarSender<Emulator>,
}

impl<Emulator, R, A, I, S> DebuggerRunnerProcess<Emulator, R, A, I, S>
    for PartialCloneRunnerProcess<Emulator>
where
    Emulator: EmulatorTrait + Send + Sync + 'static,
    R: Renderer,
    A: AudioOutput,
    I: InputPoller<Emulator::Inputs>,
    S: SaveWriter,
{
    fn run(
        &mut self,
        emulator: &mut Emulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        self.emulator_sender.update(emulator.partial_clone());

        Ok(())
    }
}

pub type DebugRenderFn<Emulator> = dyn FnMut(DebugRenderContext<'_>, &mut Emulator);

pub struct PartialCloneMainProcess<Emulator> {
    emulator_receiver: SharedVarReceiver<Emulator>,
    render_fn: Box<DebugRenderFn<Emulator>>,
}

impl<Emulator: Send + Sync + 'static> DebuggerMainProcess for PartialCloneMainProcess<Emulator> {
    fn run(
        &mut self,
        ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        if let Some(emulator) = self.emulator_receiver.get() {
            (self.render_fn)(ctx, emulator);
        }

        Ok(())
    }
}

#[must_use]
pub fn partial_clone_debug_fn<Emulator, R, A, I, S>(
    render_fn: Box<DebugRenderFn<Emulator>>,
) -> DebuggerProcesses<Emulator, R, A, I, S>
where
    Emulator: EmulatorTrait + Send + Sync + 'static,
    R: Renderer,
    A: AudioOutput,
    I: InputPoller<Emulator::Inputs>,
    S: SaveWriter,
{
    let (emulator_sender, emulator_receiver) = jgenesis_common::sync::new_shared_var();

    let runner_process = PartialCloneRunnerProcess { emulator_sender };
    let main_process = PartialCloneMainProcess { emulator_receiver, render_fn };

    (Box::new(runner_process), Box::new(main_process))
}
