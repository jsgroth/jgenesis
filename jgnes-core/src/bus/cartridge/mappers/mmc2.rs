use crate::bus::cartridge::mappers::{BankSizeKb, CpuMapResult, NametableMirroring};
use crate::bus::cartridge::MapperImpl;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ChrBankLatch {
    FD,
    FE,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Variant {
    Mmc2,
    Mmc4,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Mmc2 {
    variant: Variant,
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
    pub(crate) fn new_mmc2() -> Self {
        Self {
            variant: Variant::Mmc2,
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

    pub(crate) fn new_mmc4() -> Self {
        Self {
            variant: Variant::Mmc4,
            ..Self::new_mmc2()
        }
    }
}

impl MapperImpl<Mmc2> {
    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match (self.data.variant, address) {
            (_, 0x0000..=0x401F) => panic!("invalid CPU map address: {address:04X}"),
            (_, 0x4020..=0x5FFF) => CpuMapResult::None,
            (_, 0x6000..=0x7FFF) => {
                if !self.cartridge.prg_ram.is_empty() {
                    CpuMapResult::PrgRAM(u32::from(address & 0x1FFF))
                } else {
                    CpuMapResult::None
                }
            }
            (Variant::Mmc2, 0x8000..=0x9FFF) => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_bank, address);
                CpuMapResult::PrgROM(prg_rom_addr)
            }
            (Variant::Mmc2, 0xA000..=0xBFFF) => {
                // Fixed at third-to-last PRG ROM bank
                let prg_rom_addr = BankSizeKb::Eight.to_absolute_address_from_end(
                    3_u32,
                    self.cartridge.prg_rom.len() as u32,
                    address,
                );
                CpuMapResult::PrgROM(prg_rom_addr)
            }
            (Variant::Mmc2, 0xC000..=0xDFFF) => {
                // Fixed at second-to-last PRG ROM bank
                let prg_rom_addr = BankSizeKb::Eight.to_absolute_address_from_end(
                    2_u32,
                    self.cartridge.prg_rom.len() as u32,
                    address,
                );
                CpuMapResult::PrgROM(prg_rom_addr)
            }
            (Variant::Mmc2, 0xE000..=0xFFFF) => {
                // Fixed at last PRG ROM bank
                let prg_rom_addr = BankSizeKb::Eight
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
                CpuMapResult::PrgROM(prg_rom_addr)
            }
            (Variant::Mmc4, 0x8000..=0xBFFF) => {
                let prg_rom_addr =
                    BankSizeKb::Sixteen.to_absolute_address(self.data.prg_bank, address);
                CpuMapResult::PrgROM(prg_rom_addr)
            }
            (Variant::Mmc4, 0xC000..=0xFFFF) => {
                // Fixed at last PRG ROM bank
                let prg_rom_addr = BankSizeKb::Sixteen
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
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
            0x4020..=0x5FFF | 0x8000..=0x9FFF => {}
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    self.cartridge
                        .set_prg_ram(u32::from(address & 0x1FFF), value);
                }
            }
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
                    let chr_rom_addr =
                        BankSizeKb::Four.to_absolute_address(self.data.chr_0_fd_bank, address);
                    self.cartridge.get_chr_rom(chr_rom_addr)
                }
                ChrBankLatch::FE => {
                    let chr_rom_addr =
                        BankSizeKb::Four.to_absolute_address(self.data.chr_0_fe_bank, address);
                    self.cartridge.get_chr_rom(chr_rom_addr)
                }
            },
            0x1000..=0x1FFF => match self.data.chr_1_latch {
                ChrBankLatch::FD => {
                    let chr_rom_addr =
                        BankSizeKb::Four.to_absolute_address(self.data.chr_1_fd_bank, address);
                    self.cartridge.get_chr_rom(chr_rom_addr)
                }
                ChrBankLatch::FE => {
                    let chr_rom_addr =
                        BankSizeKb::Four.to_absolute_address(self.data.chr_1_fe_bank, address);
                    self.cartridge.get_chr_rom(chr_rom_addr)
                }
            },
            0x2000..=0x3EFF => vram[self.data.nametable_mirroring.map_to_vram(address) as usize],
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        };

        // Check for FD/FE latch updates
        match (self.data.variant, address) {
            (Variant::Mmc2, 0x0FD8) | (Variant::Mmc4, 0x0FD8..=0x0FDF) => {
                self.data.chr_0_latch = ChrBankLatch::FD;
            }
            (Variant::Mmc2, 0x0FE8) | (Variant::Mmc4, 0x0FE8..=0x0FEF) => {
                self.data.chr_0_latch = ChrBankLatch::FE;
            }
            (_, 0x1FD8..=0x1FDF) => {
                self.data.chr_1_latch = ChrBankLatch::FD;
            }
            (_, 0x1FE8..=0x1FEF) => {
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

    pub(crate) fn name(&self) -> &'static str {
        match self.data.variant {
            Variant::Mmc2 => "MMC2",
            Variant::Mmc4 => "MMC4",
        }
    }
}
