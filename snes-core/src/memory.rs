use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

const MAIN_RAM_LEN: usize = 128 * 1024;

type MainRam = [u8; MAIN_RAM_LEN];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CartridgeLocation {
    Rom(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Mapper {
    LoRom,
}

impl Mapper {
    fn guess_from_rom(_rom: &[u8]) -> Option<Self> {
        // TODO actually try to guess the mapper
        Some(Mapper::LoRom)
    }

    fn map_address(self, address: u32) -> CartridgeLocation {
        match self {
            Self::LoRom => {
                // TODO handle SRAM
                let rom_addr = ((address & 0xFF0000) >> 1) | (address & 0x007FFF);
                CartridgeLocation::Rom(rom_addr)
            }
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Cartridge {
    rom: Box<[u8]>,
    mapper: Mapper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Memory2Speed {
    Fast,
    #[default]
    Slow,
}

impl Memory2Speed {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(0) { Self::Fast } else { Self::Slow }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct CpuInternalRegisters {
    // TODO use this
    memory_2_speed: Memory2Speed,
}

impl CpuInternalRegisters {
    fn new() -> Self {
        Self { memory_2_speed: Memory2Speed::default() }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    cartridge: Cartridge,
    main_ram: Box<MainRam>,
    cpu_registers: CpuInternalRegisters,
}

impl Memory {
    pub fn from_rom(rom: Vec<u8>) -> Self {
        let mapper = Mapper::guess_from_rom(&rom).expect("unable to determine mapper");
        let cartridge = Cartridge { rom: rom.into_boxed_slice(), mapper };

        Self {
            cartridge,
            main_ram: vec![0; MAIN_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            cpu_registers: CpuInternalRegisters::new(),
        }
    }

    pub fn read_cartridge(&mut self, address: u32) -> u8 {
        match self.cartridge.mapper.map_address(address) {
            CartridgeLocation::Rom(rom_addr) => {
                // TODO figure out mirroring for unusual ROM sizes
                self.cartridge.rom[(rom_addr as usize) % self.cartridge.rom.len()]
            }
        }
    }

    pub fn write_cartridge(&mut self, address: u32, value: u8) {
        todo!("write cartridge {address:06X} {value:02X}")
    }

    pub fn read_wram(&self, address: u32) -> u8 {
        self.main_ram[(address as usize) & (MAIN_RAM_LEN - 1)]
    }

    pub fn write_wram(&mut self, address: u32, value: u8) {
        self.main_ram[(address as usize) & (MAIN_RAM_LEN - 1)] = value;
    }

    pub fn read_cpu_register(&mut self, address: u32) -> u8 {
        todo!("read CPU register {address:06X}")
    }

    pub fn write_cpu_register(&mut self, address: u32, value: u8) {
        match address & 0xFFFF {
            0x420D => {
                // MEMSEL: Memory-2 waitstate control
                self.cpu_registers.memory_2_speed = Memory2Speed::from_byte(value);
            }
            _ => todo!("write CPU register {address:06X} {value:02X}"),
        }
    }
}
