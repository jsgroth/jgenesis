use jgenesis_common::frontend::InputPoller;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ThreadedInputPoller<Inputs> {
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

    pub fn check_for_updates(&mut self) {
        if self.updated.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            == Ok(true)
        {
            self.cached = self.locked.lock().unwrap().clone();
        }
    }

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
        // TODO check for updates here? don't want to do an Acquire load on every call
        &self.cached
    }
}
