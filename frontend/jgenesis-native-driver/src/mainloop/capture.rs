mod wav;

use crate::mainloop::capture::wav::WaveWriter;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use std::{io, thread};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioCaptureFormat {
    #[default]
    I16 = 1, // WAVE_FORMAT_PCM
    F32 = 3, // WAVE_FORMAT_IEEE_FLOAT
}

impl AudioCaptureFormat {
    fn bytes_per_sample(self) -> u32 {
        match self {
            Self::I16 => 2,
            Self::F32 => 4,
        }
    }
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("I/O error in capture thread: {0}")]
    Io(#[from] io::Error),
    #[error("audio sample rate changed from {from} Hz to {to} Hz")]
    AudioSampleRateChanged { from: u32, to: u32 },
    #[error("capture thread unexpectedly disconnected")]
    ThreadDisconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureThreadCommand {
    EndRecording,
}

#[derive(Debug)]
enum CaptureThreadResponse {
    IoError(io::Error),
}

struct CaptureThread {
    output_path: PathBuf,
    audio_samples: Arc<Mutex<Vec<(f32, f32)>>>,
    audio_sample_rate: u32,
    audio_format: AudioCaptureFormat,
    audio_buffer: Vec<(f32, f32)>,
    command_receiver: Receiver<CaptureThreadCommand>,
}

pub struct CaptureThreadHandle {
    audio_sample_rate: u32,
    command_sender: Sender<CaptureThreadCommand>,
    response_receiver: Receiver<CaptureThreadResponse>,
}

impl CaptureThread {
    fn run(self, response_sender: Sender<CaptureThreadResponse>) {
        log::info!("Beginning audio capture to file '{}'", self.output_path.display());

        match self.run_inner() {
            Ok(()) => {
                log::info!("Audio capture finished");
            }
            Err(err) => {
                log::error!("Error in capture thread, ending capture: {err}");
                let _ = response_sender.send(CaptureThreadResponse::IoError(err));
            }
        }
    }

    fn run_inner(mut self) -> io::Result<()> {
        let mut writer =
            WaveWriter::new(&self.output_path, self.audio_sample_rate, self.audio_format)?;

        loop {
            match self.command_receiver.recv_timeout(Duration::from_millis(5)) {
                Ok(command) => {
                    return match command {
                        CaptureThreadCommand::EndRecording => self.finish(writer),
                    };
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    log::error!("Runner thread unexpectedly disconnected; ending capture");
                    return self.finish(writer);
                }
            }

            self.drain_audio_samples(&mut writer)?;
        }
    }

    fn drain_audio_samples(&mut self, writer: &mut WaveWriter) -> io::Result<()> {
        {
            let mut audio_samples = self.audio_samples.lock().unwrap();
            self.audio_buffer.extend_from_slice(&audio_samples);
            audio_samples.clear();
        }

        if self.audio_buffer.is_empty() {
            return Ok(());
        }

        writer.write_audio_chunk(&self.audio_buffer)?;
        self.audio_buffer.clear();

        Ok(())
    }

    fn finish(mut self, mut writer: WaveWriter) -> io::Result<()> {
        self.drain_audio_samples(&mut writer)?;
        writer.finish()?;

        Ok(())
    }
}

impl CaptureThreadHandle {
    pub fn check_audio_sample_rate(&self, sample_rate: u32) -> Result<(), CaptureError> {
        if sample_rate == self.audio_sample_rate {
            Ok(())
        } else {
            Err(CaptureError::AudioSampleRateChanged {
                from: self.audio_sample_rate,
                to: sample_rate,
            })
        }
    }

    pub fn check_status(&self) -> Result<(), CaptureError> {
        match self.response_receiver.try_recv() {
            Ok(CaptureThreadResponse::IoError(err)) => Err(CaptureError::Io(err)),
            Err(TryRecvError::Empty) => Ok(()),
            Err(TryRecvError::Disconnected) => Err(CaptureError::ThreadDisconnected),
        }
    }

    pub fn end_capture(self) {
        if self.command_sender.send(CaptureThreadCommand::EndRecording).is_err() {
            // Capture thread is already gone
            return;
        }

        // Wait for capture thread to drop its response sender (which makes recv() return Err)
        while let Ok(CaptureThreadResponse::IoError(err)) = self.response_receiver.recv() {
            log::error!("I/O error in capture thread: {err}");
        }
    }
}

pub fn spawn_thread(
    output_path: PathBuf,
    audio_samples: Arc<Mutex<Vec<(f32, f32)>>>,
    audio_sample_rate: u32,
    audio_format: AudioCaptureFormat,
) -> CaptureThreadHandle {
    let (command_sender, command_receiver) = mpsc::channel();
    let (response_sender, response_receiver) = mpsc::channel();

    let thread = CaptureThread {
        output_path,
        audio_samples,
        audio_sample_rate,
        audio_format,
        audio_buffer: Vec::with_capacity(audio_sample_rate as usize),
        command_receiver,
    };

    let handle = CaptureThreadHandle { audio_sample_rate, command_sender, response_receiver };

    thread::spawn(move || thread.run(response_sender));

    handle
}

trait WriteExt {
    fn write_u16(&mut self, value: u16) -> io::Result<()>;

    fn write_u32(&mut self, value: u32) -> io::Result<()>;
}

impl<W: Write> WriteExt for W {
    fn write_u16(&mut self, value: u16) -> io::Result<()> {
        self.write_all(&value.to_le_bytes())
    }

    fn write_u32(&mut self, value: u32) -> io::Result<()> {
        self.write_all(&value.to_le_bytes())
    }
}
