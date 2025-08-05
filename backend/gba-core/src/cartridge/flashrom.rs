use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Command {
    EnterIdMode,  // 0x90
    ExitIdMode,   // 0xF0
    PrepareErase, // 0x80
    EraseChip,    // 0x10
    EraseSector,  // 0x30
    WriteByte,    // 0xA0
    SwitchBank,   // 0xB0
}

impl Command {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x10 => Some(Self::EraseChip),
            0x30 => Some(Self::EraseSector),
            0x80 => Some(Self::PrepareErase),
            0x90 => Some(Self::EnterIdMode),
            0xA0 => Some(Self::WriteByte),
            0xB0 => Some(Self::SwitchBank),
            0xF0 => Some(Self::ExitIdMode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum State {
    Ready,
    ReceivingCommand1,      // Received $E005555 = 0xAA
    ReceivingCommand2,      // Received $E002AAA = 0x55
    EraseReady,             // Received erase command 0x80
    EraseReceivingCommand1, // Received $E005555 = 0xAA
    EraseReceivingCommand2, // Received $E002AAA = 0x55
    WriteByteReady,         // Received write byte command 0xA0
    BankSwitchReady,        // Received bank switch command 0xB0
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct FlashRom<const BANKED: bool> {
    memory: Box<[u8]>,
    state: State,
    bank_offset: u32,
    id_mode: bool,
}

pub type FlashRom64K = FlashRom<false>;
pub type FlashRom128K = FlashRom<true>;

// Arbitrary choices that are not Atmel or Macronix (which support custom commands)
const PANASONIC_64K_ID: u16 = 0x1B32;
const SANYO_128K_ID: u16 = 0x1362;

impl<const BANKED: bool> FlashRom<BANKED> {
    pub fn new(initial_save: Option<&Vec<u8>>) -> Self {
        let len = if BANKED { 128 * 1024 } else { 64 * 1024 };
        let mut memory = vec![0xFF; len].into_boxed_slice();

        if let Some(initial_save) = initial_save
            && initial_save.len() >= len
        {
            memory.copy_from_slice(&initial_save[..len]);
        }

        Self { memory, state: State::Ready, bank_offset: 0, id_mode: false }
    }

    pub fn read(&self, address: u32) -> u8 {
        log::trace!("Flash ROM read {address:08X}, current state {:?}", self.state);

        let address = address & 0xFFFF;

        if self.id_mode && address <= 0x0001 {
            let id = if BANKED { SANYO_128K_ID } else { PANASONIC_64K_ID };
            return id.to_le_bytes()[address as usize];
        }

        self.memory[(self.bank_offset | address) as usize]
    }

    pub fn write(&mut self, address: u32, value: u8) {
        log::trace!("Flash ROM write {address:08X} {value:02X}, current state {:?}", self.state);

        let address = address & 0xFFFF;

        self.state = match self.state {
            State::Ready => {
                if address == 0x5555 && value == 0xAA {
                    State::ReceivingCommand1
                } else {
                    State::Ready
                }
            }
            State::ReceivingCommand1 => {
                if address == 0x2AAA && value == 0x55 {
                    State::ReceivingCommand2
                } else {
                    State::Ready
                }
            }
            State::ReceivingCommand2 => {
                if address == 0x5555 {
                    match Command::from_byte(value) {
                        Some(Command::EnterIdMode) => {
                            self.id_mode = true;
                            State::Ready
                        }
                        Some(Command::ExitIdMode) => {
                            self.id_mode = false;
                            State::Ready
                        }
                        Some(Command::PrepareErase) => State::EraseReady,
                        Some(Command::WriteByte) => State::WriteByteReady,
                        Some(Command::SwitchBank) => {
                            if BANKED {
                                State::BankSwitchReady
                            } else {
                                State::Ready
                            }
                        }
                        Some(Command::EraseChip | Command::EraseSector) | None => State::Ready,
                    }
                } else {
                    State::Ready
                }
            }
            State::EraseReady => {
                if address == 0x5555 && value == 0xAA {
                    State::EraseReceivingCommand1
                } else {
                    State::Ready
                }
            }
            State::EraseReceivingCommand1 => {
                if address == 0x2AAA && value == 0x55 {
                    State::EraseReceivingCommand2
                } else {
                    State::Ready
                }
            }
            State::EraseReceivingCommand2 => match Command::from_byte(value) {
                Some(Command::EraseChip) if address == 0x5555 => {
                    self.memory.fill(0xFF);
                    State::Ready
                }
                Some(Command::EraseSector) => {
                    let sector_addr = (self.bank_offset | (address & 0xF000)) as usize;
                    self.memory[sector_addr..sector_addr + 0x1000].fill(0xFF);
                    State::Ready
                }
                _ => State::Ready,
            },
            State::WriteByteReady => {
                self.memory[(self.bank_offset | address) as usize] = value;
                State::Ready
            }
            State::BankSwitchReady => {
                self.bank_offset = u32::from(value & 1) << 16;
                State::Ready
            }
        };

        log::trace!("  New state {:?}", self.state);
    }

    pub fn memory(&self) -> &[u8] {
        &self.memory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read() {
        let mut flash_rom = FlashRom64K::new(None);

        assert_eq!(0xFF, flash_rom.read(0xE001357));

        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0xA0);
        flash_rom.write(0xE001357, 0x42);

        assert_eq!(0x42, flash_rom.read(0xE001357));
    }

    #[test]
    fn erase_chip() {
        let mut flash_rom = FlashRom64K::new(None);
        flash_rom.memory.fill(0x12);

        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0x80);
        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0x10);

        for address in 0xE000000..=0xE00FFFF {
            assert_eq!(0xFF, flash_rom.read(address));
        }
    }

    #[test]
    fn erase_sector() {
        let mut flash_rom = FlashRom64K::new(None);
        flash_rom.memory.fill(0x12);

        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0x80);
        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE007000, 0x30);

        for address in 0xE000000..=0xE006FFF {
            assert_eq!(0x12, flash_rom.read(address));
        }

        for address in 0xE007000..=0xE007FFF {
            assert_eq!(0xFF, flash_rom.read(address));
        }

        for address in 0xE008000..=0xE00FFFF {
            assert_eq!(0x12, flash_rom.read(address));
        }
    }

    #[test]
    fn bank_switch() {
        let mut flash_rom = FlashRom128K::new(None);

        // Write 0x42 to $1357
        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0xA0);
        flash_rom.write(0xE001357, 0x42);

        assert_eq!(0x42, flash_rom.read(0xE001357));

        // Switch to bank 1
        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0xB0);
        flash_rom.write(0xE000000, 1);

        assert_eq!(0xFF, flash_rom.read(0xE001357));

        // Write 0xBC to $1357
        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0xA0);
        flash_rom.write(0xE001357, 0xBC);

        assert_eq!(0xBC, flash_rom.read(0xE001357));

        // Switch to bank 0
        flash_rom.write(0xE005555, 0xAA);
        flash_rom.write(0xE002AAA, 0x55);
        flash_rom.write(0xE005555, 0xB0);
        flash_rom.write(0xE000000, 0);

        assert_eq!(0x42, flash_rom.read(0xE001357));

        assert_eq!(0x42, flash_rom.memory[0x01357]);
        assert_eq!(0xBC, flash_rom.memory[0x11357]);
    }
}
