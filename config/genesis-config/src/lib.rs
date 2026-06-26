pub mod cheats;

use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::{FiniteF64, FrameSize, MappableInputs, TimingMode};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};
use std::fmt::{Display, Formatter};

pub const NATIVE_M68K_DIVIDER: u64 = 7;

pub const NATIVE_SUB_CPU_DIVIDER: u64 = 4;

pub const NATIVE_SH2_MULTIPLIER: u64 = 3;

pub const MODEL_1_VA2_LPF_CUTOFF: u32 = 3390;
pub const MODEL_1_VA3_LPF_CUTOFF: u32 = 2840;
pub const MODEL_2_1ST_LPF_CUTOFF: u32 = 3789;
pub const MODEL_2_2ND_LPF_CUTOFF: u32 = 6725;

pub const DEFAULT_PCM_LPF_CUTOFF: u32 = 7973;

#[derive(Debug, Clone, Copy)]
pub struct GenParParams {
    pub force_square_in_h40: bool,
    pub adjust_for_2x_resolution: bool,
    pub anamorphic_widescreen: bool,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GenesisAspectRatio {
    #[default]
    Auto,
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl GenesisAspectRatio {
    #[inline]
    #[must_use]
    pub fn to_h40_pixel_aspect_ratio(self, timing_mode: TimingMode) -> Option<f64> {
        if self == Self::Auto {
            let auto_aspect = match timing_mode {
                TimingMode::Ntsc => Self::Ntsc,
                TimingMode::Pal => Self::Pal,
            };
            return auto_aspect.to_h40_pixel_aspect_ratio(timing_mode);
        }

        match self {
            Self::Ntsc => Some(32.0 / 35.0),
            Self::Pal => Some(11.0 / 10.0),
            Self::SquarePixels => Some(1.0),
            Self::Stretched => None,
            Self::Auto => unreachable!("Auto checked at start of function with early return"),
        }
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn to_pixel_aspect_ratio(
        self,
        timing_mode: TimingMode,
        frame_size: FrameSize,
        params: GenParParams,
    ) -> Option<FiniteF64> {
        if self == Self::Auto {
            let auto_aspect = match timing_mode {
                TimingMode::Ntsc => Self::Ntsc,
                TimingMode::Pal => Self::Pal,
            };
            return auto_aspect.to_pixel_aspect_ratio(timing_mode, frame_size, params);
        }

        let GenParParams { force_square_in_h40, adjust_for_2x_resolution, anamorphic_widescreen } =
            params;

        let mut pixel_aspect_ratio = match (self, frame_size.width) {
            (Self::SquarePixels, _) => Some(1.0),
            (Self::Stretched, _) => None,
            (Self::Ntsc, 256..=284) => {
                // NTSC H32/H256px
                Some(8.0 / 7.0)
            }
            (Self::Ntsc, 320..=347) => {
                // NTSC H40/H320px
                if force_square_in_h40 { Some(1.0) } else { Some(32.0 / 35.0) }
            }
            (Self::Pal, 256..=284) => {
                // PAL H32/H256px
                Some(11.0 / 8.0)
            }
            (Self::Pal, 320..=347) => {
                // PAL H40/H320px
                if force_square_in_h40 { Some(1.0) } else { Some(11.0 / 10.0) }
            }
            (Self::Ntsc | Self::Pal, _) => {
                log::error!("unexpected Genesis frame width: {}", frame_size.width);
                None
            }
            (Self::Auto, _) => unreachable!("Auto checked at start of function with early return"),
        };

        if adjust_for_2x_resolution && frame_size.height >= 448 {
            pixel_aspect_ratio = pixel_aspect_ratio.map(|par| par * 2.0);
        }

        if anamorphic_widescreen {
            pixel_aspect_ratio = pixel_aspect_ratio.map(|par| par * 4.0 / 3.0);
        }

        pixel_aspect_ratio.map(|par| FiniteF64::try_from(par).unwrap())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GenesisRegion {
    Americas,
    Japan,
    Europe,
}

impl GenesisRegion {
    #[must_use]
    pub fn short_name(self) -> &'static str {
        match self {
            Self::Americas => "US",
            Self::Europe => "EU",
            Self::Japan => "JP",
        }
    }

    #[must_use]
    pub fn long_name(self) -> &'static str {
        match self {
            Self::Americas => "US",
            Self::Europe => "Europe",
            Self::Japan => "Japan",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumDisplay)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum Opn2BusyBehavior {
    Ym2612,
    #[default]
    Ym3438,
    AlwaysZero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GenesisControllerType {
    ThreeButton,
    #[default]
    SixButton,
    Xe1ap,
    None,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PcmInterpolation {
    #[default]
    None,
    Linear,
    CubicHermite,
    CubicHermite6Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum S32XVideoOut {
    #[default]
    Combined,
    GenesisOnly,
    S32XOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum S32XColorTint {
    #[default]
    None,
    SlightYellow,
    Yellow,
    SlightPurple,
    Purple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum S32XVoidColor {
    PaletteRam { idx: u8 },
    Direct { r: u8, g: u8, b: u8, a: bool },
}

impl Default for S32XVoidColor {
    fn default() -> Self {
        Self::Direct { r: 0, g: 0, b: 0, a: false }
    }
}

impl Display for S32XVoidColor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::PaletteRam { idx } => write!(f, "Palette RAM index {idx}"),
            Self::Direct { r, g, b, a } => write!(f, "RGBA({r}, {g}, {b}, {})", u8::from(a)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum S32XVoidColorType {
    PaletteRam,
    #[default]
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum S32XPwmResampling {
    CubicHermite,
    #[default]
    WindowedSinc,
}

// TODO this is a little awful...
define_controller_inputs! {
    buttons: GenesisButton {
        Up -> up "Up",
        Left -> left "Left",
        Right -> right "Right",
        Down -> down "Down",
        A -> a "A",
        B -> b "B",
        C -> c "C",
        X -> x "X",
        Y -> y "Y",
        Z -> z "Z",
        Start -> start "Start",
        Mode -> mode "Mode",
    },
    non_gamepad_buttons: [
        Xe1apAnalogLeft "Stick - Left",
        Xe1apAnalogRight "Stick - Right",
        Xe1apAnalogUp "Stick - Up",
        Xe1apAnalogDown "Stick - Down",
        Xe1apSliderForward "Slider - Forward",
        Xe1apSliderBackward "Slider - Backward",
        Xe1apA "A",
        Xe1apB "B",
        Xe1apC "C",
        Xe1apD "D",
        Xe1apE1 "E1",
        Xe1apE2 "E2",
        Xe1apAp "A'",
        Xe1apBp "B'",
        Xe1apStart "Start",
        Xe1apSelect "Select",
    ],
    joypad: GenesisJoypadState impl with_allow_opposing_directions,
}

impl GenesisButton {
    #[must_use]
    pub fn is_gamepad(self) -> bool {
        matches!(
            self,
            Self::Up
                | Self::Left
                | Self::Right
                | Self::Down
                | Self::A
                | Self::B
                | Self::C
                | Self::X
                | Self::Y
                | Self::Z
                | Self::Start
                | Self::Mode
        )
    }

    #[must_use]
    pub fn is_xe1ap(self) -> bool {
        matches!(
            self,
            Self::Xe1apAnalogLeft
                | Self::Xe1apAnalogRight
                | Self::Xe1apAnalogUp
                | Self::Xe1apAnalogDown
                | Self::Xe1apSliderForward
                | Self::Xe1apSliderBackward
                | Self::Xe1apA
                | Self::Xe1apB
                | Self::Xe1apC
                | Self::Xe1apD
                | Self::Xe1apE1
                | Self::Xe1apE2
                | Self::Xe1apAp
                | Self::Xe1apBp
                | Self::Xe1apStart
                | Self::Xe1apSelect
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct Xe1apJoypadState {
    pub analog_x: u8,
    pub analog_y: u8,
    pub slider: u8,
    pub a: bool,
    pub b: bool,
    pub c: bool,
    pub d: bool,
    pub e1: bool,
    pub e2: bool,
    pub ap: bool,
    pub bp: bool,
    pub start: bool,
    pub select: bool,
    // Fields used only for mapping real world digital inputs to XE-1 AP analog inputs
    analog_left: bool,
    analog_right: bool,
    analog_up: bool,
    analog_down: bool,
    slider_forward: bool,
    slider_backward: bool,
}

impl Default for Xe1apJoypadState {
    fn default() -> Self {
        Self {
            analog_x: xe1ap_analog_positive(0),
            analog_y: xe1ap_analog_positive(0),
            slider: xe1ap_analog_positive(0),
            a: false,
            b: false,
            c: false,
            d: false,
            e1: false,
            e2: false,
            ap: false,
            bp: false,
            start: false,
            select: false,
            analog_left: false,
            analog_right: false,
            analog_up: false,
            analog_down: false,
            slider_forward: false,
            slider_backward: false,
        }
    }
}

impl Xe1apJoypadState {
    pub fn set_button(&mut self, button: GenesisButton, pressed: bool) {
        match button {
            GenesisButton::Xe1apAnalogLeft => {
                self.analog_left = pressed;
                self.analog_x = xe1ap_digital_to_analog(self.analog_left, self.analog_right);
            }
            GenesisButton::Xe1apAnalogRight => {
                self.analog_right = pressed;
                self.analog_x = xe1ap_digital_to_analog(self.analog_left, self.analog_right);
            }
            GenesisButton::Xe1apAnalogUp => {
                self.analog_up = pressed;
                self.analog_y = xe1ap_digital_to_analog(self.analog_up, self.analog_down);
            }
            GenesisButton::Xe1apAnalogDown => {
                self.analog_down = pressed;
                self.analog_y = xe1ap_digital_to_analog(self.analog_up, self.analog_down);
            }
            GenesisButton::Xe1apSliderForward => {
                self.slider_forward = pressed;
                self.slider = xe1ap_digital_to_analog(self.slider_backward, self.slider_forward);
            }
            GenesisButton::Xe1apSliderBackward => {
                self.slider_backward = pressed;
                self.slider = xe1ap_digital_to_analog(self.slider_backward, self.slider_forward);
            }
            GenesisButton::Xe1apA => self.a = pressed,
            GenesisButton::Xe1apB => self.b = pressed,
            GenesisButton::Xe1apC => self.c = pressed,
            GenesisButton::Xe1apD => self.d = pressed,
            GenesisButton::Xe1apE1 => self.e1 = pressed,
            GenesisButton::Xe1apE2 => self.e2 = pressed,
            GenesisButton::Xe1apAp => self.ap = pressed,
            GenesisButton::Xe1apBp => self.bp = pressed,
            GenesisButton::Xe1apStart => self.start = pressed,
            GenesisButton::Xe1apSelect => self.select = pressed,
            _ => {}
        }
    }

    pub fn set_analog(&mut self, button: GenesisButton, value: i16) {
        match button {
            GenesisButton::Xe1apAnalogLeft => {
                self.analog_x = xe1ap_analog_negative(value);
            }
            GenesisButton::Xe1apAnalogRight => {
                self.analog_x = xe1ap_analog_positive(value);
            }
            GenesisButton::Xe1apAnalogUp => {
                self.analog_y = xe1ap_analog_negative(value);
            }
            GenesisButton::Xe1apAnalogDown => {
                self.analog_y = xe1ap_analog_positive(value);
            }
            GenesisButton::Xe1apSliderForward => {
                self.slider = xe1ap_analog_positive(value);
            }
            GenesisButton::Xe1apSliderBackward => {
                self.slider = xe1ap_analog_negative(value);
            }
            _ => {}
        }
    }
}

fn xe1ap_analog_negative(value: i16) -> u8 {
    // Negative values map to 0-128

    // Round towards negative infinity instead of 0
    let shifted = (value >> 8) + i16::from(value & 0xFF != 0);
    (128 - shifted) as u8
}

fn xe1ap_analog_positive(value: i16) -> u8 {
    // Positive values map to 128-255
    ((value >> 8) + 128) as u8
}

fn xe1ap_digital_to_analog(negative: bool, positive: bool) -> u8 {
    if negative == positive {
        // Neither is pressed or both are pressed
        xe1ap_analog_positive(0)
    } else if negative {
        xe1ap_analog_negative(i16::MAX)
    } else {
        xe1ap_analog_positive(i16::MAX)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum GenesisController {
    ThreeButton(GenesisJoypadState),
    SixButton(GenesisJoypadState),
    Xe1ap(Xe1apJoypadState),
    None,
}

impl GenesisController {
    #[must_use]
    pub fn new(controller_type: GenesisControllerType) -> Self {
        match controller_type {
            GenesisControllerType::ThreeButton => Self::ThreeButton(GenesisJoypadState::default()),
            GenesisControllerType::SixButton => Self::SixButton(GenesisJoypadState::default()),
            GenesisControllerType::Xe1ap => Self::Xe1ap(Xe1apJoypadState::default()),
            GenesisControllerType::None => Self::None,
        }
    }

    pub fn set_field(&mut self, button: GenesisButton, pressed: bool) {
        match self {
            Self::ThreeButton(state) | Self::SixButton(state) => state.set_button(button, pressed),
            Self::Xe1ap(state) => state.set_button(button, pressed),
            Self::None => {}
        }
    }

    pub fn set_analog(&mut self, button: GenesisButton, value: i16) {
        if let Self::Xe1ap(state) = self {
            state.set_analog(button, value);
        }
    }

    #[must_use]
    pub fn controller_type(self) -> GenesisControllerType {
        match self {
            Self::ThreeButton(_) => GenesisControllerType::ThreeButton,
            Self::SixButton(_) => GenesisControllerType::SixButton,
            Self::Xe1ap(_) => GenesisControllerType::Xe1ap,
            Self::None => GenesisControllerType::None,
        }
    }
}

impl Default for GenesisController {
    fn default() -> Self {
        Self::new(GenesisControllerType::SixButton)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct GenesisInputs {
    pub p1: GenesisController,
    pub p2: GenesisController,
}

impl Default for GenesisInputs {
    fn default() -> Self {
        Self { p1: GenesisController::default(), p2: GenesisController::None }
    }
}

impl MappableInputs<GenesisButton> for GenesisInputs {
    fn set_field(&mut self, button: GenesisButton, player: Player, pressed: bool) {
        match player {
            Player::One => self.p1.set_field(button, pressed),
            Player::Two => self.p2.set_field(button, pressed),
            _ => {}
        }
    }

    fn set_analog(&mut self, button: GenesisButton, player: Player, value: i16) {
        match player {
            Player::One => self.p1.set_analog(button, value),
            Player::Two => self.p2.set_analog(button, value),
            _ => {}
        }
    }
}
