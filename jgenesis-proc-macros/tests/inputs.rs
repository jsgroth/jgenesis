use jgenesis_common::input::Player;
use jgenesis_proc_macros::define_controller_inputs;

define_controller_inputs! {
    button_ident: SmsGgButton,
    joypad_ident: SmsGgJoypadState,
    inputs_ident: SmsGgInputs,
    buttons: [Up, Left, Right, Down, Button1, Button2],
    console_buttons: [Pause],
    inputs: {
        p1: (Player One),
        p2: (Player Two),
        pause: (Button Pause),
    },
}

#[test]
fn button_enum() {
    assert_eq!(SmsGgButton::Button1, SmsGgButton::Button1);
    assert_eq!(SmsGgButton::Pause, SmsGgButton::Pause);

    assert_eq!(SmsGgButton::ALL.len(), 7);
}

#[test]
fn joypad_struct() {
    let mut joypad_state = SmsGgJoypadState::default();
    joypad_state.set_button(SmsGgButton::Button2, true);
    assert!(joypad_state.button2);

    let joypad_state =
        joypad_state.with_button(SmsGgButton::Left, true).with_button(SmsGgButton::Button2, false);
    assert!(joypad_state.left);
    assert!(!joypad_state.button2);

    assert_eq!(joypad_state, joypad_state.with_button(SmsGgButton::Pause, true));
}

#[test]
fn inputs_struct() {
    let mut inputs = SmsGgInputs::default();
    inputs.set_button(SmsGgButton::Pause, Player::One, true);
    assert!(inputs.pause);

    inputs = inputs.with_button(SmsGgButton::Pause, Player::Two, false);
    assert!(!inputs.pause);

    inputs.set_button(SmsGgButton::Left, Player::Two, true);
    assert!(inputs.p2.left);

    inputs = inputs.with_button(SmsGgButton::Button2, Player::One, true);
    assert!(inputs.p1.button2);
    assert!(inputs.p2.left);
}
