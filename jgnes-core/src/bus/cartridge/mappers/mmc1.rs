use crate::bus::cartridge::mappers::{ChrType, CpuMapResult, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::MapperImpl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mmc1Mirroring {
    OneScreenLowerBank,
    OneScreenUpperBank,
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mmc1PrgBankingMode {
    Switch32Kb,
    Switch16KbFirstBankFixed,
    Switch16KbLastBankFixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mmc1ChrBankingMode {
    Single8KbBank,
    Two4KbBanks,
}

#[derive(Debug, Clone)]
pub(crate) struct Mmc1 {
    chr_type: ChrType,
    shift_register: u8,
    shift_register_len: u8,
    written_this_cycle: bool,
    written_last_cycle: bool,
    nametable_mirroring: Mmc1Mirroring,
    prg_banking_mode: Mmc1PrgBankingMode,
    chr_banking_mode: Mmc1ChrBankingMode,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,
}

impl Mmc1 {
    pub(crate) fn new(chr_type: ChrType) -> Self {
        Self {
            chr_type,
            shift_register: 0,
            shift_register_len: 0,
            written_this_cycle: false,
            written_last_cycle: false,
            nametable_mirroring: Mmc1Mirroring::Horizontal,
            prg_banking_mode: Mmc1PrgBankingMode::Switch16KbLastBankFixed,
            chr_banking_mode: Mmc1ChrBankingMode::Single8KbBank,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
        }
    }
}

impl MapperImpl<Mmc1> {
    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x5FFF => CpuMapResult::None,
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    CpuMapResult::PrgRAM(u32::from(address & 0x1FFF))
                } else {
                    CpuMapResult::None
                }
            }
            0x8000..=0xFFFF => match self.data.prg_banking_mode {
                Mmc1PrgBankingMode::Switch32Kb => {
                    let bank_address = u32::from(self.data.prg_bank & 0x0E) << 15;
                    CpuMapResult::PrgROM(bank_address + u32::from(address & 0x7FFF))
                }
                Mmc1PrgBankingMode::Switch16KbFirstBankFixed => match address {
                    0x8000..=0xBFFF => CpuMapResult::PrgROM(u32::from(address) & 0x3FFF),
                    0xC000..=0xFFFF => {
                        let bank_address = u32::from(self.data.prg_bank) << 14;
                        CpuMapResult::PrgROM(bank_address + (u32::from(address) & 0x3FFF))
                    }
                    _ => unreachable!("match arm should be unreachable"),
                },
                Mmc1PrgBankingMode::Switch16KbLastBankFixed => match address {
                    0x8000..=0xBFFF => {
                        let bank_address = u32::from(self.data.prg_bank) << 14;
                        CpuMapResult::PrgROM(bank_address + (u32::from(address) & 0x3FFF))
                    }
                    0xC000..=0xFFFF => {
                        let last_bank_address = self.cartridge.prg_rom.len() as u32 - 0x4000;
                        CpuMapResult::PrgROM(last_bank_address + (u32::from(address) & 0x3FFF))
                    }
                    _ => unreachable!("match arm should be unreachable"),
                },
            },
        }
    }

    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        self.map_cpu_address(address).read(&self.cartridge)
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    self.cartridge
                        .set_prg_ram(u32::from(address & 0x1FFF), value);
                }
            }
            0x8000..=0xFFFF => {
                self.data.written_this_cycle = true;

                if value & 0x80 != 0 {
                    self.data.shift_register = 0;
                    self.data.shift_register_len = 0;
                    self.data.prg_banking_mode = Mmc1PrgBankingMode::Switch16KbLastBankFixed;
                    return;
                }

                if self.data.written_last_cycle {
                    return;
                }

                self.data.shift_register = (self.data.shift_register >> 1) | ((value & 0x01) << 4);
                self.data.shift_register_len += 1;

                if self.data.shift_register_len == 5 {
                    let shift_register = self.data.shift_register;

                    self.data.shift_register = 0;
                    self.data.shift_register_len = 0;

                    match address {
                        0x8000..=0x9FFF => {
                            self.data.nametable_mirroring = match shift_register & 0x03 {
                                0x00 => Mmc1Mirroring::OneScreenLowerBank,
                                0x01 => Mmc1Mirroring::OneScreenUpperBank,
                                0x02 => Mmc1Mirroring::Vertical,
                                0x03 => Mmc1Mirroring::Horizontal,
                                _ => unreachable!(
                                    "{shift_register} & 0x03 was not 0x00/0x01/0x02/0x03",
                                ),
                            };

                            self.data.prg_banking_mode = match shift_register & 0x0C {
                                0x00 | 0x04 => Mmc1PrgBankingMode::Switch32Kb,
                                0x08 => Mmc1PrgBankingMode::Switch16KbFirstBankFixed,
                                0x0C => Mmc1PrgBankingMode::Switch16KbLastBankFixed,
                                _ => unreachable!(
                                    "{shift_register} & 0x0C was not 0x00/0x04/0x08/0x0C"
                                ),
                            };

                            self.data.chr_banking_mode = if shift_register & 0x10 != 0 {
                                Mmc1ChrBankingMode::Two4KbBanks
                            } else {
                                Mmc1ChrBankingMode::Single8KbBank
                            };
                        }
                        0xA000..=0xBFFF => {
                            self.data.chr_bank_0 = shift_register;
                        }
                        0xC000..=0xDFFF => {
                            self.data.chr_bank_1 = shift_register;
                        }
                        0xE000..=0xFFFF => {
                            self.data.prg_bank = shift_register;
                        }
                        _ => unreachable!("match arm should be unreachable"),
                    }
                }
            }
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => match self.data.chr_banking_mode {
                Mmc1ChrBankingMode::Two4KbBanks => {
                    let (bank_number, relative_addr) = if address < 0x1000 {
                        (self.data.chr_bank_0, address)
                    } else {
                        (self.data.chr_bank_1, address - 0x1000)
                    };
                    let bank_address = u32::from(bank_number) * 4 * 1024;
                    let chr_address = bank_address + u32::from(relative_addr);
                    self.data.chr_type.to_map_result(chr_address)
                }
                Mmc1ChrBankingMode::Single8KbBank => {
                    let chr_bank = self.data.chr_bank_0 & 0x1E;
                    let bank_address = u32::from(chr_bank) * 4 * 1024;
                    let chr_address = bank_address + u32::from(address);
                    self.data.chr_type.to_map_result(chr_address)
                }
            },
            0x2000..=0x3EFF => match self.data.nametable_mirroring {
                Mmc1Mirroring::OneScreenLowerBank => PpuMapResult::Vram(address & 0x03FF),
                Mmc1Mirroring::OneScreenUpperBank => {
                    PpuMapResult::Vram(0x0400 + (address & 0x03FF))
                }
                Mmc1Mirroring::Vertical => {
                    PpuMapResult::Vram(NametableMirroring::Vertical.map_to_vram(address))
                }
                Mmc1Mirroring::Horizontal => {
                    PpuMapResult::Vram(NametableMirroring::Horizontal.map_to_vram(address))
                }
            },
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

    pub(crate) fn tick_cpu(&mut self) {
        self.data.written_last_cycle = self.data.written_this_cycle;
        self.data.written_this_cycle = false;
    }
}
