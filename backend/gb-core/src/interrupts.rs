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
        self.flags |= interrupt_type.register_mask();
    }

    pub fn clear_flag(&mut self, interrupt_type: InterruptType) {
        self.flags &= !interrupt_type.register_mask();
    }

    pub fn highest_priority_interrupt(&self) -> Option<InterruptType> {
        let interrupts_triggered = self.enabled & self.flags;
        (interrupts_triggered != 0)
            .then(|| {
                InterruptType::ALL.into_iter().find(|&interrupt_type| {
                    interrupts_triggered & interrupt_type.register_mask() != 0
                })
            })
            .flatten()
    }
}
