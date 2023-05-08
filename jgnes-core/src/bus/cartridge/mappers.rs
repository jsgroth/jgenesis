mod konami;
mod mmc1;
mod mmc2;
mod mmc3;
mod mmc5;
mod nrom;
mod sunsoft;

use crate::bus::cartridge::Cartridge;

pub(crate) use konami::{Vrc4, Vrc6};
pub(crate) use mmc1::Mmc1;
pub(crate) use mmc2::Mmc2;
pub(crate) use mmc3::Mmc3;
pub(crate) use mmc5::Mmc5;
pub(crate) use nrom::{Axrom, Cnrom, ColorDreams, Nrom, Uxrom};
pub(crate) use sunsoft::Sunsoft;

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
    SingleScreenBank0,
    SingleScreenBank1,
}

impl NametableMirroring {
    fn map_to_vram(self, address: u16) -> u16 {
        assert!((0x2000..=0x3EFF).contains(&address));

        let relative_addr = address & 0x0FFF;

        match self {
            Self::Horizontal => ((relative_addr & 0x0800) >> 1) | (relative_addr & 0x03FF),
            Self::Vertical => relative_addr & 0x07FF,
            Self::SingleScreenBank0 => relative_addr & 0x03FF,
            Self::SingleScreenBank1 => 0x0400 | (relative_addr & 0x03FF),
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
            Self::PrgROM(address) => cartridge.get_prg_rom(address),
            Self::PrgRAM(address) => cartridge.get_prg_ram(address),
            Self::None => 0xFF,
        }
    }

    fn write(self, value: u8, cartridge: &mut Cartridge) {
        if let Self::PrgRAM(address) = self {
            cartridge.set_prg_ram(address, value);
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
            Self::ChrROM(address) => cartridge.get_chr_rom(address),
            Self::ChrRAM(address) => cartridge.get_chr_ram(address),
            Self::Vram(address) => vram[address as usize],
        }
    }

    fn write(self, value: u8, cartridge: &mut Cartridge, vram: &mut [u8; 2048]) {
        match self {
            Self::ChrROM(_) => {}
            Self::ChrRAM(address) => {
                cartridge.set_chr_ram(address, value);
            }
            Self::Vram(address) => {
                vram[address as usize] = value;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BankSizeKb {
    One,
    Two,
    Four,
    Eight,
    Sixteen,
    ThirtyTwo,
}

impl BankSizeKb {
    fn shift(self) -> u32 {
        match self {
            Self::One => 10,
            Self::Two => 11,
            Self::Four => 12,
            Self::Eight => 13,
            Self::Sixteen => 14,
            Self::ThirtyTwo => 15,
        }
    }

    fn address_mask(self) -> u16 {
        match self {
            Self::One => 0x03FF,
            Self::Two => 0x07FF,
            Self::Four => 0x0FFF,
            Self::Eight => 0x1FFF,
            Self::Sixteen => 0x3FFF,
            Self::ThirtyTwo => 0x7FFF,
        }
    }

    fn to_absolute_address<N: Into<u32>>(self, bank_number: N, address: u16) -> u32 {
        (bank_number.into() << self.shift()) | u32::from(address & self.address_mask())
    }

    fn to_absolute_address_from_end<N: Into<u32>>(
        self,
        inverse_bank_number: N,
        memory_len: u32,
        address: u16,
    ) -> u32 {
        (((memory_len >> self.shift()) - inverse_bank_number.into()) << self.shift())
            | u32::from(address & self.address_mask())
    }

    fn to_absolute_address_last_bank(self, memory_len: u32, address: u16) -> u32 {
        self.to_absolute_address_from_end(1_u32, memory_len, address)
    }
}

fn cpu_open_bus(address: u16) -> u8 {
    (address >> 8) as u8
}

#[cfg(test)]
pub(crate) fn new_mmc1(prg_rom: Vec<u8>) -> super::Mapper {
    use super::{Mapper, MapperImpl};

    Mapper::Mmc1(MapperImpl {
        cartridge: Cartridge {
            prg_rom,
            prg_ram: vec![0; 8192],
            has_ram_battery: false,
            prg_ram_dirty_bit: false,
            chr_rom: vec![0; 8192],
            chr_ram: Vec::new(),
        },
        data: Mmc1::new(ChrType::ROM),
    })
}
