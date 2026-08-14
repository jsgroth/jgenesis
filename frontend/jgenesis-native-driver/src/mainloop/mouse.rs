use std::collections::VecDeque;
use std::time::{Duration, Instant};

// SDL motion values are in pixels, so divide by some value to normalize them; 1000px is arbitrary
const DIVISOR: f64 = 1000.0;
const MULTIPLIER: f64 = 1.0 / DIVISOR;

// Slightly more than 1 frame at 60 FPS
const RECORDING_WINDOW: Duration = Duration::from_millis(20);

#[derive(Debug, Clone)]
struct MotionEvent {
    dx: f64,
    dy: f64,
    time: Instant,
}

#[derive(Debug, Clone)]
pub struct MouseVelocityTracker {
    motion_events: VecDeque<MotionEvent>,
    mouse_sensitivity: f64,
}

impl MouseVelocityTracker {
    pub fn new(mouse_sensitivity: f64) -> Self {
        Self { motion_events: VecDeque::with_capacity(10), mouse_sensitivity }
    }

    pub fn record_motion(&mut self, (dx, dy): (f32, f32)) {
        let multiplier = MULTIPLIER * self.mouse_sensitivity;
        let dx = multiplier * f64::from(dx);
        let dy = multiplier * f64::from(dy);

        self.motion_events.push_back(MotionEvent { dx, dy, time: Instant::now() });
    }

    fn prune_outdated_events(&mut self) {
        let min_keep_time = Instant::now() - RECORDING_WINDOW;

        while self.motion_events.front().is_some_and(|event| event.time < min_keep_time) {
            self.motion_events.pop_front();
        }
    }

    pub fn current(&mut self) -> (f64, f64) {
        self.prune_outdated_events();

        let (dx, dy) = self
            .motion_events
            .iter()
            .fold((0.0, 0.0), |(dx, dy), event| (dx + event.dx, dy + event.dy));

        (dx.clamp(-1.0, 1.0), dy.clamp(-1.0, 1.0))
    }

    pub fn set_mouse_sensitivity(&mut self, mouse_sensitivity: f64) {
        self.mouse_sensitivity = mouse_sensitivity;
    }
}
