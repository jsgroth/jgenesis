use crate::mainloop::bincode_config;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{EmulatorTrait, Renderer};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const FRAME_DIVIDER: u64 = 10;
const REWIND_SPEED: u64 = 2;

struct CompressorThread<State> {
    state_receiver: Receiver<State>,
    bytes_sender: Sender<Vec<u8>>,
    requests_in_flight: Arc<AtomicI32>,
}

struct CompressorThreadHandle<Emulator: EmulatorTrait> {
    state_sender: SyncSender<Box<Emulator::SaveState>>,
    bytes_receiver: Receiver<Vec<u8>>,
    requests_in_flight: Arc<AtomicI32>,
}

impl<Emulator: EmulatorTrait> CompressorThreadHandle<Emulator> {
    fn send_state(&self, state: Box<Emulator::SaveState>) {
        if self.state_sender.send(state).is_err() {
            log::error!("Lost connection to rewind compression thread; this is probably a bug");
            return;
        }

        self.requests_in_flight.fetch_add(1, Ordering::AcqRel);
    }

    fn try_recv_compressed_bytes(&self) -> Option<Vec<u8>> {
        self.bytes_receiver.try_recv().ok()
    }

    fn any_requests_in_flight(&self) -> bool {
        self.requests_in_flight.load(Ordering::Acquire) > 0
    }
}

fn spawn_compressor_thread<Emulator: EmulatorTrait>() -> CompressorThreadHandle<Emulator> {
    // Bound state sender channel to prevent it from growing infinitely if compressor can't keep up
    let (state_sender, state_receiver) = mpsc::sync_channel(10);
    let (bytes_sender, bytes_receiver) = mpsc::channel();
    let requests_in_flight = Arc::new(AtomicI32::new(0));

    let compressor_thread = CompressorThread {
        state_receiver,
        bytes_sender,
        requests_in_flight: Arc::clone(&requests_in_flight),
    };

    thread::spawn(move || run_compressor_thread(compressor_thread));

    CompressorThreadHandle { state_sender, bytes_receiver, requests_in_flight }
}

fn run_compressor_thread<State: Encode>(compressor: CompressorThread<State>) {
    loop {
        let Ok(state) = compressor.state_receiver.recv() else {
            // Runner thread has dropped sender; stop running
            return;
        };

        let compressed_bytes = compress_state(&state);
        if let Some(compressed_bytes) = compressed_bytes
            && compressor.bytes_sender.send(compressed_bytes).is_err()
        {
            // Runner thread has dropped receiver; stop running
            return;
        }

        compressor.requests_in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

fn compress_state<State: Encode>(state: &State) -> Option<Vec<u8>> {
    let mut encoder = zstd::Encoder::new(vec![], 0).ok()?;
    bincode::encode_into_std_write(state, &mut encoder, bincode_config!()).ok()?;
    encoder.finish().ok()
}

fn decompress_state<State: Decode<()>>(bytes: &[u8]) -> Option<State> {
    let mut decoder = zstd::Decoder::new(bytes).ok()?;
    bincode::decode_from_std_read(&mut decoder, bincode_config!()).ok()
}

pub struct Rewinder<Emulator: EmulatorTrait> {
    previous_states: VecDeque<Vec<u8>>,
    buffer_len: usize,
    frame_count: u64,
    last_rewind_time: Option<Instant>,
    compressor_handle: CompressorThreadHandle<Emulator>,
}

impl<Emulator: EmulatorTrait> Rewinder<Emulator> {
    pub fn new(buffer_duration: Duration) -> Self {
        let buffer_len = duration_to_buffer_len(buffer_duration);
        let compressor_handle = spawn_compressor_thread();

        Self {
            previous_states: VecDeque::with_capacity(buffer_len + 1),
            buffer_len,
            frame_count: 0,
            last_rewind_time: None,
            compressor_handle,
        }
    }

    pub fn record_frame(&mut self, emulator: &Emulator) {
        if self.buffer_len == 0 {
            return;
        }

        self.frame_count += 1;

        if self.frame_count.is_multiple_of(FRAME_DIVIDER) {
            self.compressor_handle.send_state(Box::new(emulator.to_save_state()));
        }

        self.recv_queued_compressed_bytes();
    }

    fn recv_queued_compressed_bytes(&mut self) {
        while let Some(compressed_bytes) = self.compressor_handle.try_recv_compressed_bytes() {
            self.previous_states.push_back(compressed_bytes);

            while self.previous_states.len() > self.buffer_len {
                self.previous_states.pop_front();
            }
        }
    }

    pub fn start_rewinding(&mut self) {
        if self.last_rewind_time.is_none() {
            self.last_rewind_time = Some(Instant::now());
        }

        while self.compressor_handle.any_requests_in_flight() {
            thread::sleep(Duration::from_millis(1));
        }

        self.recv_queued_compressed_bytes();
    }

    pub fn stop_rewinding(&mut self) {
        self.last_rewind_time = None;
    }

    pub fn is_rewinding(&self) -> bool {
        self.last_rewind_time.is_some()
    }

    pub fn tick<R>(
        &mut self,
        emulator: &mut Emulator,
        renderer: &mut R,
        config: &Emulator::Config,
    ) -> Result<(), R::Err>
    where
        Emulator: EmulatorTrait,
        R: Renderer,
    {
        let Some(last_rewind_time) = self.last_rewind_time else { return Ok(()) };

        let rewind_interval_secs = 1.0 / 60.0 * (FRAME_DIVIDER as f64) / (REWIND_SPEED as f64);

        let now = Instant::now();
        if now.duration_since(last_rewind_time) >= Duration::from_secs_f64(rewind_interval_secs) {
            let Some(compressed_bytes) = self.previous_states.pop_back() else { return Ok(()) };

            if let Some(state) = decompress_state(&compressed_bytes) {
                emulator.load_state(state);

                emulator.reload_config(config);
                emulator.force_render(renderer)?;
            }

            self.last_rewind_time = Some(now);
        }

        Ok(())
    }

    pub fn set_buffer_duration(&mut self, duration: Duration) {
        self.set_buffer_len(duration_to_buffer_len(duration));
    }

    fn set_buffer_len(&mut self, buffer_len: usize) {
        self.buffer_len = buffer_len;

        // If size increased, immediately resize deque to avoid incremental allocations later
        if buffer_len + 1 > self.previous_states.capacity() {
            self.previous_states.reserve(buffer_len + 1 - self.previous_states.capacity());
        }

        // If size decreased, immediately drop unused states
        while self.previous_states.len() > buffer_len {
            self.previous_states.pop_front();
        }
    }
}

fn duration_to_buffer_len(duration: Duration) -> usize {
    // Not really a better place for this, and this should get optimized out anyway
    assert_eq!(FRAME_DIVIDER % REWIND_SPEED, 0);

    (duration.as_secs() * 60 / (FRAME_DIVIDER / REWIND_SPEED)) as usize
}
