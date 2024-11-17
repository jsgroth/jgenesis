use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;

#[derive(Debug, Clone, Encode, Decode)]
pub struct ControlRegisters {
    // IE: Interrupts enabled
    pub interrupts_enabled: u16,
    // IF: Interrupt flags
    pub interrupt_flags: u16,
    // WAITCNT: Waitstate control
    // TODO implement memory access timings
    pub waitcnt: u16,
    // IME: Interrupt master enable flag
    pub ime: bool,
}

impl ControlRegisters {
    pub fn new() -> Self {
        Self { interrupts_enabled: 0, interrupt_flags: 0, waitcnt: 0, ime: false }
    }

    // $04000200: IE (Interrupts enabled)
    pub fn write_ie(&mut self, value: u16) {
        self.interrupts_enabled = value;

        log::trace!("IE write: {value:04X}");
        log::trace!("  VBlank: {}", value.bit(0));
        log::trace!("  HBlank: {}", value.bit(1));
        log::trace!("  V counter match: {}", value.bit(2));
        log::trace!("  Timer 0 overflow: {}", value.bit(3));
        log::trace!("  Timer 1 overflow: {}", value.bit(4));
        log::trace!("  Timer 2 overflow: {}", value.bit(5));
        log::trace!("  Timer 3 overflow: {}", value.bit(6));
        log::trace!("  Serial: {}", value.bit(7));
        log::trace!("  DMA 0: {}", value.bit(8));
        log::trace!("  DMA 1: {}", value.bit(9));
        log::trace!("  DMA 2: {}", value.bit(10));
        log::trace!("  DMA 3: {}", value.bit(11));
        log::trace!("  Keypad: {}", value.bit(12));
        log::trace!("  Game Pak: {}", value.bit(13));
    }

    // $04000202: IF (Interrupt flags)
    pub fn write_if(&mut self, value: u16) {
        // IF writes clear all bits set to 1 in the written value
        self.interrupt_flags &= !value;

        log::trace!("IF write: {value:04X}");
    }

    // $04000204: WAITCNT (Waitstate control)
    pub fn read_waitcnt(&self) -> u16 {
        self.waitcnt
    }

    // $04000204: WAITCNT (Waitstate control)
    pub fn write_waitcnt(&mut self, value: u16) {
        // Bit 15 (GBA cartridge vs. GBC cartridge) is not writable
        self.waitcnt = value & 0x7FFF;

        log::warn!(
            "Unhandled WAITCNT write: {value:04X}, prefetch buffer enabled: {}",
            value.bit(14)
        );
    }

    // $04000208: IME (Interrupt master enable)
    pub fn read_ime(&self) -> u16 {
        self.ime.into()
    }

    // $04000208: IME (Interrupt master enable)
    pub fn write_ime(&mut self, value: u16) {
        self.ime = value.bit(0);

        log::trace!("IME: {}", self.ime);
    }
}
