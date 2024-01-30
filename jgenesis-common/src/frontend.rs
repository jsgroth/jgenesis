use bincode::{Decode, Encode};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use std::error::Error;
use std::fmt::{Debug, Display};
use std::num::NonZeroU32;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable, Encode, Decode)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    #[must_use]
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

impl Default for Color {
    #[inline]
    fn default() -> Self {
        Self::BLACK
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
pub struct PixelAspectRatio(f64);

impl PixelAspectRatio {
    pub const SQUARE: Self = Self(1.0);

    #[must_use]
    #[inline]
    pub fn from_width_and_height(width: NonZeroU32, height: NonZeroU32) -> Self {
        Self(f64::from(width.get()) / f64::from(height.get()))
    }
}

impl From<PixelAspectRatio> for f64 {
    #[inline]
    fn from(value: PixelAspectRatio) -> Self {
        value.0
    }
}

impl TryFrom<f64> for PixelAspectRatio {
    type Error = String;

    #[inline]
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(format!("invalid pixel aspect ratio: {value}"))
        }
    }
}

pub trait Renderer {
    type Err;

    /// Render a frame.
    ///
    /// The frame buffer may be larger than the specified frame size, but the len must be at least
    /// (`frame_width` * `frame_height`). Colors past the first (`frame_width` * `frame_height`)
    /// will be ignored.
    ///
    /// If pixel aspect ratio is None, the frame will be stretched to fill the window. If it is
    /// Some, the frame will be rendered in the largest possible area that maintains the specified
    /// pixel aspect ratio.
    ///
    /// # Errors
    ///
    /// This method will return an error if it is unable to render the frame.
    fn render_frame(
        &mut self,
        frame_buffer: &[Color],
        frame_size: FrameSize,
        pixel_aspect_ratio: Option<PixelAspectRatio>,
    ) -> Result<(), Self::Err>;
}

pub trait AudioOutput {
    type Err;

    /// Push a stereo audio sample.
    ///
    /// # Errors
    ///
    /// This method will return an error if it is unable to push the sample to the audio device.
    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err>;
}

pub trait SaveWriter {
    type Err;

    /// Read an array of bytes using the given extension.
    ///
    /// # Errors
    ///
    /// Will propagate any errors encountered while reading the file.
    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err>;

    /// Write a slice of bytes using the given extension.
    ///
    /// # Errors
    ///
    /// Will propagate any errors encountered while writing the file.
    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err>;

    /// Load a serialized value using the given extension.
    ///
    /// For loading raw bytes, use `load_bytes` instead which does not assume that the length is serialized.
    ///
    /// # Errors
    ///
    /// Will propagate any errors encountered while reading the file or deserializing the data.
    fn load_serialized<D: Decode>(&mut self, extension: &str) -> Result<D, Self::Err>;

    /// Write a serialized value using the given extension.
    ///
    /// For writing raw bytes, use `persist_bytes` instead which does not serialize the slice length.
    ///
    /// # Errors
    ///
    /// Will propagate any errors encountered while writing the file or serializing the data.
    fn persist_serialized<E: Encode>(&mut self, extension: &str, data: E) -> Result<(), Self::Err>;
}

pub trait PartialClone {
    /// Create a partial clone of `self`, which clones all emulation state but may not clone
    /// read-only fields such as ROMs and frame buffers.
    #[must_use]
    fn partial_clone(&self) -> Self;
}

pub use jgenesis_proc_macros::PartialClone;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumFromStr, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TimingMode {
    #[default]
    Ntsc,
    Pal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickEffect {
    None,
    FrameRendered,
}

pub type TickResult<Err> = Result<TickEffect, Err>;

pub trait EmulatorTrait: Encode + Decode + PartialClone {
    type Inputs;
    type Config;

    type Err<RErr: Debug + Display + Send + Sync + 'static, AErr: Debug + Display + Send + Sync + 'static, SErr: Debug + Display + Send + Sync + 'static>: Error + Send + Sync + 'static;

    /// Tick the emulator for a small amount of time, e.g. a single CPU instruction.
    ///
    /// # Errors
    ///
    /// This method should propagate any errors encountered while rendering frames, pushing audio
    /// samples, or persisting save files.
    #[allow(clippy::type_complexity)]
    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> TickResult<Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        R::Err: Debug + Display + Send + Sync + 'static,
        A: AudioOutput,
        A::Err: Debug + Display + Send + Sync + 'static,
        S: SaveWriter,
        S::Err: Debug + Display + Send + Sync + 'static;

    /// Forcibly render the current frame buffer.
    ///
    /// # Errors
    ///
    /// This method can propagate any error returned by the renderer.
    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer;

    fn reload_config(&mut self, config: &Self::Config);

    fn take_rom_from(&mut self, other: &mut Self);

    fn soft_reset(&mut self);

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S);

    fn timing_mode(&self) -> TimingMode;
}
