#[cfg(test)]
mod tests;

use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;
use std::fmt::Debug;

pub trait EepromState: Copy + Default {
    fn start(self) -> Self;

    fn stop(self) -> Self;

    fn is_stopped(self) -> bool;

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
    PostSend {
        address: u8,
    },
}

impl EepromState for X24C01State {
    fn start(self) -> Self {
        log::trace!("Start");
        if self == Self::Stopped { Self::Standby } else { self }
    }

    fn stop(self) -> Self {
        log::trace!("Stop");
        Self::Stopped
    }

    fn is_stopped(self) -> bool {
        self == Self::Stopped
    }

    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self {
        match self {
            Self::Standby => Self::ReceivingAddress { bits_received: 0, bits_remaining: 8 },
            Self::Stopped => Self::Stopped,
            Self::ReceivingAddress { bits_received, bits_remaining } => {
                if bits_remaining > 0 {
                    let bits_received = (bits_received << 1) | u8::from(data);
                    Self::ReceivingAddress { bits_received, bits_remaining: bits_remaining - 1 }
                } else if bits_received.bit(0) {
                    // Read operation
                    let address = bits_received >> 1;
                    Self::SendingData { address, bits_remaining: 7 }
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
                if bits_remaining == 0 {
                    // Acknowledged, continue sequential read
                    let address = (address + 1) & 127;
                    Self::PostSend { address }
                } else {
                    Self::SendingData { address, bits_remaining: bits_remaining - 1 }
                }
            }
            Self::PostSend { address } => {
                if !data {
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    Self::Stopped
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

// Used to emulate the X24C02, X24C08, and X24C16 which all function very similarly
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum X24C16State<const ADDRESS_MASK: u16, const PAGE_MASK: u16> {
    Standby { address: u16 },
    Stopped { address: u16 },
    ReceivingDeviceAddress { address: u16, bits_received: u8, bits_remaining: u8 },
    ReceivingWriteAddress { address: u16, bits_received: u8, bits_remaining: u8 },
    ReceivingData { address: u16, bits_received: u8, bits_remaining: u8 },
    SendingData { address: u16, bits_remaining: u8 },
    PostSend { address: u16 },
}

impl<const ADDRESS_MASK: u16, const PAGE_MASK: u16> Default
    for X24C16State<ADDRESS_MASK, PAGE_MASK>
{
    fn default() -> Self {
        Self::Stopped { address: 0 }
    }
}

impl<const ADDRESS_MASK: u16, const PAGE_MASK: u16> EepromState
    for X24C16State<ADDRESS_MASK, PAGE_MASK>
{
    fn start(self) -> Self {
        log::trace!("transitioning to Start from {self:?}");
        match self {
            Self::Standby { address }
            | Self::Stopped { address }
            | Self::ReceivingDeviceAddress { address, .. }
            | Self::ReceivingWriteAddress { address, .. }
            | Self::ReceivingData { address, .. }
            | Self::SendingData { address, .. }
            | Self::PostSend { address } => Self::Standby { address },
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
            | Self::SendingData { address, .. }
            | Self::PostSend { address } => Self::Stopped { address },
        }
    }

    fn is_stopped(self) -> bool {
        matches!(self, Self::Stopped { .. })
    }

    // TODO address mask
    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self {
        match self {
            Self::Standby { address } => {
                Self::ReceivingDeviceAddress { address, bits_received: 0, bits_remaining: 8 }
            }
            Self::Stopped { address } => Self::Stopped { address },
            Self::ReceivingDeviceAddress { address, bits_received, bits_remaining } => {
                if bits_remaining > 0 {
                    let bits_received = (bits_received << 1) | u8::from(data);
                    Self::ReceivingDeviceAddress {
                        address,
                        bits_received,
                        bits_remaining: bits_remaining - 1,
                    }
                } else {
                    // Bits 3-1 of device address byte are used as bits 10-8 in address
                    let high_bits = u16::from(bits_received & 0x0E) << 7;
                    let address = ((address & 0x00FF) | high_bits) & ADDRESS_MASK;

                    if bits_received.bit(0) {
                        // Read operation
                        Self::SendingData { address, bits_remaining: 7 }
                    } else {
                        // Write operation
                        Self::ReceivingWriteAddress { address, bits_received: 0, bits_remaining: 8 }
                    }
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
                        address: (address & 0xFF00) | u16::from(bits_received),
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
                    // Continue sequential write - but only increment the lowest N bits
                    let address = (address & !PAGE_MASK) | (address.wrapping_add(1) & PAGE_MASK);
                    Self::ReceivingData { address, bits_received: 0, bits_remaining: 8 }
                }
            }
            Self::SendingData { address, bits_remaining } => {
                if bits_remaining > 0 {
                    Self::SendingData { address, bits_remaining: bits_remaining - 1 }
                } else {
                    let address = (address + 1) & ADDRESS_MASK;
                    Self::PostSend { address }
                }
            }
            Self::PostSend { address } => {
                if !data {
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    Self::Stopped { address }
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

        if self.state.is_stopped() {
            return self.last_data;
        }

        self.state.read(&self.memory).unwrap_or(false)
    }

    pub fn handle_data_write(&mut self, data: bool) {
        self.handle_dual_write(data, self.last_clock);
    }

    pub fn handle_clock_write(&mut self, clock: bool) {
        self.handle_dual_write(self.last_data, clock);
    }

    pub fn handle_dual_write(&mut self, data: bool, clock: bool) {
        log::trace!("EEPROM write sda={} clock={}", u8::from(data), u8::from(clock));
        if self.last_clock && clock && data != self.last_data {
            if data {
                // Low to high
                self.state = self.state.stop();
            } else {
                // High to low
                self.state = self.state.start();
            }
        } else if self.last_clock && !clock {
            let last_state = self.state;
            self.state = self.state.clock(self.last_data, &mut self.memory, &mut self.dirty);
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
pub type X24C02Chip = EepromChip<X24C16State<0x0FF, 0x03>, 256>;
pub type X24C08Chip = EepromChip<X24C16State<0x3FF, 0x0F>, 1024>;
pub type X24C16Chip = EepromChip<X24C16State<0x7FF, 0x0F>, 2048>;
// TODO 24C64
