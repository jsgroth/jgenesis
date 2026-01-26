mod finitefloat;

use bincode::{Decode, Encode};
pub use finitefloat::{FiniteF32, FiniteF64};
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};
use std::borrow::Cow;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;

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

    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    #[must_use]
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    #[must_use]
    #[inline]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Color {
    #[inline]
    fn default() -> Self {
        Self::BLACK
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct DisplayArea {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Encode, Decode)]
pub enum ColorCorrection {
    #[default]
    None,
    GbcLcd {
        screen_gamma: FiniteF32,
    },
    GbaLcd {
        screen_gamma: FiniteF32,
    },
}

impl Display for ColorCorrection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            &Self::GbcLcd { screen_gamma } => {
                write!(f, "Game Boy Color LCD (gamma {:.1})", f32::from(screen_gamma))
            }
            &Self::GbaLcd { screen_gamma } => {
                write!(f, "Game Boy Advance LCD (gamma {:.1})", f32::from(screen_gamma))
            }
        }
    }
}

/// Rendering options that are not required to be explicitly specified, unlike frame size
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderFrameOptions {
    pub pixel_aspect_ratio: Option<FiniteF64>,
    pub color_correction: ColorCorrection,
    pub frame_blending: bool,
}

impl RenderFrameOptions {
    #[must_use]
    pub fn pixel_aspect_ratio(pixel_aspect_ratio: Option<FiniteF64>) -> Self {
        Self { pixel_aspect_ratio, ..Self::default() }
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
        options: RenderFrameOptions,
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
    fn load_serialized<D: Decode<()>>(&mut self, extension: &str) -> Result<D, Self::Err>;

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

use crate::input::Player;
pub use jgenesis_proc_macros::PartialClone;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
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

pub struct InputModal {
    pub id: Option<Cow<'static, str>>,
    pub text: String,
}

pub trait MappableInputs<Button> {
    fn set_field(&mut self, button: Button, player: Player, pressed: bool);

    #[allow(unused_variables)]
    fn handle_mouse_motion(
        &mut self,
        x: f32,
        y: f32,
        frame_size: FrameSize,
        display_area: DisplayArea,
    ) {
    }

    fn handle_mouse_leave(&mut self) {}

    #[allow(unused_variables)]
    fn modal_for_input(&self, button: Button, player: Player, pressed: bool) -> Option<InputModal> {
        None
    }
}

pub trait EmulatorConfigTrait: Clone {
    #[must_use]
    fn with_overclocking_disabled(&self) -> Self {
        self.clone()
    }
}

pub trait EmulatorTrait: Encode + Decode<()> + PartialClone {
    type Button: Debug + Copy + Eq + Hash;
    type Inputs: Default + MappableInputs<Self::Button>;
    type Config: EmulatorConfigTrait;

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

    #[must_use]
    fn save_state_version() -> &'static str {
        "0.11.0-3"
    }

    fn target_fps(&self) -> f64;

    fn update_audio_output_frequency(&mut self, output_frequency: u64);
}
