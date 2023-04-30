use crate::bus::cartridge::mappers::{CpuMapResult, NametableMirroring};
use crate::bus::cartridge::MapperImpl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChrBankLatch {
    FD,
    FE,
}

#[derive(Debug, Clone)]
pub(crate) struct Mmc2 {
    prg_bank: u8,
    chr_0_fd_bank: u8,
    chr_0_fe_bank: u8,
    chr_0_latch: ChrBankLatch,
    chr_1_fd_bank: u8,
    chr_1_fe_bank: u8,
    chr_1_latch: ChrBankLatch,
    nametable_mirroring: NametableMirroring,
}

impl Mmc2 {
    pub(crate) fn new() -> Self {
        Self {
            prg_bank: 0,
            chr_0_fd_bank: 0,
            chr_0_fe_bank: 0,
            chr_0_latch: ChrBankLatch::FD,
            chr_1_fd_bank: 0,
            chr_1_fe_bank: 0,
            chr_1_latch: ChrBankLatch::FD,
            nametable_mirroring: NametableMirroring::Vertical,
        }
    }
}

fn to_prg_rom_address(bank_number: u8, address: u16) -> u32 {
    // 8KB banks
    (u32::from(bank_number) << 13) | u32::from(address & 0x1FFF)
}

fn to_chr_rom_address(bank_number: u8, address: u16) -> u32 {
    // 4KB banks
    (u32::from(bank_number) << 12) | u32::from(address & 0x0FFF)
}

impl MapperImpl<Mmc2> {
    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x7FFF => CpuMapResult::None,
            0x8000..=0x9FFF => {
                CpuMapResult::PrgROM(to_prg_rom_address(self.data.prg_bank, address))
            }
            0xA000..=0xBFFF => {
                // Fixed at third-to-last PRG ROM bank
                let bank_number = ((self.cartridge.prg_rom.len() >> 13) - 3) as u8;
                CpuMapResult::PrgROM(to_prg_rom_address(bank_number, address))
            }
            0xC000..=0xDFFF => {
                // Fixed at second-to-last PRG ROM bank
                let bank_number = ((self.cartridge.prg_rom.len() >> 13) - 2) as u8;
                CpuMapResult::PrgROM(to_prg_rom_address(bank_number, address))
            }
            0xE000..=0xFFFF => {
                // Fixed at last PRG ROM bank
                let bank_number = ((self.cartridge.prg_rom.len() >> 13) - 1) as u8;
                CpuMapResult::PrgROM(to_prg_rom_address(bank_number, address))
            }
        }
    }

    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        self.map_cpu_address(address).read(&self.cartridge)
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x9FFF => {}
            0xA000..=0xAFFF => {
                self.data.prg_bank = value & 0x0F;
            }
            0xB000..=0xBFFF => {
                self.data.chr_0_fd_bank = value & 0x1F;
            }
            0xC000..=0xCFFF => {
                self.data.chr_0_fe_bank = value & 0x1F;
            }
            0xD000..=0xDFFF => {
                self.data.chr_1_fd_bank = value & 0x1F;
            }
            0xE000..=0xEFFF => {
                self.data.chr_1_fe_bank = value & 0x1F;
            }
            0xF000..=0xFFFF => {
                self.data.nametable_mirroring = if value & 0x01 != 0 {
                    NametableMirroring::Horizontal
                } else {
                    NametableMirroring::Vertical
                };
            }
        }
    }

    pub(crate) fn read_ppu_address(&mut self, address: u16, vram: &[u8; 2048]) -> u8 {
        let value = match address {
            0x0000..=0x0FFF => match self.data.chr_0_latch {
                ChrBankLatch::FD => {
                    let chr_rom_addr = to_chr_rom_address(self.data.chr_0_fd_bank, address);
                    self.cartridge.chr_rom[chr_rom_addr as usize]
                }
                ChrBankLatch::FE => {
                    let chr_rom_addr = to_chr_rom_address(self.data.chr_0_fe_bank, address);
                    self.cartridge.chr_rom[chr_rom_addr as usize]
                }
            },
            0x1000..=0x1FFF => match self.data.chr_1_latch {
                ChrBankLatch::FD => {
                    let chr_rom_addr = to_chr_rom_address(self.data.chr_1_fd_bank, address);
                    self.cartridge.chr_rom[chr_rom_addr as usize]
                }
                ChrBankLatch::FE => {
                    let chr_rom_addr = to_chr_rom_address(self.data.chr_1_fe_bank, address);
                    self.cartridge.chr_rom[chr_rom_addr as usize]
                }
            },
            0x2000..=0x3EFF => vram[self.data.nametable_mirroring.map_to_vram(address) as usize],
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        };

        // Check for FD/FE latch updates
        match address {
            0x0FD8 => {
                self.data.chr_0_latch = ChrBankLatch::FD;
            }
            0x0FE8 => {
                self.data.chr_0_latch = ChrBankLatch::FE;
            }
            0x1FD8..=0x1FDF => {
                self.data.chr_1_latch = ChrBankLatch::FD;
            }
            0x1FE8..=0x1FEF => {
                self.data.chr_1_latch = ChrBankLatch::FE;
            }
            _ => {}
        }

        value
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        match address {
            0x0000..=0x1FFF => {}
            0x2000..=0x3EFF => {
                let vram_addr = self.data.nametable_mirroring.map_to_vram(address);
                vram[vram_addr as usize] = value;
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        }
    }
}
