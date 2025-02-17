use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum FlashRomCommandState {
    // Initial state
    Idle,
    // Received 0xAA to $0E005555
    ReceivedPrefix,
    // Received 0x55 to $0e002AAA
    AwaitingCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum FlashRomState {
    // $80
    Erase,
    // $90
    IdMode,
    // $A0
    WriteByte,
    // $B0
    ChangeBank,
    // $F0
    Ready,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct FlashRom<const LEN: usize> {
    data: BoxedByteArray<LEN>,
    bank: u8,
    state: FlashRomState,
    command_state: FlashRomCommandState,
}

impl<const LEN: usize> FlashRom<LEN> {
    pub fn new() -> Self {
        Self {
            data: BoxedByteArray::new(),
            bank: 0,
            state: FlashRomState::Ready,
            command_state: FlashRomCommandState::Idle,
        }
    }

    pub fn read(&mut self, address: u32) -> u8 {
        match self.state {
            FlashRomState::IdMode => {
                if !address.bit(0) {
                    // Manufacturer; hardcode Macronix ID
                    0xC2
                } else {
                    // Device; hardcode Macronix 64KB/128KB IDs
                    if LEN == 64 * 1024 {
                        // Macronix 64KB ID
                        0x1C
                    } else {
                        // Macronix 128KB ID
                        0x09
                    }
                }
            }
            FlashRomState::Ready => {
                let data_addr = flash_rom_addr::<LEN>(address, self.bank);
                self.data[data_addr]
            }
            _ => todo!("Flash ROM read {address:08X} in state {:?}", self.state),
        }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        log::trace!(
            "Flash ROM write {address:08X} {value:02X}, state={:?} cmdstate={:?}",
            self.state,
            self.command_state
        );

        match self.command_state {
            FlashRomCommandState::Idle => match self.state {
                FlashRomState::ChangeBank => {
                    if address & 0xFFFF == 0x0000 {
                        self.bank = value;
                        self.state = FlashRomState::Ready;
                    }
                }
                FlashRomState::WriteByte => {
                    let data_addr = flash_rom_addr::<LEN>(address, self.bank);
                    self.data[data_addr] = value;

                    self.state = FlashRomState::Ready;
                }
                _ => {
                    if address & 0xFFFF == 0x5555 && value == 0xAA {
                        self.command_state = FlashRomCommandState::ReceivedPrefix;
                    } else {
                        log::warn!("WRITE {address:08X} {value:02X} {:?}", self.state);
                    }
                }
            },
            FlashRomCommandState::ReceivedPrefix => {
                self.command_state = if address & 0xFFFF == 0x2AAA && value == 0x55 {
                    FlashRomCommandState::AwaitingCommand
                } else {
                    // TODO is this right?
                    todo!("WRITE2 {address:08X} {value:02X} {:?}", self.state);
                    FlashRomCommandState::Idle
                };
            }
            FlashRomCommandState::AwaitingCommand => {
                self.command_state = FlashRomCommandState::Idle;

                match self.state {
                    FlashRomState::Erase => {
                        if value == 0x30 && address & 0x0FFF == 0x0000 {
                            let data_addr = flash_rom_addr::<LEN>(address & 0xF000, self.bank);
                            self.data[data_addr..data_addr + 0x1000].fill(0xFF);

                            self.state = FlashRomState::Ready;
                        }
                    }
                    _ => {
                        if address & 0xFFFF == 0x5555 {
                            self.state = match value {
                                0x80 => FlashRomState::Erase,
                                0x90 => FlashRomState::IdMode,
                                0xA0 => FlashRomState::WriteByte,
                                0xB0 => FlashRomState::ChangeBank,
                                0xF0 => FlashRomState::Ready,
                                _ => todo!("Flash ROM command {address:08X} {value:02X}"),
                            }
                        };
                    }
                }
            }
        }
    }
}

fn flash_rom_addr<const LEN: usize>(address: u32, bank: u8) -> usize {
    (((address & 0xFFFF) as usize) | (usize::from(bank) << 16)) & (LEN - 1)
}
