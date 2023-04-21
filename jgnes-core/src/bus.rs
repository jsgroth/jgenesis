pub mod cartridge;

use crate::bus::cartridge::{CpuMapResult, Mapper, PpuMapResult};
use cartridge::Cartridge;
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

pub const CPU_STACK_START: u16 = 0x0100;
pub const CPU_NMI_VECTOR: u16 = 0xFFFA;
pub const CPU_RESET_VECTOR: u16 = 0xFFFC;
pub const CPU_IRQ_VECTOR: u16 = 0xFFFE;

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

// TODO implement
pub struct PpuRegisters {
    data: [u8; 8],
}

impl PpuRegisters {
    pub fn new() -> Self {
        Self { data: [0; 8] }
    }
}

// TODO implement
pub struct IoRegisters;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptLine {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqSource {
    ApuDmc,
    ApuFrameCounter,
    Mapper,
}

impl IrqSource {
    fn to_low_pull_bit(self) -> u8 {
        match self {
            Self::ApuDmc => 0x01,
            Self::ApuFrameCounter => 0x02,
            Self::Mapper => 0x04,
        }
    }
}

#[derive(Debug)]
pub struct InterruptLines {
    nmi_line: InterruptLine,
    next_nmi_line: InterruptLine,
    nmi_triggered: bool,
    irq_line: InterruptLine,
    irq_low_pulls: u8,
}

impl InterruptLines {
    fn new() -> Self {
        Self {
            nmi_line: InterruptLine::High,
            next_nmi_line: InterruptLine::High,
            nmi_triggered: false,
            irq_line: InterruptLine::High,
            irq_low_pulls: 0x00,
        }
    }

    fn tick(&mut self) {
        if self.nmi_line == InterruptLine::High && self.next_nmi_line == InterruptLine::Low {
            self.nmi_triggered = true;
        }

        self.nmi_line = self.next_nmi_line;
        self.irq_line = if self.irq_low_pulls != 0 {
            InterruptLine::Low
        } else {
            InterruptLine::High
        };
    }

    pub fn nmi_triggered(&self) -> bool {
        self.nmi_triggered
    }

    pub fn clear_nmi_triggered(&mut self) {
        self.nmi_triggered = false;
    }

    pub fn ppu_set_nmi_line(&mut self, interrupt_line: InterruptLine) {
        self.next_nmi_line = interrupt_line;
    }

    pub fn irq_triggered(&self) -> bool {
        self.irq_line == InterruptLine::Low
    }

    pub fn pull_irq_low(&mut self, source: IrqSource) {
        self.irq_low_pulls |= source.to_low_pull_bit();
    }

    pub fn release_irq_low_pull(&mut self, source: IrqSource) {
        self.irq_low_pulls &= !source.to_low_pull_bit();
    }
}

pub struct Bus {
    cartridge: Cartridge,
    mapper: Mapper,
    cpu_internal_ram: [u8; 2048],
    ppu_registers: PpuRegisters,
    io_registers: IoRegisters,
    ppu_vram: [u8; 2048],
    ppu_palette_ram: [u8; 64],
    ppu_oam: [u8; 256],
    interrupt_lines: InterruptLines,
    pending_writes: ArrayVec<[PendingWrite; 5]>,
}

impl Bus {
    pub(crate) fn from_cartridge(cartridge: Cartridge, mapper: Mapper) -> Self {
        Self {
            cartridge,
            mapper,
            cpu_internal_ram: [0; 2048],
            ppu_registers: PpuRegisters::new(),
            io_registers: IoRegisters,
            ppu_vram: [0; 2048],
            ppu_palette_ram: [0; 64],
            ppu_oam: [0; 256],
            interrupt_lines: InterruptLines::new(),
            pending_writes: ArrayVec::new(),
        }
    }

    pub fn cpu(&mut self) -> CpuBus<'_> {
        CpuBus(self)
    }

    pub fn ppu(&mut self) -> PpuBus<'_> {
        PpuBus(self)
    }

    pub fn interrupt_lines(&mut self) -> &mut InterruptLines {
        &mut self.interrupt_lines
    }

    pub fn tick(&mut self) {
        let writes: ArrayVec<[PendingWrite; 5]> = self.pending_writes.drain(..).collect();
        for write in writes {
            match write.address_type {
                AddressType::Cpu => {
                    self.cpu().apply_write(write.address, write.value);
                }
                AddressType::Ppu => {
                    todo!()
                }
            }
        }
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

    fn apply_write(&mut self, address: u16, value: u8) {
        match address {
            address @ CPU_RAM_START..=CPU_RAM_END => {
                let ram_address = address & CPU_RAM_MASK;
                self.0.cpu_internal_ram[ram_address as usize] = value;
            }
            address @ CPU_PPU_REGISTERS_START..=CPU_PPU_REGISTERS_END => {
                let ppu_register_relative_addr =
                    (address - CPU_PPU_REGISTERS_START) & CPU_PPU_REGISTERS_MASK;
                self.write_ppu_register(ppu_register_relative_addr as usize, value);
            }
            address @ CPU_IO_REGISTERS_START..=CPU_IO_REGISTERS_END => {
                todo!()
            }
            _address @ CPU_IO_TEST_MODE_START..=CPU_IO_TEST_MODE_END => {}
            address @ CPU_CARTRIDGE_START..=CPU_CARTRIDGE_END => {
                self.0.mapper.write_cpu_address(address, value);
            }
        }
    }

    pub fn write_address(&mut self, address: u16, value: u8) {
        self.0.pending_writes.push(PendingWrite {
            address_type: AddressType::Cpu,
            address,
            value,
        });
    }

    pub fn interrupt_lines(&mut self) -> &mut InterruptLines {
        &mut self.0.interrupt_lines
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
