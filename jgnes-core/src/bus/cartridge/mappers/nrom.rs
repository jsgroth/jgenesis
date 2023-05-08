use crate::bus::cartridge::mappers::{BankSizeKb, ChrType, NametableMirroring, PpuMapResult};
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

    #[allow(clippy::unused_self)]
    pub(crate) fn write_cpu_address(&self, _address: u16, _value: u8) {}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UxromVariant {
    Uxrom,
    Codemasters,
    FireHawk,
}

#[derive(Debug, Clone)]
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
            0x4020..=0x7FFF => 0xFF,
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
                self.data.nametable_mirroring = if value & 0x10 != 0 {
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

    pub(crate) fn name(&self) -> &'static str {
        match self.data.variant {
            UxromVariant::Uxrom => "UxROM",
            UxromVariant::Codemasters => "Codemasters",
            UxromVariant::FireHawk => "Codemasters (Fire Hawk variant)",
        }
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
                let chr_addr = BankSizeKb::Eight.to_absolute_address(self.data.chr_bank, address);
                self.data.chr_type.to_map_result(chr_addr)
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
            return 0xFF;
        }

        let prg_rom_addr = BankSizeKb::ThirtyTwo.to_absolute_address(self.data.prg_bank, address);
        self.cartridge.get_prg_rom(prg_rom_addr)
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        if address < 0x8000 {
            return;
        }

        self.data.prg_bank = value & 0x07;
        self.data.nametable_mirroring = if value & 0x10 != 0 {
            NametableMirroring::SingleScreenBank1
        } else {
            NametableMirroring::SingleScreenBank0
        };
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
