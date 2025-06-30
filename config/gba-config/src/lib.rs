use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::MappableInputs;
use jgenesis_common::input::Player;

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
    joypad: GbaInputs,
}

impl MappableInputs<GbaButton> for GbaInputs {
    fn set_field(&mut self, button: GbaButton, _player: Player, pressed: bool) {
        self.set_button(button, pressed);
    }
}
