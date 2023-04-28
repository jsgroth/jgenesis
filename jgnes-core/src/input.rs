use sdl2::keyboard::Keycode;

#[derive(Debug, Clone, Copy, Default)]
pub struct JoypadState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub a: bool,
    pub b: bool,
    pub start: bool,
    pub select: bool,
}

impl JoypadState {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_field_mut(&mut self, keycode: Keycode) -> Option<&mut bool> {
        let field = match keycode {
            Keycode::Up => &mut self.up,
            Keycode::Down => &mut self.down,
            Keycode::Left => &mut self.left,
            Keycode::Right => &mut self.right,
            Keycode::Z => &mut self.a,
            Keycode::X => &mut self.b,
            Keycode::Return => &mut self.start,
            Keycode::RShift => &mut self.select,
            _ => return None,
        };

        Some(field)
    }

    pub fn key_down(&mut self, keycode: Keycode) {
        if let Some(field) = self.get_field_mut(keycode) {
            *field = true;
        }

        // Don't allow inputs of opposite directions
        match keycode {
            Keycode::Up => {
                self.down = false;
            }
            Keycode::Down => {
                self.up = false;
            }
            Keycode::Left => {
                self.right = false;
            }
            Keycode::Right => {
                self.left = false;
            }
            _ => {}
        }
    }

    pub fn key_up(&mut self, keycode: Keycode) {
        if let Some(field) = self.get_field_mut(keycode) {
            *field = false;
        }
    }

    pub fn latch(self) -> LatchedJoypadState {
        let bitstream = (u8::from(self.right) << 7)
            | (u8::from(self.left) << 6)
            | (u8::from(self.down) << 5)
            | (u8::from(self.up) << 4)
            | (u8::from(self.start) << 3)
            | (u8::from(self.select) << 2)
            | (u8::from(self.b) << 1)
            | u8::from(self.a);
        LatchedJoypadState(bitstream)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LatchedJoypadState(u8);

impl LatchedJoypadState {
    pub fn next_bit(self) -> u8 {
        self.0 & 0x01
    }

    #[must_use]
    pub fn shift(self) -> Self {
        Self((self.0 >> 1) | 0x80)
    }
}
