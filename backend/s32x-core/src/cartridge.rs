use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::ops::Deref;

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
pub struct Rom(pub Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Rom {
    pub fn get_u16(&self, address: u32) -> u16 {
        let address = address as usize;
        if address + 1 < self.0.len() {
            u16::from_be_bytes(self.0[address..address + 2].try_into().unwrap())
        } else {
            0xFFFF
        }
    }

    pub fn get_u32(&self, address: u32) -> u32 {
        let address = address as usize;
        if address + 3 < self.0.len() {
            u32::from_be_bytes(self.0[address..address + 4].try_into().unwrap())
        } else {
            0xFFFFFFFF
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CartridgeRam {
    pub ram: Box<[u8]>,
    pub dirty: bool,
    pub start_address: u32,
    pub end_address_exclusive: u32,
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Cartridge {
    #[partial_clone(default)]
    pub rom: Rom,
    pub ram: Option<CartridgeRam>,
    pub ram_mapped: bool,
}

impl Cartridge {
    pub fn new(rom: Box<[u8]>, initial_ram: Option<Vec<u8>>) -> Self {
        let has_ram = &rom[0x1B0..0x1B2] == "RA".as_bytes();

        // TODO check RAM type? assuming 8-bit at odd addresses

        let ram = has_ram.then(|| {
            let start_address = u32::from_be_bytes(rom[0x1B4..0x1B8].try_into().unwrap());
            let end_address = u32::from_be_bytes(rom[0x1B8..0x1BC].try_into().unwrap());
            let ram_len = (((end_address >> 1) + 1) - (start_address >> 1)) as usize;

            let ram = match initial_ram {
                Some(initial_ram) if initial_ram.len() == ram_len => initial_ram.into_boxed_slice(),
                _ => vec![0xFF; ram_len].into_boxed_slice(),
            };

            log::info!("Cartridge RAM address range: ${start_address:06X}-${end_address:06X}");

            CartridgeRam {
                ram,
                dirty: false,
                start_address,
                end_address_exclusive: (end_address & !1) + 2,
            }
        });

        Self { rom: Rom(rom), ram, ram_mapped: false }
    }

    pub fn read_byte(&self, address: u32) -> u8 {
        if self.ram_mapped {
            if let Some(ram) = &self.ram {
                if (ram.start_address..ram.end_address_exclusive).contains(&address) {
                    return ram.ram[((address - ram.start_address) >> 1) as usize];
                }
            }
        }

        self.rom.get(address as usize).copied().unwrap_or(0xFF)
    }

    pub fn read_word(&self, address: u32) -> u16 {
        // TODO handle cartridge RAM reads?
        self.rom.get_u16(address)
    }

    pub fn read_longword(&self, address: u32) -> u32 {
        // TODO handle cartridge RAM reads?
        self.rom.get_u32(address)
    }

    pub fn write_byte(&mut self, address: u32, value: u8) {
        if !self.ram_mapped {
            return;
        }

        let Some(ram) = &mut self.ram else { return };

        if !(ram.start_address..ram.end_address_exclusive).contains(&address) {
            return;
        }

        let ram_addr = (address - ram.start_address) >> 1;
        ram.ram[ram_addr as usize] = value;
        ram.dirty = true;
    }

    pub fn read_ram_register(&self) -> u8 {
        self.ram_mapped.into()
    }

    pub fn write_ram_register(&mut self, value: u8) {
        self.ram_mapped = value.bit(0);
        log::trace!("Cartridge RAM register write ({value:02X}); RAM mapped = {}", self.ram_mapped);
    }
}
