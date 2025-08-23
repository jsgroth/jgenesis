//! EEPROM save memory code
//!
//! Comes in 512-byte and 8KB variants; protocol is the same except for the number of address bits

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Request {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
enum EepromState {
    Idle,
    ReceivingRequestType,
    ReceivingAddress { address: u16, remaining: u8, request: Request },
    ReceivingData { bits: u64, remaining: u8, address: u16 },
    PreparingSend { bits: u64, wait_remaining: u8 },
    SendingData { bits: u64, remaining: u8 },
    AwaitingEndMarker { request: Request, address: u16 },
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Eeprom<const LEN: usize, const ADDRESS_BITS: u8> {
    memory: Box<[u8]>,
    state: EepromState,
}

pub type Eeprom512 = Eeprom<512, 6>;
pub type Eeprom8K = Eeprom<8192, 14>;

impl<const LEN: usize, const ADDRESS_BITS: u8> Eeprom<LEN, ADDRESS_BITS> {
    pub fn new(initial_save: Option<&Vec<u8>>) -> Self {
        let mut memory = vec![0xFF; LEN].into_boxed_slice();

        if let Some(initial_save) = initial_save
            && initial_save.len() >= LEN
        {
            memory.copy_from_slice(&initial_save[..LEN]);
        }

        Self { memory, state: EepromState::Idle }
    }

    pub fn read(&mut self) -> bool {
        log::trace!("EEPROM read, current state {:X?}", self.state);

        match self.state {
            EepromState::PreparingSend { bits, mut wait_remaining } => {
                wait_remaining -= 1;
                self.state = if wait_remaining == 0 {
                    EepromState::SendingData { bits, remaining: 64 }
                } else {
                    EepromState::PreparingSend { bits, wait_remaining }
                };
                true
            }
            EepromState::SendingData { mut bits, mut remaining } => {
                let bit = bits.bit(63);
                bits <<= 1;
                remaining -= 1;

                self.state = if remaining == 0 {
                    EepromState::Idle
                } else {
                    EepromState::SendingData { bits, remaining }
                };

                bit
            }
            _ => true,
        }
    }

    pub fn write(&mut self, bit: bool) {
        log::trace!("EEPROM write {}, current state {:X?}", u8::from(bit), self.state);

        self.state = match self.state {
            EepromState::Idle => {
                if bit {
                    EepromState::ReceivingRequestType
                } else {
                    EepromState::Idle
                }
            }
            EepromState::ReceivingRequestType => EepromState::ReceivingAddress {
                address: 0,
                remaining: ADDRESS_BITS,
                request: if bit { Request::Read } else { Request::Write },
            },
            EepromState::ReceivingAddress { mut address, mut remaining, request } => {
                address = (address << 1) | u16::from(bit);
                remaining -= 1;

                if remaining == 0 {
                    match request {
                        Request::Read => EepromState::AwaitingEndMarker { request, address },
                        Request::Write => {
                            EepromState::ReceivingData { bits: 0, remaining: 64, address }
                        }
                    }
                } else {
                    EepromState::ReceivingAddress { address, remaining, request }
                }
            }
            EepromState::ReceivingData { mut bits, mut remaining, address } => {
                bits = (bits << 1) | u64::from(bit);
                remaining -= 1;

                if remaining == 0 {
                    let byte_addr = ((address << 3) as usize) & (LEN - 1);
                    self.memory[byte_addr..byte_addr + 8].copy_from_slice(&bits.to_be_bytes());
                    EepromState::AwaitingEndMarker { request: Request::Write, address }
                } else {
                    EepromState::ReceivingData { bits, remaining, address }
                }
            }
            EepromState::PreparingSend { bits, wait_remaining } => {
                EepromState::PreparingSend { bits, wait_remaining }
            }
            EepromState::SendingData { bits, remaining } => {
                EepromState::SendingData { bits, remaining }
            }
            EepromState::AwaitingEndMarker { request, address } => match request {
                Request::Read => {
                    let byte_addr = ((address << 3) as usize) & (LEN - 1);
                    let bits = u64::from_be_bytes(
                        self.memory[byte_addr..byte_addr + 8].try_into().unwrap(),
                    );
                    EepromState::PreparingSend { bits, wait_remaining: 4 }
                }
                Request::Write => EepromState::Idle,
            },
        };

        log::trace!("  New state {:X?}", self.state);
    }

    pub fn memory(&self) -> &[u8] {
        &self.memory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eeprom_write<const LEN: usize, const ADDRESS_BITS: u8>(
        eeprom: &mut Eeprom<LEN, ADDRESS_BITS>,
        address: u16,
        value: u64,
    ) {
        // Write: 10, then address (MSB first), then data, then 0
        eeprom.write(true);
        eeprom.write(false);

        let mut address_shift = address;
        for _ in 0..ADDRESS_BITS {
            eeprom.write(address_shift.bit(ADDRESS_BITS - 1));
            address_shift <<= 1;
        }

        let mut value_shift = value;
        for _ in 0..64 {
            eeprom.write(value_shift.bit(63));
            value_shift <<= 1;
        }

        eeprom.write(false);
    }

    fn eeprom_read<const LEN: usize, const ADDRESS_BITS: u8>(
        eeprom: &mut Eeprom<LEN, ADDRESS_BITS>,
        address: u16,
    ) -> u64 {
        // Read: 10, then address (MSB first), then 0, then read data (ignore first 4 bits)
        eeprom.write(true);
        eeprom.write(true);

        let mut address_shift = address;
        for _ in 0..ADDRESS_BITS {
            eeprom.write(address_shift.bit(ADDRESS_BITS - 1));
            address_shift <<= 1;
        }

        eeprom.write(false);

        // Skip first 4 bits
        for _ in 0..4 {
            eeprom.read();
        }

        let mut value: u64 = 0;
        for _ in 0..64 {
            value = (value << 1) | u64::from(eeprom.read());
        }

        value
    }

    fn write_then_read<const LEN: usize, const ADDRESS_BITS: u8>(
        mut eeprom: Eeprom<LEN, ADDRESS_BITS>,
    ) {
        for _ in 0..100 {
            eeprom.memory.fill(0);

            let address: u16 = rand::random();

            let mut value: u64 = 0;
            while value == 0 {
                value = rand::random();
            }

            eeprom_write(&mut eeprom, address, value);
            assert_eq!(value, eeprom_read(&mut eeprom, address));
            assert_eq!(0, eeprom_read(&mut eeprom, address.wrapping_add(1)));
            assert_eq!(0, eeprom_read(&mut eeprom, address.wrapping_sub(1)));
        }
    }

    #[test]
    fn write_then_read_512() {
        write_then_read(Eeprom512::new(None));
    }

    #[test]
    fn write_then_read_8k() {
        write_then_read(Eeprom8K::new(None));

        // Test that highest 4 address bits are ignored
        let mut eeprom = Eeprom8K::new(None);
        eeprom.memory.fill(0);

        let value = 0x0123456789ABCDEF;
        eeprom_write(&mut eeprom, 0x001F, value);
        assert_eq!(value, eeprom_read(&mut eeprom, 0x001F));
        assert_eq!(value, eeprom_read(&mut eeprom, 0xFC1F));
        assert_eq!(0, eeprom_read(&mut eeprom, 0xFE1F));
    }
}
