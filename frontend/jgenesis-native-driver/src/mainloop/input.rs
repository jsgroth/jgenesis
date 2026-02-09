use jgenesis_common::frontend::InputPoller;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct ThreadedInputPoller<Inputs> {
    cached: Inputs,
    locked: Arc<Mutex<Inputs>>,
    updated: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct ThreadedInputPollerHandle<Inputs> {
    cached: Inputs,
    locked: Arc<Mutex<Inputs>>,
    updated: Arc<AtomicBool>,
}

impl<Inputs: Clone + Eq> ThreadedInputPoller<Inputs> {
    pub fn new(initial_inputs: Inputs) -> Self {
        Self {
            cached: initial_inputs.clone(),
            locked: Arc::new(Mutex::new(initial_inputs)),
            updated: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn handle(&self) -> ThreadedInputPollerHandle<Inputs> {
        ThreadedInputPollerHandle {
            cached: self.cached.clone(),
            locked: Arc::clone(&self.locked),
            updated: Arc::clone(&self.updated),
        }
    }
}

impl<Inputs: Clone + Eq> ThreadedInputPollerHandle<Inputs> {
    pub fn update_inputs(&mut self, inputs: &Inputs) {
        if inputs == &self.cached {
            return;
        }

        self.cached = inputs.clone();
        *self.locked.lock().unwrap() = inputs.clone();
        self.updated.store(true, Ordering::Release);
    }
}

impl<Inputs: Clone + Eq> InputPoller<Inputs> for ThreadedInputPoller<Inputs> {
    fn poll(&mut self) -> &Inputs {
        if self.updated.load(Ordering::Relaxed)
            && self.updated.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
                == Ok(true)
        {
            self.cached = self.locked.lock().unwrap().clone();
        }

        &self.cached
    }
}
