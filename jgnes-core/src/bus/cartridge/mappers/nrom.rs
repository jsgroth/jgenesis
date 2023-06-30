//! Code for the NROM board (iNES mapper 0) as well as simple NROM variants.
//!
//! Variants implemented here include:
//! * UxROM (iNES mapper 2)
//! * CNROM (iNES mapper 3)
//! * AxROM (iNES mapper 7)
//! * Color Dreams unlicensed board (iNES mapper 11, very similar to GxROM)
//! * BNROM (iNES mapper 34)
//! * NINA-001 (also iNES mapper 34, very similar to BNROM)
//! * GxROM (iNES mapper 66)
//! * Codemasters unlicensed board (iNES mapper 71, very similar to UxROM)
//! * Jaleco JF-11 / JF-14 (iNES mapper 140, very similar to GxROM)

use crate::bus;
use crate::bus::cartridge::mappers::{BankSizeKb, ChrType, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::{Cartridge, HasBasicPpuMapping, MapperImpl};
use crate::num::GetBit;
use bincode::{Decode, Encode};

fn basic_read_cpu_address(address: u16, cartridge: &Cartridge) -> u8 {
    match address {
        0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
        0x4020..=0x7FFF => bus::cpu_open_bus(address),
        0x8000..=0xFFFF => cartridge.get_prg_rom(u32::from(address & 0x7FFF)),
    }
}

fn basic_map_ppu_address(
    address: u16,
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
) -> PpuMapResult {
    match address {
        0x0000..=0x1FFF => chr_type.to_map_result(address.into()),
        0x2000..=0x3EFF => PpuMapResult::Vram(nametable_mirroring.map_to_vram(address)),
        _ => panic!("invalid PPU map address: 0x{address:04X}"),
    }
}

#[derive(Debug, Clone, Encode, Decode)]
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
        basic_read_cpu_address(address, &self.cartridge)
    }

    // Intentionally blank implementation that is present because every MapperImpl must have a
    // write_cpu_address method; see Mapper::write_cpu_address
    #[allow(clippy::unused_self)]
    pub(crate) fn write_cpu_address(&self, _address: u16, _value: u8) {}
}

impl HasBasicPpuMapping for MapperImpl<Nrom> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        basic_map_ppu_address(address, self.data.chr_type, self.data.nametable_mirroring)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub(crate) enum UxromVariant {
    Uxrom,
    Codemasters,
    FireHawk,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Uxrom {
    variant: UxromVariant,
    prg_bank: u8,
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
}

impl Uxrom {
    pub(crate) fn new(
        mapper_number: u16,
        sub_mapper_number: u8,
        chr_type: ChrType,
        nametable_mirroring: NametableMirroring,
    ) -> Self {
        let variant = match (mapper_number, sub_mapper_number) {
            (2, _) => UxromVariant::Uxrom,
            (71, 0) => UxromVariant::Codemasters,
            (71, 1) => UxromVariant::FireHawk,
            _ => panic!("invalid UxROM mapper/submapper: mapper={mapper_number}, submapper={sub_mapper_number}"),
        };

        let nametable_mirroring = match variant {
            UxromVariant::FireHawk => NametableMirroring::SingleScreenBank0,
            UxromVariant::Uxrom | UxromVariant::Codemasters => nametable_mirroring,
        };
        Self {
            variant,
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
            0x4020..=0x7FFF => bus::cpu_open_bus(address),
            0x8000..=0xBFFF => {
                let prg_rom_addr =
                    BankSizeKb::Sixteen.to_absolute_address(self.data.prg_bank, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xC000..=0xFFFF => {
                let prg_rom_addr = BankSizeKb::Sixteen
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match (self.data.variant, address) {
            (_, 0x0000..=0x401F) => panic!("invalid CPU map address: 0x{address:04X}"),
            (UxromVariant::Uxrom, 0x8000..=0xFFFF)
            | (UxromVariant::Codemasters | UxromVariant::FireHawk, 0xC000..=0xFFFF) => {
                self.data.prg_bank = value;
            }
            (UxromVariant::FireHawk, 0x8000..=0x9FFF) => {
                self.data.nametable_mirroring = if value.bit(4) {
                    NametableMirroring::SingleScreenBank1
                } else {
                    NametableMirroring::SingleScreenBank0
                };
            }
            (_, 0x4020..=0x7FFF)
            | (UxromVariant::Codemasters, 0x8000..=0x9FFF)
            | (UxromVariant::Codemasters | UxromVariant::FireHawk, 0xA000..=0xBFFF) => {}
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self.data.variant {
            UxromVariant::Uxrom => "UxROM",
            UxromVariant::Codemasters => "Codemasters",
            UxromVariant::FireHawk => "Codemasters (Fire Hawk variant)",
        }
    }
}

impl HasBasicPpuMapping for MapperImpl<Uxrom> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        basic_map_ppu_address(address, self.data.chr_type, self.data.nametable_mirroring)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
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
        basic_read_cpu_address(address, &self.cartridge)
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
}

impl HasBasicPpuMapping for MapperImpl<Cnrom> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let chr_addr = BankSizeKb::Eight.to_absolute_address(self.data.chr_bank, address);
                self.data.chr_type.to_map_result(chr_addr)
            }
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Axrom {
    chr_type: ChrType,
    prg_bank: u8,
    nametable_mirroring: NametableMirroring,
}

impl Axrom {
    pub(crate) fn new(chr_type: ChrType) -> Self {
        Self {
            chr_type,
            prg_bank: 0,
            nametable_mirroring: NametableMirroring::SingleScreenBank0,
        }
    }
}

impl MapperImpl<Axrom> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        if address < 0x8000 {
            return bus::cpu_open_bus(address);
        }

        let prg_rom_addr = BankSizeKb::ThirtyTwo.to_absolute_address(self.data.prg_bank, address);
        self.cartridge.get_prg_rom(prg_rom_addr)
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        if address < 0x8000 {
            return;
        }

        self.data.prg_bank = value & 0x07;
        self.data.nametable_mirroring = if value.bit(4) {
            NametableMirroring::SingleScreenBank1
        } else {
            NametableMirroring::SingleScreenBank0
        };
    }
}

impl HasBasicPpuMapping for MapperImpl<Axrom> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        basic_map_ppu_address(address, self.data.chr_type, self.data.nametable_mirroring)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum GxromVariant {
    Gxrom,
    ColorDreams,
    Jaleco,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Gxrom {
    variant: GxromVariant,
    prg_bank: u8,
    chr_bank: u8,
    nametable_mirroring: NametableMirroring,
}

impl Gxrom {
    pub(crate) fn new(mapper_number: u16, nametable_mirroring: NametableMirroring) -> Self {
        let variant = match mapper_number {
            11 => GxromVariant::ColorDreams,
            66 => GxromVariant::Gxrom,
            140 => GxromVariant::Jaleco,
            _ => panic!("invalid GxROM mapper number: {mapper_number}"),
        };

        Self {
            variant,
            prg_bank: 0,
            chr_bank: 0,
            nametable_mirroring,
        }
    }
}

impl MapperImpl<Gxrom> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x7FFF => bus::cpu_open_bus(address),
            0x8000..=0xFFFF => {
                let prg_rom_addr =
                    BankSizeKb::ThirtyTwo.to_absolute_address(self.data.prg_bank, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match (self.data.variant, address) {
            (_, 0x0000..=0x401F) => panic!("invalid CPU map address: {address:04X}"),
            (_, 0x4020..=0x5FFF)
            | (GxromVariant::Gxrom | GxromVariant::ColorDreams, 0x6000..=0x7FFF)
            | (GxromVariant::Jaleco, 0x8000..=0xFFFF) => {}
            (GxromVariant::Gxrom | GxromVariant::ColorDreams, 0x8000..=0xFFFF)
            | (GxromVariant::Jaleco, 0x6000..=0x7FFF) => {
                let high_nibble = (value & 0xF0) >> 4;
                let low_nibble = value & 0x0F;

                match self.data.variant {
                    GxromVariant::ColorDreams => {
                        self.data.prg_bank = low_nibble;
                        self.data.chr_bank = high_nibble;
                    }
                    GxromVariant::Gxrom | GxromVariant::Jaleco => {
                        self.data.prg_bank = high_nibble;
                        self.data.chr_bank = low_nibble;
                    }
                }
            }
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self.data.variant {
            GxromVariant::Gxrom => "GxROM",
            GxromVariant::Jaleco => "Jaleco JF-11 / JF-14",
            GxromVariant::ColorDreams => "Color Dreams",
        }
    }
}

impl HasBasicPpuMapping for MapperImpl<Gxrom> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let chr_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.chr_bank, address);
                PpuMapResult::ChrROM(chr_rom_addr)
            }
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Bnrom {
    prg_bank: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
}

impl Bnrom {
    pub(crate) fn new(chr_type: ChrType, nametable_mirroring: NametableMirroring) -> Self {
        Self {
            prg_bank: 0,
            chr_bank_0: 0,
            chr_bank_1: 1,
            chr_type,
            nametable_mirroring,
        }
    }
}

impl MapperImpl<Bnrom> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => bus::cpu_open_bus(address),
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    self.cartridge.get_prg_ram((address & 0x1FFF).into())
                } else {
                    bus::cpu_open_bus(address)
                }
            }
            0x8000..=0xFFFF => {
                let prg_rom_addr =
                    BankSizeKb::ThirtyTwo.to_absolute_address(self.data.prg_bank, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFC => {
                if !self.cartridge.prg_ram.is_empty() {
                    self.cartridge.set_prg_ram((address & 0x1FFF).into(), value);
                }
            }
            0x7FFD | 0x8000..=0xFFFF => {
                self.data.prg_bank = value & 0x03;
            }
            0x7FFE => {
                self.data.chr_bank_0 = value & 0x0F;
            }
            0x7FFF => {
                self.data.chr_bank_1 = value & 0x0F;
            }
        }
    }
}

impl HasBasicPpuMapping for MapperImpl<Bnrom> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x0FFF => {
                let chr_addr = BankSizeKb::Four.to_absolute_address(self.data.chr_bank_0, address);
                self.data.chr_type.to_map_result(chr_addr)
            }
            0x1000..=0x1FFF => {
                let chr_addr = BankSizeKb::Four.to_absolute_address(self.data.chr_bank_1, address);
                self.data.chr_type.to_map_result(chr_addr)
            }
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        }
    }
}
