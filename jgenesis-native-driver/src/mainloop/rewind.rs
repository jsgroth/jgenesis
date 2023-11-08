use jgenesis_common::frontend::{PartialClone, Renderer, TakeRomFrom, TickableEmulator};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const FRAME_DIVIDER: u64 = 10;

pub struct Rewinder<Emulator: PartialClone> {
    previous_states: VecDeque<Emulator>,
    buffer_len: usize,
    frame_count: u64,
    last_rewind_time: Option<Instant>,
}

impl<Emulator: PartialClone> Rewinder<Emulator> {
    pub fn new(buffer_duration: Duration) -> Self {
        let buffer_len = duration_to_buffer_len(buffer_duration);
        Self {
            previous_states: VecDeque::with_capacity(buffer_len + 1),
            buffer_len,
            frame_count: 0,
            last_rewind_time: None,
        }
    }

    pub fn record_frame(&mut self, emulator: &Emulator) {
        if self.buffer_len == 0 {
            return;
        }

        self.frame_count += 1;

        if self.frame_count % FRAME_DIVIDER == 0 {
            self.previous_states.push_back(emulator.partial_clone());

            while self.previous_states.len() > self.buffer_len {
                self.previous_states.pop_front();
            }
        }
    }

    pub fn start_rewinding(&mut self) {
        if self.last_rewind_time.is_none() {
            self.last_rewind_time = Some(Instant::now());
        }
    }

    pub fn stop_rewinding(&mut self) {
        self.last_rewind_time = None;
    }

    pub fn is_rewinding(&self) -> bool {
        self.last_rewind_time.is_some()
    }

    pub fn tick<R>(&mut self, emulator: &mut Emulator, renderer: &mut R) -> Result<(), R::Err>
    where
        Emulator: TickableEmulator + TakeRomFrom,
        R: Renderer,
    {
        let Some(last_rewind_time) = self.last_rewind_time else { return Ok(()) };

        // Divide by 2 so rewind runs at double speed
        let rewind_interval_secs = 1.0 / 60.0 * FRAME_DIVIDER as f64 / 2.0;

        let now = Instant::now();
        if now.duration_since(last_rewind_time) >= Duration::from_secs_f64(rewind_interval_secs) {
            let Some(mut clone) = self.previous_states.pop_back() else { return Ok(()) };
            clone.take_rom_from(emulator);
            *emulator = clone;

            emulator.force_render(renderer)?;

            self.last_rewind_time = Some(now);
        }

        Ok(())
    }

    pub fn set_buffer_duration(&mut self, duration: Duration) {
        self.set_buffer_len(duration_to_buffer_len(duration));
    }

    fn set_buffer_len(&mut self, buffer_len: usize) {
        self.buffer_len = buffer_len;

        if buffer_len + 1 > self.previous_states.capacity() {
            // Immediately resize deque to avoid incremental allocations later
            let mut resized = VecDeque::with_capacity(buffer_len + 1);
            resized.extend(self.previous_states.drain(..));
            self.previous_states = resized;
        }
    }
}

fn duration_to_buffer_len(duration: Duration) -> usize {
    (duration.as_secs() * 60 / 5) as usize
}
