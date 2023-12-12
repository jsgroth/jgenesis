//! Code for the X24C01 and X24C02 EEPROM chips, which are used in some of Bandai's FCG boards.

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::fmt::Debug;

pub trait EepromState: Copy + Default {
    fn start(self) -> Self;

    fn stop(self) -> Self;

    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self;

    fn read(self, memory: &[u8]) -> Option<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum X24C01State {
    Standby,
    #[default]
    Stopped,
    ReceivingAddress {
        bits_received: u8,
        bits_remaining: u8,
    },
    ReceivingData {
        address: u8,
        bits_received: u8,
        bits_remaining: u8,
    },
    SendingData {
        address: u8,
        bits_remaining: u8,
    },
}

impl EepromState for X24C01State {
    fn start(self) -> Self {
        if self == Self::Stopped { Self::Standby } else { self }
    }

    fn stop(self) -> Self {
        Self::Stopped
    }

    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self {
        match self {
            Self::Standby => {
                Self::ReceivingAddress { bits_received: u8::from(data), bits_remaining: 7 }
            }
            Self::Stopped => Self::Stopped,
            Self::ReceivingAddress { bits_received, bits_remaining } => {
                if bits_remaining > 0 {
                    let bits_received = (bits_received << 1) | u8::from(data);
                    Self::ReceivingAddress { bits_received, bits_remaining: bits_remaining - 1 }
                } else if bits_received.bit(0) {
                    // Read operation
                    let address = bits_received >> 1;
                    Self::SendingData { address, bits_remaining: 8 }
                } else {
                    // Write operation
                    let address = bits_received >> 1;
                    Self::ReceivingData { address, bits_received: 0, bits_remaining: 8 }
                }
            }
            Self::ReceivingData { address, bits_received, bits_remaining } => {
                if bits_remaining == 0 {
                    // Continue sequential write - but only increment the lowest 2 bits
                    let address = (address & 0xFC) | (address.wrapping_add(1) & 0x03);
                    Self::ReceivingData { address, bits_received: 0, bits_remaining: 8 }
                } else {
                    let bits_received = (bits_received << 1) | u8::from(data);
                    if bits_remaining == 1 {
                        log::trace!("Writing {bits_received:02X} to {address:02X}");
                        memory[address as usize] = bits_received;
                        *dirty = true;
                    }
                    Self::ReceivingData {
                        address,
                        bits_received,
                        bits_remaining: bits_remaining - 1,
                    }
                }
            }
            Self::SendingData { address, bits_remaining } => {
                if bits_remaining == 0 && !data {
                    // Acknowledged, continue sequential read
                    let address = (address + 1) & 127;
                    Self::SendingData { address, bits_remaining: 8 }
                } else if bits_remaining == 0 && data {
                    Self::Stopped
                } else {
                    Self::SendingData { address, bits_remaining: bits_remaining - 1 }
                }
            }
        }
    }

    fn read(self, memory: &[u8]) -> Option<bool> {
        let Self::SendingData { address, bits_remaining } = self else {
            return None;
        };

        if bits_remaining == 8 {
            return None;
        }

        Some(memory[address as usize].bit(bits_remaining))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum X24C02State {
    Standby { address: u8 },
    Stopped { address: u8 },
    ReceivingDeviceAddress { address: u8, bits_received: u8, bits_remaining: u8 },
    ReceivingWriteAddress { address: u8, bits_received: u8, bits_remaining: u8 },
    ReceivingData { address: u8, bits_received: u8, bits_remaining: u8 },
    SendingData { address: u8, bits_remaining: u8 },
}

impl Default for X24C02State {
    fn default() -> Self {
        Self::Stopped { address: 0 }
    }
}

impl EepromState for X24C02State {
    fn start(self) -> Self {
        log::trace!("transitioning to Start from {self:?}");
        match self {
            Self::Standby { address }
            | Self::Stopped { address }
            | Self::ReceivingDeviceAddress { address, .. }
            | Self::ReceivingWriteAddress { address, .. }
            | Self::ReceivingData { address, .. }
            | Self::SendingData { address, .. } => Self::Standby { address },
        }
    }

    fn stop(self) -> Self {
        log::trace!("transitioning to Stop from {self:?}");
        match self {
            Self::Standby { address }
            | Self::Stopped { address }
            | Self::ReceivingDeviceAddress { address, .. }
            | Self::ReceivingWriteAddress { address, .. }
            | Self::ReceivingData { address, .. }
            | Self::SendingData { address, .. } => Self::Stopped { address },
        }
    }

    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self {
        match self {
            Self::Standby { address } => Self::ReceivingDeviceAddress {
                address,
                bits_received: u8::from(data),
                bits_remaining: 7,
            },
            Self::Stopped { address } => Self::Stopped { address },
            Self::ReceivingDeviceAddress { address, bits_received, bits_remaining } => {
                if bits_remaining > 0 {
                    let bits_received = (bits_received << 1) | u8::from(data);
                    Self::ReceivingDeviceAddress {
                        address,
                        bits_received,
                        bits_remaining: bits_remaining - 1,
                    }
                } else if bits_received.bit(0) {
                    // Read operation
                    Self::SendingData { address, bits_remaining: 8 }
                } else {
                    // Write operation
                    Self::ReceivingWriteAddress { address, bits_received: 0, bits_remaining: 8 }
                }
            }
            Self::ReceivingWriteAddress { address, bits_received, bits_remaining } => {
                if bits_remaining > 0 {
                    let bits_received = (bits_received << 1) | u8::from(data);
                    Self::ReceivingWriteAddress {
                        address,
                        bits_received,
                        bits_remaining: bits_remaining - 1,
                    }
                } else {
                    Self::ReceivingData {
                        address: bits_received,
                        bits_received: 0,
                        bits_remaining: 8,
                    }
                }
            }
            Self::ReceivingData { address, bits_received, bits_remaining } => {
                if bits_remaining > 0 {
                    let bits_received = (bits_received << 1) | u8::from(data);
                    if bits_remaining == 1 {
                        memory[address as usize] = bits_received;
                        *dirty = true;
                    }
                    Self::ReceivingData {
                        address,
                        bits_received,
                        bits_remaining: bits_remaining - 1,
                    }
                } else {
                    // Continue sequential write - but only increment the lowest 2 bits
                    let address = (address & 0xFC) | (address.wrapping_add(1) & 0x03);
                    Self::ReceivingData { address, bits_received: 0, bits_remaining: 8 }
                }
            }
            Self::SendingData { address, bits_remaining } => {
                if bits_remaining > 0 {
                    Self::SendingData { address, bits_remaining: bits_remaining - 1 }
                } else if !data {
                    // Acknowledged, continue sequential read
                    let address = address.wrapping_add(1);
                    Self::SendingData { address, bits_remaining: 8 }
                } else {
                    Self::Stopped { address: address.wrapping_add(1) }
                }
            }
        }
    }

    fn read(self, memory: &[u8]) -> Option<bool> {
        let Self::SendingData { address, bits_remaining } = self else {
            return None;
        };

        if bits_remaining == 8 {
            return None;
        }

        Some(memory[address as usize].bit(bits_remaining))
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct EepromChip<State, const N: usize> {
    memory: [u8; N],
    dirty: bool,
    state: State,
    last_data: bool,
    last_clock: bool,
}

impl<State: EepromState + Debug, const N: usize> EepromChip<State, N> {
    pub fn new(sav_bytes: Option<&Vec<u8>>) -> Self {
        let mut memory = [0; N];
        if let Some(sav_bytes) = sav_bytes {
            if sav_bytes.len() == N {
                memory.copy_from_slice(sav_bytes);
            }
        }

        Self { memory, dirty: true, state: State::default(), last_data: false, last_clock: false }
    }

    pub fn handle_read(&self) -> bool {
        log::trace!("EEPROM read");
        self.state.read(&self.memory).unwrap_or(false)
    }

    pub fn handle_write(&mut self, value: u8) {
        let data = value.bit(6);
        let clock = value.bit(5);

        log::trace!("EEPROM write: {value:02X}");
        if self.last_clock && clock && data != self.last_data {
            if data {
                // Low to high
                self.state = self.state.stop();
            } else {
                // High to low
                self.state = self.state.start();
            }
        } else if !self.last_clock && clock {
            let last_state = self.state;
            self.state = self.state.clock(data, &mut self.memory, &mut self.dirty);
            log::trace!("transitioned from {last_state:?} to {:?}, data is {data}", self.state);
        }

        self.last_data = data;
        self.last_clock = clock;
    }

    pub fn get_and_clear_dirty_bit(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub fn get_memory(&self) -> &[u8] {
        &self.memory
    }
}

pub type X24C01Chip = EepromChip<X24C01State, 128>;
pub type X24C02Chip = EepromChip<X24C02State, 256>;
