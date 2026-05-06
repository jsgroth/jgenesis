use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use pce_config::PceInputs;

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    inputs: PceInputs,
    select_pin: bool,
    clear_pin: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self { inputs: PceInputs::default(), select_pin: false, clear_pin: false }
    }

    pub fn update_inputs(&mut self, inputs: PceInputs) {
        self.inputs = inputs;
    }

    pub fn read_port(&mut self) -> u8 {
        let data = match (self.select_pin, self.clear_pin) {
            (false, false) => {
                [self.inputs.button1, self.inputs.button2, self.inputs.select, self.inputs.run]
            }
            (true, false) => {
                [self.inputs.up, self.inputs.right, self.inputs.down, self.inputs.left]
            }
            (_, true) => [true; 4],
        };

        // Bit 7: CD-ROM present (1 = not attached)
        // Bit 6: Region (0 = TG16)
        // Bits 5 and 4 always read 1
        let value = (1 << 7)
            | (1 << 5)
            | (1 << 4)
            | (u8::from(!data[3]) << 3)
            | (u8::from(!data[2]) << 2)
            | (u8::from(!data[1]) << 1)
            | u8::from(!data[0]);

        log::trace!(
            "I/O port read; SEL={} CLR={}, value={value:02X}",
            u8::from(self.select_pin),
            u8::from(self.clear_pin)
        );

        value
    }

    pub fn write_port(&mut self, value: u8) {
        self.clear_pin = value.bit(1);
        self.select_pin = value.bit(0);

        log::trace!(
            "I/O port write: {value:02X} (SEL={} CLR={})",
            u8::from(self.select_pin),
            u8::from(self.clear_pin)
        );
    }
}
