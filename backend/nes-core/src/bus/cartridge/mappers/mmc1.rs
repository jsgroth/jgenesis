//! Code for the MMC1 board (iNES mapper 1).

use crate::bus::cartridge::mappers::{
    BankSizeKb, ChrType, CpuMapResult, NametableMirroring, PpuMapResult,
};
use crate::bus::cartridge::{HasBasicPpuMapping, MapperImpl};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub(crate) enum PrgBankingMode {
    Switch32Kb,
    Switch16KbFirstBankFixed,
    Switch16KbLastBankFixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub(crate) enum ChrBankingMode {
    Single8KbBank,
    Two4KbBanks,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Mmc1 {
    chr_type: ChrType,
    shift_register: u8,
    shift_register_len: u8,
    written_this_cycle: bool,
    written_last_cycle: bool,
    nametable_mirroring: NametableMirroring,
    prg_banking_mode: PrgBankingMode,
    chr_banking_mode: ChrBankingMode,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,
    prg_bank_upper_bit: u8,
}

impl Mmc1 {
    pub(crate) fn new(chr_type: ChrType) -> Self {
        Self {
            chr_type,
            shift_register: 0,
            shift_register_len: 0,
            written_this_cycle: false,
            written_last_cycle: false,
            nametable_mirroring: NametableMirroring::Horizontal,
            prg_banking_mode: PrgBankingMode::Switch16KbLastBankFixed,
            chr_banking_mode: ChrBankingMode::Single8KbBank,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            prg_bank_upper_bit: 0,
        }
    }

    fn full_prg_bank(&self) -> u8 {
        self.prg_bank_upper_bit | self.prg_bank
    }
}

impl MapperImpl<Mmc1> {
    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x5FFF => CpuMapResult::None { original_address: address },
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    CpuMapResult::PrgRAM(u32::from(address & 0x1FFF))
                } else {
                    CpuMapResult::None { original_address: address }
                }
            }
            0x8000..=0xFFFF => match self.data.prg_banking_mode {
                PrgBankingMode::Switch32Kb => {
                    // In 32KB mode, treat the bank number as a 16KB bank number but ignore the lowest bit
                    let prg_rom_addr = BankSizeKb::ThirtyTwo
                        .to_absolute_address(self.data.full_prg_bank() >> 1, address);
                    CpuMapResult::PrgROM(prg_rom_addr)
                }
                PrgBankingMode::Switch16KbFirstBankFixed => match address {
                    0x8000..=0xBFFF => {
                        // If PRG ROM is 512KB, the upper bit affects the fixed bank
                        let rom_addr = u32::from(address & BankSizeKb::Sixteen.address_mask())
                            | (u32::from(self.data.prg_bank_upper_bit) << 18);
                        CpuMapResult::PrgROM(rom_addr)
                    }
                    0xC000..=0xFFFF => {
                        let prg_rom_addr = BankSizeKb::Sixteen
                            .to_absolute_address(self.data.full_prg_bank(), address);
                        CpuMapResult::PrgROM(prg_rom_addr)
                    }
                    _ => unreachable!("match arm should be unreachable"),
                },
                PrgBankingMode::Switch16KbLastBankFixed => match address {
                    0x8000..=0xBFFF => {
                        let prg_rom_addr = BankSizeKb::Sixteen
                            .to_absolute_address(self.data.full_prg_bank(), address);
                        CpuMapResult::PrgROM(prg_rom_addr)
                    }
                    0xC000..=0xFFFF => {
                        // If PRG ROM is 512KB, the upper bit affects the fixed bank
                        let last_bank = self.data.prg_bank_upper_bit | 0xF;
                        let prg_rom_addr =
                            BankSizeKb::Sixteen.to_absolute_address(last_bank, address);
                        CpuMapResult::PrgROM(prg_rom_addr)
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
                    self.cartridge.set_prg_ram(u32::from(address & 0x1FFF), value);
                }
            }
            0x8000..=0xFFFF => {
                self.data.written_this_cycle = true;

                if value.bit(7) {
                    self.data.shift_register = 0;
                    self.data.shift_register_len = 0;
                    self.data.prg_banking_mode = PrgBankingMode::Switch16KbLastBankFixed;
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
                                0x00 => NametableMirroring::SingleScreenBank0,
                                0x01 => NametableMirroring::SingleScreenBank1,
                                0x02 => NametableMirroring::Vertical,
                                0x03 => NametableMirroring::Horizontal,
                                _ => unreachable!(
                                    "{shift_register} & 0x03 was not 0x00/0x01/0x02/0x03",
                                ),
                            };

                            self.data.prg_banking_mode = match shift_register & 0x0C {
                                0x00 | 0x04 => PrgBankingMode::Switch32Kb,
                                0x08 => PrgBankingMode::Switch16KbFirstBankFixed,
                                0x0C => PrgBankingMode::Switch16KbLastBankFixed,
                                _ => unreachable!(
                                    "{shift_register} & 0x0C was not 0x00/0x04/0x08/0x0C"
                                ),
                            };

                            self.data.chr_banking_mode = if shift_register.bit(4) {
                                ChrBankingMode::Two4KbBanks
                            } else {
                                ChrBankingMode::Single8KbBank
                            };
                        }
                        0xA000..=0xBFFF => {
                            self.data.chr_bank_0 = shift_register;

                            // Some MMC1 variants use bit 4 of CHR bank 0 as PRG ROM A18
                            // Dragon Warrior 3 & 4 depend on this
                            if self.cartridge.prg_rom.len() >= 512 * 1024 {
                                self.data.prg_bank_upper_bit = shift_register & 0x10;
                            }
                        }
                        0xC000..=0xDFFF => {
                            self.data.chr_bank_1 = shift_register;

                            // Some MMC1 variants use bit 4 of CHR bank 1 as PRG ROM A18, but only
                            // when not in 8KB mode
                            if self.cartridge.prg_rom.len() >= 512 * 1024
                                && self.data.chr_banking_mode != ChrBankingMode::Single8KbBank
                            {
                                self.data.prg_bank_upper_bit = shift_register & 0x10;
                            }
                        }
                        0xE000..=0xFFFF => {
                            self.data.prg_bank = shift_register & 0x0F;
                        }
                        _ => unreachable!("match arm should be unreachable"),
                    }
                }
            }
        }
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.written_last_cycle = self.data.written_this_cycle;
        self.data.written_this_cycle = false;
    }
}

impl HasBasicPpuMapping for MapperImpl<Mmc1> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => match self.data.chr_banking_mode {
                ChrBankingMode::Two4KbBanks => {
                    let bank_number =
                        if address < 0x1000 { self.data.chr_bank_0 } else { self.data.chr_bank_1 };
                    let chr_addr = BankSizeKb::Four.to_absolute_address(bank_number, address);
                    self.data.chr_type.to_map_result(chr_addr)
                }
                ChrBankingMode::Single8KbBank => {
                    // In 8KB mode, use CHR bank 0 and treat it as a 4KB bank number while ignoring
                    // the lowest bit
                    let chr_addr =
                        BankSizeKb::Eight.to_absolute_address(self.data.chr_bank_0 >> 1, address);
                    self.data.chr_type.to_map_result(chr_addr)
                }
            },
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }
}
