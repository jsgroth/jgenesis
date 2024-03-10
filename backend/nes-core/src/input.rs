use bincode::{Decode, Encode};
use jgenesis_proc_macros::define_controller_inputs;

define_controller_inputs! {
    button_ident: NesButton,
    joypad_ident: NesJoypadState,
    inputs_ident: NesInputs,
    buttons: [Up, Left, Right, Down, A, B, Start, Select],
    inputs: {
        p1: (Player One),
        p2: (Player Two),
    },
}

impl NesJoypadState {
    /// Prevent left+right or up+down from being pressed simultaneously from the NES's perspective.
    ///
    /// If left+right are pressed simultaneously, left will be preferred.
    /// If up+down are pressed simultaneously, up will be preferred.
    #[must_use]
    pub(crate) fn sanitize_opposing_directions(self) -> Self {
        let mut sanitized = self;

        if self.left && self.right {
            // Arbitrarily prefer left
            sanitized.right = false;
        }

        if self.up && self.down {
            // Arbitrarily prefer up
            sanitized.down = false;
        }

        sanitized
    }

    pub(crate) fn latch(self) -> LatchedJoypadState {
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

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub(crate) struct LatchedJoypadState(u8);

impl LatchedJoypadState {
    pub fn next_bit(self) -> u8 {
        self.0 & 0x01
    }

    #[must_use]
    pub fn shift(self) -> Self {
        Self((self.0 >> 1) | 0x80)
    }
}
