use std::cmp::Ordering;
use tinyvec::ArrayVec;

pub const CPU_RAM_START: u16 = 0x0000;
pub const CPU_RAM_END: u16 = 0x1FFF;
pub const CPU_RAM_MASK: u16 = 0x07FF;

pub const CPU_PPU_REGISTERS_START: u16 = 0x2000;
pub const CPU_PPU_REGISTERS_END: u16 = 0x3FFF;
pub const CPU_PPU_REGISTERS_MASK: u16 = 0x0007;

pub const CPU_IO_REGISTERS_START: u16 = 0x4000;
pub const CPU_IO_REGISTERS_END: u16 = 0x4017;

pub const CPU_IO_TEST_MODE_START: u16 = 0x4018;
pub const CPU_IO_TEST_MODE_END: u16 = 0x401F;

pub const CPU_CARTRIDGE_START: u16 = 0x4020;
pub const CPU_CARTRIDGE_END: u16 = 0xFFFF;

pub const PPU_PATTERN_TABLES_START: u16 = 0x0000;
pub const PPU_PATTERN_TABLES_END: u16 = 0x1FFF;

pub const PPU_NAMETABLES_START: u16 = 0x2000;
pub const PPU_NAMETABLES_END: u16 = 0x3EFF;
pub const PPU_NAMETABLES_MASK: u16 = 0x0FFF;

pub const PPU_PALETTES_START: u16 = 0x3F00;
pub const PPU_PALETTES_END: u16 = 0x3FFF;
pub const PPU_PALETTES_MASK: u16 = 0x001F;

#[derive(Debug, Clone, Copy)]
enum CpuMapResult {
    PrgROM(u16),
    PrgRAM(u16),
    None,
}

#[derive(Debug, Clone, Copy)]
enum PpuMapResult {
    ChrROM(u16),
    ChrRAM(u16),
    Vram(u16),
    None,
}

#[derive(Debug, Clone)]
enum Mapper {
    Nrom,
}

impl Mapper {
    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        todo!()
    }

    fn write_cpu_address(&mut self, address: u16, value: u8) {
        todo!()
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        todo!()
    }

    fn write_ppu_address(&mut self, address: u16, value: u8) {
        todo!()
    }
}

#[derive(Debug, Clone)]
struct Cartridge {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteSource {
    Cpu,
    Ppu,
}

impl PartialOrd for WriteSource {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WriteSource {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Cpu, Self::Ppu) => Ordering::Less,
            (Self::Ppu, Self::Cpu) => Ordering::Greater,
            _ => Ordering::Equal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressType {
    Cpu,
    Ppu,
}

impl PartialOrd for AddressType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AddressType {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Cpu, Self::Ppu) => Ordering::Less,
            (Self::Ppu, Self::Cpu) => Ordering::Greater,
            _ => Ordering::Equal,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingWrite {
    address_type: AddressType,
    address: u16,
    value: u8,
}

impl Default for PendingWrite {
    fn default() -> Self {
        Self {
            address_type: AddressType::Cpu,
            address: 0,
            value: 0,
        }
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuRegister {
    PPUCTRL,
    PPUMASK,
    PPUSTATUS,
    OAMADDR,
    OAMDATA,
    PPUSCROLL,
    PPUADDR,
    PPUDATA,
}

impl PpuRegister {
    fn from_relative_address(relative_addr: usize) -> Option<Self> {
        match relative_addr {
            0x00 => Some(Self::PPUCTRL),
            0x01 => Some(Self::PPUMASK),
            0x02 => Some(Self::PPUSTATUS),
            0x03 => Some(Self::OAMADDR),
            0x04 => Some(Self::OAMDATA),
            0x05 => Some(Self::PPUSCROLL),
            0x06 => Some(Self::PPUADDR),
            0x07 => Some(Self::PPUDATA),
            _ => None,
        }
    }

    const fn to_relative_address(self) -> usize {
        match self {
            Self::PPUCTRL => 0x00,
            Self::PPUMASK => 0x01,
            Self::PPUSTATUS => 0x02,
            Self::OAMADDR => 0x03,
            Self::OAMDATA => 0x04,
            Self::PPUSCROLL => 0x05,
            Self::PPUADDR => 0x06,
            Self::PPUDATA => 0x07,
        }
    }
}

pub struct PpuRegisters {
    data: [u8; 8],
}

pub struct IoRegisters;

pub struct Bus {
    cartridge: Cartridge,
    mapper: Mapper,
    cpu_internal_ram: [u8; 2048],
    ppu_registers: PpuRegisters,
    io_registers: IoRegisters,
    ppu_vram: [u8; 2048],
    ppu_palette_ram: [u8; 64],
    ppu_oam: [u8; 256],
    pending_writes: ArrayVec<[PendingWrite; 10]>,
}

impl Bus {
    pub fn cpu(&mut self) -> CpuBus<'_> {
        CpuBus(self)
    }

    pub fn ppu(&mut self) -> PpuBus<'_> {
        PpuBus(self)
    }

    pub fn flush(&mut self) {
        todo!()
    }
}

pub struct CpuBus<'a>(&'a mut Bus);

impl<'a> CpuBus<'a> {
    pub fn read_address(&mut self, address: u16) -> u8 {
        match address {
            address @ CPU_RAM_START..=CPU_RAM_END => {
                let ram_address = address & CPU_RAM_MASK;
                self.0.cpu_internal_ram[ram_address as usize]
            }
            address @ CPU_PPU_REGISTERS_START..=CPU_PPU_REGISTERS_END => {
                let ppu_register_relative_addr =
                    (address - CPU_PPU_REGISTERS_START) & CPU_PPU_REGISTERS_MASK;
                self.read_ppu_register(ppu_register_relative_addr as usize)
            }
            address @ CPU_IO_REGISTERS_START..=CPU_IO_REGISTERS_END => {
                todo!()
            }
            _address @ CPU_IO_TEST_MODE_START..=CPU_IO_TEST_MODE_END => 0xFF,
            address @ CPU_CARTRIDGE_START..=CPU_CARTRIDGE_END => {
                match self.0.mapper.map_cpu_address(address) {
                    CpuMapResult::PrgROM(prg_rom_address) => {
                        self.0.cartridge.prg_rom[prg_rom_address as usize]
                    }
                    CpuMapResult::PrgRAM(prg_ram_address) => {
                        self.0.cartridge.prg_ram[prg_ram_address as usize]
                    }
                    CpuMapResult::None => 0xFF,
                }
            }
        }
    }

    pub fn write_address(&mut self, address: u16, value: u8) {
        todo!()
    }

    fn read_ppu_register(&mut self, relative_addr: usize) -> u8 {
        todo!()
    }

    fn write_ppu_register(&mut self, relative_addr: usize, value: u8) {
        todo!()
    }
}

pub struct PpuBus<'a>(&'a mut Bus);

impl<'a> PpuBus<'a> {
    pub fn read_address(&self, address: u16) -> u8 {
        // PPU bus only has 14-bit addressing
        let address = address & 0x3FFF;
        match address {
            address @ PPU_PATTERN_TABLES_START..=PPU_NAMETABLES_END => {
                match self.0.mapper.map_ppu_address(address) {
                    PpuMapResult::ChrROM(chr_rom_address) => {
                        self.0.cartridge.chr_rom[chr_rom_address as usize]
                    }
                    PpuMapResult::ChrRAM(chr_ram_address) => {
                        self.0.cartridge.chr_ram[chr_ram_address as usize]
                    }
                    PpuMapResult::Vram(vram_address) => self.0.ppu_vram[vram_address as usize],
                    PpuMapResult::None => 0xFF,
                }
            }
            address @ PPU_PALETTES_START..=PPU_PALETTES_END => {
                let palette_relative_addr =
                    ((address - PPU_PALETTES_START) & PPU_PALETTES_MASK) as usize;
                self.0.ppu_palette_ram[palette_relative_addr]
            }
            _address @ 0x4000..=0xFFFF => {
                panic!("{address} should be <= 0x3FFF after masking with 0x3FFF")
            }
        }
    }

    pub fn write_address(&self, address: u16, value: u8) {
        todo!()
    }
}
