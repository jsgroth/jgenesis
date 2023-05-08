use crate::bus::cartridge::mappers::konami::irq::VrcIrqCounter;
use crate::bus::cartridge::mappers::{BankSizeKb, ChrType, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::{mappers, MapperImpl};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Vrc6a,
    Vrc6b,
}

impl Variant {
    fn remap_address(self, address: u16) -> u16 {
        match self {
            // Leave address unchanged
            Self::Vrc6a => address,
            // Swap A0 and A1
            Self::Vrc6b => {
                (address & 0xFFFC) | ((address & 0x0001) << 1) | ((address & 0x0002) >> 1)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Vrc6 {
    variant: Variant,
    prg_16kb_bank: u8,
    prg_8kb_bank: u8,
    chr_banks: [u8; 8],
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
    ram_enabled: bool,
    irq: VrcIrqCounter,
}

impl Vrc6 {
    pub(crate) fn new(mapper_number: u16, chr_type: ChrType) -> Self {
        let variant = match mapper_number {
            24 => Variant::Vrc6a,
            26 => Variant::Vrc6b,
            _ => panic!("invalid VRC6 mapper number, expected 24/26: {mapper_number}"),
        };

        log::info!("VRC6 variant: {variant:?}");

        Self {
            variant,
            prg_16kb_bank: 0,
            prg_8kb_bank: 0,
            chr_banks: [0; 8],
            chr_type,
            nametable_mirroring: NametableMirroring::Vertical,
            ram_enabled: false,
            irq: VrcIrqCounter::new(),
        }
    }
}

impl MapperImpl<Vrc6> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => mappers::cpu_open_bus(address),
            0x6000..=0x7FFF => {
                if self.data.ram_enabled && !self.cartridge.prg_ram.is_empty() {
                    self.cartridge.get_prg_ram((address & 0x1FFF).into())
                } else {
                    mappers::cpu_open_bus(address)
                }
            }
            0x8000..=0xBFFF => {
                let prg_rom_addr =
                    BankSizeKb::Sixteen.to_absolute_address(self.data.prg_16kb_bank, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xC000..=0xDFFF => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_8kb_bank, address);
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
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {
                if self.data.ram_enabled {
                    self.cartridge.set_prg_ram((address & 0x1FFF).into(), value);
                }
            }
            0x8000..=0xFFFF => {
                let remapped = self.data.variant.remap_address(address & 0xF003);
                match remapped {
                    0x8000..=0x8003 => {
                        self.data.prg_16kb_bank = value & 0x0F;
                    }
                    0xB003 => {
                        self.data.nametable_mirroring = match value & 0x0C {
                            0x00 => NametableMirroring::Vertical,
                            0x04 => NametableMirroring::Horizontal,
                            0x08 => NametableMirroring::SingleScreenBank0,
                            0x0C => NametableMirroring::SingleScreenBank1,
                            _ => unreachable!("value & 0x0C should always be 0x00/0x04/0x08/0x0C"),
                        };
                        self.data.ram_enabled = value & 0x80 != 0;
                    }
                    0xC000..=0xC003 => {
                        self.data.prg_8kb_bank = value & 0x1F;
                    }
                    0xD000..=0xE003 => {
                        // $D000 => 0
                        // $D001 => 1
                        // $D002 => 2
                        // $D003 => 3
                        // $E000 => 4
                        // $E001 => 5
                        // $E002 => 6
                        // $E003 => 7
                        let chr_bank_index = 4 * ((remapped - 0xD000) / 0x1000) + (remapped & 0x03);
                        self.data.chr_banks[chr_bank_index as usize] = value;
                    }
                    0xF000 => {
                        self.data.irq.set_reload_value(value);
                    }
                    0xF001 => {
                        self.data.irq.set_control(value);
                    }
                    0xF002 => {
                        self.data.irq.acknowledge();
                    }
                    _ => {}
                }
            }
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let chr_bank_index = address / 0x0400;
                let chr_bank_number = self.data.chr_banks[chr_bank_index as usize];
                let chr_addr = BankSizeKb::One.to_absolute_address(chr_bank_number, address);
                self.data.chr_type.to_map_result(chr_addr)
            }
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        }
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.irq.interrupt_flag()
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.irq.tick_cpu();
    }
}
