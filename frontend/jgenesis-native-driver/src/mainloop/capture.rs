mod avi;
mod wav;

use crate::mainloop::capture::avi::AviWriter;
use crate::mainloop::capture::wav::WaveWriter;
use jgenesis_common::frontend::{Color, FiniteF64, FrameSize};
use jgenesis_renderer::renderer::{ReadCapturedFrameCallback, RendererCaptureOutput};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use std::{cmp, io, iter, mem, thread};
use thiserror::Error;

// Roughly 3.8GB
// AVI and WAV files can't be larger than 4GB, so end recording a little before hitting that
const FILE_SIZE_LIMIT: u32 = 4_080_218_931;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCaptureCodec {
    None,
    CamStudio { level: u32 },
}

impl Default for VideoCaptureCodec {
    fn default() -> Self {
        Self::CamStudio { level: 3 }
    }
}

impl VideoCaptureCodec {
    fn fourcc(self) -> [u8; 4] {
        match self {
            Self::None => [0; 4],
            Self::CamStudio { .. } => *b"CSCD",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoCaptureCodecType {
    None,
    #[default]
    CamStudio,
}

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

    fn encode_samples(self, samples: &[(f32, f32)], writer: &mut impl Write) -> io::Result<()> {
        match self {
            Self::I16 => {
                for &(sample_l, sample_r) in samples {
                    for sample in [sample_l, sample_r] {
                        let sample =
                            (sample * 32768.0).round().clamp(i16::MIN.into(), i16::MAX.into())
                                as i16;
                        writer.write_all(&sample.to_le_bytes())?;
                    }
                }
            }
            Self::F32 => {
                for &(sample_l, sample_r) in samples {
                    writer.write_all(&sample_l.to_le_bytes())?;
                    writer.write_all(&sample_r.to_le_bytes())?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("I/O error in capture thread: {0}")]
    Io(#[from] io::Error),
    #[error("audio sample rate changed from {from} Hz to {to} Hz")]
    AudioSampleRateChanged { from: u32, to: u32 },
    #[error("AVI file is too large")]
    AviFileTooLarge,
    #[error("WAV file is too large")]
    WavFileTooLarge,
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
    AviFileTooLarge,
    WavFileTooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureType {
    Video(RendererCaptureOutput, VideoCaptureCodec),
    Audio,
}

impl CaptureType {
    #[must_use]
    pub fn is_video(self) -> bool {
        matches!(self, Self::Video(..))
    }

    pub(crate) fn type_str(self) -> &'static str {
        match self {
            Self::Video(..) => "video",
            Self::Audio => "audio",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StartCapture {
    Video {
        codec: VideoCaptureCodec,
        frame_size: FrameSize,
        pixel_aspect_ratio: Option<FiniteF64>,
        fps: f64,
    },
    Audio,
}

enum CaptureWriter {
    Video(AviWriter),
    Audio(WaveWriter),
}

struct CaptureThread {
    output_path: PathBuf,
    video_buffer_sender: SyncSender<Vec<Color>>,
    video_buffer_receiver: Receiver<Vec<Color>>,
    fps: Option<f64>,
    audio_samples: Arc<Mutex<Vec<(f32, f32)>>>,
    last_audio_sample: (f32, f32),
    audio_sample_rate: u32,
    audio_format: AudioCaptureFormat,
    audio_buffer: Vec<(f32, f32)>,
    command_receiver: Receiver<CaptureThreadCommand>,
    total_video_time: f64,
    total_audio_time: f64,
    skip_audio_samples: usize,
}

pub struct CaptureThreadHandle {
    capture_type: CaptureType,
    video_buffer_sender: SyncSender<Vec<Color>>,
    video_buffer_receiver: Receiver<Vec<Color>>,
    frame_size: Option<FrameSize>,
    audio_sample_rate: u32,
    command_sender: Sender<CaptureThreadCommand>,
    response_receiver: Receiver<CaptureThreadResponse>,
}

impl CaptureThread {
    fn run(
        self,
        start_capture: StartCapture,
        capture_type: CaptureType,
        response_sender: Sender<CaptureThreadResponse>,
    ) {
        let capture_type_str = capture_type.type_str();
        log::info!("Beginning {capture_type_str} capture to file '{}'", self.output_path.display());

        let writer = match self.create_writer(start_capture) {
            Ok(writer) => writer,
            Err(err) => {
                log::error!("Error initializing capture file writer, aborting capture: {err}");
                let _ = response_sender.send(CaptureThreadResponse::IoError(err));
                return;
            }
        };

        match self.run_inner(writer, &response_sender) {
            Ok(()) => {
                log::info!("Ended {capture_type_str} capture");
            }
            Err(err) => {
                log::error!("Error in capture thread, ending capture: {err}");
                let _ = response_sender.send(CaptureThreadResponse::IoError(err));
            }
        }
    }

    fn create_writer(&self, start_capture: StartCapture) -> io::Result<CaptureWriter> {
        let writer = match start_capture {
            StartCapture::Video { codec, frame_size, pixel_aspect_ratio, fps } => {
                CaptureWriter::Video(AviWriter::new(
                    &self.output_path,
                    codec,
                    frame_size,
                    pixel_aspect_ratio,
                    fps,
                    self.audio_sample_rate,
                    self.audio_format,
                )?)
            }
            StartCapture::Audio => CaptureWriter::Audio(WaveWriter::new(
                &self.output_path,
                self.audio_sample_rate,
                self.audio_format,
            )?),
        };

        Ok(writer)
    }

    fn run_inner(
        mut self,
        mut writer: CaptureWriter,
        response_sender: &Sender<CaptureThreadResponse>,
    ) -> io::Result<()> {
        loop {
            match self.command_receiver.recv_timeout(Duration::from_millis(1)) {
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

            match self.video_buffer_receiver.recv_timeout(Duration::from_millis(5)) {
                Ok(video_buffer) => {
                    if let CaptureWriter::Video(avi_writer) = &mut writer {
                        self.write_video_audio_chunks_avi(avi_writer, &video_buffer)?;
                    }

                    if self.video_buffer_sender.send(video_buffer).is_err() {
                        log::error!("Runner thread unexpectedly disconnected; ending capture");
                        return self.finish(writer);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    log::error!("Runner thread unexpectedly disconnected; ending capture");
                    return self.finish(writer);
                }
            }

            if let CaptureWriter::Audio(wave_writer) = &mut writer {
                self.drain_captured_audio(|samples| wave_writer.write_audio_chunk(samples))?;
            }

            match &writer {
                CaptureWriter::Video(avi_writer) => {
                    if avi_writer.current_file_len() > FILE_SIZE_LIMIT {
                        let _ = response_sender.send(CaptureThreadResponse::AviFileTooLarge);
                        return self.finish(writer);
                    }
                }
                CaptureWriter::Audio(wave_writer) => {
                    if wave_writer.current_file_len() > FILE_SIZE_LIMIT {
                        let _ = response_sender.send(CaptureThreadResponse::WavFileTooLarge);
                        return self.finish(writer);
                    }
                }
            }
        }
    }

    fn write_video_audio_chunks_avi(
        &mut self,
        avi_writer: &mut AviWriter,
        video_buffer: &[Color],
    ) -> io::Result<()> {
        avi_writer.write_video_chunk(video_buffer)?;
        self.drain_captured_audio(|samples| avi_writer.write_audio_chunk(samples))?;

        if let Some(fps) = self.fps {
            let secs_per_frame = 1.0 / fps;
            self.total_video_time += secs_per_frame;

            let catch_up_threshold = 3.0 * secs_per_frame;

            if self.total_video_time - self.total_audio_time >= catch_up_threshold {
                // Audio is more than 2 frames behind video; can happen when e.g. rewinding, which
                // doesn't produce any audio.
                // "Catch up" by padding with the last recorded sample
                log::warn!(
                    "Audio stream is too far behind video; padding audio stream with silence"
                );

                let samples = ((self.total_video_time - self.total_audio_time)
                    * f64::from(self.audio_sample_rate))
                .round();
                if samples > 0.0 {
                    self.audio_buffer
                        .extend(iter::repeat_n(self.last_audio_sample, samples as usize));
                    self.drain_captured_audio(|samples| avi_writer.write_audio_chunk(samples))?;
                }
            } else if self.total_audio_time - self.total_video_time >= catch_up_threshold {
                // Video is more than 2 frames behind audio; can happen with GB/GBC when games turn
                // the PPU on and off.
                // Just drop a bunch of audio samples to catch up
                log::warn!("Video stream is too far behind audio; skipping some audio samples");

                let samples = ((self.total_audio_time - self.total_video_time)
                    * f64::from(self.audio_sample_rate))
                .round();
                self.skip_audio_samples = samples as usize;
            }
        }

        Ok(())
    }

    fn drain_captured_audio(
        &mut self,
        write_fn: impl FnOnce(&[(f32, f32)]) -> io::Result<()>,
    ) -> io::Result<()> {
        {
            let mut audio_samples = self.audio_samples.lock().unwrap();
            self.audio_buffer.extend_from_slice(&audio_samples);
            audio_samples.clear();
        }

        if self.skip_audio_samples != 0 {
            let skip_samples = cmp::min(self.skip_audio_samples, self.audio_buffer.len());
            self.skip_audio_samples -= skip_samples;
            self.audio_buffer =
                mem::take(&mut self.audio_buffer).into_iter().skip(skip_samples).collect();
        }

        if self.audio_buffer.is_empty() {
            return Ok(());
        }

        self.total_audio_time +=
            (self.audio_buffer.len() as f64) / f64::from(self.audio_sample_rate);

        write_fn(&self.audio_buffer)?;
        self.audio_buffer.clear();

        Ok(())
    }

    fn finish(self, writer: CaptureWriter) -> io::Result<()> {
        match writer {
            CaptureWriter::Video(avi_writer) => avi_writer.finish(),
            CaptureWriter::Audio(wave_writer) => self.finish_wav(wave_writer),
        }
    }

    fn finish_wav(mut self, mut wave_writer: WaveWriter) -> io::Result<()> {
        self.drain_captured_audio(|samples| wave_writer.write_audio_chunk(samples))?;
        wave_writer.finish()?;

        Ok(())
    }
}

impl CaptureThreadHandle {
    pub fn video_capture_callback(&mut self) -> Option<impl ReadCapturedFrameCallback> {
        let frame_size = self.frame_size?;

        Some(move |buffer: &[Color], row_stride: usize| {
            let Ok(mut video_buffer) = self.video_buffer_receiver.recv() else { return };
            video_buffer.clear();

            let frame_width = frame_size.width as usize;
            for row in 0..frame_size.height as usize {
                let buffer_idx = row * row_stride;
                video_buffer.extend_from_slice(&buffer[buffer_idx..buffer_idx + frame_width]);
            }

            let _ = self.video_buffer_sender.send(video_buffer);
        })
    }

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
            Ok(CaptureThreadResponse::WavFileTooLarge) => Err(CaptureError::WavFileTooLarge),
            Ok(CaptureThreadResponse::AviFileTooLarge) => Err(CaptureError::AviFileTooLarge),
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

    pub fn capture_type(&self) -> CaptureType {
        self.capture_type
    }
}

pub fn spawn_thread(
    output_path: PathBuf,
    audio_samples: Arc<Mutex<Vec<(f32, f32)>>>,
    audio_sample_rate: u32,
    audio_format: AudioCaptureFormat,
    start_capture: StartCapture,
    capture_type: CaptureType,
) -> CaptureThreadHandle {
    let (command_sender, command_receiver) = mpsc::channel();
    let (response_sender, response_receiver) = mpsc::channel();

    let (thread_video_sender, handle_video_receiver) = mpsc::sync_channel(1);
    let (handle_video_sender, thread_video_receiver) = mpsc::sync_channel(1);

    let (frame_size, fps) = match start_capture {
        StartCapture::Video { frame_size, fps, .. } => (Some(frame_size), Some(fps)),
        StartCapture::Audio => (None, None),
    };

    // Prepopulate so handle will have the buffer ready when the first frame comes in
    let _ = thread_video_sender
        .send(Vec::with_capacity(frame_size.map(|size| size.len() as usize).unwrap_or(0)));

    let thread = CaptureThread {
        output_path,
        video_buffer_sender: thread_video_sender,
        video_buffer_receiver: thread_video_receiver,
        fps,
        audio_samples,
        last_audio_sample: (0.0, 0.0),
        audio_sample_rate,
        audio_format,
        audio_buffer: Vec::with_capacity(audio_sample_rate as usize),
        command_receiver,
        total_video_time: 0.0,
        total_audio_time: 0.0,
        skip_audio_samples: 0,
    };

    let handle = CaptureThreadHandle {
        capture_type,
        video_buffer_sender: handle_video_sender,
        video_buffer_receiver: handle_video_receiver,
        frame_size,
        audio_sample_rate,
        command_sender,
        response_receiver,
    };

    thread::spawn(move || thread.run(start_capture, capture_type, response_sender));

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
