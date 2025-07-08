use crate::config::CommonConfig;
use jgenesis_common::audio::DynamicResamplingRate;
use jgenesis_common::frontend::AudioOutput;
use sdl2::AudioSubsystem;
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use thiserror::Error;

// Always output in stereo
const CHANNELS: u8 = 2;

// Number of samples to buffer before locking and pushing to the audio queue
const INTERNAL_AUDIO_BUFFER_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Error opening SDL2 audio queue: {0}")]
    OpenQueue(String),
    #[error("Error pushing audio samples to SDL2 audio queue: {0}")]
    QueueAudio(String),
}

struct AudioQueueCallback {
    queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
}

impl AudioCallback for AudioQueueCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [Self::Channel]) {
        let mut queue = self.queue.lock().unwrap();

        let mut last = (0.0, 0.0);
        for chunk in out.chunks_exact_mut(2) {
            last = queue.pop_front().unwrap_or(last);
            (chunk[0], chunk[1]) = last;
        }
    }
}

pub struct SdlAudioOutput {
    muted: bool,
    audio_device: AudioDevice<AudioQueueCallback>,
    audio_queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
    audio_buffer: Vec<(f32, f32)>,
    audio_sync: bool,
    dynamic_resampling_ratio_enabled: bool,
    dynamic_resampling_rate: DynamicResamplingRate,
    audio_buffer_size: u32,
    audio_gain_multiplier: f64,
    sample_count: u64,
    speed_multiplier: u64,
}

impl SdlAudioOutput {
    pub fn create_and_init(
        audio: &AudioSubsystem,
        config: &CommonConfig,
    ) -> Result<Self, AudioError> {
        let OpenedAudioDevice { device, queue } = open_audio_device(audio, config)?;
        let output_frequency = device.spec().freq;

        Ok(Self {
            muted: config.mute_audio,
            audio_device: device,
            audio_queue: queue,
            audio_buffer: Vec::with_capacity(INTERNAL_AUDIO_BUFFER_LEN),
            audio_sync: config.audio_sync,
            dynamic_resampling_ratio_enabled: config.audio_dynamic_resampling_ratio,
            dynamic_resampling_rate: DynamicResamplingRate::new(
                output_frequency as u32,
                config.audio_buffer_size,
            ),
            audio_buffer_size: config.audio_buffer_size,
            audio_gain_multiplier: decibels_to_multiplier(config.audio_gain_db),
            sample_count: 0,
            speed_multiplier: 1,
        })
    }

    pub fn reload_config(&mut self, config: &CommonConfig) -> Result<(), AudioError> {
        self.muted = config.mute_audio;
        self.audio_sync = config.audio_sync;
        self.dynamic_resampling_ratio_enabled = config.audio_dynamic_resampling_ratio;
        self.audio_buffer_size = config.audio_buffer_size;
        self.audio_gain_multiplier = decibels_to_multiplier(config.audio_gain_db);

        let spec = self.audio_device.spec();
        if config.audio_output_frequency != spec.freq as u64
            || config.audio_hardware_queue_size != spec.samples
        {
            log::info!(
                "Recreating SDL audio queue with freq {} and size {}",
                config.audio_output_frequency,
                config.audio_hardware_queue_size
            );
            self.audio_device.pause();

            let OpenedAudioDevice { device, queue } =
                open_audio_device(self.audio_device.subsystem(), config)?;
            self.audio_device = device;
            self.audio_queue = queue;
        } else if self.audio_queue_len_samples() >= 4 * self.audio_buffer_size {
            // Truncate audio queue on config reloads if it is way oversized
            self.audio_queue.lock().unwrap().clear();
        }

        self.dynamic_resampling_rate
            .update_config(self.audio_device.spec().freq as u32, self.audio_buffer_size);

        Ok(())
    }

    pub fn set_speed_multiplier(&mut self, speed_multiplier: u64) {
        self.speed_multiplier = speed_multiplier;
    }

    pub fn adjust_dynamic_resampling_ratio(&mut self) {
        if !self.dynamic_resampling_ratio_enabled {
            return;
        }

        self.dynamic_resampling_rate.adjust(self.audio_queue_len_samples());
    }

    #[must_use]
    pub fn output_frequency(&self) -> u64 {
        if self.dynamic_resampling_ratio_enabled {
            self.dynamic_resampling_rate.current_output_frequency().into()
        } else {
            self.audio_device.spec().freq as u64
        }
    }

    fn audio_queue_len_samples(&self) -> u32 {
        self.audio_queue.lock().unwrap().len() as u32
    }
}

struct OpenedAudioDevice {
    device: AudioDevice<AudioQueueCallback>,
    queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
}

fn open_audio_device(
    audio: &AudioSubsystem,
    config: &CommonConfig,
) -> Result<OpenedAudioDevice, AudioError> {
    let queue =
        Arc::new(Mutex::new(VecDeque::with_capacity(2 * config.audio_buffer_size as usize)));
    let audio_callback = AudioQueueCallback { queue: Arc::clone(&queue) };

    let device = audio
        .open_playback(
            None,
            &AudioSpecDesired {
                freq: Some(config.audio_output_frequency as i32),
                channels: Some(CHANNELS),
                samples: Some(config.audio_hardware_queue_size),
            },
            move |_| audio_callback,
        )
        .map_err(AudioError::OpenQueue)?;
    device.resume();

    if config.audio_output_frequency as i32 != device.spec().freq {
        log::error!(
            "Audio device does not support requested frequency {}; set to {} instead",
            config.audio_output_frequency,
            device.spec().freq
        );
    }

    Ok(OpenedAudioDevice { device, queue })
}

fn decibels_to_multiplier(decibels: f64) -> f64 {
    10.0_f64.powf(decibels / 20.0)
}

impl AudioOutput for SdlAudioOutput {
    type Err = AudioError;

    #[inline]
    fn push_sample(&mut self, mut sample_l: f64, mut sample_r: f64) -> Result<(), Self::Err> {
        self.sample_count += 1;
        if self.sample_count % self.speed_multiplier != 0 {
            return Ok(());
        }

        if self.muted {
            sample_l = 0.0;
            sample_r = 0.0;
        }

        sample_l *= self.audio_gain_multiplier;
        sample_r *= self.audio_gain_multiplier;

        self.audio_buffer.push((sample_l as f32, sample_r as f32));

        if self.audio_buffer.len() >= INTERNAL_AUDIO_BUFFER_LEN {
            let audio_buffer_threshold = if self.dynamic_resampling_ratio_enabled {
                // If dynamic resampling ratio is enabled, let the audio buffer grow to double size
                // before dropping samples because the audio buffer size is also the target length
                // for dynamic resampling
                2 * self.audio_buffer_size
            } else {
                self.audio_buffer_size
            };

            if self.audio_sync {
                // Block until audio queue is not full
                while self.audio_queue_len_samples() > audio_buffer_threshold {
                    thread::sleep(Duration::from_micros(250));
                }
            } else if self.audio_queue_len_samples() > audio_buffer_threshold {
                // Audio queue is full; drop samples
                log::debug!("Dropping audio samples because buffer is full");
                self.audio_buffer.clear();
                return Ok(());
            }

            {
                let mut audio_queue = self.audio_queue.lock().unwrap();

                if audio_queue.is_empty() {
                    log::debug!("Potential audio buffer underflow");
                }

                audio_queue.extend(&self.audio_buffer);
            }

            self.audio_buffer.clear();
        }

        Ok(())
    }
}
