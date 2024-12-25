#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Player {
    One,
    Two,
}

#[inline]
#[must_use]
pub fn viewport_position_to_frame_position(
    x: i32,
    y: i32,
    frame_size: FrameSize,
    display_area: DisplayArea,
) -> Option<(u16, u16)> {
    let display_left = display_area.x as i32;
    let display_right = display_left + display_area.width as i32;
    let display_top = display_area.y as i32;
    let display_bottom = display_top + display_area.height as i32;

    if !(display_left..display_right).contains(&x) || !(display_top..display_bottom).contains(&y) {
        return None;
    }

    let x: f64 = x.into();
    let y: f64 = y.into();
    let display_left: f64 = display_left.into();
    let display_width: f64 = display_area.width.into();
    let frame_width: f64 = frame_size.width.into();
    let display_top: f64 = display_top.into();
    let display_height: f64 = display_area.height.into();
    let frame_height: f64 = frame_size.height.into();

    let frame_x = ((x - display_left) * frame_width / display_width).round() as u16;
    let frame_y = ((y - display_top) * frame_height / display_height).round() as u16;

    log::trace!(
        "Mapped mouse position ({x}, {y}) to ({frame_x}, {frame_y}) (frame size {frame_size:?}, display_area {display_area:?})"
    );

    Some((frame_x, frame_y))
}

#[macro_export]
macro_rules! define_controller_inputs {
    (
        buttons: $button_enum:ident {
            $($button:ident -> $button_field:ident),* $(,)?
        }
        $(, non_gamepad_buttons: [$($non_gamepad_button:ident),* $(,)?])?
        , joypad: $joypad_struct:ident
        $(
            , inputs: $inputs_struct:ident {
                players: {
                    $($player_field:ident: Player::$player_value:ident),* $(,)?
                }
                $(, buttons: [$($ex_button:ident -> $ex_button_field:ident),* $(,)?])?
                $(,)?
            }
        )?
        $(,)?
    ) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            ::std::hash::Hash,
            ::bincode::Encode,
            ::bincode::Decode,
            ::jgenesis_proc_macros::EnumAll,
            ::jgenesis_proc_macros::EnumDisplay,
            ::jgenesis_proc_macros::EnumFromStr,
        )]
        pub enum $button_enum {
            $(
                $button,
            )*
            $($(
                $non_gamepad_button,
            )*)?
        }

        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Default,
            ::std::hash::Hash,
            ::bincode::Encode,
            ::bincode::Decode,
        )]
        pub struct $joypad_struct {
            $(
                pub $button_field: bool,
            )*
        }

        impl $joypad_struct {
            #[inline]
            pub fn set_button(&mut self, button: $button_enum, pressed: bool) {
                match button {
                    $(
                        $button_enum::$button => self.$button_field = pressed,
                    )*
                    $(
                        $(
                            $button_enum::$non_gamepad_button => {}
                        )*
                    )?
                }
            }

            #[inline]
            pub fn with_button(mut self, button: $button_enum, pressed: bool) -> Self {
                self.set_button(button, pressed);
                self
            }
        }

        $(
            #[derive(
                Debug,
                Clone,
                Copy,
                PartialEq,
                Eq,
                Default,
                ::std::hash::Hash,
                ::bincode::Encode,
                ::bincode::Decode,
            )]
            pub struct $inputs_struct {
                $(
                    pub $player_field: $joypad_struct,
                )*
                $($(
                    pub $ex_button_field: bool,
                )*)?
            }

            impl ::jgenesis_common::frontend::MappableInputs<$button_enum> for $inputs_struct {
                #[inline]
                fn set_field(
                    &mut self,
                    button: $button_enum,
                    player: ::jgenesis_common::input::Player,
                    pressed: bool,
                ) {
                    match (button, player) {
                        $($(
                            ($button_enum::$ex_button, _) => {
                                self.$ex_button_field = pressed;
                            }
                        )*)?
                        $(
                            (button, ::jgenesis_common::input::Player::$player_value) => {
                                self.$player_field.set_button(button, pressed);
                            }
                        )*
                    }
                }
            }
        )?
    }
}

use crate::frontend::{DisplayArea, FrameSize};
pub use define_controller_inputs;
