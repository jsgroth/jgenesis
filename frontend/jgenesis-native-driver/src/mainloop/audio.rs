use crate::config::CommonConfig;
use jgenesis_common::frontend::AudioOutput;
use sdl2::AudioSubsystem;
use sdl2::audio::{AudioQueue, AudioSpecDesired};
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

pub struct SdlAudioOutput {
    audio_queue: AudioQueue<f32>,
    audio_buffer: Vec<f32>,
    audio_sync: bool,
    audio_buffer_size: u32,
    audio_gain_multiplier: f64,
    sample_count: u64,
    speed_multiplier: u64,
}

impl SdlAudioOutput {
    pub fn create_and_init<KC, JC>(
        audio: &AudioSubsystem,
        config: &CommonConfig<KC, JC>,
    ) -> Result<Self, AudioError> {
        let audio_queue = open_audio_queue(audio, config)?;

        Ok(Self {
            audio_queue,
            audio_buffer: Vec::with_capacity(INTERNAL_AUDIO_BUFFER_LEN),
            audio_sync: config.audio_sync,
            audio_buffer_size: config.audio_buffer_size,
            audio_gain_multiplier: decibels_to_multiplier(config.audio_gain_db),
            sample_count: 0,
            speed_multiplier: 1,
        })
    }

    pub fn reload_config<KC, JC>(
        &mut self,
        config: &CommonConfig<KC, JC>,
    ) -> Result<(), AudioError> {
        self.audio_sync = config.audio_sync;
        self.audio_buffer_size = config.audio_buffer_size;
        self.audio_gain_multiplier = decibels_to_multiplier(config.audio_gain_db);

        let spec = self.audio_queue.spec();
        if config.audio_output_frequency != spec.freq as u64
            || config.audio_hardware_queue_size != spec.samples
        {
            log::info!(
                "Recreating SDL audio queue with freq {} and size {}",
                config.audio_output_frequency,
                config.audio_hardware_queue_size
            );
            self.audio_queue.pause();

            let new_audio_queue = open_audio_queue(self.audio_queue.subsystem(), config)?;
            self.audio_queue = new_audio_queue;
        }

        Ok(())
    }

    pub fn set_speed_multiplier(&mut self, speed_multiplier: u64) {
        self.speed_multiplier = speed_multiplier;
    }

    #[must_use]
    pub fn output_frequency(&self) -> i32 {
        self.audio_queue.spec().freq
    }

    fn audio_queue_len_samples(&self) -> u32 {
        // 2 channels, 4 bytes per sample
        self.audio_queue.size() / 2 / 4
    }
}

fn open_audio_queue<KC, JC>(
    audio: &AudioSubsystem,
    config: &CommonConfig<KC, JC>,
) -> Result<AudioQueue<f32>, AudioError> {
    let audio_queue = audio
        .open_queue(None, &AudioSpecDesired {
            freq: Some(config.audio_output_frequency as i32),
            channels: Some(CHANNELS),
            samples: Some(config.audio_hardware_queue_size),
        })
        .map_err(AudioError::OpenQueue)?;
    audio_queue.resume();

    if config.audio_output_frequency as i32 != audio_queue.spec().freq {
        log::error!(
            "Audio device does not support requested frequency {}; set to {} instead",
            config.audio_output_frequency,
            audio_queue.spec().freq
        );
    }

    Ok(audio_queue)
}

fn decibels_to_multiplier(decibels: f64) -> f64 {
    10.0_f64.powf(decibels / 20.0)
}

impl AudioOutput for SdlAudioOutput {
    type Err = AudioError;

    #[inline]
    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err> {
        self.sample_count += 1;
        if self.sample_count % self.speed_multiplier != 0 {
            return Ok(());
        }

        self.audio_buffer.push((sample_l * self.audio_gain_multiplier) as f32);
        self.audio_buffer.push((sample_r * self.audio_gain_multiplier) as f32);

        if self.audio_buffer.len() >= INTERNAL_AUDIO_BUFFER_LEN {
            if self.audio_queue_len_samples() > self.audio_buffer_size {
                if !self.audio_sync {
                    // Audio queue is full; drop samples
                    log::debug!("Dropping audio samples because buffer is full");
                    self.audio_buffer.clear();
                    return Ok(());
                }

                // Block until audio queue is not full
                loop {
                    jgenesis_common::sleep(Duration::from_micros(250));
                    if self.audio_queue_len_samples() <= self.audio_buffer_size {
                        break;
                    }
                }
            }

            if log::log_enabled!(log::Level::Debug) {
                if self.audio_queue.size() == 0 {
                    log::debug!("Potential audio buffer underflow");
                }
            }

            self.audio_queue.queue_audio(&self.audio_buffer).map_err(AudioError::QueueAudio)?;
            self.audio_buffer.clear();
        }

        Ok(())
    }
}
