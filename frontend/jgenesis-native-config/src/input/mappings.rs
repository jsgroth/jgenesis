use crate::input::{GenericInput, Hotkey, KeyboardInput};
use gb_config::GameBoyButton;
use gba_config::GbaButton;
use genesis_config::{GenesisButton, GenesisControllerType};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{ConfigDisplay, EnumAll, EnumDisplay};
use nes_config::NesButton;
use pce_config::PceButton;
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use serde::{Deserialize, Serialize};
use smsgg_config::SmsGgButton;
use snes_config::SnesButton;
use std::fmt::Formatter;
use std::sync::LazyLock;

pub type ButtonMappingVec<'a, Button> = Vec<((Button, Player), &'a Vec<GenericInput>)>;
pub type HotkeyMappingVec<'a> = Vec<(Hotkey, &'a Vec<GenericInput>)>;

macro_rules! key_input {
    ($key:ident) => {
        Some(vec![GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::$key))])
    };
}

macro_rules! define_controller_mapping {
    (
        $name:ident,
        $button_enum:ident,
        [$($field:ident: $enum_value:ident),* $(,)?] $(,)?
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
        pub struct $name {
            $(
                pub $field: Option<Vec<GenericInput>>,
            )*
        }

        impl $name {
            pub fn to_mapping_vec<'a>(&'a self, player: Player, out: &mut ButtonMappingVec<'a, $button_enum>) {
                $(
                    if let Some(mapping) = &self.$field {
                        out.push((($button_enum::$enum_value, player), mapping));
                    }
                )*
            }

            #[must_use]
            pub fn access_value(&mut self, button: $button_enum) -> Option<&mut Option<Vec<GenericInput>>> {
                match button {
                    $(
                        $button_enum::$enum_value => Some(&mut self.$field),
                    )*
                    #[allow(unreachable_patterns)]
                    _ => None,
                }
            }

            #[must_use]
            pub fn access_value_shared(&self, button: $button_enum) -> Option<&Option<Vec<GenericInput>>> {
                match button {
                    $(
                        $button_enum::$enum_value => Some(&self.$field),
                    )*
                    #[allow(unreachable_patterns)]
                    _ => None,
                }
            }

            pub fn clone_from(&mut self, other: &Self, buttons: &[$button_enum]) {
                for &button in buttons {
                    if let Some(value) = self.access_value(button)
                        && let Some(other_value) = other.access_value_shared(button)
                    {
                        *value = other_value.clone();
                    }
                }
            }
        }

        impl std::fmt::Display for $name {
            // Last `first_mapping = false` will be flagged as unused without this #[allow(..)]
            #[allow(unused_assignments)]
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                fmt_input_mapping(&[
                    $(
                        ($button_enum::$enum_value, &self.$field),
                    )*
                ], f)
            }
        }
    }
}

fn fmt_input_mapping<Button: std::fmt::Display>(
    buttons: &[(Button, &Option<Vec<GenericInput>>)],
    f: &mut Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "{{ ")?;

    let mut first_mapping = true;
    for (button, mapping) in buttons {
        let Some(mapping) = mapping else { continue };
        if mapping.is_empty() {
            continue;
        }

        if !first_mapping {
            write!(f, ", ")?;
        }
        first_mapping = false;

        write!(f, "{button} -> '")?;
        fmt_input_mapping_field(mapping, f)?;
        write!(f, "'")?;
    }

    write!(f, " }}")
}

fn fmt_input_mapping_field(mapping: &[GenericInput], f: &mut Formatter<'_>) -> std::fmt::Result {
    if mapping.is_empty() {
        return Ok(());
    }

    let mut first_input = true;
    for input in mapping {
        if !first_input {
            write!(f, " + ")?;
        }
        first_input = false;

        write!(f, "{input}")?;
    }

    Ok(())
}

macro_rules! impl_to_mapping_vec {
    ($button:ty) => {
        #[must_use]
        pub fn to_mapping_vec(&self) -> ButtonMappingVec<'_, $button> {
            let mut out = Vec::new();

            self.mapping_1.to_mapping_vec(&mut out);
            self.mapping_2.to_mapping_vec(&mut out);

            out
        }
    };
}

macro_rules! impl_to_turbo_mapping_vec {
    ($button:ty) => {
        impl_to_turbo_mapping_vec!($button, [(One, p1_turbo), (Two, p2_turbo)]);
    };
    ($button:ty, [$(($player:ident, $field:ident)),* $(,)?]) => {
        #[must_use]
        pub fn to_turbo_mapping_vec(&self) -> ButtonMappingVec<'_, $button> {
            let mut out = Vec::new();

            for mapping in [&self.mapping_1, &self.mapping_2] {
                $(
                    mapping.$field.to_mapping_vec(Player::$player, &mut out);
                )*
            }

            out
        }
    };
}

macro_rules! impl_player_mapping {
    ($mapping:ty) => {
        impl_player_mapping!($mapping, [(One, p1, p1_turbo), (Two, p2, p2_turbo)]);
    };
    ($mapping:ty, [$(($player:ident, $field:ident, $turbo_field:ident)),* $(,)?]) => {
        #[must_use]
        pub fn player_mapping(&mut self, player: Player, turbo: bool) -> Option<&mut $mapping> {
            match (player, turbo) {
                $(
                    (Player::$player, false) => Some(&mut self.$field),
                    (Player::$player, true) => Some(&mut self.$turbo_field),
                )*
                _ => None,
            }
        }
    };
}

define_controller_mapping!(SmsGgControllerMapping, SmsGgButton, [
    up: Up,
    left: Left,
    right: Right,
    down: Down,
    button1: Button1,
    button2: Button2,
]);

impl SmsGgControllerMapping {
    #[must_use]
    pub fn keyboard_arrows() -> Self {
        Self {
            up: key_input!(Up),
            left: key_input!(Left),
            right: key_input!(Right),
            down: key_input!(Down),
            button1: key_input!(S),
            button2: key_input!(A),
        }
    }

    #[must_use]
    pub fn keyboard_wasd() -> Self {
        Self {
            up: key_input!(W),
            left: key_input!(A),
            right: key_input!(D),
            down: key_input!(S),
            button1: key_input!(K),
            button2: key_input!(L),
        }
    }

    #[must_use]
    pub fn keyboard_pause() -> Option<Vec<GenericInput>> {
        key_input!(Return)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct SmsGgInputMapping {
    pub p1: SmsGgControllerMapping,
    pub p2: SmsGgControllerMapping,
    pub p1_turbo: SmsGgControllerMapping,
    pub p2_turbo: SmsGgControllerMapping,
    #[cfg_display(debug_fmt)]
    pub pause: Option<Vec<GenericInput>>,
}

impl SmsGgInputMapping {
    impl_player_mapping!(SmsGgControllerMapping);

    pub fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, SmsGgButton>) {
        self.p1.to_mapping_vec(Player::One, out);
        self.p2.to_mapping_vec(Player::Two, out);

        if let Some(pause) = &self.pause {
            out.push(((SmsGgButton::Pause, Player::One), pause));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct SmsGgInputConfig {
    #[cfg_display(indent_nested)]
    pub mapping_1: SmsGgInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: SmsGgInputMapping,
}

impl SmsGgInputConfig {
    impl_to_mapping_vec!(SmsGgButton);

    impl_to_turbo_mapping_vec!(SmsGgButton);
}

fn default_smsgg_mapping_1() -> SmsGgInputMapping {
    SmsGgInputMapping {
        p1: SmsGgControllerMapping::keyboard_arrows(),
        p2: SmsGgControllerMapping::default(),
        p1_turbo: SmsGgControllerMapping::default(),
        p2_turbo: SmsGgControllerMapping::default(),
        pause: key_input!(Return),
    }
}

impl Default for SmsGgInputConfig {
    fn default() -> Self {
        Self { mapping_1: default_smsgg_mapping_1(), mapping_2: SmsGgInputMapping::default() }
    }
}

define_controller_mapping!(GenesisControllerMapping, GenesisButton, [
    up: Up,
    left: Left,
    right: Right,
    down: Down,
    a: A,
    b: B,
    c: C,
    x: X,
    y: Y,
    z: Z,
    start: Start,
    mode: Mode,
    xe1ap_analog_left: Xe1apAnalogLeft,
    xe1ap_analog_right: Xe1apAnalogRight,
    xe1ap_analog_up: Xe1apAnalogUp,
    xe1ap_analog_down: Xe1apAnalogDown,
    xe1ap_slider_forward: Xe1apSliderForward,
    xe1ap_slider_backward: Xe1apSliderBackward,
    xe1ap_a: Xe1apA,
    xe1ap_b: Xe1apB,
    xe1ap_c: Xe1apC,
    xe1ap_d: Xe1apD,
    xe1ap_e1: Xe1apE1,
    xe1ap_e2: Xe1apE2,
    xe1ap_ap: Xe1apAp,
    xe1ap_bp: Xe1apBp,
    xe1ap_start: Xe1apStart,
    xe1ap_select: Xe1apSelect,
]);

impl GenesisControllerMapping {
    #[must_use]
    pub fn keyboard_arrows() -> Self {
        Self {
            up: key_input!(Up),
            left: key_input!(Left),
            right: key_input!(Right),
            down: key_input!(Down),
            a: key_input!(A),
            b: key_input!(S),
            c: key_input!(D),
            x: key_input!(Q),
            y: key_input!(W),
            z: key_input!(E),
            start: key_input!(Return),
            mode: key_input!(RShift),
            xe1ap_analog_left: None,
            xe1ap_analog_right: None,
            xe1ap_analog_up: None,
            xe1ap_analog_down: None,
            xe1ap_slider_forward: None,
            xe1ap_slider_backward: None,
            xe1ap_a: None,
            xe1ap_b: None,
            xe1ap_c: None,
            xe1ap_d: None,
            xe1ap_e1: None,
            xe1ap_e2: None,
            xe1ap_ap: None,
            xe1ap_bp: None,
            xe1ap_start: None,
            xe1ap_select: None,
        }
    }

    #[must_use]
    pub fn keyboard_wasd() -> Self {
        Self {
            up: key_input!(W),
            left: key_input!(A),
            right: key_input!(D),
            down: key_input!(S),
            a: key_input!(J),
            b: key_input!(K),
            c: key_input!(L),
            x: key_input!(U),
            y: key_input!(I),
            z: key_input!(O),
            start: key_input!(Return),
            mode: key_input!(RShift),
            xe1ap_analog_left: None,
            xe1ap_analog_right: None,
            xe1ap_analog_up: None,
            xe1ap_analog_down: None,
            xe1ap_slider_forward: None,
            xe1ap_slider_backward: None,
            xe1ap_a: None,
            xe1ap_b: None,
            xe1ap_c: None,
            xe1ap_d: None,
            xe1ap_e1: None,
            xe1ap_e2: None,
            xe1ap_ap: None,
            xe1ap_bp: None,
            xe1ap_start: None,
            xe1ap_select: None,
        }
    }

    pub fn clone_from_type(&mut self, other: &Self, controller_type: GenesisControllerType) {
        static GAMEPAD_BUTTONS: LazyLock<Vec<GenesisButton>> = LazyLock::new(|| {
            GenesisButton::ALL.into_iter().filter(|&button| button.is_gamepad()).collect()
        });

        static XE1AP_BUTTONS: LazyLock<Vec<GenesisButton>> = LazyLock::new(|| {
            GenesisButton::ALL.into_iter().filter(|&button| button.is_xe1ap()).collect()
        });

        match controller_type {
            GenesisControllerType::Xe1ap => self.clone_from(other, &XE1AP_BUTTONS),
            _ => self.clone_from(other, &GAMEPAD_BUTTONS),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct GenesisInputMapping {
    #[cfg_display(indent_nested)]
    pub p1: GenesisControllerMapping,
    #[cfg_display(indent_nested)]
    pub p2: GenesisControllerMapping,
    #[cfg_display(indent_nested)]
    pub p1_turbo: GenesisControllerMapping,
    #[cfg_display(indent_nested)]
    pub p2_turbo: GenesisControllerMapping,
}

impl GenesisInputMapping {
    impl_player_mapping!(GenesisControllerMapping);

    pub fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, GenesisButton>) {
        self.p1.to_mapping_vec(Player::One, out);
        self.p2.to_mapping_vec(Player::Two, out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct GenesisInputConfig {
    pub p1_type: GenesisControllerType,
    pub p2_type: GenesisControllerType,
    #[cfg_display(indent_nested)]
    pub mapping_1: GenesisInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: GenesisInputMapping,
}

impl GenesisInputConfig {
    impl_to_mapping_vec!(GenesisButton);

    impl_to_turbo_mapping_vec!(GenesisButton);
}

fn default_genesis_mapping_1() -> GenesisInputMapping {
    GenesisInputMapping {
        p1: GenesisControllerMapping::keyboard_arrows(),
        p2: GenesisControllerMapping::default(),
        p1_turbo: GenesisControllerMapping::default(),
        p2_turbo: GenesisControllerMapping::default(),
    }
}

impl Default for GenesisInputConfig {
    fn default() -> Self {
        Self {
            p1_type: GenesisControllerType::default(),
            p2_type: GenesisControllerType::None,
            mapping_1: default_genesis_mapping_1(),
            mapping_2: GenesisInputMapping::default(),
        }
    }
}

define_controller_mapping!(NesControllerMapping, NesButton, [
    up: Up,
    left: Left,
    right: Right,
    down: Down,
    a: A,
    b: B,
    start: Start,
    select: Select,
]);

impl NesControllerMapping {
    #[must_use]
    pub fn keyboard_arrows() -> Self {
        Self {
            up: key_input!(Up),
            left: key_input!(Left),
            right: key_input!(Right),
            down: key_input!(Down),
            a: key_input!(A),
            b: key_input!(S),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }

    #[must_use]
    pub fn keyboard_wasd() -> Self {
        Self {
            up: key_input!(W),
            left: key_input!(A),
            right: key_input!(D),
            down: key_input!(S),
            a: key_input!(L),
            b: key_input!(K),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }
}

define_controller_mapping!(NesZapperMapping, NesButton, [
    fire: ZapperFire,
    force_offscreen: ZapperForceOffscreen,
]);

impl NesZapperMapping {
    #[must_use]
    pub fn mouse() -> Self {
        Self {
            fire: Some(vec![GenericInput::Mouse(MouseButton::Left)]),
            force_offscreen: Some(vec![GenericInput::Mouse(MouseButton::Right)]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct NesInputMapping {
    #[cfg_display(indent_nested)]
    pub p1: NesControllerMapping,
    #[cfg_display(indent_nested)]
    pub p2: NesControllerMapping,
    #[cfg_display(indent_nested)]
    pub p1_turbo: NesControllerMapping,
    #[cfg_display(indent_nested)]
    pub p2_turbo: NesControllerMapping,
    #[cfg_display(indent_nested)]
    pub zapper: NesZapperMapping,
}

impl NesInputMapping {
    impl_player_mapping!(NesControllerMapping);

    pub fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, NesButton>) {
        self.p1.to_mapping_vec(Player::One, out);
        self.p2.to_mapping_vec(Player::Two, out);
        self.zapper.to_mapping_vec(Player::One, out);
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum NesControllerType {
    #[default]
    Gamepad,
    Zapper,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct NesInputConfig {
    pub p2_type: NesControllerType,
    #[cfg_display(indent_nested)]
    pub mapping_1: NesInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: NesInputMapping,
}

impl NesInputConfig {
    impl_to_mapping_vec!(NesButton);

    impl_to_turbo_mapping_vec!(NesButton);
}

fn default_nes_mapping_1() -> NesInputMapping {
    NesInputMapping {
        p1: NesControllerMapping::keyboard_arrows(),
        p2: NesControllerMapping::default(),
        p1_turbo: NesControllerMapping::default(),
        p2_turbo: NesControllerMapping::default(),
        zapper: NesZapperMapping::mouse(),
    }
}

impl Default for NesInputConfig {
    fn default() -> Self {
        Self {
            p2_type: NesControllerType::default(),
            mapping_1: default_nes_mapping_1(),
            mapping_2: NesInputMapping::default(),
        }
    }
}

define_controller_mapping!(SnesControllerMapping, SnesButton, [
    up: Up,
    left: Left,
    right: Right,
    down: Down,
    a: A,
    b: B,
    x: X,
    y: Y,
    l: L,
    r: R,
    start: Start,
    select: Select,
]);

impl SnesControllerMapping {
    #[must_use]
    pub fn keyboard_arrows() -> Self {
        Self {
            up: key_input!(Up),
            left: key_input!(Left),
            right: key_input!(Right),
            down: key_input!(Down),
            a: key_input!(S),
            b: key_input!(X),
            x: key_input!(A),
            y: key_input!(Z),
            l: key_input!(D),
            r: key_input!(C),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }

    #[must_use]
    pub fn keyboard_wasd() -> Self {
        Self {
            up: key_input!(W),
            left: key_input!(A),
            right: key_input!(D),
            down: key_input!(S),
            a: key_input!(L),
            b: key_input!(K),
            x: key_input!(I),
            y: key_input!(J),
            l: key_input!(U),
            r: key_input!(O),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }
}

define_controller_mapping!(SnesSuperScopeMapping, SnesButton, [
    fire: SuperScopeFire,
    cursor: SuperScopeCursor,
    pause: SuperScopePause,
    turbo_toggle: SuperScopeTurboToggle,
]);

impl SnesSuperScopeMapping {
    #[must_use]
    pub fn mouse() -> Self {
        Self {
            fire: Some(vec![GenericInput::Mouse(MouseButton::Left)]),
            cursor: Some(vec![GenericInput::Mouse(MouseButton::Right)]),
            pause: Some(vec![GenericInput::Mouse(MouseButton::Middle)]),
            turbo_toggle: key_input!(T),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct SnesInputMapping {
    #[cfg_display(indent_nested)]
    pub p1: SnesControllerMapping,
    #[cfg_display(indent_nested)]
    pub p2: SnesControllerMapping,
    #[cfg_display(indent_nested)]
    pub p1_turbo: SnesControllerMapping,
    #[cfg_display(indent_nested)]
    pub p2_turbo: SnesControllerMapping,
    #[cfg_display(indent_nested)]
    pub super_scope: SnesSuperScopeMapping,
}

impl SnesInputMapping {
    impl_player_mapping!(SnesControllerMapping);

    pub fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, SnesButton>) {
        self.p1.to_mapping_vec(Player::One, out);
        self.p2.to_mapping_vec(Player::Two, out);
        self.super_scope.to_mapping_vec(Player::One, out);
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum SnesControllerType {
    #[default]
    Gamepad,
    SuperScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct SnesInputConfig {
    pub p2_type: SnesControllerType,
    #[cfg_display(indent_nested)]
    pub mapping_1: SnesInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: SnesInputMapping,
}

impl SnesInputConfig {
    impl_to_mapping_vec!(SnesButton);

    impl_to_turbo_mapping_vec!(SnesButton);
}

fn default_snes_mapping_1() -> SnesInputMapping {
    SnesInputMapping {
        p1: SnesControllerMapping::keyboard_arrows(),
        p2: SnesControllerMapping::default(),
        p1_turbo: SnesControllerMapping::default(),
        p2_turbo: SnesControllerMapping::default(),
        super_scope: SnesSuperScopeMapping::mouse(),
    }
}

impl Default for SnesInputConfig {
    fn default() -> Self {
        Self {
            p2_type: SnesControllerType::default(),
            mapping_1: default_snes_mapping_1(),
            mapping_2: SnesInputMapping::default(),
        }
    }
}

define_controller_mapping!(GameBoyInputMapping, GameBoyButton, [
    up: Up,
    left: Left,
    right: Right,
    down: Down,
    a: A,
    b: B,
    start: Start,
    select: Select,
]);

impl GameBoyInputMapping {
    #[must_use]
    pub fn keyboard_arrows() -> Self {
        Self {
            up: key_input!(Up),
            left: key_input!(Left),
            right: key_input!(Right),
            down: key_input!(Down),
            a: key_input!(A),
            b: key_input!(S),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }

    #[must_use]
    pub fn keyboard_wasd() -> Self {
        Self {
            up: key_input!(W),
            left: key_input!(A),
            right: key_input!(D),
            down: key_input!(S),
            a: key_input!(L),
            b: key_input!(K),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct GameBoyInputConfig {
    #[cfg_display(indent_nested)]
    pub mapping_1: GameBoyInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: GameBoyInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_1_turbo: GameBoyInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2_turbo: GameBoyInputMapping,
}

impl GameBoyInputConfig {
    #[must_use]
    pub fn to_mapping_vec(&self) -> ButtonMappingVec<'_, GameBoyButton> {
        let mut out = Vec::new();

        self.mapping_1.to_mapping_vec(Player::One, &mut out);
        self.mapping_2.to_mapping_vec(Player::One, &mut out);

        out
    }

    #[must_use]
    pub fn to_turbo_mapping_vec(&self) -> ButtonMappingVec<'_, GameBoyButton> {
        let mut out = Vec::new();

        self.mapping_1_turbo.to_mapping_vec(Player::One, &mut out);
        self.mapping_2_turbo.to_mapping_vec(Player::One, &mut out);

        out
    }
}

fn default_gb_mapping_1() -> GameBoyInputMapping {
    GameBoyInputMapping::keyboard_arrows()
}

impl Default for GameBoyInputConfig {
    fn default() -> Self {
        Self {
            mapping_1: default_gb_mapping_1(),
            mapping_2: GameBoyInputMapping::default(),
            mapping_1_turbo: GameBoyInputMapping::default(),
            mapping_2_turbo: GameBoyInputMapping::default(),
        }
    }
}

define_controller_mapping!(GbaJoypadMapping, GbaButton, [
    up: Up,
    left: Left,
    right: Right,
    down: Down,
    a: A,
    b: B,
    l: L,
    r: R,
    start: Start,
    select: Select,
]);

impl GbaJoypadMapping {
    #[must_use]
    pub fn keyboard_arrows() -> Self {
        Self {
            up: key_input!(Up),
            left: key_input!(Left),
            right: key_input!(Right),
            down: key_input!(Down),
            a: key_input!(A),
            b: key_input!(S),
            l: key_input!(Q),
            r: key_input!(W),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }

    #[must_use]
    pub fn keyboard_wasd() -> Self {
        Self {
            up: key_input!(W),
            left: key_input!(A),
            right: key_input!(D),
            down: key_input!(S),
            a: key_input!(L),
            b: key_input!(K),
            l: key_input!(I),
            r: key_input!(O),
            start: key_input!(Return),
            select: key_input!(RShift),
        }
    }
}

define_controller_mapping!(GbaSolarMapping, GbaButton, [
    increase_brightness: SolarIncreaseBrightness,
    decrease_brightness: SolarDecreaseBrightness,
    min_brightness: SolarMinBrightness,
    max_brightness: SolarMaxBrightness,
]);

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct GbaInputMapping {
    #[cfg_display(indent_nested)]
    pub joypad: GbaJoypadMapping,
    #[cfg_display(indent_nested)]
    pub solar: GbaSolarMapping,
}

impl GbaInputMapping {
    fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, GbaButton>) {
        self.joypad.to_mapping_vec(Player::One, out);
        self.solar.to_mapping_vec(Player::One, out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct GbaInputConfig {
    #[cfg_display(indent_nested)]
    pub mapping_1: GbaInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: GbaInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_1_turbo: GbaInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2_turbo: GbaInputMapping,
}

impl GbaInputConfig {
    #[must_use]
    pub fn to_mapping_vec(&self) -> ButtonMappingVec<'_, GbaButton> {
        let mut out = Vec::new();

        self.mapping_1.to_mapping_vec(&mut out);
        self.mapping_2.to_mapping_vec(&mut out);

        out
    }

    #[must_use]
    pub fn to_turbo_mapping_vec(&self) -> ButtonMappingVec<'_, GbaButton> {
        let mut out = Vec::new();

        self.mapping_1_turbo.to_mapping_vec(&mut out);
        self.mapping_2_turbo.to_mapping_vec(&mut out);

        out
    }
}

fn default_gba_mapping_1() -> GbaInputMapping {
    GbaInputMapping {
        joypad: GbaJoypadMapping::keyboard_arrows(),
        solar: GbaSolarMapping::default(),
    }
}

impl Default for GbaInputConfig {
    fn default() -> Self {
        Self {
            mapping_1: default_gba_mapping_1(),
            mapping_2: GbaInputMapping::default(),
            mapping_1_turbo: GbaInputMapping::default(),
            mapping_2_turbo: GbaInputMapping::default(),
        }
    }
}

define_controller_mapping!(PceJoypadMapping, PceButton, [
    up: Up,
    left: Left,
    right: Right,
    down: Down,
    button1: Button1,
    button2: Button2,
    run: Run,
    select: Select,
]);

impl PceJoypadMapping {
    #[must_use]
    pub fn keyboard_arrows() -> Self {
        Self {
            up: key_input!(Up),
            left: key_input!(Left),
            right: key_input!(Right),
            down: key_input!(Down),
            button1: key_input!(A),
            button2: key_input!(S),
            run: key_input!(Return),
            select: key_input!(RShift),
        }
    }

    #[must_use]
    pub fn keyboard_wasd() -> Self {
        Self {
            up: key_input!(W),
            left: key_input!(A),
            right: key_input!(S),
            down: key_input!(D),
            button1: key_input!(L),
            button2: key_input!(K),
            run: key_input!(Return),
            select: key_input!(RShift),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct PceInputMapping {
    #[cfg_display(indent_nested)]
    pub p1: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p2: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p3: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p4: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p5: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p1_turbo: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p2_turbo: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p3_turbo: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p4_turbo: PceJoypadMapping,
    #[cfg_display(indent_nested)]
    pub p5_turbo: PceJoypadMapping,
}

impl PceInputMapping {
    impl_player_mapping!(
        PceJoypadMapping,
        [
            (One, p1, p1_turbo),
            (Two, p2, p2_turbo),
            (Three, p3, p3_turbo),
            (Four, p4, p4_turbo),
            (Five, p5, p5_turbo),
        ]
    );

    pub fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, PceButton>) {
        self.p1.to_mapping_vec(Player::One, out);
        self.p2.to_mapping_vec(Player::Two, out);
        self.p3.to_mapping_vec(Player::Three, out);
        self.p4.to_mapping_vec(Player::Four, out);
        self.p5.to_mapping_vec(Player::Five, out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct PceInputConfig {
    #[cfg_display(indent_nested)]
    pub mapping_1: PceInputMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: PceInputMapping,
}

impl PceInputConfig {
    impl_to_mapping_vec!(PceButton);

    impl_to_turbo_mapping_vec!(
        PceButton,
        [(One, p1_turbo), (Two, p2_turbo), (Three, p3_turbo), (Four, p4_turbo), (Five, p5_turbo)]
    );
}

impl Default for PceInputConfig {
    fn default() -> Self {
        Self {
            mapping_1: PceInputMapping {
                p1: PceJoypadMapping::keyboard_arrows(),
                p2: PceJoypadMapping::default(),
                p3: PceJoypadMapping::default(),
                p4: PceJoypadMapping::default(),
                p5: PceJoypadMapping::default(),
                p1_turbo: PceJoypadMapping::default(),
                p2_turbo: PceJoypadMapping::default(),
                p3_turbo: PceJoypadMapping::default(),
                p4_turbo: PceJoypadMapping::default(),
                p5_turbo: PceJoypadMapping::default(),
            },
            mapping_2: PceInputMapping::default(),
        }
    }
}

macro_rules! define_hotkey_mapping {
    (@default none) => {
        None
    };
    (@default $default:ident) => {
        Some(vec![GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::$default))])
    };
    (@default ($($default:ident $(+)?)*)) => {
        Some(vec![
            $(
                GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::$default)),
            )*
        ])
    };
    ($($value:ident: $hotkey:ident $label:literal default $default:tt),* $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
        pub struct HotkeyMapping {
            $(
                pub $value: Option<Vec<GenericInput>>,
            )*
        }

        impl HotkeyMapping {
            #[must_use]
            pub fn default_keyboard() -> Self {
                Self {
                    $(
                        $value: define_hotkey_mapping!(@default $default),
                    )*
                }
            }

            pub fn to_mapping_vec<'a>(&'a self, out: &mut HotkeyMappingVec<'a>) {
                $(
                    if let Some(mapping) = &self.$value {
                        out.push((Hotkey::$hotkey, mapping));
                    }
                )*
            }

            #[must_use]
            pub fn access_value(&mut self, hotkey: Hotkey) -> &mut Option<Vec<GenericInput>> {
                match hotkey {
                    $(
                        Hotkey::$hotkey => &mut self.$value,
                    )*
                }
            }
        }

        impl std::fmt::Display for HotkeyMapping {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                fmt_input_mapping(&[
                    $(
                        (Hotkey::$hotkey, &self.$value),
                    )*
                ], f)
            }
        }

        impl Hotkey {
            #[must_use]
            pub fn label(self) -> &'static str {
                match self {
                    $(
                        Self::$hotkey => $label,
                    )*
                }
            }
        }
    };
}

define_hotkey_mapping!(
    power_off: PowerOff "Power off emulated system" default Escape,
    exit: Exit "Exit application" default (LCtrl + Q),
    toggle_fullscreen: ToggleFullscreen "Toggle fullscreen" default F9,
    save_state: SaveState "Save state to current slot" default F5,
    load_state: LoadState "Load state from current slot" default F6,
    next_save_state_slot: NextSaveStateSlot "Next save state slot" default RightBracket,
    prev_save_state_slot: PrevSaveStateSlot "Previous save state slot" default LeftBracket,
    soft_reset: SoftReset "Soft reset" default F1,
    hard_reset: HardReset "Hard reset" default F2,
    pause: Pause "Pause" default P,
    step_frame: StepFrame "Step to next frame" default N,
    fast_forward: FastForward "Fast forward" default Tab,
    rewind: Rewind "Rewind" default Grave,
    toggle_overclocking: ToggleOverclocking "Toggle overclocking enabled" default Semicolon,
    open_debugger: OpenDebugger "Open memory viewer" default Apostrophe,
    save_state_slot_0: SaveStateSlot0 "Save state to slot 0" default none,
    save_state_slot_1: SaveStateSlot1 "Save state to slot 1" default none,
    save_state_slot_2: SaveStateSlot2 "Save state to slot 2" default none,
    save_state_slot_3: SaveStateSlot3 "Save state to slot 3" default none,
    save_state_slot_4: SaveStateSlot4 "Save state to slot 4" default none,
    save_state_slot_5: SaveStateSlot5 "Save state to slot 5" default none,
    save_state_slot_6: SaveStateSlot6 "Save state to slot 6" default none,
    save_state_slot_7: SaveStateSlot7 "Save state to slot 7" default none,
    save_state_slot_8: SaveStateSlot8 "Save state to slot 8" default none,
    save_state_slot_9: SaveStateSlot9 "Save state to slot 9" default none,
    load_state_slot_0: LoadStateSlot0 "Load state from slot 0" default none,
    load_state_slot_1: LoadStateSlot1 "Load state from slot 1" default none,
    load_state_slot_2: LoadStateSlot2 "Load state from slot 2" default none,
    load_state_slot_3: LoadStateSlot3 "Load state from slot 3" default none,
    load_state_slot_4: LoadStateSlot4 "Load state from slot 4" default none,
    load_state_slot_5: LoadStateSlot5 "Load state from slot 5" default none,
    load_state_slot_6: LoadStateSlot6 "Load state from slot 6" default none,
    load_state_slot_7: LoadStateSlot7 "Load state from slot 7" default none,
    load_state_slot_8: LoadStateSlot8 "Load state from slot 8" default none,
    load_state_slot_9: LoadStateSlot9 "Load state from slot 9" default none,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
#[serde(default)]
pub struct HotkeyConfig {
    #[cfg_display(indent_nested)]
    pub mapping_1: HotkeyMapping,
    #[cfg_display(indent_nested)]
    pub mapping_2: HotkeyMapping,
}

impl HotkeyConfig {
    #[must_use]
    pub fn to_mapping_vec(&self) -> HotkeyMappingVec<'_> {
        let mut out = Vec::new();

        self.mapping_1.to_mapping_vec(&mut out);
        self.mapping_2.to_mapping_vec(&mut out);

        out
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self { mapping_1: HotkeyMapping::default_keyboard(), mapping_2: HotkeyMapping::default() }
    }
}
