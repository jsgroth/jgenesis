use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct GenesisJoypadState {
    pub up: bool,
    pub left: bool,
    pub right: bool,
    pub down: bool,
    pub a: bool,
    pub b: bool,
    pub c: bool,
    pub start: bool,
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct GenesisInputs {
    pub p1: GenesisJoypadState,
    pub p2: GenesisJoypadState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum InputPinDirection {
    #[default]
    Input,
    Output,
}

impl InputPinDirection {
    fn from_ctrl_bit(bit: bool) -> Self {
        if bit { Self::Output } else { Self::Input }
    }

    fn to_ctrl_bit(self) -> bool {
        self == Self::Output
    }

    fn to_data_bit(self, joypad_bit: bool, data_bit: bool) -> bool {
        match self {
            Self::Input => joypad_bit,
            Self::Output => data_bit,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct PinDirections {
    last_data_write: u8,
    th: InputPinDirection,
    tl: InputPinDirection,
    tr: InputPinDirection,
    right: InputPinDirection,
    left: InputPinDirection,
    down: InputPinDirection,
    up: InputPinDirection,
}

impl PinDirections {
    fn from_ctrl_byte(byte: u8, last_data_write: u8) -> Self {
        let th = InputPinDirection::from_ctrl_bit(byte.bit(6));
        let tl = InputPinDirection::from_ctrl_bit(byte.bit(5));
        let tr = InputPinDirection::from_ctrl_bit(byte.bit(4));
        let right = InputPinDirection::from_ctrl_bit(byte.bit(3));
        let left = InputPinDirection::from_ctrl_bit(byte.bit(2));
        let down = InputPinDirection::from_ctrl_bit(byte.bit(1));
        let up = InputPinDirection::from_ctrl_bit(byte.bit(0));

        Self { last_data_write, th, tl, tr, right, left, down, up }
    }

    fn to_data_byte(self, joypad_state: GenesisJoypadState) -> u8 {
        let th = self.th.to_data_bit(true, self.last_data_write.bit(6));

        let tl_joypad = if th { !joypad_state.c } else { !joypad_state.start };
        let tr_joypad = if th { !joypad_state.b } else { !joypad_state.a };
        let right_joypad = th && !joypad_state.right;
        let left_joypad = th && !joypad_state.left;

        let last_data_write = self.last_data_write;
        (last_data_write & 0x80)
            | (u8::from(th) << 6)
            | (u8::from(self.tl.to_data_bit(tl_joypad, last_data_write.bit(5))) << 5)
            | (u8::from(self.tr.to_data_bit(tr_joypad, last_data_write.bit(4))) << 4)
            | (u8::from(self.right.to_data_bit(right_joypad, last_data_write.bit(3))) << 3)
            | (u8::from(self.left.to_data_bit(left_joypad, last_data_write.bit(2))) << 2)
            | (u8::from(self.down.to_data_bit(!joypad_state.down, last_data_write.bit(1))) << 1)
            | u8::from(self.up.to_data_bit(!joypad_state.up, last_data_write.bit(0)))
    }

    fn to_ctrl_byte(self) -> u8 {
        (u8::from(self.th.to_ctrl_bit()) << 6)
            | (u8::from(self.tl.to_ctrl_bit()) << 5)
            | (u8::from(self.tr.to_ctrl_bit()) << 4)
            | (u8::from(self.right.to_ctrl_bit()) << 3)
            | (u8::from(self.left.to_ctrl_bit()) << 2)
            | (u8::from(self.down.to_ctrl_bit()) << 1)
            | u8::from(self.up.to_ctrl_bit())
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct InputState {
    inputs: GenesisInputs,
    p1_pin_directions: PinDirections,
    p2_pin_directions: PinDirections,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_inputs(&mut self, inputs: &GenesisInputs) {
        self.inputs = inputs.clone();
    }

    pub fn read_p1_data(&self) -> u8 {
        self.p1_pin_directions.to_data_byte(self.inputs.p1)
    }

    pub fn read_p2_data(&self) -> u8 {
        self.p2_pin_directions.to_data_byte(self.inputs.p2)
    }

    pub fn write_p1_data(&mut self, value: u8) {
        self.p1_pin_directions.last_data_write = value;
    }

    pub fn write_p2_data(&mut self, value: u8) {
        self.p2_pin_directions.last_data_write = value;
    }

    pub fn read_p1_ctrl(&self) -> u8 {
        self.p1_pin_directions.to_ctrl_byte()
    }

    pub fn read_p2_ctrl(&self) -> u8 {
        self.p2_pin_directions.to_ctrl_byte()
    }

    pub fn write_p1_ctrl(&mut self, value: u8) {
        self.p1_pin_directions =
            PinDirections::from_ctrl_byte(value, self.p1_pin_directions.last_data_write);
    }

    pub fn write_p2_ctrl(&mut self, value: u8) {
        self.p2_pin_directions =
            PinDirections::from_ctrl_byte(value, self.p2_pin_directions.last_data_write);
    }
}
