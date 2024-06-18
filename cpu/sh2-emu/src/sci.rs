//! SH7604 serial communication interface (SCI)

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SerialInterface {}

impl SerialInterface {
    pub fn new() -> Self {
        Self {}
    }

    pub fn write_register(&mut self, address: u32, value: u8) {
        match address {
            0xFFFFFE00 => self.write_mode(value),
            0xFFFFFE01 => self.write_bit_rate(value),
            0xFFFFFE02 => self.write_control(value),
            0xFFFFFE04 => self.write_status(value),
            _ => todo!("SCI write {address:08X} {value:02X}"),
        }
    }

    // $FFFFFE00: SMR (Serial mode)
    fn write_mode(&mut self, value: u8) {
        log::debug!("SMR write: {value:02X}");
        log::debug!("  Clocked synchronous mode: {}", value.bit(7));
        log::debug!("  Character length: {}", if value.bit(6) { "7-bit" } else { "8-bit" });
        log::debug!("  Parity check enabled: {}", value.bit(5));
        log::debug!("  Parity mode odd/even flag: {}", value.bit(4));
        log::debug!("  Stop bit length bit: {}", value.bit(3));
        log::debug!("  Multiprocessor mode: {}", value.bit(2));
        log::debug!(
            "  Clock select: {}",
            match value & 3 {
                0 => "sysclk/4",
                1 => "sysclk/16",
                2 => "sysclk/64",
                3 => "sysclk/256",
                _ => unreachable!(),
            }
        );
    }

    // $FFFFFE01: BRR (Bit rate)
    fn write_bit_rate(&mut self, value: u8) {
        log::debug!("BRR write: {value:02X}");
    }

    // $FFFFFE02: SCR (Serial control)
    fn write_control(&mut self, value: u8) {
        log::debug!("SCR write: {value:02X}");
        log::debug!("  TX interrupt enabled: {}", value.bit(7));
        log::debug!("  RX interrupt enabled: {}", value.bit(6));
        log::debug!("  TX enabled: {}", value.bit(5));
        log::debug!("  RX enabled: {}", value.bit(4));
        log::debug!("  Multiprocessor interrupt enabled: {}", value.bit(3));
        log::debug!("  Transfer end interrupt enabled: {}", value.bit(2));
        log::debug!("  Clock enabled bits: {}", value & 3);
    }

    // $FFFFFE04: SSR (Serial status)
    fn write_status(&mut self, value: u8) {
        log::debug!("SSR write: {value:02X}");
        log::debug!("  Multiprocessor bit: {}", value & 1);
    }
}
