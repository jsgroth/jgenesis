use crate::config::CommonConfig;
use crate::mainloop;
use jgenesis_common::frontend::AudioOutput;
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::AudioSubsystem;
use std::time::Duration;
use thiserror::Error;

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
    internal_audio_buffer_len: u32,
    audio_sync_threshold: u32,
    audio_gain_multiplier: f64,
    sample_count: u64,
    speed_multiplier: u64,
}

impl SdlAudioOutput {
    pub fn create_and_init<KC, JC>(
        audio: &AudioSubsystem,
        config: &CommonConfig<KC, JC>,
    ) -> Result<Self, AudioError> {
        let audio_queue = audio
            .open_queue(
                None,
                &AudioSpecDesired {
                    freq: Some(48000),
                    channels: Some(2),
                    samples: Some(config.audio_device_queue_size),
                },
            )
            .map_err(AudioError::OpenQueue)?;
        audio_queue.resume();

        Ok(Self {
            audio_queue,
            audio_buffer: Vec::with_capacity(config.internal_audio_buffer_size as usize),
            audio_sync: config.audio_sync,
            internal_audio_buffer_len: config.internal_audio_buffer_size,
            audio_sync_threshold: config.audio_sync_threshold,
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
        self.internal_audio_buffer_len = config.internal_audio_buffer_size;
        self.audio_sync_threshold = config.audio_sync_threshold;
        self.audio_gain_multiplier = decibels_to_multiplier(config.audio_gain_db);

        if config.audio_device_queue_size != self.audio_queue.spec().samples {
            log::info!("Recreating SDL audio queue with size {}", config.audio_device_queue_size);
            self.audio_queue.pause();

            let new_audio_queue = self
                .audio_queue
                .subsystem()
                .open_queue(
                    None,
                    &AudioSpecDesired {
                        freq: Some(48000),
                        channels: Some(2),
                        samples: Some(config.audio_device_queue_size),
                    },
                )
                .map_err(AudioError::OpenQueue)?;
            self.audio_queue = new_audio_queue;
            self.audio_queue.resume();
        }

        Ok(())
    }

    pub fn set_speed_multiplier(&mut self, speed_multiplier: u64) {
        self.speed_multiplier = speed_multiplier;
    }
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

        if self.audio_buffer.len() >= self.internal_audio_buffer_len as usize {
            if self.audio_sync {
                // Wait until audio queue is not full
                while self.audio_queue.size() >= self.audio_sync_threshold {
                    mainloop::sleep(Duration::from_micros(250));
                }
            } else if self.audio_queue.size() >= self.audio_sync_threshold {
                // Audio queue is full; drop samples
                self.audio_buffer.clear();
                return Ok(());
            }

            self.audio_queue.queue_audio(&self.audio_buffer).map_err(AudioError::QueueAudio)?;
            self.audio_buffer.clear();
        }

        Ok(())
    }
}
