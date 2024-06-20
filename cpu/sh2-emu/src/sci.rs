//! SH7604 serial communication interface (SCI)
//!
//! No 32X games use this but some test cartridges do

use crate::bus::BusInterface;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SerialInterface {
    name: String,
    tx_enabled: bool,
    rx_enabled: bool,
    tx_interrupt_enabled: bool,
    rx_interrupt_enabled: bool,
    transfer_data: u8,
    transfer_shift: u8,
    transfer_clocks: u64,
    receive_data: u8,
    tx_data_empty: bool,
    rx_data_full: bool,
    transfer_end: bool,
    clock_select: u8,
    bit_rate: u8,
}

impl SerialInterface {
    pub fn new(name: String) -> Self {
        Self {
            name,
            tx_enabled: false,
            rx_enabled: false,
            tx_interrupt_enabled: false,
            rx_interrupt_enabled: false,
            transfer_data: 0xFF,
            transfer_shift: 0xFF,
            transfer_clocks: 0,
            receive_data: 0x00,
            tx_data_empty: true,
            rx_data_full: false,
            transfer_end: true,
            clock_select: 0,
            bit_rate: 0,
        }
    }

    pub fn process<B: BusInterface>(&mut self, sh2_clocks_elapsed: u64, bus: &mut B) {
        if self.rx_enabled && !self.rx_data_full {
            if let Some(rx) = bus.serial_rx() {
                self.receive_data = rx;
                self.rx_data_full = true;
            }
        }

        if self.transfer_clocks == 0 {
            if self.tx_enabled && !self.tx_data_empty {
                // TODO TX interrupt
                self.transfer_shift = self.transfer_data;
                self.transfer_clocks = estimate_tx_clocks(self.clock_select, self.bit_rate);
                self.tx_data_empty = true;
                log::debug!("TX clocks: {}", self.transfer_clocks);
            } else {
                return;
            }
        }

        self.transfer_clocks = self.transfer_clocks.saturating_sub(sh2_clocks_elapsed);
        if self.transfer_clocks == 0 {
            bus.serial_tx(self.transfer_shift);
            if !self.tx_data_empty {
                // TODO TX interrupt
                self.transfer_shift = self.transfer_data;
                self.transfer_clocks = estimate_tx_clocks(self.clock_select, self.bit_rate);
                self.tx_data_empty = true;
                log::debug!("TX clocks: {}", self.transfer_clocks);
            } else {
                // TODO transfer end interrupt
                self.transfer_end = true;
            }
        }
    }

    pub fn read_register(&self, address: u32) -> u8 {
        log::debug!("[{}] SCI read {address:08X}", self.name);

        match address {
            0xFFFFFE00 => self.read_mode(),
            0xFFFFFE01 => self.bit_rate,
            0xFFFFFE02 => self.read_control(),
            0xFFFFFE03 => self.transfer_data,
            0xFFFFFE04 => self.read_status(),
            0xFFFFFE05 => self.read_rx(),
            _ => panic!("Invalid SCI register address: {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u8) {
        match address {
            0xFFFFFE00 => self.write_mode(value),
            0xFFFFFE01 => self.write_bit_rate(value),
            0xFFFFFE02 => self.write_control(value),
            0xFFFFFE03 => self.write_tx(value),
            0xFFFFFE04 => self.write_status(value),
            // RX data register, ignore writes
            0xFFFFFE05 => {}
            _ => panic!("Invalid SCI register address: {address:08X} {value:02X}"),
        }
    }

    // $FFFFFE00: SMR (Serial mode)
    fn read_mode(&self) -> u8 {
        self.clock_select
    }

    // $FFFFFE00: SMR (Serial mode)
    fn write_mode(&mut self, value: u8) {
        self.clock_select = value & 3;

        log::debug!("[{}] SMR write: {value:02X}", self.name);
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
        self.bit_rate = value;
        log::debug!("[{}] BRR write: {value:02X}", self.name);
    }

    // $FFFFFE02: SCR (Serial control)
    fn read_control(&self) -> u8 {
        (u8::from(self.tx_interrupt_enabled) << 7)
            | (u8::from(self.rx_interrupt_enabled) << 6)
            | (u8::from(self.tx_enabled) << 5)
            | (u8::from(self.rx_enabled) << 4)
    }

    // $FFFFFE02: SCR (Serial control)
    fn write_control(&mut self, value: u8) {
        self.tx_interrupt_enabled = value.bit(7);
        self.rx_interrupt_enabled = value.bit(6);
        self.tx_enabled = value.bit(5);
        self.rx_enabled = value.bit(4);

        log::debug!("[{}] SCR write: {value:02X}", self.name);
        log::debug!("  TX interrupt enabled: {}", self.tx_interrupt_enabled);
        log::debug!("  RX interrupt enabled: {}", self.rx_interrupt_enabled);
        log::debug!("  TX enabled: {}", self.tx_enabled);
        log::debug!("  RX enabled: {}", self.rx_enabled);
        log::debug!("  Multiprocessor interrupt enabled: {}", value.bit(3));
        log::debug!("  Transfer end interrupt enabled: {}", value.bit(2));
        log::debug!("  Clock enabled bits: {}", value & 3);
    }

    // $FFFFFE03: TDR (Transfer data register)
    fn write_tx(&mut self, value: u8) {
        self.transfer_data = value;

        log::trace!("[{}] TDR write: {value:02X}", self.name);
    }

    // $FFFFFE04: SSR (Serial status)
    fn read_status(&self) -> u8 {
        (u8::from(self.tx_data_empty) << 7)
            | (u8::from(self.rx_data_full) << 6)
            | (u8::from(self.transfer_end) << 2)
    }

    // $FFFFFE04: SSR (Serial status)
    fn write_status(&mut self, value: u8) {
        self.tx_data_empty &= value.bit(7);
        self.transfer_end &= value.bit(7);
        self.rx_data_full &= value.bit(6);

        log::debug!("[{}] SSR write: {value:02X}", self.name);
        log::debug!("  Clear TX data empty: {}", !value.bit(7));
        log::debug!("  Clear RX data full: {}", !value.bit(6));
        log::debug!("  Multiprocessor bit: {}", value & 1);
    }

    // $FFFFFE05: RDR (Receive data register)
    fn read_rx(&self) -> u8 {
        self.receive_data
    }

    pub fn rx_interrupt_pending(&self) -> bool {
        self.rx_interrupt_enabled && self.rx_data_full
    }
}

fn estimate_tx_clocks(clock_select: u8, bit_rate: u8) -> u64 {
    let clocks_per_bit = if clock_select == 0 {
        128 * (u64::from(bit_rate) + 1)
    } else {
        256 * (1 << (2 * clock_select - 1)) * (u64::from(bit_rate) + 1)
    };

    8 * clocks_per_bit
}
