use crate::bus::cartridge::mappers::{ChrType, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::MapperImpl;

#[derive(Debug, Clone)]
pub(crate) struct Nrom {
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
}

impl Nrom {
    pub(crate) fn new(chr_type: ChrType, nametable_mirroring: NametableMirroring) -> Self {
        Self {
            chr_type,
            nametable_mirroring,
        }
    }
}

impl MapperImpl<Nrom> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => 0xFF,
            0x8000..=0xFFFF => self.cartridge.get_prg_rom(u32::from(address & 0x7FFF)),
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => self.data.chr_type.to_map_result(address.into()),
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Uxrom {
    prg_bank: u8,
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
}

impl Uxrom {
    pub(crate) fn new(chr_type: ChrType, nametable_mirroring: NametableMirroring) -> Self {
        Self {
            prg_bank: 0,
            chr_type,
            nametable_mirroring,
        }
    }
}

impl MapperImpl<Uxrom> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => 0xFF,
            0x8000..=0xBFFF => {
                let bank_address = u32::from(self.data.prg_bank) << 14;
                let prg_rom_addr = bank_address + u32::from(address & 0x3FFF);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xC000..=0xFFFF => {
                let last_bank_address = self.cartridge.prg_rom.len() - (1 << 14);
                let prg_rom_addr = last_bank_address + usize::from(address & 0x3FFF);
                self.cartridge.get_prg_rom(prg_rom_addr as u32)
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => {}
            0x8000..=0xFFFF => {
                self.data.prg_bank = value;
            }
        }
    }

    pub(crate) fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => self.data.chr_type.to_map_result(address.into()),
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Cnrom {
    chr_type: ChrType,
    chr_bank: u8,
    nametable_mirroring: NametableMirroring,
}

impl Cnrom {
    pub(crate) fn new(chr_type: ChrType, nametable_mirroring: NametableMirroring) -> Self {
        Self {
            chr_type,
            chr_bank: 0,
            nametable_mirroring,
        }
    }
}

impl MapperImpl<Cnrom> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => 0xFF,
            0x8000..=0xFFFF => self.cartridge.get_prg_rom(u32::from(address & 0x7FFF)),
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => {}
            0x8000..=0xFFFF => {
                self.data.chr_bank = value;
            }
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let chr_address = u32::from(self.data.chr_bank) * 8192 + u32::from(address);
                self.data.chr_type.to_map_result(chr_address)
            }
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Axrom {
    chr_type: ChrType,
    prg_bank: u8,
    nametable_vram_bank: u8,
}

impl Axrom {
    pub(crate) fn new(chr_type: ChrType) -> Self {
        Self {
            chr_type,
            prg_bank: 0,
            nametable_vram_bank: 0,
        }
    }
}

impl MapperImpl<Axrom> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        if address < 0x8000 {
            return 0xFF;
        }

        let bank_address = u32::from(self.data.prg_bank) << 15;
        self.cartridge
            .get_prg_rom(bank_address | u32::from(address & 0x7FFF))
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        if address < 0x8000 {
            return;
        }

        self.data.prg_bank = value & 0x07;
        self.data.nametable_vram_bank = (value & 0x10) >> 4;
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => self.data.chr_type.to_map_result(address.into()),
            0x2000..=0x3EFF => PpuMapResult::Vram(
                (u16::from(self.data.nametable_vram_bank) << 10) | (address & 0x03FF),
            ),
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }
}
