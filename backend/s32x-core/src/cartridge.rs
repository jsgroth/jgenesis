use bincode::{Decode, Encode};
use genesis_core::memory::SegaMapper;
use genesis_core::memory::eeprom::X24C02Chip;
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

const EEPROM_SCL_ADDRESS: u32 = 0x200000;
const EEPROM_SDA_ADDRESS: u32 = 0x200001;

#[derive(Debug, Clone, Encode, Decode)]
enum PersistentMemory {
    None,
    Ram { ram: Box<[u8]>, start_address: u32, end_address_exclusive: u32, dirty: bool },
    Eeprom { chip: Box<X24C02Chip>, dirty: bool },
}

impl PersistentMemory {
    fn read_byte(&self, address: u32) -> Option<u8> {
        match self {
            Self::None => None,
            Self::Ram { ram, start_address, end_address_exclusive, .. } => {
                if (*start_address..*end_address_exclusive).contains(&address) {
                    return Some(ram[map_ram_address(address, *start_address)]);
                }

                None
            }
            Self::Eeprom { chip, .. } => match address {
                EEPROM_SDA_ADDRESS => Some(chip.handle_read().into()),
                _ => None,
            },
        }
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match self {
            Self::None => {}
            Self::Ram { ram, start_address, end_address_exclusive, dirty } => {
                if (*start_address..*end_address_exclusive).contains(&address) {
                    let address = map_ram_address(address, *start_address);
                    ram[address] = value;
                    *dirty = true;
                }
            }
            Self::Eeprom { chip, dirty } => match address {
                EEPROM_SCL_ADDRESS => {
                    chip.handle_clock_write(value.bit(0));
                }
                EEPROM_SDA_ADDRESS => {
                    chip.handle_data_write(value.bit(0));
                    *dirty = true;
                }
                _ => {}
            },
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        match self {
            Self::None => {}
            Self::Ram { ram, start_address, end_address_exclusive, dirty } => {
                if (*start_address..*end_address_exclusive).contains(&address) {
                    let address = map_ram_address(address, *start_address);
                    // TODO assuming RAM is always at the odd addresses
                    ram[address] = value as u8;
                    *dirty = true;
                }
            }
            Self::Eeprom { chip, dirty } => {
                if address == EEPROM_SCL_ADDRESS {
                    let scl = value.bit(8);
                    let sda = value.bit(0);
                    chip.handle_dual_write(sda, scl);
                    *dirty = true;
                }
            }
        }
    }
}

fn map_ram_address(address: u32, start_address: u32) -> usize {
    ((address - start_address) >> 1) as usize
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Cartridge {
    #[partial_clone(default)]
    pub rom: Rom,
    mapper: Option<SegaMapper>,
    persistent: PersistentMemory,
    ram_mapped: bool,
}

impl Cartridge {
    pub fn new(rom: Box<[u8]>, initial_ram: Option<Vec<u8>>) -> Self {
        let mapper = SegaMapper::should_use(&rom).then(SegaMapper::new);

        log::info!("Using SSF mapper for ROM banking: {}", mapper.is_some());

        let has_ram = &rom[0x1B0..0x1B2] == "RA".as_bytes();
        let has_eeprom = has_ram && rom[0x1B2] == 0xE8;

        // TODO check RAM type? assuming 8-bit at odd addresses

        let persistent = if has_eeprom {
            log::info!("Cartridge has EEPROM, assuming 24C02 chip mapped to $200000-$200001");

            PersistentMemory::Eeprom {
                chip: Box::new(X24C02Chip::new(initial_ram.as_ref())),
                dirty: false,
            }
        } else if has_ram {
            let start_address = u32::from_be_bytes(rom[0x1B4..0x1B8].try_into().unwrap());
            let end_address = u32::from_be_bytes(rom[0x1B8..0x1BC].try_into().unwrap());
            let ram_len = (((end_address >> 1) + 1) - (start_address >> 1)) as usize;

            let ram = match initial_ram {
                Some(initial_ram) if initial_ram.len() == ram_len => initial_ram.into_boxed_slice(),
                _ => vec![0xFF; ram_len].into_boxed_slice(),
            };

            log::info!("Cartridge RAM address range: ${start_address:06X}-${end_address:06X}");

            PersistentMemory::Ram {
                ram,
                start_address,
                end_address_exclusive: (end_address & !1) + 2,
                dirty: false,
            }
        } else {
            PersistentMemory::None
        };

        // Map RAM by default unless there is none
        let ram_mapped = !matches!(persistent, PersistentMemory::None);

        Self { rom: Rom(rom), mapper, persistent, ram_mapped }
    }

    pub fn read_byte(&self, address: u32) -> u8 {
        if self.ram_mapped {
            if let Some(value) = self.persistent.read_byte(address) {
                return value;
            }
        }

        let rom_addr = self.mapper.map_or(address, |mapper| mapper.map_address(address));
        self.rom.get(rom_addr as usize).copied().unwrap_or(0xFF)
    }

    pub fn read_word(&self, address: u32) -> u16 {
        // TODO handle cartridge RAM reads?
        let rom_addr = self.mapper.map_or(address, |mapper| mapper.map_address(address));
        self.rom.get_u16(rom_addr)
    }

    pub fn read_longword(&self, address: u32) -> u32 {
        // TODO handle cartridge RAM reads?
        let rom_addr = self.mapper.map_or(address, |mapper| mapper.map_address(address));
        self.rom.get_u32(rom_addr)
    }

    pub fn write_byte(&mut self, address: u32, value: u8) {
        if !self.ram_mapped {
            return;
        }

        self.persistent.write_byte(address, value);
    }

    pub fn write_word(&mut self, address: u32, value: u16) {
        if !self.ram_mapped {
            return;
        }

        self.persistent.write_word(address, value);
    }

    pub fn read_ram_register(&self) -> u8 {
        self.ram_mapped.into()
    }

    pub fn write_ram_register(&mut self, value: u8) {
        self.ram_mapped = value.bit(0);
        log::trace!("Cartridge RAM register write ({value:02X}); RAM mapped = {}", self.ram_mapped);
    }

    pub fn write_mapper_bank_register(&mut self, address: u32, value: u8) {
        let Some(mapper) = &mut self.mapper else { return };
        mapper.write(address, value);
    }

    pub fn persistent_memory(&self) -> &[u8] {
        match &self.persistent {
            PersistentMemory::None => &[],
            PersistentMemory::Ram { ram, .. } => ram,
            PersistentMemory::Eeprom { chip, .. } => chip.get_memory(),
        }
    }

    pub fn persistent_memory_dirty(&self) -> bool {
        match &self.persistent {
            PersistentMemory::None => false,
            &PersistentMemory::Ram { dirty, .. } | &PersistentMemory::Eeprom { dirty, .. } => dirty,
        }
    }

    pub fn clear_persistent_dirty_bit(&mut self) {
        match &mut self.persistent {
            PersistentMemory::None => {}
            PersistentMemory::Ram { dirty, .. } | PersistentMemory::Eeprom { dirty, .. } => {
                *dirty = false;
            }
        }
    }
}
