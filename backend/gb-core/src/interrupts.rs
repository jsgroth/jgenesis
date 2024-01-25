//! Game Boy interrupt registers, which didn't seem to naturally fit anywhere else

use crate::sm83::InterruptType;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct InterruptRegisters {
    enabled: u8,
    flags: u8,
}

impl InterruptRegisters {
    pub fn read_ie(&self) -> u8 {
        // TODO should bits 5-7 read 1?
        self.enabled | 0xE0
    }

    pub fn write_ie(&mut self, value: u8) {
        self.enabled = value & 0x1F;
    }

    pub fn read_if(&self) -> u8 {
        self.flags | 0xE0
    }

    pub fn write_if(&mut self, value: u8) {
        self.flags = value & 0x1F;
    }

    pub fn set_flag(&mut self, interrupt_type: InterruptType) {
        log::trace!("Interrupt flag set: {interrupt_type:?}");

        self.flags |= interrupt_type.register_mask();
    }

    pub fn clear_flag(&mut self, interrupt_type: InterruptType) {
        log::trace!("Interrupt flag cleared: {interrupt_type:?}");

        self.flags &= !interrupt_type.register_mask();
    }
}
