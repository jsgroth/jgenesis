//! `Renderer` implementation that delegates to another thread to perform the actual rendering
//!
//! When `render_frame()` is called, blocks until the other thread acknowledges that it has
//! completed rendering

use jgenesis_common::frontend::{Color, FrameSize, RenderFrameOptions, Renderer};
use std::error::Error;
use std::mem;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, SyncSender};
use thiserror::Error;

pub struct FrameMessage {
    pub frame_buffer: Vec<Color>,
    pub frame_size: FrameSize,
    pub target_fps: f64,
    pub options: RenderFrameOptions,
}

pub struct DoneMessage {
    pub result: Result<Vec<Color>, (Vec<Color>, Box<dyn Error + Send + Sync + 'static>)>,
}

#[derive(Debug, Error)]
pub enum ThreadedRendererError {
    #[error("Invalid frame buffer length {len} for size {width}x{height}")]
    InvalidFrameBufferLen { len: usize, width: u32, height: u32 },
    #[error("Lost connection to main thread")]
    LostConnection,
    #[error("Error from underlying renderer: {0}")]
    Render(#[source] Box<dyn Error + Send + Sync + 'static>),
}

pub struct ThreadedRenderer {
    frame_buffer: Vec<Color>,
    frame_sender: SyncSender<FrameMessage>,
    done_receiver: Receiver<DoneMessage>,
}

impl ThreadedRenderer {
    pub fn new() -> (Self, Receiver<FrameMessage>, SyncSender<DoneMessage>) {
        let (frame_sender, frame_receiver) = mpsc::sync_channel(1);
        let (done_sender, done_receiver) = mpsc::sync_channel(1);

        let renderer =
            Self { frame_buffer: Vec::with_capacity(1300 * 240), frame_sender, done_receiver };

        (renderer, frame_receiver, done_sender)
    }
}

impl Renderer for ThreadedRenderer {
    type Err = ThreadedRendererError;

    fn render_frame(
        &mut self,
        frame_buffer: &[Color],
        frame_size: FrameSize,
        target_fps: f64,
        options: RenderFrameOptions,
    ) -> Result<(), Self::Err> {
        let frame_len = frame_size.len() as usize;
        if frame_len > frame_buffer.len() {
            return Err(ThreadedRendererError::InvalidFrameBufferLen {
                len: frame_len,
                width: frame_size.width,
                height: frame_size.height,
            });
        }

        self.frame_buffer.clear();
        self.frame_buffer.extend_from_slice(&frame_buffer[..frame_len]);

        let frame_message = FrameMessage {
            frame_buffer: mem::take(&mut self.frame_buffer),
            frame_size,
            target_fps,
            options,
        };
        if let Err(_) = self.frame_sender.send(frame_message) {
            return Err(ThreadedRendererError::LostConnection);
        }

        match self.done_receiver.recv() {
            Ok(message) => match message.result {
                Ok(frame_buffer) => {
                    self.frame_buffer = frame_buffer;
                }
                Err((frame_buffer, err)) => {
                    self.frame_buffer = frame_buffer;
                    return Err(ThreadedRendererError::Render(err));
                }
            },
            Err(_) => {
                return Err(ThreadedRendererError::LostConnection);
            }
        }

        Ok(())
    }
}
