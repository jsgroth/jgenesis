use crate::input::{GenericInput, Hotkey};
use gb_config::GameBoyButton;
use gba_config::GbaButton;
use genesis_config::{GenesisButton, GenesisControllerType};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{ConfigDisplay, EnumAll, EnumDisplay};
use nes_config::NesButton;
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use serde::{Deserialize, Serialize};
use smsgg_config::SmsGgButton;
use snes_config::SnesButton;
use std::fmt::Formatter;

pub type ButtonMappingVec<'a, Button> = Vec<((Button, Player), &'a Vec<GenericInput>)>;
pub type HotkeyMappingVec<'a> = Vec<(Hotkey, &'a Vec<GenericInput>)>;

macro_rules! key_input {
    ($key:ident) => {
        Some(vec![GenericInput::Keyboard(Keycode::$key)])
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
pub struct SmsGgInputMapping {
    #[serde(default)]
    pub p1: SmsGgControllerMapping,
    #[serde(default)]
    pub p2: SmsGgControllerMapping,
    #[cfg_display(debug_fmt)]
    pub pause: Option<Vec<GenericInput>>,
}

impl SmsGgInputMapping {
    pub fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, SmsGgButton>) {
        self.p1.to_mapping_vec(Player::One, out);
        self.p2.to_mapping_vec(Player::Two, out);

        if let Some(pause) = &self.pause {
            out.push(((SmsGgButton::Pause, Player::One), pause));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
pub struct SmsGgInputConfig {
    #[serde(default = "default_smsgg_mapping_1")]
    #[cfg_display(indent_nested)]
    pub mapping_1: SmsGgInputMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub mapping_2: SmsGgInputMapping,
}

impl SmsGgInputConfig {
    impl_to_mapping_vec!(SmsGgButton);
}

fn default_smsgg_mapping_1() -> SmsGgInputMapping {
    SmsGgInputMapping {
        p1: SmsGgControllerMapping::keyboard_arrows(),
        p2: SmsGgControllerMapping::default(),
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ConfigDisplay)]
pub struct GenesisInputMapping {
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub p1: GenesisControllerMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub p2: GenesisControllerMapping,
}

impl GenesisInputMapping {
    pub fn to_mapping_vec<'a>(&'a self, out: &mut ButtonMappingVec<'a, GenesisButton>) {
        self.p1.to_mapping_vec(Player::One, out);
        self.p2.to_mapping_vec(Player::Two, out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
pub struct GenesisInputConfig {
    #[serde(default)]
    pub p1_type: GenesisControllerType,
    #[serde(default)]
    pub p2_type: GenesisControllerType,
    #[serde(default = "default_genesis_mapping_1")]
    #[cfg_display(indent_nested)]
    pub mapping_1: GenesisInputMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub mapping_2: GenesisInputMapping,
}

impl GenesisInputConfig {
    impl_to_mapping_vec!(GenesisButton);
}

fn default_genesis_mapping_1() -> GenesisInputMapping {
    GenesisInputMapping {
        p1: GenesisControllerMapping::keyboard_arrows(),
        p2: GenesisControllerMapping::default(),
    }
}

impl Default for GenesisInputConfig {
    fn default() -> Self {
        Self {
            p1_type: GenesisControllerType::default(),
            p2_type: GenesisControllerType::default(),
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
pub struct NesInputMapping {
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub p1: NesControllerMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub p2: NesControllerMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub zapper: NesZapperMapping,
}

impl NesInputMapping {
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
pub struct NesInputConfig {
    #[serde(default)]
    pub p2_type: NesControllerType,
    #[serde(default = "default_nes_mapping_1")]
    #[cfg_display(indent_nested)]
    pub mapping_1: NesInputMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub mapping_2: NesInputMapping,
}

impl NesInputConfig {
    impl_to_mapping_vec!(NesButton);
}

fn default_nes_mapping_1() -> NesInputMapping {
    NesInputMapping {
        p1: NesControllerMapping::keyboard_arrows(),
        p2: NesControllerMapping::default(),
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
pub struct SnesInputMapping {
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub p1: SnesControllerMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub p2: SnesControllerMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub super_scope: SnesSuperScopeMapping,
}

impl SnesInputMapping {
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
pub struct SnesInputConfig {
    #[serde(default)]
    pub p2_type: SnesControllerType,
    #[serde(default = "default_snes_mapping_1")]
    #[cfg_display(indent_nested)]
    pub mapping_1: SnesInputMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub mapping_2: SnesInputMapping,
}

impl SnesInputConfig {
    impl_to_mapping_vec!(SnesButton);
}

fn default_snes_mapping_1() -> SnesInputMapping {
    SnesInputMapping {
        p1: SnesControllerMapping::keyboard_arrows(),
        p2: SnesControllerMapping::default(),
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
pub struct GameBoyInputConfig {
    #[serde(default = "default_gb_mapping_1")]
    #[cfg_display(indent_nested)]
    pub mapping_1: GameBoyInputMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub mapping_2: GameBoyInputMapping,
}

impl GameBoyInputConfig {
    #[must_use]
    pub fn to_mapping_vec(&self) -> ButtonMappingVec<'_, GameBoyButton> {
        let mut out = Vec::new();

        self.mapping_1.to_mapping_vec(Player::One, &mut out);
        self.mapping_2.to_mapping_vec(Player::One, &mut out);

        out
    }
}

fn default_gb_mapping_1() -> GameBoyInputMapping {
    GameBoyInputMapping::keyboard_arrows()
}

impl Default for GameBoyInputConfig {
    fn default() -> Self {
        Self { mapping_1: default_gb_mapping_1(), mapping_2: GameBoyInputMapping::default() }
    }
}

define_controller_mapping!(GbaInputMapping, GbaButton, [
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

impl GbaInputMapping {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
pub struct GbaInputConfig {
    #[serde(default = "default_gba_mapping_1")]
    #[cfg_display(indent_nested)]
    pub mapping_1: GbaInputMapping,
    #[serde(default)]
    #[cfg_display(indent_nested)]
    pub mapping_2: GbaInputMapping,
}

impl GbaInputConfig {
    #[must_use]
    pub fn to_mapping_vec(&self) -> ButtonMappingVec<'_, GbaButton> {
        let mut out = Vec::new();

        self.mapping_1.to_mapping_vec(Player::One, &mut out);
        self.mapping_2.to_mapping_vec(Player::One, &mut out);

        out
    }
}

fn default_gba_mapping_1() -> GbaInputMapping {
    GbaInputMapping::keyboard_arrows()
}

impl Default for GbaInputConfig {
    fn default() -> Self {
        Self { mapping_1: default_gba_mapping_1(), mapping_2: GbaInputMapping::default() }
    }
}

macro_rules! define_hotkey_mapping {
    (@default none) => {
        None
    };
    (@default $default:ident) => {
        Some(vec![GenericInput::Keyboard(Keycode::$default)])
    };
    (@default ($($default:ident $(+)?)*)) => {
        Some(vec![
            $(
                GenericInput::Keyboard(Keycode::$default),
            )*
        ])
    };
    ($($value:ident: $hotkey:ident default $default:tt,)* $(,)?) => {
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
    };
}

define_hotkey_mapping!(
    power_off: PowerOff default Escape,
    exit: Exit default (LCtrl + Q),
    toggle_fullscreen: ToggleFullscreen default F9,
    save_state: SaveState default F5,
    load_state: LoadState default F6,
    next_save_state_slot: NextSaveStateSlot default RightBracket,
    prev_save_state_slot: PrevSaveStateSlot default LeftBracket,
    soft_reset: SoftReset default F1,
    hard_reset: HardReset default F2,
    pause: Pause default P,
    step_frame: StepFrame default N,
    fast_forward: FastForward default Tab,
    rewind: Rewind default Backquote,
    toggle_overclocking: ToggleOverclocking default Semicolon,
    open_debugger: OpenDebugger default Quote,
    save_state_slot_0: SaveStateSlot0 default none,
    save_state_slot_1: SaveStateSlot1 default none,
    save_state_slot_2: SaveStateSlot2 default none,
    save_state_slot_3: SaveStateSlot3 default none,
    save_state_slot_4: SaveStateSlot4 default none,
    save_state_slot_5: SaveStateSlot5 default none,
    save_state_slot_6: SaveStateSlot6 default none,
    save_state_slot_7: SaveStateSlot7 default none,
    save_state_slot_8: SaveStateSlot8 default none,
    save_state_slot_9: SaveStateSlot9 default none,
    load_state_slot_0: LoadStateSlot0 default none,
    load_state_slot_1: LoadStateSlot1 default none,
    load_state_slot_2: LoadStateSlot2 default none,
    load_state_slot_3: LoadStateSlot3 default none,
    load_state_slot_4: LoadStateSlot4 default none,
    load_state_slot_5: LoadStateSlot5 default none,
    load_state_slot_6: LoadStateSlot6 default none,
    load_state_slot_7: LoadStateSlot7 default none,
    load_state_slot_8: LoadStateSlot8 default none,
    load_state_slot_9: LoadStateSlot9 default none,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_mapping_1")]
    #[cfg_display(indent_nested)]
    pub mapping_1: HotkeyMapping,
    #[serde(default)]
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

fn default_hotkey_mapping_1() -> HotkeyMapping {
    HotkeyMapping::default_keyboard()
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self { mapping_1: HotkeyMapping::default_keyboard(), mapping_2: HotkeyMapping::default() }
    }
}
