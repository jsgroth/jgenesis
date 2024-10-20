use std::collections::VecDeque;
use std::env;
use std::time::{Duration, Instant};

const LOG_INTERVAL_SECONDS: u64 = 1;
const LOG_INTERVAL: Duration = Duration::from_secs(LOG_INTERVAL_SECONDS);
const WINDOW_INTERVAL_SECONDS: u64 = 3;
const WINDOW_INTERVAL: Duration = Duration::from_secs(WINDOW_INTERVAL_SECONDS);

#[derive(Debug, Clone)]
pub struct FpsTracker {
    last_window_time: Instant,
    last_log_time: Instant,
    frame_times: VecDeque<Instant>,
}

impl FpsTracker {
    pub fn new() -> Self {
        Self {
            last_window_time: Instant::now() - (WINDOW_INTERVAL - LOG_INTERVAL),
            last_log_time: Instant::now(),
            frame_times: VecDeque::with_capacity((2 * WINDOW_INTERVAL_SECONDS * 60) as usize),
        }
    }

    pub fn record_frame(&mut self) {
        let now = Instant::now();
        self.frame_times.push_back(now);

        let next_log_time = self.last_log_time + LOG_INTERVAL;
        if now >= next_log_time {
            let window_end_time = self.last_window_time + WINDOW_INTERVAL;
            let mut frame_count = 0;
            for &time in &self.frame_times {
                if time >= window_end_time {
                    break;
                }
                frame_count += 1;
            }

            let next_window_time = self.last_window_time + LOG_INTERVAL;
            while self.frame_times.front().is_some_and(|&time| time < next_window_time) {
                self.frame_times.pop_front();
            }

            self.last_window_time = next_window_time;
            self.last_log_time = next_log_time;

            // TODO expose FPS in the UI somewhere?
            if env::var("JGENESIS_LOG_FPS").is_ok_and(|var| !var.is_empty()) {
                let fps = f64::from(frame_count) / (WINDOW_INTERVAL_SECONDS as f64);
                log::info!("FPS: {}", fps.round());
            }
        }
    }
}
