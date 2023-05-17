//! Code for the Namco 175 and Namco 340 boards (iNES mapper 210).

use crate::bus;
use crate::bus::cartridge::mappers::{BankSizeKb, ChrType, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::MapperImpl;
use crate::num::GetBit;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Variant {
    Namco175,
    Namco340,
    Unknown,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Namco175 {
    variant: Variant,
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
    prg_banks: [u8; 3],
    chr_banks: [u8; 8],
    ram_enabled: bool,
}

impl Namco175 {
    pub(crate) fn new(
        sub_mapper_number: u8,
        chr_type: ChrType,
        nametable_mirroring: NametableMirroring,
    ) -> Self {
        let variant = match sub_mapper_number {
            1 => Variant::Namco175,
            2 => Variant::Namco340,
            _ => Variant::Unknown,
        };

        log::info!("Namco 175 variant: {variant:?}");

        Self {
            variant,
            chr_type,
            nametable_mirroring,
            prg_banks: [0; 3],
            chr_banks: [0; 8],
            ram_enabled: false,
        }
    }
}

impl MapperImpl<Namco175> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => bus::cpu_open_bus(address),
            0x6000..=0x7FFF => {
                if self.data.ram_enabled && !self.cartridge.prg_ram.is_empty() {
                    self.cartridge.get_prg_ram((address & 0x1FFF).into())
                } else {
                    bus::cpu_open_bus(address)
                }
            }
            0x8000..=0xDFFF => {
                let bank_index = (address & 0x7FFF) / 0x2000;
                let bank_number = self.data.prg_banks[bank_index as usize];
                let prg_rom_addr = BankSizeKb::Eight.to_absolute_address(bank_number, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xE000..=0xFFFF => {
                let prg_rom_addr = BankSizeKb::Eight
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x6000..=0x7FFF => {
                if self.data.ram_enabled && !self.cartridge.prg_ram.is_empty() {
                    self.cartridge.set_prg_ram((address & 0x1FFF).into(), value);
                }
            }
            0x8000..=0xBFFF => {
                let bank_index = (address & 0x7FFF) / 0x0800;
                self.data.chr_banks[bank_index as usize] = value;
            }
            0xC000..=0xC7FF => {
                if matches!(self.data.variant, Variant::Namco175 | Variant::Unknown) {
                    self.data.ram_enabled = value.bit(0);
                }
            }
            0xE000..=0xE7FF => {
                self.data.prg_banks[0] = value & 0x3F;

                if matches!(self.data.variant, Variant::Namco340 | Variant::Unknown) {
                    self.data.nametable_mirroring = match value & 0xC0 {
                        0x00 => NametableMirroring::SingleScreenBank0,
                        0x40 => NametableMirroring::Vertical,
                        0x80 => NametableMirroring::SingleScreenBank1,
                        0xC0 => NametableMirroring::Horizontal,
                        _ => unreachable!("value & 0xC0 should always be 0x00/0x40/0x80/0xC0"),
                    };
                }
            }
            0xE800..=0xEFFF => {
                self.data.prg_banks[1] = value & 0x3F;
            }
            0xF000..=0xF7FF => {
                self.data.prg_banks[2] = value & 0x3F;
            }
            _ => {}
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let bank_index = address / 0x0400;
                let bank_number = self.data.chr_banks[bank_index as usize];
                let chr_addr = BankSizeKb::One.to_absolute_address(bank_number, address);
                self.data.chr_type.to_map_result(chr_addr)
            }
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map result: {address:04X}"),
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
