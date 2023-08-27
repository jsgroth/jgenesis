use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use smsgg_core::SmsGgInputs;

fn get_field(inputs: &mut SmsGgInputs, keycode: Keycode) -> Option<&mut bool> {
    let field = match keycode {
        Keycode::Up => &mut inputs.p1.up,
        Keycode::Left => &mut inputs.p1.left,
        Keycode::Right => &mut inputs.p1.right,
        Keycode::Down => &mut inputs.p1.down,
        Keycode::A => &mut inputs.p1.button_2,
        Keycode::S => &mut inputs.p1.button_1,
        Keycode::Return => &mut inputs.pause,
        Keycode::Tab => &mut inputs.reset,
        _ => {
            return None;
        }
    };

    Some(field)
}

pub fn update_inputs(event: &Event, inputs: &mut SmsGgInputs) {
    match *event {
        Event::KeyDown {
            keycode: Some(keycode),
            ..
        } => {
            if let Some(field) = get_field(inputs, keycode) {
                *field = true;
            }
        }
        Event::KeyUp {
            keycode: Some(keycode),
            ..
        } => {
            if let Some(field) = get_field(inputs, keycode) {
                *field = false;
            }
        }
        _ => {}
    }
}
