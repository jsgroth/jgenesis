//! Game Boy serial port
//!
//! Accessories that use the serial port (e.g. link cable) are not emulated, but some games depend
//! on the serial port responding correctly to reads/writes.

use crate::HardwareMode;
use crate::interrupts::InterruptRegisters;
use crate::sm83::InterruptType;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

// Base serial transfer rate is 8192 bits/second == 1024 bytes/second
// The normal-speed CPU M-cycle clock is 1.048576 MHz
// (1048576 cycles/second) / (1024 bytes/second) == 1024 cycles/byte
const BASE_CYCLES_PER_BYTE: u32 = 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SerialPort {
    hardware_mode: HardwareMode,
    transfer_enabled: bool,
    gbc_high_speed: bool,
    internal_clock: bool,
    transfer_cycles_remaining: u32,
    transfer_data: u8,
}

impl SerialPort {
    pub fn new(hardware_mode: HardwareMode) -> Self {
        Self {
            hardware_mode,
            transfer_enabled: false,
            gbc_high_speed: false,
            internal_clock: false,
            transfer_cycles_remaining: 0,
            transfer_data: 0,
        }
    }

    pub fn tick(&mut self, interrupt_registers: &mut InterruptRegisters) {
        if !self.transfer_enabled || !self.internal_clock || self.transfer_cycles_remaining == 0 {
            return;
        }

        self.transfer_cycles_remaining -= 1;
        if self.transfer_cycles_remaining == 0 {
            self.transfer_enabled = false;
            interrupt_registers.set_flag(InterruptType::Serial);
        }
    }

    // $FF01: SB (Serial transfer data)
    pub fn read_data(&self) -> u8 {
        self.transfer_data
    }

    // $FF01: SB (Serial transfer data)
    pub fn write_data(&mut self, value: u8) {
        self.transfer_data = value;

        log::trace!("SB write: {value:02X}");
    }

    // $FF02: SC (Serial transfer control)
    pub fn read_control(&self) -> u8 {
        (u8::from(self.transfer_enabled) << 7)
            | (u8::from(self.gbc_high_speed) << 1)
            | u8::from(self.internal_clock)
    }

    // $FF02: SC (Serial transfer control)
    pub fn write_control(&mut self, value: u8) {
        self.transfer_enabled = value.bit(7);
        self.gbc_high_speed = self.hardware_mode == HardwareMode::Cgb && value.bit(1);
        self.internal_clock = value.bit(0);

        if self.transfer_enabled && self.internal_clock {
            self.transfer_cycles_remaining = BASE_CYCLES_PER_BYTE >> u8::from(self.gbc_high_speed);
        }

        log::trace!("SC write: {value:02X}");
        log::trace!("  Transfer enabled: {}", self.transfer_enabled);
        log::trace!("  GBC high speed: {}", self.gbc_high_speed);
        log::trace!("  Internal clock: {}", self.internal_clock);
    }
}
