use gba_config::GbaInputs;

pub trait GbaInputsExt {
    fn to_keyinput(&self) -> u16;
}

impl GbaInputsExt for GbaInputs {
    // $4000130: KEYINPUT
    fn to_keyinput(&self) -> u16 {
        [
            (self.a, 0),
            (self.b, 1),
            (self.select, 2),
            (self.start, 3),
            (self.right, 4),
            (self.left, 5),
            (self.up, 6),
            (self.down, 7),
            (self.r, 8),
            (self.l, 9),
        ]
        .into_iter()
        .map(|(pressed, bit)| u16::from(!pressed) << bit)
        .reduce(|a, b| a | b)
        .unwrap()
    }
}
