use crate::num::GetBit;

#[derive(Debug, Clone)]
struct Cartridge {
    rom: Vec<u8>,
    ram: Vec<u8>,
    rom_bank_0: u32,
    rom_bank_1: u32,
    rom_bank_2: u32,
    ram_mapped: bool,
    ram_bank: u32,
}

const CARTRIDGE_RAM_SIZE: usize = 32 * 1024;

impl Cartridge {
    fn from_rom(rom: Vec<u8>) -> Self {
        Self {
            rom,
            ram: vec![0; CARTRIDGE_RAM_SIZE],
            rom_bank_0: 0,
            rom_bank_1: 1,
            rom_bank_2: 2,
            ram_mapped: false,
            ram_bank: 0,
        }
    }

    fn read_rom_address(&self, address: u32) -> u8 {
        let wrapped_addr = (address as usize) & (self.rom.len() - 1);
        self.rom[wrapped_addr]
    }

    fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x03FF => self.rom[address as usize],
            0x0400..=0x3FFF => {
                let rom_addr = (self.rom_bank_0 << 14) | u32::from(address);
                self.read_rom_address(rom_addr)
            }
            0x4000..=0x7FFF => {
                let rom_addr = (self.rom_bank_1 << 14) | u32::from(address & 0x3FFF);
                self.read_rom_address(rom_addr)
            }
            0x8000..=0xBFFF => {
                if self.ram_mapped {
                    let ram_addr = (self.ram_bank << 14) | u32::from(address & 0x3FFF);
                    self.ram[ram_addr as usize]
                } else {
                    let rom_addr = (self.rom_bank_2 << 14) | u32::from(address & 0x3FFF);
                    self.read_rom_address(rom_addr)
                }
            }
            _ => panic!("0xC000..=0xFFFF should never be read from cartridge"),
        }
    }

    fn write_ram(&mut self, address: u16, value: u8) {
        if self.ram_mapped {
            let ram_addr = (self.ram_bank << 14) | u32::from(address & 0x3FFF);
            self.ram[ram_addr as usize] = value;
        }
    }
}

const SYSTEM_RAM_SIZE: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct Memory {
    cartridge: Cartridge,
    ram: [u8; SYSTEM_RAM_SIZE],
}

impl Memory {
    pub fn new(rom: Vec<u8>) -> Self {
        Self {
            cartridge: Cartridge::from_rom(rom),
            ram: [0; SYSTEM_RAM_SIZE],
        }
    }

    pub fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0xBFFF => self.cartridge.read(address),
            0xC000..=0xFFFF => {
                let ram_addr = address & 0x1FFF;
                self.ram[ram_addr as usize]
            }
        }
    }

    pub fn write(&mut self, address: u16, value: u8) {
        if address >= 0xC000 {
            let ram_addr = address & 0x1FFF;
            self.ram[ram_addr as usize] = value;
        }

        match address {
            0x8000..=0xBFFF => {
                self.cartridge.write_ram(address, value);
            }
            0xFFFC => {
                log::trace!("RAM flags set to {value:02X}");
                self.cartridge.ram_mapped = value.bit(3);
                self.cartridge.ram_bank = value.bit(2).into();
            }
            0xFFFD => {
                log::trace!("ROM bank 0 set to {value:02X}");
                self.cartridge.rom_bank_0 = value.into();
            }
            0xFFFE => {
                log::trace!("ROM bank 1 set to {value:02X}");
                self.cartridge.rom_bank_1 = value.into();
            }
            0xFFFF => {
                log::trace!("ROM bank 2 set to {value:02X}");
                self.cartridge.rom_bank_2 = value.into();
            }
            _ => {}
        }
    }
}
