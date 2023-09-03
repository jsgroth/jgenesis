use bincode::{Decode, Encode};
use std::num::NonZeroU32;

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    bincode::Encode,
    bincode::Decode,
)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    #[must_use]
    #[inline]
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

impl Default for Color {
    #[inline]
    fn default() -> Self {
        Self::rgb(0, 0, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, bincode::Encode, bincode::Decode)]
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

    /// Persist cartridge SRAM.
    ///
    /// # Errors
    ///
    /// This method will return an error if it is unable to persist the given save bytes.
    fn persist_save(&mut self, save_bytes: &[u8]) -> Result<(), Self::Err>;
}

pub trait TakeRomFrom {
    fn take_rom_from(&mut self, other: &mut Self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickEffect {
    None,
    FrameRendered,
}

#[allow(clippy::type_complexity)]
pub trait TickableEmulator {
    type Inputs;
    type Err<RErr, AErr, SErr>;

    /// Tick the emulator for a small amount of time, e.g. a single CPU instruction.
    ///
    /// # Errors
    ///
    /// This method should propagate any errors encountered while rendering frames, pushing audio
    /// samples, or persisting save files.
    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> Result<TickEffect, Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        A: AudioOutput,
        S: SaveWriter;
}

pub trait Resettable {
    fn soft_reset(&mut self);

    fn hard_reset(&mut self);
}

// Trait that simply combines useful trait bounds
pub trait EmulatorTrait<Inputs>:
    TickableEmulator<Inputs = Inputs> + Encode + Decode + TakeRomFrom + Resettable
{
}
