use crate::mainloop::debug::DebugRenderContext;
use jgenesis_common::frontend::PartialClone;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub trait DebuggerRunnerProcess<Emulator>: Send + Sync + 'static {
    fn run(
        &mut self,
        emulator: &mut Emulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;
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

impl<Emulator> DebuggerRunnerProcess<Emulator> for NullDebugger {
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

pub fn null_debug_fn<Emulator>()
-> (Box<dyn DebuggerRunnerProcess<Emulator>>, Box<dyn DebuggerMainProcess>) {
    (Box::new(NullDebugger), Box::new(NullDebugger))
}

pub struct PartialCloneRunnerProcess<Emulator> {
    latest: Arc<Mutex<Option<Emulator>>>,
    updated: Arc<AtomicBool>,
}

impl<Emulator: PartialClone + Send + Sync + 'static> DebuggerRunnerProcess<Emulator>
    for PartialCloneRunnerProcess<Emulator>
{
    fn run(
        &mut self,
        emulator: &mut Emulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        *self.latest.lock().unwrap() = Some(emulator.partial_clone());
        self.updated.store(true, Ordering::Release);

        Ok(())
    }
}

pub type DebugRenderFn<Emulator> = dyn FnMut(DebugRenderContext<'_>, &mut Emulator);

pub struct PartialCloneMainProcess<Emulator> {
    latest: Arc<Mutex<Option<Emulator>>>,
    cached: Option<Emulator>,
    updated: Arc<AtomicBool>,
    render_fn: Box<DebugRenderFn<Emulator>>,
}

impl<Emulator: Send + Sync + 'static> DebuggerMainProcess for PartialCloneMainProcess<Emulator> {
    fn run(
        &mut self,
        ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        if self.updated.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            == Ok(true)
        {
            let emulator = self.latest.lock().unwrap().take();
            if let Some(emulator) = emulator {
                self.cached = Some(emulator);
            }
        }

        if let Some(emulator) = &mut self.cached {
            (self.render_fn)(ctx, emulator);
        }

        Ok(())
    }
}

pub fn partial_clone_debug_fn<Emulator>(
    render_fn: Box<DebugRenderFn<Emulator>>,
) -> (Box<dyn DebuggerRunnerProcess<Emulator>>, Box<dyn DebuggerMainProcess>)
where
    Emulator: PartialClone + Send + Sync + 'static,
{
    let runner_process = PartialCloneRunnerProcess {
        latest: Arc::new(Mutex::new(None)),
        updated: Arc::new(AtomicBool::new(false)),
    };

    let main_process = PartialCloneMainProcess {
        latest: Arc::clone(&runner_process.latest),
        cached: None,
        updated: Arc::clone(&runner_process.updated),
        render_fn,
    };

    (Box::new(runner_process), Box::new(main_process))
}
