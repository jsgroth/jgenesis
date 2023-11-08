use bincode::{Decode, Encode};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use std::error::Error;
use std::fmt::{Debug, Display};
use std::num::NonZeroU32;
use thiserror::Error;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable, Encode, Decode)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    #[must_use]
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
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

    /// Persist any save data that should be persistent, such as cartridge SRAM.
    ///
    /// `save_bytes` is an iterator to enable concatenating multiple arrays
    /// if desired, such as with the Sega CD (internal backup RAM + RAM cartridge).
    ///
    /// # Errors
    ///
    /// This method will return an error if it is unable to persist the given save bytes.
    fn persist_save<'a>(
        &mut self,
        save_bytes: impl Iterator<Item = &'a [u8]>,
    ) -> Result<(), Self::Err>;
}

pub trait ConfigReload {
    type Config;

    fn reload_config(&mut self, config: &Self::Config);
}

pub trait PartialClone {
    /// Create a partial clone of `self`, which clones all emulation state but may not clone
    /// read-only fields such as ROMs and frame buffers.
    #[must_use]
    fn partial_clone(&self) -> Self;
}

pub use jgenesis_proc_macros::PartialClone;

pub trait TakeRomFrom {
    fn take_rom_from(&mut self, other: &mut Self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickEffect {
    None,
    FrameRendered,
}

#[derive(Debug, Error)]
pub enum EmulatorError<RErr, AErr, SErr> {
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    SaveWrite(SErr),
}

#[allow(clippy::type_complexity)]
pub trait TickableEmulator {
    type Inputs;
    type Err<RErr: Debug + Display + Send + Sync + 'static, AErr: Debug + Display + Send + Sync + 'static, SErr: Debug + Display + Send + Sync + 'static>: Error + Send + Sync + 'static;

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
}

pub trait Resettable {
    fn soft_reset(&mut self);

    fn hard_reset(&mut self);
}

pub const VRAM_DEBUG_ROW_LEN: u32 = 64;

pub trait EmulatorDebug {
    // CRAM size
    const NUM_PALETTES: u32;
    const PALETTE_LEN: u32;

    // VRAM size
    const PATTERN_TABLE_LEN: u32;

    const SUPPORTS_VRAM_DEBUG: bool;

    fn debug_cram(&self, out: &mut [Color]);

    fn debug_vram(&self, out: &mut [Color], palette: u8);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumFromStr, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TimingMode {
    #[default]
    Ntsc,
    Pal,
}

pub trait EmulatorTrait:
    TickableEmulator<Inputs = Self::EmulatorInputs>
    + Encode
    + Decode
    + ConfigReload<Config = Self::EmulatorConfig>
    + PartialClone
    + TakeRomFrom
    + Resettable
    + EmulatorDebug
{
    type EmulatorInputs;
    type EmulatorConfig;

    fn timing_mode(&self) -> TimingMode;
}
