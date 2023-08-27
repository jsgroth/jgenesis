use genesis_core::GenesisInputs;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;

fn get_field(inputs: &mut GenesisInputs, keycode: Keycode) -> Option<&mut bool> {
    let field = match keycode {
        Keycode::Up => &mut inputs.p1.up,
        Keycode::Left => &mut inputs.p1.left,
        Keycode::Right => &mut inputs.p1.right,
        Keycode::Down => &mut inputs.p1.down,
        Keycode::A => &mut inputs.p1.a,
        Keycode::S => &mut inputs.p1.b,
        Keycode::D => &mut inputs.p1.c,
        Keycode::Return => &mut inputs.p1.start,
        _ => {
            return None;
        }
    };

    Some(field)
}

pub fn update_inputs(event: &Event, inputs: &mut GenesisInputs) {
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
