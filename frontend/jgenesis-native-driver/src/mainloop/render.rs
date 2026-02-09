//! `Renderer` implementation that delegates to another thread to perform the actual rendering
//!
//! When `render_frame()` is called, blocks until the other thread acknowledges that it has
//! completed rendering

use jgenesis_common::frontend::{Color, FrameSize, RenderFrameOptions, Renderer};
use std::slice;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::Duration;
use thiserror::Error;

struct FrameMessage {
    frame_buffer: *const Color,
    frame_buffer_len: usize,
    frame_size: FrameSize,
    target_fps: f64,
    options: RenderFrameOptions,
}

// SAFETY: This pointer-containing struct is only sent across threads in one place in this module,
// and the struct is private so it cannot be used outside of this module
unsafe impl Send for FrameMessage {}

pub type DoneMessage = Result<(), ()>;

#[derive(Debug, Error)]
pub enum ThreadedRendererError {
    #[error("Invalid frame buffer length {len} for size {width}x{height}")]
    InvalidFrameBufferLen { len: usize, width: u32, height: u32 },
    #[error("Lost connection to main thread")]
    LostConnection,
    #[error("Error from underlying renderer")]
    Render,
}

pub struct ThreadedRenderer {
    frame_sender: SyncSender<FrameMessage>,
    done_receiver: Receiver<DoneMessage>,
}

pub struct ThreadedRendererHandle {
    frame_receiver: Receiver<FrameMessage>,
    done_sender: SyncSender<DoneMessage>,
}

impl ThreadedRenderer {
    pub fn new() -> (Self, ThreadedRendererHandle) {
        let (frame_sender, frame_receiver) = mpsc::sync_channel(1);
        let (done_sender, done_receiver) = mpsc::sync_channel(1);

        let renderer = Self { frame_sender, done_receiver };

        let handle = ThreadedRendererHandle { frame_receiver, done_sender };

        (renderer, handle)
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

        // SAFETY: This sends a frame buffer raw pointer to the main thread. This function must not
        // return before the main thread has signaled that it is no longer using the frame buffer
        let frame_message = FrameMessage {
            frame_buffer: frame_buffer.as_ptr(),
            frame_buffer_len: frame_buffer.len(),
            frame_size,
            target_fps,
            options,
        };
        if self.frame_sender.send(frame_message).is_err() {
            return Err(ThreadedRendererError::LostConnection);
        }

        match self.done_receiver.recv() {
            Ok(Ok(())) => {}
            Ok(Err(())) => return Err(ThreadedRendererError::Render),
            Err(_) => return Err(ThreadedRendererError::LostConnection),
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RecvFrameError<RErr> {
    #[error("recv error: {0}")]
    Recv(#[from] RecvTimeoutError),
    #[error("lost connection to other thread")]
    LostConnection,
    #[error("renderer error: {0}")]
    Render(RErr),
}

impl ThreadedRendererHandle {
    pub fn try_recv_frame<R: Renderer>(
        &self,
        renderer: &mut R,
        timeout: Duration,
    ) -> Result<(), RecvFrameError<R::Err>> {
        let frame_message = self.frame_receiver.recv_timeout(timeout)?;

        // SAFETY: The slice is reconstructed from raw parts sent by the runner thread. The main
        // thread must not use the slice after it sends the done signal to the runner thread
        unsafe {
            let frame_buffer =
                slice::from_raw_parts(frame_message.frame_buffer, frame_message.frame_buffer_len);

            match renderer.render_frame(
                frame_buffer,
                frame_message.frame_size,
                frame_message.target_fps,
                frame_message.options,
            ) {
                Ok(()) => {
                    self.done_sender.send(Ok(())).map_err(|_| RecvFrameError::LostConnection)?;
                }
                Err(err) => {
                    let _ = self.done_sender.send(Err(()));
                    return Err(RecvFrameError::Render(err));
                }
            }
        }

        Ok(())
    }
}
