pub(crate) mod action52;
pub(crate) mod bandai;
pub(crate) mod konami;
pub(crate) mod mmc1;
pub(crate) mod mmc2;
pub(crate) mod mmc3;
pub(crate) mod mmc5;
pub(crate) mod namco163;
pub(crate) mod namco175;
pub(crate) mod nrom;
pub(crate) mod sunsoft;
pub(crate) mod unrom512;

use crate::bus::cartridge::Cartridge;
use bincode::{Decode, Encode};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
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
    fn read(self, cartridge: &Cartridge) -> Option<u8> {
        match self {
            Self::PrgROM(address) => Some(cartridge.get_prg_rom(address)),
            Self::PrgRAM(address) => Some(cartridge.get_prg_ram(address)),
            Self::None => None,
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
    pub(super) fn read(self, cartridge: &Cartridge, vram: &[u8; 2048]) -> u8 {
        match self {
            Self::ChrROM(address) => cartridge.get_chr_rom(address),
            Self::ChrRAM(address) => cartridge.get_chr_ram(address),
            Self::Vram(address) => vram[address as usize],
        }
    }

    pub(super) fn write(self, value: u8, cartridge: &mut Cartridge, vram: &mut [u8; 2048]) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
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

#[cfg(test)]
pub(crate) fn new_mmc1(prg_rom: Vec<u8>) -> super::Mapper {
    use super::mappers::mmc1::Mmc1;
    use super::{Mapper, MapperImpl, TimingMode};

    Mapper::Mmc1(MapperImpl {
        cartridge: Cartridge {
            timing_mode: TimingMode::Ntsc,
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
