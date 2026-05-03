use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::MappableInputs;
use jgenesis_common::input::Player;

define_controller_inputs! {
    buttons: PceButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        Button1 -> button1,
        Button2 -> button2,
        Run -> run,
        Select -> select,
    },
    joypad: PceInputs,
}

impl MappableInputs<PceButton> for PceInputs {
    fn set_field(&mut self, button: PceButton, _player: Player, pressed: bool) {
        self.set_button(button, pressed);
    }
}
