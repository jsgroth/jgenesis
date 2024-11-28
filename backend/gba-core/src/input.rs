//! GBA input model

use jgenesis_proc_macros::define_controller_inputs;

define_controller_inputs! {
    enum GbaButton {
        Up,
        Left,
        Right,
        Down,
        A,
        B,
        L,
        R,
        Start,
        Select,
    }

    struct GbaInputs {
        buttons!
    }
}
