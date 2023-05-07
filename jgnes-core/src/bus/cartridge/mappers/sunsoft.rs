use crate::bus::cartridge::mappers::{ChrType, CpuMapResult, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::MapperImpl;

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrgType {
    ROM,
    RAM,
}

#[derive(Debug, Clone)]
pub(crate) struct Sunsoft {
    prg_banks: [u8; 4],
    prg_bank_0_type: PrgType,
    prg_ram_enabled: bool,
    chr_type: ChrType,
    chr_banks: [u8; 8],
    nametable_mirroring: NametableMirroring,
    command_register: u8,
    irq_enabled: bool,
    irq_counter_enabled: bool,
    irq_counter: u16,
    irq_triggered: bool,
}

impl Sunsoft {
    pub(crate) fn new(chr_type: ChrType) -> Self {
        Self {
            prg_banks: [0; 4],
            prg_bank_0_type: PrgType::ROM,
            prg_ram_enabled: false,
            chr_type,
            chr_banks: [0; 8],
            nametable_mirroring: NametableMirroring::Vertical,
            command_register: 0,
            irq_enabled: false,
            irq_counter_enabled: false,
            irq_counter: 0,
            irq_triggered: false,
        }
    }
}

impl MapperImpl<Sunsoft> {
    fn prg_address(bank_number: u8, address: u16) -> u32 {
        (u32::from(bank_number) << 13) | u32::from(address & 0x1FFF)
    }

    fn chr_address(bank_number: u8, address: u16) -> u32 {
        (u32::from(bank_number) << 10) | u32::from(address & 0x03FF)
    }

    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => CpuMapResult::None,
            0x6000..=0x7FFF => match self.data.prg_bank_0_type {
                PrgType::ROM => {
                    CpuMapResult::PrgROM(Self::prg_address(self.data.prg_banks[0], address))
                }
                PrgType::RAM => {
                    if self.data.prg_ram_enabled && !self.cartridge.prg_ram.is_empty() {
                        CpuMapResult::PrgRAM(Self::prg_address(self.data.prg_banks[0], address))
                    } else {
                        CpuMapResult::None
                    }
                }
            },
            0x8000..=0xDFFF => {
                // 0x8000..=0x9FFF to bank index 1
                // 0xA000..=0xBFFF to bank index 2
                // 0xC000..=0xDFFF to bank index 3
                let bank_index = (address - 0x6000) / 0x2000;
                CpuMapResult::PrgROM(Self::prg_address(
                    self.data.prg_banks[bank_index as usize],
                    address,
                ))
            }
            0xE000..=0xFFFF => {
                let prg_rom_addr =
                    self.cartridge.prg_rom.len() as u32 - 8192 + u32::from(address & 0x1FFF);
                CpuMapResult::PrgROM(prg_rom_addr)
            }
        }
    }

    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        self.map_cpu_address(address).read(&self.cartridge)
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF | 0xC000..=0xFFFF => {}
            0x6000..=0x7FFF => {
                self.map_cpu_address(address)
                    .write(value, &mut self.cartridge);
            }
            0x8000..=0x9FFF => {
                self.data.command_register = value & 0x0F;
            }
            0xA000..=0xBFFF => match self.data.command_register {
                0x00..=0x07 => {
                    self.data.chr_banks[self.data.command_register as usize] = value;
                }
                0x08 => {
                    self.data.prg_banks[0] = value & 0x3F;
                    self.data.prg_bank_0_type = if value & 0x40 != 0 {
                        PrgType::RAM
                    } else {
                        PrgType::ROM
                    };
                    self.data.prg_ram_enabled = value & 0x80 != 0;
                }
                0x09..=0x0B => {
                    let prg_bank_index = self.data.command_register - 0x08;
                    self.data.prg_banks[prg_bank_index as usize] = value & 0x3F;
                }
                0x0C => {
                    self.data.nametable_mirroring = match value & 0x03 {
                        0x00 => NametableMirroring::Vertical,
                        0x01 => NametableMirroring::Horizontal,
                        0x02 => NametableMirroring::SingleScreenBank0,
                        0x03 => NametableMirroring::SingleScreenBank1,
                        _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                    };
                }
                0x0D => {
                    self.data.irq_enabled = value & 0x01 != 0;
                    self.data.irq_counter_enabled = value & 0x80 != 0;
                    self.data.irq_triggered = false;
                }
                0x0E => {
                    self.data.irq_counter = (self.data.irq_counter & 0xFF00) | u16::from(value);
                }
                0x0F => {
                    self.data.irq_counter =
                        (self.data.irq_counter & 0x00FF) | (u16::from(value) << 8);
                }
                _ => panic!("command register should always contain 0-15"),
            },
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let chr_bank_index = address / 0x0400;
                let chr_addr =
                    Self::chr_address(self.data.chr_banks[chr_bank_index as usize], address);
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
        self.data.irq_triggered
    }

    pub(crate) fn tick_cpu(&mut self) {
        if !self.data.irq_counter_enabled {
            return;
        }

        if self.data.irq_enabled && self.data.irq_counter == 0 {
            self.data.irq_triggered = true;
        }
        self.data.irq_counter = self.data.irq_counter.wrapping_sub(1);
    }
}
