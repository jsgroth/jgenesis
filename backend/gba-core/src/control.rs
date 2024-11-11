use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Encode, Decode)]
pub struct ControlRegisters {
    // IME: Interrupt master enable flag
    pub ime: bool,
}

impl ControlRegisters {
    pub fn new() -> Self {
        Self { ime: false }
    }

    pub fn write_ime(&mut self, value: u32) {
        self.ime = value.bit(0);

        log::trace!("IME: {}", self.ime);
    }
}
