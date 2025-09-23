//! Code for Konami's VRC2 and VRC4 boards (iNES mappers 21 + 22 + 23 + 25).

use crate::bus::cartridge::mappers::konami::irq::VrcIrqCounter;
use crate::bus::cartridge::mappers::{
    BankSizeKb, ChrType, NametableMirroring, PpuMapResult, konami,
};
use crate::bus::cartridge::{HasBasicPpuMapping, MapperImpl};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type {
    Vrc2,
    Vrc4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum SingleVariant {
    Vrc2a,
    Vrc2b,
    Vrc2c,
    Vrc4a,
    Vrc4b,
    Vrc4c,
    Vrc4d,
    Vrc4e,
    Vrc4f,
}

impl SingleVariant {
    fn a0(self, address: u16) -> bool {
        match self {
            Self::Vrc2b | Self::Vrc4f => address.bit(0),
            Self::Vrc2a | Self::Vrc2c | Self::Vrc4a | Self::Vrc4b => address.bit(1),
            Self::Vrc4c => address.bit(6),
            Self::Vrc4d => address.bit(3),
            Self::Vrc4e => address.bit(2),
        }
    }

    fn a1(self, address: u16) -> bool {
        match self {
            Self::Vrc2b | Self::Vrc4f => address.bit(1),
            Self::Vrc2a | Self::Vrc2c | Self::Vrc4b => address.bit(0),
            Self::Vrc4a | Self::Vrc4d => address.bit(2),
            Self::Vrc4c => address.bit(7),
            Self::Vrc4e => address.bit(3),
        }
    }

    fn remap_address(self, address: u16) -> Option<u16> {
        let a0 = self.a0(address);
        let a1 = self.a1(address);

        (a0 || a1).then_some((address & 0xFF00) | (u16::from(a1) << 1) | u16::from(a0))
    }

    fn to_type(self) -> Type {
        match self {
            Self::Vrc2a | Self::Vrc2b | Self::Vrc2c => Type::Vrc2,
            Self::Vrc4a | Self::Vrc4b | Self::Vrc4c | Self::Vrc4d | Self::Vrc4e | Self::Vrc4f => {
                Type::Vrc4
            }
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Vrc2a => "VRC2a",
            Self::Vrc2b => "VRC2b",
            Self::Vrc2c => "VRC2c",
            Self::Vrc4a => "VRC4a",
            Self::Vrc4b => "VRC4b",
            Self::Vrc4c => "VRC4c",
            Self::Vrc4d => "VRC4d",
            Self::Vrc4e => "VRC4e",
            Self::Vrc4f => "VRC4f",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Variant {
    Single(SingleVariant),
    Multi(SingleVariant, SingleVariant),
}

impl Variant {
    fn to_type(self) -> Type {
        match self {
            Self::Single(variant) => variant.to_type(),
            Self::Multi(..) => Type::Vrc4,
        }
    }

    fn remap_address(self, address: u16) -> u16 {
        (match self {
            Self::Single(variant) => variant.remap_address(address),
            Self::Multi(a, b) => a.remap_address(address).or(b.remap_address(address)),
        })
        .unwrap_or(address & 0xFF00)
    }

    fn name(self) -> String {
        match self {
            Self::Single(variant) => variant.name().into(),
            Self::Multi(a, b) => format!("VRC4 multi ({} or {})", a.name(), b.name()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PrgMode {
    Mode0,
    Mode1,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Vrc4 {
    variant: Variant,
    prg_mode: PrgMode,
    prg_bank_0: u8,
    prg_bank_1: u8,
    chr_type: ChrType,
    chr_banks: [u16; 8],
    nametable_mirroring: NametableMirroring,
    ram_enabled: bool,
    irq: VrcIrqCounter,
    vrc2_ram_bit: u8,
}

impl Vrc4 {
    pub(crate) fn new(mapper_number: u16, sub_mapper_number: u8, chr_type: ChrType) -> Self {
        let variant = match (mapper_number, sub_mapper_number) {
            (21, 1) => Variant::Single(SingleVariant::Vrc4a),
            (21, 2) => Variant::Single(SingleVariant::Vrc4c),
            (21, _) => Variant::Multi(SingleVariant::Vrc4a, SingleVariant::Vrc4c),
            (22, _) => Variant::Single(SingleVariant::Vrc2a),
            (23, 1) => Variant::Single(SingleVariant::Vrc4f),
            (23, 2) => Variant::Single(SingleVariant::Vrc4e),
            (23, 3) => Variant::Single(SingleVariant::Vrc2b),
            (23, _) => Variant::Multi(SingleVariant::Vrc4e, SingleVariant::Vrc4f),
            (25, 1) => Variant::Single(SingleVariant::Vrc4b),
            (25, 2) => Variant::Single(SingleVariant::Vrc4d),
            (25, 3) => Variant::Single(SingleVariant::Vrc2c),
            (25, _) => Variant::Multi(SingleVariant::Vrc4b, SingleVariant::Vrc4d),
            _ => {
                panic!("unsupported VRC2/VRC4 mapper number, expected 21/22/23/25: {mapper_number}")
            }
        };

        log::info!("VRC2/VRC4 variant: {}", variant.name());

        Self {
            variant,
            prg_mode: PrgMode::Mode0,
            prg_bank_0: 0,
            prg_bank_1: 0,
            chr_type,
            chr_banks: [0; 8],
            nametable_mirroring: NametableMirroring::Vertical,
            ram_enabled: false,
            irq: VrcIrqCounter::new(),
            vrc2_ram_bit: 0,
        }
    }
}

impl MapperImpl<Vrc4> {
    pub(crate) fn read_cpu_address(&self, address: u16, cpu_open_bus: u8) -> u8 {
        match (self.data.variant.to_type(), address) {
            (_, 0x0000..=0x401F) => panic!("invalid CPU map address: {address:04X}"),
            (_, 0x4020..=0x5FFF) => cpu_open_bus,
            (Type::Vrc2, 0x6000..=0x7FFF) => {
                if !self.cartridge.prg_ram.is_empty() {
                    self.cartridge.get_prg_ram((address & 0x1FFF).into())
                } else if address < 0x7000 {
                    (cpu_open_bus & 0xFE) | self.data.vrc2_ram_bit
                } else {
                    cpu_open_bus
                }
            }
            (Type::Vrc4, 0x6000..=0x7FFF) => {
                if self.data.ram_enabled {
                    match (self.cartridge.prg_ram.len(), address) {
                        (8192, _) => self.cartridge.get_prg_ram((address & 0x1FFF).into()),
                        (2048, 0x6000..=0x6FFF) => {
                            self.cartridge.get_prg_ram((address & 0x07FF).into())
                        }
                        _ => cpu_open_bus,
                    }
                } else {
                    cpu_open_bus
                }
            }
            (_, 0x8000..=0xFFFF) => match (self.data.prg_mode, address) {
                (_, 0x0000..=0x7FFF) => unreachable!("nested match expressions"),
                (PrgMode::Mode0, 0x8000..=0x9FFF) | (PrgMode::Mode1, 0xC000..=0xDFFF) => {
                    let prg_rom_addr =
                        BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_0, address);
                    self.cartridge.get_prg_rom(prg_rom_addr)
                }
                (_, 0xA000..=0xBFFF) => {
                    let prg_rom_addr =
                        BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_1, address);
                    self.cartridge.get_prg_rom(prg_rom_addr)
                }
                (PrgMode::Mode0, 0xC000..=0xDFFF) | (PrgMode::Mode1, 0x8000..=0x9FFF) => {
                    let prg_rom_addr = BankSizeKb::Eight.to_absolute_address_from_end(
                        2_u32,
                        self.cartridge.prg_rom.len() as u32,
                        address,
                    );
                    self.cartridge.get_prg_rom(prg_rom_addr)
                }
                (_, 0xE000..=0xFFFF) => {
                    let prg_rom_addr = BankSizeKb::Eight.to_absolute_address_last_bank(
                        self.cartridge.prg_rom.len() as u32,
                        address,
                    );
                    self.cartridge.get_prg_rom(prg_rom_addr)
                }
            },
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        log::trace!("CPU write: address={address:04X}, value={value:02X}");
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => match self.data.variant.to_type() {
                Type::Vrc2 => {
                    if !self.cartridge.prg_ram.is_empty() {
                        self.cartridge.set_prg_ram((address & 0x1FFF).into(), value);
                    } else if address < 0x7000 {
                        self.data.vrc2_ram_bit = value & 0x01;
                    }
                }
                Type::Vrc4 => {
                    if self.data.ram_enabled {
                        match (self.cartridge.prg_ram.len(), address) {
                            (8192, 0x6000..=0x7FFF) => {
                                self.cartridge.set_prg_ram((address & 0x1FFF).into(), value);
                            }
                            (2048, 0x6000..=0x6FFF) => {
                                self.cartridge.set_prg_ram((address & 0x07FF).into(), value);
                            }
                            _ => {}
                        }
                    }
                }
            },
            0x8000..=0x8FFF => {
                self.data.prg_bank_0 = value & 0x1F;
            }
            0x9000..=0x9FFF => {
                let remapped = self.data.variant.remap_address(address) & 0x9003;
                match (self.data.variant.to_type(), remapped) {
                    (Type::Vrc2, 0x9000..=0x9003) => {
                        self.data.nametable_mirroring = if value.bit(0) {
                            NametableMirroring::Horizontal
                        } else {
                            NametableMirroring::Vertical
                        };
                    }
                    (Type::Vrc4, 0x9000) => {
                        self.data.nametable_mirroring = match value & 0x03 {
                            0x00 => NametableMirroring::Vertical,
                            0x01 => NametableMirroring::Horizontal,
                            0x02 => NametableMirroring::SingleScreenBank0,
                            0x03 => NametableMirroring::SingleScreenBank1,
                            _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                        };
                    }
                    (Type::Vrc4, 0x9002) => {
                        self.data.ram_enabled = value.bit(0);
                        self.data.prg_mode =
                            if value.bit(1) { PrgMode::Mode1 } else { PrgMode::Mode0 };
                    }
                    _ => {}
                }
            }
            0xA000..=0xAFFF => {
                self.data.prg_bank_1 = value & 0x1F;
            }
            0xB000..=0xEFFF => {
                let remapped = self.data.variant.remap_address(address);
                // $B000, $B001 => 0
                // $B002, $B003 => 1
                // $C000, $C001 => 2
                // $C002, $C003 => 3
                // $D000, $D001 => 4
                // $D002, $D003 => 5
                // $E000, $E001 => 6
                // $E002, $E003 => 7
                let chr_bank_index = 2 * ((remapped - 0xB000) / 0x1000) + ((remapped & 0x02) >> 1);
                let existing_value = self.data.chr_banks[chr_bank_index as usize];
                if !remapped.bit(0) {
                    match self.data.variant {
                        Variant::Single(SingleVariant::Vrc2a) => {
                            // In VRC2a, everything is shifted right one
                            self.data.chr_banks[chr_bank_index as usize] =
                                (existing_value & !(0x0F >> 1)) | u16::from((value & 0x0F) >> 1);
                        }
                        _ => {
                            self.data.chr_banks[chr_bank_index as usize] =
                                (existing_value & !0x0F) | u16::from(value & 0x0F);
                        }
                    }
                } else {
                    match self.data.variant {
                        Variant::Single(SingleVariant::Vrc2a) => {
                            // In VRC2a, everything is shifted right one
                            self.data.chr_banks[chr_bank_index as usize] =
                                (existing_value & 0x07) | (u16::from(value & 0x0F) << 3);
                        }
                        _ => match self.data.variant.to_type() {
                            Type::Vrc2 => {
                                self.data.chr_banks[chr_bank_index as usize] =
                                    (existing_value & 0x0F) | (u16::from(value & 0x0F) << 4);
                            }
                            Type::Vrc4 => {
                                self.data.chr_banks[chr_bank_index as usize] =
                                    (existing_value & 0x0F) | (u16::from(value & 0x1F) << 4);
                            }
                        },
                    }
                }
            }
            0xF000..=0xFFFF => {
                if self.data.variant.to_type() == Type::Vrc4 {
                    match self.data.variant.remap_address(address) & 0xF003 {
                        0xF000 => {
                            self.data.irq.set_reload_value_low_4_bits(value & 0x0F);
                        }
                        0xF001 => {
                            self.data.irq.set_reload_value_high_4_bits(value & 0x0F);
                        }
                        0xF002 => {
                            self.data.irq.set_control(value);
                        }
                        0xF003 => {
                            self.data.irq.acknowledge();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.variant.to_type() == Type::Vrc4 && self.data.irq.interrupt_flag()
    }

    pub(crate) fn tick_cpu(&mut self) {
        if self.data.variant.to_type() == Type::Vrc4 {
            self.data.irq.tick_cpu();
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self.data.variant.to_type() {
            Type::Vrc2 => "VRC2",
            Type::Vrc4 => "VRC4",
        }
    }
}

impl HasBasicPpuMapping for MapperImpl<Vrc4> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        konami::map_ppu_address(
            address,
            &self.data.chr_banks,
            self.data.chr_type,
            self.data.nametable_mirroring,
        )
    }
}
