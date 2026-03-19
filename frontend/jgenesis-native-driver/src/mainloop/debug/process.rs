use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::debug::DebugRenderContext;
use crate::mainloop::input::ThreadedInputPoller;
use crate::mainloop::render::ThreadedRenderer;
use crate::mainloop::runner::RunTillNextErr;
use crate::mainloop::save::FsSaveWriter;
use jgenesis_common::frontend::{EmulatorTrait, TickEffect};
use jgenesis_common::sync::{SharedVarReceiver, SharedVarSender};
use std::error::Error;

pub trait DebuggerRunnerProcess<Emulator: EmulatorTrait>: Send + 'static {
    fn run(
        &mut self,
        emulator: &mut Emulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;

    fn run_emulator_till_next_frame(
        &mut self,
        emulator: &mut Emulator,
        renderer: &mut ThreadedRenderer,
        audio_output: &mut SdlAudioOutput,
        input_poller: &mut ThreadedInputPoller<Emulator::Inputs>,
        save_writer: &mut FsSaveWriter,
    ) -> Result<(), RunTillNextErr<Emulator>> {
        while emulator.tick(renderer, audio_output, input_poller, save_writer)?
            != TickEffect::FrameRendered
        {}

        Ok(())
    }
}

pub trait DebuggerMainProcess {
    fn run(
        &mut self,
        ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;
}

pub type DebugFn<Emulator> =
    fn() -> (Box<dyn DebuggerRunnerProcess<Emulator>>, Box<dyn DebuggerMainProcess>);

pub struct NullDebugger;

impl<Emulator: EmulatorTrait> DebuggerRunnerProcess<Emulator> for NullDebugger {
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

pub fn null_debug_fn<Emulator: EmulatorTrait>()
-> (Box<dyn DebuggerRunnerProcess<Emulator>>, Box<dyn DebuggerMainProcess>) {
    (Box::new(NullDebugger), Box::new(NullDebugger))
}

pub struct PartialCloneRunnerProcess<Emulator> {
    emulator_sender: SharedVarSender<Emulator>,
}

impl<Emulator: EmulatorTrait + Send + Sync + 'static> DebuggerRunnerProcess<Emulator>
    for PartialCloneRunnerProcess<Emulator>
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

pub fn partial_clone_debug_fn<Emulator>(
    render_fn: Box<DebugRenderFn<Emulator>>,
) -> (Box<dyn DebuggerRunnerProcess<Emulator>>, Box<dyn DebuggerMainProcess>)
where
    Emulator: EmulatorTrait + Send + Sync + 'static,
{
    let (emulator_sender, emulator_receiver) = jgenesis_common::sync::new_shared_var();

    let runner_process = PartialCloneRunnerProcess { emulator_sender };
    let main_process = PartialCloneMainProcess { emulator_receiver, render_fn };

    (Box::new(runner_process), Box::new(main_process))
}
