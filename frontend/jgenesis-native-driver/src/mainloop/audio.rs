use crate::config::CommonConfig;
use jgenesis_common::audio::DynamicResamplingRate;
use jgenesis_common::frontend::AudioOutput;
use sdl3::AudioSubsystem;
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::Thread;
use std::time::Duration;
use std::{cmp, thread};
use thiserror::Error;

// Always output in stereo
const CHANNELS: i32 = 2;

// Number of samples to buffer before locking and pushing to the audio queue
const INTERNAL_AUDIO_BUFFER_LEN: usize = 16;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Error opening SDL3 audio stream: {0}")]
    OpenStream(sdl3::Error),
    #[error("Error pausing SDL3 audio stream: {0}")]
    PauseStream(sdl3::Error),
    #[error("Error pushing audio samples to SDL3 audio stream: {0}")]
    QueueAudio(sdl3::Error),
}

pub type AudioResult<T> = Result<T, AudioError>;

struct AudioCallbackState {
    queue: VecDeque<(f32, f32)>,
    hardware_queue_size: u32,
    unpark_threshold: u32,
    emulator_thread: Thread,
    error: Option<sdl3::Error>,
}

impl AudioCallbackState {
    fn new(config: &CommonConfig) -> Self {
        Self {
            queue: VecDeque::with_capacity(2 * config.audio_buffer_size as usize),
            hardware_queue_size: config.audio_hardware_queue_size,
            unpark_threshold: audio_sync_threshold(config),
            emulator_thread: thread::current(),
            error: None,
        }
    }
}

struct AudioQueueCallback {
    state: Arc<Mutex<AudioCallbackState>>,
}

impl AudioCallback<f32> for AudioQueueCallback {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        if requested <= 0 {
            return;
        }

        let mut state = self.state.lock().unwrap();

        let stereo_samples = cmp::max(state.hardware_queue_size, ((requested + 1) / 2) as u32);
        for _ in 0..stereo_samples {
            let Some((sample_l, sample_r)) = state.queue.pop_front() else { break };

            if let Err(err) = stream.put_data_f32(&[sample_l, sample_r]) {
                log::error!("Error pushing audio samples: {err}");
                state.error = Some(err);
                state.emulator_thread.unpark();
                break;
            }
        }

        if state.queue.len() <= state.unpark_threshold as usize {
            state.emulator_thread.unpark();
        }
    }
}

pub struct SdlAudioOutputHandle {
    audio_subsystem: AudioSubsystem,
    audio_stream: AudioStreamWithCallback<AudioQueueCallback>,
    callback_state: Arc<Mutex<AudioCallbackState>>,
    output_frequency: u64,
}

pub struct SdlAudioOutput {
    callback_state: Arc<Mutex<AudioCallbackState>>,
    muted: bool,
    audio_buffer: Vec<(f32, f32)>,
    audio_sync: bool,
    audio_sync_threshold: u32,
    dynamic_resampling_ratio_enabled: bool,
    dynamic_resampling_rate: DynamicResamplingRate,
    output_frequency: u64,
    audio_buffer_size: u32,
    audio_gain_multiplier: f64,
    sample_count: u64,
    speed_multiplier: u64,
}

impl SdlAudioOutputHandle {
    pub fn reload_config(&mut self, config: &CommonConfig) -> AudioResult<()> {
        let freq_changed = self.output_frequency != config.audio_output_frequency;

        self.output_frequency = config.audio_output_frequency;

        if freq_changed {
            // Recreate audio stream on sample rate changes

            log::info!("Recreating SDL audio queue with freq {}", config.audio_output_frequency);

            self.audio_stream.pause().map_err(AudioError::PauseStream)?;

            self.audio_stream =
                open_audio_stream(&self.audio_subsystem, config, Arc::clone(&self.callback_state))?;
        }

        {
            let mut state = self.callback_state.lock().unwrap();
            state.hardware_queue_size = config.audio_hardware_queue_size;
            state.unpark_threshold = audio_sync_threshold(config);

            // Truncate audio queue on config reloads if it is way oversized OR if sample rate changed
            if freq_changed || state.queue.len() >= (4 * config.audio_buffer_size) as usize {
                state.queue.clear();
            }
        }

        Ok(())
    }

    pub fn set_emulator_thread(&self, thread: Thread) {
        self.callback_state.lock().unwrap().emulator_thread = thread;
    }
}

impl SdlAudioOutput {
    pub fn create_and_init(
        audio_subsystem: AudioSubsystem,
        config: &CommonConfig,
    ) -> AudioResult<(Self, SdlAudioOutputHandle)> {
        let callback_state = Arc::new(Mutex::new(AudioCallbackState::new(config)));

        let audio_stream =
            open_audio_stream(&audio_subsystem, config, Arc::clone(&callback_state))?;

        let audio_output = Self {
            muted: config.mute_audio,
            callback_state: Arc::clone(&callback_state),
            audio_buffer: Vec::with_capacity(INTERNAL_AUDIO_BUFFER_LEN),
            audio_sync: config.audio_sync,
            audio_sync_threshold: audio_sync_threshold(config),
            dynamic_resampling_ratio_enabled: config.audio_dynamic_resampling_ratio,
            dynamic_resampling_rate: DynamicResamplingRate::new(
                config.audio_output_frequency as u32,
                config.audio_buffer_size,
            ),
            output_frequency: config.audio_output_frequency,
            audio_buffer_size: config.audio_buffer_size,
            audio_gain_multiplier: decibels_to_multiplier(config.audio_gain_db),
            sample_count: 0,
            speed_multiplier: 1,
        };

        let handle = SdlAudioOutputHandle {
            audio_subsystem,
            audio_stream,
            callback_state,
            output_frequency: config.audio_output_frequency,
        };

        Ok((audio_output, handle))
    }

    pub fn reload_config(&mut self, config: &CommonConfig) {
        let freq_changed = self.output_frequency != config.audio_output_frequency;
        let buffer_size_changed = self.audio_buffer_size != config.audio_buffer_size;

        self.muted = config.mute_audio;
        self.audio_sync = config.audio_sync;
        self.dynamic_resampling_ratio_enabled = config.audio_dynamic_resampling_ratio;
        self.output_frequency = config.audio_output_frequency;
        self.audio_buffer_size = config.audio_buffer_size;
        self.audio_gain_multiplier = decibels_to_multiplier(config.audio_gain_db);
        self.audio_sync_threshold = audio_sync_threshold(config);

        if freq_changed || buffer_size_changed {
            self.dynamic_resampling_rate
                .update_config(self.output_frequency as u32, self.audio_buffer_size);
        }
    }

    pub fn set_speed_multiplier(&mut self, speed_multiplier: u64) {
        self.speed_multiplier = speed_multiplier;
    }

    pub fn adjust_dynamic_resampling_ratio(&mut self) {
        if !self.dynamic_resampling_ratio_enabled {
            return;
        }

        let audio_queue_len = self.callback_state.lock().unwrap().queue.len();
        self.dynamic_resampling_rate.adjust(audio_queue_len as u32);
    }

    pub fn output_frequency(&self) -> u64 {
        if self.dynamic_resampling_ratio_enabled {
            self.dynamic_resampling_rate.current_output_frequency().into()
        } else {
            self.output_frequency
        }
    }
}

fn open_audio_stream(
    audio: &AudioSubsystem,
    config: &CommonConfig,
    callback_state: Arc<Mutex<AudioCallbackState>>,
) -> AudioResult<AudioStreamWithCallback<AudioQueueCallback>> {
    let audio_callback = AudioQueueCallback { state: callback_state };

    let stream = audio
        .open_playback_stream(
            &AudioSpec {
                freq: Some(config.audio_output_frequency as i32),
                channels: Some(CHANNELS),
                format: Some(AudioFormat::f32_sys()),
            },
            audio_callback,
        )
        .map_err(AudioError::OpenStream)?;
    stream.resume().map_err(AudioError::OpenStream)?;

    Ok(stream)
}

fn decibels_to_multiplier(decibels: f64) -> f64 {
    10.0_f64.powf(decibels / 20.0)
}

impl AudioOutput for SdlAudioOutput {
    type Err = AudioError;

    #[inline]
    fn push_sample(&mut self, mut sample_l: f64, mut sample_r: f64) -> Result<(), Self::Err> {
        self.sample_count += 1;
        if !self.sample_count.is_multiple_of(self.speed_multiplier) {
            return Ok(());
        }

        if self.muted {
            sample_l = 0.0;
            sample_r = 0.0;
        }

        sample_l *= self.audio_gain_multiplier;
        sample_r *= self.audio_gain_multiplier;

        self.audio_buffer.push((sample_l as f32, sample_r as f32));

        if self.audio_buffer.len() < INTERNAL_AUDIO_BUFFER_LEN {
            return Ok(());
        }

        let queue_threshold = self.audio_sync_threshold as usize;

        let mut state_lock = if self.audio_sync {
            perform_audio_sync(&self.callback_state, queue_threshold)?
        } else {
            let state_lock = self.callback_state.lock().unwrap();

            if state_lock.queue.len() > queue_threshold {
                // Audio queue is full; drop samples
                log::debug!("Dropping audio samples because buffer is full");
                self.audio_buffer.clear();
                return Ok(());
            }

            state_lock
        };

        state_lock.queue.extend(&self.audio_buffer);
        let callback_error = state_lock.error.take();

        drop(state_lock);

        self.audio_buffer.clear();

        match callback_error {
            None => Ok(()),
            Some(err) => Err(AudioError::QueueAudio(err)),
        }
    }
}

fn audio_sync_threshold(config: &CommonConfig) -> u32 {
    if config.audio_dynamic_resampling_ratio {
        // If dynamic resampling ratio is enabled, let the audio buffer grow to double size
        // before dropping samples because the audio buffer size is also the target length
        // for dynamic resampling
        2 * config.audio_buffer_size
    } else {
        config.audio_buffer_size
    }
}

fn perform_audio_sync(
    state: &Arc<Mutex<AudioCallbackState>>,
    queue_threshold: usize,
) -> AudioResult<MutexGuard<'_, AudioCallbackState>> {
    // Block until audio queue is not full
    loop {
        {
            let mut state = state.lock().unwrap();

            if let Some(err) = state.error.take() {
                return Err(AudioError::QueueAudio(err));
            }

            if state.queue.len() <= queue_threshold {
                return Ok(state);
            }
        }

        thread::park_timeout(Duration::from_secs(1));
    }
}
