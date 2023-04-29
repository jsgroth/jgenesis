mod mmc1;
mod mmc3;
mod mmc5;
mod nrom;

use crate::bus::cartridge::Cartridge;

pub(crate) use mmc1::Mmc1;
pub(crate) use mmc3::Mmc3;
pub(crate) use mmc5::Mmc5;
pub(crate) use nrom::{Axrom, Cnrom, Nrom, Uxrom};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChrType {
    ROM,
    RAM,
}

impl ChrType {
    fn to_map_result(self, address: u32) -> PpuMapResult {
        match self {
            Self::ROM => PpuMapResult::ChrROM(address),
            Self::RAM => PpuMapResult::ChrRAM(address),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NametableMirroring {
    Horizontal,
    Vertical,
}

impl NametableMirroring {
    fn map_to_vram(self, address: u16) -> u16 {
        assert!((0x2000..=0x3EFF).contains(&address));

        let relative_addr = address & 0x0FFF;

        match self {
            Self::Horizontal => ((relative_addr & 0x0800) >> 1) | (relative_addr & 0x03FF),
            Self::Vertical => relative_addr & 0x07FF,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CpuMapResult {
    PrgROM(u32),
    PrgRAM(u32),
    None,
}

impl CpuMapResult {
    fn read(self, cartridge: &Cartridge) -> u8 {
        match self {
            Self::PrgROM(address) => {
                cartridge.prg_rom[(address as usize) & (cartridge.prg_rom.len() - 1)]
            }
            Self::PrgRAM(address) => {
                cartridge.prg_ram[(address as usize) & (cartridge.prg_ram.len() - 1)]
            }
            Self::None => 0xFF,
        }
    }

    fn write(self, value: u8, cartridge: &mut Cartridge) {
        if let Self::PrgRAM(address) = self {
            cartridge.prg_ram[address as usize] = value;
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PpuMapResult {
    ChrROM(u32),
    ChrRAM(u32),
    Vram(u16),
}

impl PpuMapResult {
    fn read(self, cartridge: &Cartridge, vram: &[u8; 2048]) -> u8 {
        match self {
            Self::ChrROM(address) => cartridge.chr_rom[address as usize],
            Self::ChrRAM(address) => cartridge.chr_ram[address as usize],
            Self::Vram(address) => vram[address as usize],
        }
    }

    fn write(self, value: u8, cartridge: &mut Cartridge, vram: &mut [u8; 2048]) {
        match self {
            Self::ChrROM(_) => {}
            Self::ChrRAM(address) => {
                cartridge.chr_ram[address as usize] = value;
            }
            Self::Vram(address) => {
                vram[address as usize] = value;
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn new_mmc1(prg_rom: Vec<u8>) -> super::Mapper {
    use super::{Mapper, MapperImpl};

    Mapper::Mmc1(MapperImpl {
        cartridge: Cartridge {
            prg_rom,
            prg_ram: vec![0; 8192],
            chr_rom: vec![0; 8192],
            chr_ram: Vec::new(),
        },
        data: Mmc1::new(ChrType::ROM),
    })
}
