//! Implementations for the 24C01, 24C02, 24C08, and 24C16 EEPROM chips

#[cfg(test)]
mod tests;

use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use std::fmt::Debug;

pub trait EepromState: Copy + Default {
    #[must_use]
    fn start(self) -> Self;

    #[must_use]
    fn stop(self) -> Self;

    #[must_use]
    fn is_stopped(self) -> bool;

    #[must_use]
    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self;

    #[must_use]
    fn read(self, memory: &[u8]) -> Option<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct ShiftRegister {
    pub received: u8,
    pub remaining: u8,
}

impl ShiftRegister {
    fn new() -> Self {
        Self { received: 0, remaining: 8 }
    }

    #[must_use]
    fn write(self, data: bool) -> Self {
        Self { received: (self.received << 1) | u8::from(data), remaining: self.remaining - 1 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum X24C01State {
    Started,
    #[default]
    Standby,
    ReceivingAddress(ShiftRegister),
    ReceivingData {
        address: u8,
        bits: ShiftRegister,
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
        if self == Self::Standby { Self::Started } else { self }
    }

    fn stop(self) -> Self {
        log::trace!("Stop");
        Self::Standby
    }

    fn is_stopped(self) -> bool {
        self == Self::Standby
    }

    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self {
        match self {
            Self::Started => Self::ReceivingAddress(ShiftRegister::new()),
            Self::Standby => Self::Standby,
            Self::ReceivingAddress(bits) => {
                if bits.remaining > 0 {
                    Self::ReceivingAddress(bits.write(data))
                } else if bits.received.bit(0) {
                    // Read operation
                    let address = bits.received >> 1;
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    // Write operation
                    let address = bits.received >> 1;
                    Self::ReceivingData { address, bits: ShiftRegister::new() }
                }
            }
            Self::ReceivingData { address, bits: ShiftRegister { remaining: 0, .. } } => {
                // Continue sequential write - but only increment the lowest 2 bits
                let address = (address & 0xFC) | (address.wrapping_add(1) & 0x03);
                Self::ReceivingData { address, bits: ShiftRegister::new() }
            }
            Self::ReceivingData { address, bits } => {
                let bits = bits.write(data);
                if bits.remaining == 0 {
                    log::trace!("Writing {:02X} to {address:02X}", bits.received);
                    memory[address as usize] = bits.received;
                    *dirty = true;
                }
                Self::ReceivingData { address, bits }
            }
            Self::SendingData { address, bits_remaining: 0 } => {
                // Acknowledged, continue sequential read
                let address = (address + 1) & 127;
                Self::PostSend { address }
            }
            Self::SendingData { address, bits_remaining } => {
                Self::SendingData { address, bits_remaining: bits_remaining - 1 }
            }
            Self::PostSend { address } => {
                if !data {
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    Self::Standby
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
    Started { address: u16 },
    Standby { address: u16 },
    ReceivingDeviceAddress { address: u16, bits: ShiftRegister },
    ReceivingWriteAddress { address: u16, bits: ShiftRegister },
    ReceivingData { address: u16, bits: ShiftRegister },
    SendingData { address: u16, bits_remaining: u8 },
    PostSend { address: u16 },
}

impl<const ADDRESS_MASK: u16, const PAGE_MASK: u16> Default
    for X24C16State<ADDRESS_MASK, PAGE_MASK>
{
    fn default() -> Self {
        Self::Standby { address: 0 }
    }
}

impl<const ADDRESS_MASK: u16, const PAGE_MASK: u16> EepromState
    for X24C16State<ADDRESS_MASK, PAGE_MASK>
{
    fn start(self) -> Self {
        log::trace!("transitioning to Start from {self:?}");
        match self {
            Self::Started { address }
            | Self::Standby { address }
            | Self::ReceivingDeviceAddress { address, .. }
            | Self::ReceivingWriteAddress { address, .. }
            | Self::ReceivingData { address, .. }
            | Self::SendingData { address, .. }
            | Self::PostSend { address } => Self::Started { address },
        }
    }

    fn stop(self) -> Self {
        log::trace!("transitioning to Stop from {self:?}");
        match self {
            Self::Started { address }
            | Self::Standby { address }
            | Self::ReceivingDeviceAddress { address, .. }
            | Self::ReceivingWriteAddress { address, .. }
            | Self::ReceivingData { address, .. }
            | Self::SendingData { address, .. }
            | Self::PostSend { address } => Self::Standby { address },
        }
    }

    fn is_stopped(self) -> bool {
        matches!(self, Self::Standby { .. })
    }

    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self {
        match self {
            Self::Started { address } => {
                Self::ReceivingDeviceAddress { address, bits: ShiftRegister::new() }
            }
            Self::Standby { address } => Self::Standby { address },
            Self::ReceivingDeviceAddress {
                address,
                bits: ShiftRegister { received, remaining: 0 },
            } => {
                // Bits 3-1 of device address byte are used as bits 10-8 in address
                let high_bits = u16::from(received & 0x0E) << 7;
                let address = ((address & 0x00FF) | high_bits) & ADDRESS_MASK;

                if received.bit(0) {
                    // Read operation
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    // Write operation
                    Self::ReceivingWriteAddress { address, bits: ShiftRegister::new() }
                }
            }
            Self::ReceivingDeviceAddress { address, bits } => {
                Self::ReceivingDeviceAddress { address, bits: bits.write(data) }
            }
            Self::ReceivingWriteAddress {
                address,
                bits: ShiftRegister { received, remaining: 0 },
            } => Self::ReceivingData {
                address: (address & 0xFF00) | u16::from(received),
                bits: ShiftRegister::new(),
            },
            Self::ReceivingWriteAddress { address, bits } => {
                Self::ReceivingWriteAddress { address, bits: bits.write(data) }
            }
            Self::ReceivingData { address, bits: ShiftRegister { remaining: 0, .. } } => {
                // Continue sequential write - but only increment the lowest N bits
                let address = (address & !PAGE_MASK) | (address.wrapping_add(1) & PAGE_MASK);
                Self::ReceivingData { address, bits: ShiftRegister::new() }
            }
            Self::ReceivingData { address, bits } => {
                let bits = bits.write(data);
                if bits.remaining == 0 {
                    memory[address as usize] = bits.received;
                    *dirty = true;
                }
                Self::ReceivingData { address, bits }
            }
            Self::SendingData { address, bits_remaining: 0 } => {
                Self::PostSend { address: (address + 1) & ADDRESS_MASK }
            }
            Self::SendingData { address, bits_remaining } => {
                Self::SendingData { address, bits_remaining: bits_remaining - 1 }
            }
            Self::PostSend { address } => {
                if !data {
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    Self::Standby { address }
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

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub enum X24C64State {
    Started { address: u16 },
    Standby { address: u16 },
    ReceivingDeviceAddress { address: u16, bits: ShiftRegister },
    ReceivingDataAddressFirst { address: u16, bits: ShiftRegister },
    ReceivingDataAddressSecond { address: u16, bits: ShiftRegister },
    ReceivingData { address: u16, bits: ShiftRegister },
    SendingData { address: u16, bits_remaining: u8 },
    PostSend { address: u16 },
}

impl Default for X24C64State {
    fn default() -> Self {
        Self::Standby { address: 0 }
    }
}

impl X24C64State {
    fn address(self) -> u16 {
        match self {
            Self::Started { address }
            | Self::Standby { address }
            | Self::ReceivingDeviceAddress { address, .. }
            | Self::ReceivingDataAddressFirst { address, .. }
            | Self::ReceivingDataAddressSecond { address, .. }
            | Self::ReceivingData { address, .. }
            | Self::SendingData { address, .. }
            | Self::PostSend { address } => address,
        }
    }
}

impl EepromState for X24C64State {
    fn start(self) -> Self {
        log::trace!("Start");
        Self::Started { address: self.address() }
    }

    fn stop(self) -> Self {
        log::trace!("Stop");
        Self::Standby { address: self.address() }
    }

    fn is_stopped(self) -> bool {
        matches!(self, Self::Standby { .. })
    }

    fn clock(self, data: bool, memory: &mut [u8], dirty: &mut bool) -> Self {
        match self {
            Self::Started { address } => {
                Self::ReceivingDeviceAddress { address, bits: ShiftRegister::new() }
            }
            Self::Standby { address } => Self::Standby { address },
            Self::ReceivingDeviceAddress {
                address,
                bits: ShiftRegister { received, remaining: 0 },
            } => {
                let device_address = received >> 1;
                if device_address != 0b101_0000 {
                    Self::Standby { address }
                } else if received.bit(0) {
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    Self::ReceivingDataAddressFirst { address, bits: ShiftRegister::new() }
                }
            }
            Self::ReceivingDeviceAddress { address, bits } => {
                Self::ReceivingDeviceAddress { address, bits: bits.write(data) }
            }
            Self::ReceivingDataAddressFirst {
                address,
                bits: ShiftRegister { received, remaining: 0 },
            } => Self::ReceivingDataAddressSecond {
                address: u16::from_be_bytes([received & 0x1F, address.lsb()]),
                bits: ShiftRegister::new(),
            },
            Self::ReceivingDataAddressFirst { address, bits } => {
                Self::ReceivingDataAddressFirst { address, bits: bits.write(data) }
            }
            Self::ReceivingDataAddressSecond {
                address,
                bits: ShiftRegister { received, remaining: 0 },
            } => Self::ReceivingData {
                address: u16::from_be_bytes([address.msb(), received]),
                bits: ShiftRegister::new(),
            },
            Self::ReceivingDataAddressSecond { address, bits } => {
                Self::ReceivingDataAddressSecond { address, bits: bits.write(data) }
            }
            Self::ReceivingData { address, bits: ShiftRegister { received, remaining: 0 } } => {
                memory[address as usize] = received;
                *dirty = true;

                Self::ReceivingData {
                    address: ((address + 1) & 0x1F) | (address & !0x1F),
                    bits: ShiftRegister::new(),
                }
            }
            Self::ReceivingData { address, bits } => {
                Self::ReceivingData { address, bits: bits.write(data) }
            }
            Self::SendingData { address, bits_remaining: 0 } => {
                Self::PostSend { address: (address + 1) & 0x1FFF }
            }
            Self::SendingData { address, bits_remaining } => {
                Self::SendingData { address, bits_remaining: bits_remaining - 1 }
            }
            Self::PostSend { address } => {
                if !data {
                    Self::SendingData { address, bits_remaining: 7 }
                } else {
                    Self::Standby { address }
                }
            }
        }
    }

    fn read(self, memory: &[u8]) -> Option<bool> {
        let Self::SendingData { address, bits_remaining } = self else { return None };

        let byte = memory[address as usize];
        Some(byte.bit(bits_remaining))
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
    #[must_use]
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
pub type X24C64Chip = EepromChip<X24C64State, 8192>;
