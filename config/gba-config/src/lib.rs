use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::{InputModal, MappableInputs, PixelAspectRatio};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};

define_controller_inputs! {
    buttons: GbaButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        L -> l,
        R -> r,
        Start -> start,
        Select -> select,
    },
    non_gamepad_buttons: [
        SolarIncreaseBrightness,
        SolarDecreaseBrightness,
        SolarMinBrightness,
        SolarMaxBrightness,
    ],
    joypad: GbaJoypadInputs,
}

impl GbaButton {
    #[must_use]
    pub fn is_joypad(self) -> bool {
        #[allow(clippy::wildcard_imports)]
        use GbaButton::*;
        matches!(self, Up | Left | Right | Down | A | B | L | R | Start | Select)
    }

    #[must_use]
    pub fn is_solar_sensor(self) -> bool {
        #[allow(clippy::wildcard_imports)]
        use GbaButton::*;
        matches!(
            self,
            SolarIncreaseBrightness
                | SolarDecreaseBrightness
                | SolarMinBrightness
                | SolarMaxBrightness
        )
    }
}

pub const DEFAULT_SOLAR_BRIGHTNESS_STEP: u8 = 5;
pub const DEFAULT_SOLAR_MIN_BRIGHTNESS: u8 = 255 - 0xE8;
pub const DEFAULT_SOLAR_MAX_BRIGHTNESS: u8 = 255 - 0x50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct SolarSensorState {
    pub brightness: u8,
    pub brightness_step: u8,
    pub min_brightness: u8,
    pub max_brightness: u8,
}

impl Default for SolarSensorState {
    fn default() -> Self {
        Self {
            brightness: DEFAULT_SOLAR_MIN_BRIGHTNESS,
            brightness_step: DEFAULT_SOLAR_BRIGHTNESS_STEP,
            min_brightness: DEFAULT_SOLAR_MIN_BRIGHTNESS,
            max_brightness: DEFAULT_SOLAR_MAX_BRIGHTNESS,
        }
    }
}

impl SolarSensorState {
    pub fn handle_button_press(&mut self, button: GbaButton) {
        match button {
            GbaButton::SolarIncreaseBrightness => {
                self.brightness =
                    self.brightness.saturating_add(self.brightness_step).min(self.max_brightness);
            }
            GbaButton::SolarDecreaseBrightness => {
                self.brightness =
                    self.brightness.saturating_sub(self.brightness_step).max(self.min_brightness);
            }
            GbaButton::SolarMinBrightness => {
                self.brightness = self.min_brightness;
            }
            GbaButton::SolarMaxBrightness => {
                self.brightness = self.max_brightness;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct GbaInputs {
    pub joypad: GbaJoypadInputs,
    pub solar: SolarSensorState,
}

impl MappableInputs<GbaButton> for GbaInputs {
    fn set_field(&mut self, button: GbaButton, _player: Player, pressed: bool) {
        if button.is_solar_sensor() {
            if pressed {
                self.solar.handle_button_press(button);
            }
        } else {
            self.joypad.set_button(button, pressed);
        }
    }

    fn modal_for_input(
        &self,
        button: GbaButton,
        _player: Player,
        pressed: bool,
    ) -> Option<InputModal> {
        if !pressed || !button.is_solar_sensor() {
            return None;
        }

        let text = format!("Solar sensor brightness: {}", self.solar.brightness);
        Some(InputModal { id: Some("solar_sensor".into()), text })
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaAspectRatio {
    #[default]
    SquarePixels,
    Stretched,
}

impl GbaAspectRatio {
    #[must_use]
    pub fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaColorCorrection {
    None,
    #[default]
    GbaLcd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaSaveMemory {
    Sram,
    EepromUnknownSize,
    Eeprom512,
    Eeprom8K,
    FlashRom64K,
    FlashRom128K,
    None,
}

impl GbaSaveMemory {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Sram => "SRAM",
            Self::EepromUnknownSize => "EEPROM (unspecified size)",
            Self::Eeprom512 => "EEPROM 512 bytes",
            Self::Eeprom8K => "EEPROM 8 KB",
            Self::FlashRom64K => "Flash ROM 64 KB",
            Self::FlashRom128K => "Flash ROM 128 KB",
            Self::None => "None",
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaAudioInterpolation {
    #[default]
    NearestNeighbor,
    WindowedSinc,
}
