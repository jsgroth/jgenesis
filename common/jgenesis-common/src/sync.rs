use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct SharedVarState<T> {
    locked: Mutex<Option<T>>,
    updated: AtomicBool,
}

pub struct SharedVarReceiver<T> {
    latest: Option<T>,
    state: Arc<SharedVarState<T>>,
}

pub struct SharedVarSender<T> {
    state: Arc<SharedVarState<T>>,
}

impl<T> SharedVarReceiver<T> {
    /// Returns the most recently received value, or `None` if no value has been received yet.
    ///
    /// Returns a mutable reference for convenience, but any mutations will only affect the current
    /// value and will get discarded when a new value is received.
    #[must_use]
    #[allow(clippy::missing_panics_doc)] // Mutex poisoning is impossible here
    pub fn get(&mut self) -> Option<&mut T> {
        if self.state.updated.load(Ordering::Relaxed)
            && self.state.updated.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
                == Ok(true)
            && let Some(value) = self.state.locked.lock().unwrap().take()
        {
            self.latest = Some(value);
        }

        self.latest.as_mut()
    }
}

impl<T> SharedVarSender<T> {
    /// Replace the current value with a new one.
    #[allow(clippy::missing_panics_doc)] // Mutex poisoning is impossible here
    pub fn update(&self, value: T) {
        *self.state.locked.lock().unwrap() = Some(value);
        self.state.updated.store(true, Ordering::Release);
    }
}

/// Creates a shared var.
///
/// This is similar to an SPSC channel, but the receiver retains the most recent value received and
/// will return it on repeated [`SharedVarReceiver::get`] calls. It _only_ retains the most recent
/// value, so the receiver will miss values if the sender sends multiple values in between
/// [`SharedVarReceiver::get`] calls.
pub fn new_shared_var<T>() -> (SharedVarSender<T>, SharedVarReceiver<T>) {
    let sender = SharedVarSender {
        state: Arc::new(SharedVarState {
            locked: Mutex::new(None),
            updated: AtomicBool::new(false),
        }),
    };

    let receiver = SharedVarReceiver { latest: None, state: Arc::clone(&sender.state) };

    (sender, receiver)
}
