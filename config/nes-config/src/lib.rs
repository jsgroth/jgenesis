pub mod palettes;

#[cfg(feature = "serde")]
mod serialization;

use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::PixelAspectRatio;
use jgenesis_proc_macros::{EnumAll, EnumDisplay};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::ops::Index;
use std::path::Path;
use std::{array, io};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum NesAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl NesAspectRatio {
    #[inline]
    #[must_use]
    pub fn to_pixel_aspect_ratio_f64(self) -> Option<f64> {
        match self {
            Self::Ntsc => Some(8.0 / 7.0),
            Self::Pal => Some(11.0 / 8.0),
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE.into()),
            Self::Stretched => None,
        }
    }

    #[inline]
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        self.to_pixel_aspect_ratio_f64().map(|par| PixelAspectRatio::try_from(par).unwrap())
    }
}

#[derive(Debug)]
pub enum PaletteLoadError {
    Io(io::Error),
    IncorrectSize(u64),
}

impl Display for PaletteLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error loading palette: {err}"),
            Self::IncorrectSize(len) => {
                write!(f, "Incorrect palette size; expected {} bytes, was {len}", 512 * 3)
            }
        }
    }
}

impl Error for PaletteLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::IncorrectSize(_) => None,
        }
    }
}

fn bytes_to_triples_array<const LEN: usize>(bytes: &[u8]) -> [(u8, u8, u8); LEN] {
    array::from_fn(|i| (bytes[3 * i], bytes[3 * i + 1], bytes[3 * i + 2]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct NesPalette(pub [(u8, u8, u8); 512]);

impl NesPalette {
    const DEFAULT_BYTES: &'static [u8; 512 * 3] = include_bytes!("nespalette.pal");

    /// Load a 512-color or 64-color palette from a file.
    ///
    /// 64-color palettes will be extrapolated to 512 colors.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is too small or if there is an I/O error reading it.
    pub fn read_from(path: &Path) -> Result<Self, PaletteLoadError> {
        let file = File::open(path).map_err(PaletteLoadError::Io)?;
        let metadata = file.metadata().map_err(PaletteLoadError::Io)?;
        if metadata.len() < 64 * 3 {
            return Err(PaletteLoadError::IncorrectSize(metadata.len()));
        }

        if metadata.len() < 512 * 3 {
            // Assume 64-color palette
            return Self::read_from_64_color(file);
        }

        let mut reader = BufReader::new(file);
        let mut bytes = [0_u8; 512 * 3];
        reader.read_exact(&mut bytes).map_err(PaletteLoadError::Io)?;

        Ok(Self(bytes_to_triples_array(&bytes)))
    }

    fn read_from_64_color(file: File) -> Result<Self, PaletteLoadError> {
        let mut reader = BufReader::new(file);
        let mut bytes = [0_u8; 64 * 3];
        reader.read_exact(&mut bytes).map_err(PaletteLoadError::Io)?;

        let palette_64_color: [_; 64] = bytes_to_triples_array(&bytes);
        Ok(palettes::extrapolate_64_to_512(&palette_64_color))
    }

    /// Write palette to a file.
    ///
    /// # Errors
    ///
    /// Propagates any I/O errors encountered while creating or writing the file.
    pub fn write_to(&self, path: &Path) -> io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        for (r, g, b) in self.0 {
            writer.write_all(&[r, g, b])?;
        }

        Ok(())
    }
}

impl Default for NesPalette {
    fn default() -> Self {
        Self(bytes_to_triples_array(Self::DEFAULT_BYTES))
    }
}

impl Index<usize> for NesPalette {
    type Output = (u8, u8, u8);

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Overscan {
    pub top: u16,
    pub bottom: u16,
    pub left: u16,
    pub right: u16,
}

impl Overscan {
    pub const NONE: Self = Self { top: 0, bottom: 0, left: 0, right: 0 };
}

impl Display for Overscan {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Overscan {{ top={}, bottom={}, left={}, right={} }}",
            self.top, self.bottom, self.left, self.right
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum NesAudioResampler {
    LowPassNearestNeighbor,
    #[default]
    WindowedSinc,
}

define_controller_inputs! {
    buttons: NesButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        Start -> start,
        Select -> select,
    },
    non_gamepad_buttons: [ZapperFire, ZapperForceOffscreen],
    joypad: NesJoypadState,
}

impl NesButton {
    #[inline]
    #[must_use]
    pub fn is_zapper(self) -> bool {
        matches!(self, Self::ZapperFire | Self::ZapperForceOffscreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_aspect_ratios_valid() {
        for par in NesAspectRatio::ALL {
            let _ = par.to_pixel_aspect_ratio();
        }
    }
}
